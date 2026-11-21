//! Import one powered-off virtual Mac. Pin the selected directory and its four files before
//! asking for confirmation; publish the complete copy atomically without replacing any base.
use crate::app_runner::runtime::{MAC_BASE_FILES, MAC_IDENTITY_LIMIT, macos_base_dir};
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

pub(super) fn import(app: &AppHandle, check: impl Fn() -> io::Result<()>) -> Result<bool, String> {
    let Some(selected) = app
        .dialog()
        .file()
        .set_title("Choose a prepared, shut-down virtual Mac")
        .blocking_pick_folder()
    else {
        return Ok(false);
    };
    let path = selected.into_path().map_err(|e| e.to_string())?;
    let base = Base::open(&path).map_err(|e| format!("Invalid virtual Mac: {e}"))?;
    let size = base.size().map_err(|e| e.to_string())?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let confirmed = app
        .dialog()
        .message(format!(
            "Import the four virtual Mac files from {name} ({size} bytes) into Dial’s private app data? Shut down the source VM first. Use a licensed guest with the Dial guest companion and no personal accounts. Nothing is downloaded or run on your desktop."
        ))
        .title("Import private macOS guest?")
        .buttons(MessageDialogButtons::OkCancel)
        .blocking_show();
    if !confirmed {
        return Ok(false);
    }
    check().map_err(|e| e.to_string())?;
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    base.install(&macos_base_dir(&data), check)
        .map_err(|e| format!("Could not import virtual Mac: {e}"))?;
    Ok(true)
}

/// Import the four files in `folder` into the base location under `data_dir`, without a dialog.
pub(super) fn import_folder(data_dir: &Path, folder: &Path) -> Result<(), String> {
    let base = Base::open(folder).map_err(|e| format!("Invalid virtual Mac: {e}"))?;
    base.install(&macos_base_dir(data_dir), || Ok(()))
        .map_err(|e| e.to_string())
}

struct Part {
    name: &'static str,
    file: File,
    size: u64,
}
struct Base(Vec<Part>);

impl Base {
    fn open(path: &Path) -> io::Result<Self> {
        let dir = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(path)?;
        let mut parts = Vec::new();
        for name in MAC_BASE_FILES {
            let c_name = CString::new(name)?;
            // SAFETY: `dir` is an open directory and `c_name` is a live NUL-terminated basename.
            let fd = unsafe {
                libc::openat(
                    dir.as_raw_fd(),
                    c_name.as_ptr(),
                    libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
                )
            };
            if fd < 0 {
                return Err(io::Error::other(format!(
                    "{name}: {}",
                    io::Error::last_os_error()
                )));
            }
            // SAFETY: openat succeeded and sole ownership of this descriptor passes to File.
            let file = unsafe { File::from_raw_fd(fd) };
            let meta = file.metadata()?;
            if !meta.is_file() || meta.len() == 0 || (name == "Disk.img" && meta.len() % 512 != 0) {
                return Err(io::Error::other(format!(
                    "{name} is not a valid regular guest file"
                )));
            }
            if matches!(name, "HardwareModel" | "MachineIdentifier")
                && meta.len() > MAC_IDENTITY_LIMIT
            {
                return Err(io::Error::other(format!(
                    "{name} exceeds {MAC_IDENTITY_LIMIT} bytes"
                )));
            }
            parts.push(Part {
                name,
                file,
                size: meta.len(),
            });
        }
        Ok(Self(parts))
    }

    fn size(&self) -> io::Result<u64> {
        self.0.iter().try_fold(0u64, |n, part| {
            n.checked_add(part.size)
                .ok_or_else(|| io::Error::other("guest size overflow"))
        })
    }

    fn install(self, dest: &Path, check: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
        let parent = dest
            .parent()
            .ok_or_else(|| io::Error::other("no import directory"))?;
        fs::create_dir_all(parent)?;
        if !fs::symlink_metadata(parent)?.is_dir() {
            return Err(io::Error::other("invalid import directory"));
        }
        if dest.try_exists()? {
            return Err(io::Error::other("a macOS base is already imported"));
        }
        let tmp = tempfile::Builder::new()
            .prefix(".import-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(parent)?;
        let directory = File::open(tmp.path())?;
        for part in self.0 {
            let before = part.file.metadata()?;
            let name = CString::new(part.name)?;
            // CLONE_NOOWNERCOPY from Darwin's <sys/clonefile.h>, not exported by libc.
            const NO_OWNER_COPY: u32 = 0x0002;
            // SAFETY: both descriptors stay open during the call; the destination is a fixed
            // basename in a private staging directory. fclonefileat creates a new file.
            let cloned = unsafe {
                libc::fclonefileat(
                    part.file.as_raw_fd(),
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    NO_OWNER_COPY,
                )
            };
            let path = tmp.path().join(part.name);
            if cloned != 0 {
                let err = io::Error::last_os_error();
                if !matches!(err.raw_os_error(), Some(libc::EXDEV | libc::ENOTSUP)) {
                    return Err(err);
                }
                let mut output = File::create_new(&path)?;
                let count = io::copy(
                    &mut (&part.file).take(part.size.saturating_add(1)),
                    &mut output,
                )?;
                if count != part.size {
                    return Err(io::Error::other("guest changed during import"));
                }
                output.sync_all()?;
            }
            let after = part.file.metadata()?;
            if fs::metadata(&path)?.len() != part.size
                || after.len() != part.size
                || before.modified()? != after.modified()?
            {
                return Err(io::Error::other("guest changed during import"));
            }
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
        check()?;
        let from = CString::new(tmp.path().as_os_str().as_bytes())?;
        let to = CString::new(dest.as_os_str().as_bytes())?;
        // SAFETY: both path strings remain live. RENAME_EXCL publishes the whole directory
        // without replacing another import, including an empty directory created concurrently.
        if unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_EXCL) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let _published = tmp.keep();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
