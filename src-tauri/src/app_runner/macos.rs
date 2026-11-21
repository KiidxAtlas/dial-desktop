//! The macOS guest: a packaged helper that the app runs in place of QEMU. The helper is
//! `macos-guest/dial-mac-guest` under the resource directory; the packager signs and stages it.
//!
//! The helper is started as `dial-mac-guest run RUN_DIR [PROJECT_ISO]`. It reads only the files in
//! RUN_DIR and speaks the same bounded QMP subset as QEMU on its stdio, so the handshake, frame
//! capture and input code are shared with the QEMU backend.
//!
//! The base files live under the app data directory in `app-runner/macos/` and are never attached
//! to a guest. Each run gets private copies in its own folder: the large disk and the firmware
//! store by APFS `clonefile` (no-follow), and the two small identity files by a bounded copy. A
//! clone that cannot be made is a refusal; there is no synchronous copy of the large files.
//!
//! Only Apple silicon macOS has this backend, and nothing here falls back to QEMU.

use std::ffi::OsString;
use std::fs::{self, Metadata};
use std::io;
use std::path::{Path, PathBuf};

use super::runtime::{
    self, MAC_BASE_FILES, MAC_IDENTITY_LIMIT, MACOS_HELPER, MACOS_IMAGE, RuntimeError,
};

/// The large base files, cloned into each run: the disk and the firmware variable store.
const CLONED: [&str; 2] = ["Disk.img", "AuxiliaryStorage"];
/// The identity files, copied into each run. A larger file is refused rather than copied.
const COPIED: [&str; 2] = ["MachineIdentifier", "HardwareModel"];

/// The checked files the macOS backend starts from: the helper and the base folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacRuntime {
    pub helper: PathBuf,
    pub base: PathBuf,
}

/// Check the packaged helper and the base files. Every component must be a plain entry that is
/// not a link; the app-data ones must also belong to the current user. A missing part is named by
/// its component id, and anything present but unusable is refused with the reason.
pub fn discover(resource_dir: &Path, data_dir: &Path) -> Result<MacRuntime, RuntimeError> {
    let helper = trusted_path(
        resource_dir,
        &[runtime::HELPER_DIR, runtime::HELPER_NAME],
        MACOS_HELPER,
        false,
    )?;
    let meta = fs::metadata(&helper)
        .map_err(|error| RuntimeError::Refused(format!("{}: {error}", helper.display())))?;
    if !executable(&meta) {
        return Err(RuntimeError::Refused(format!(
            "{} is not executable",
            helper.display()
        )));
    }
    for name in MAC_BASE_FILES {
        trusted_path(
            data_dir,
            &[runtime::APP_RUNNER_DIR, runtime::MAC_BASE_DIR, name],
            MACOS_IMAGE,
            true,
        )?;
    }
    Ok(MacRuntime {
        helper,
        base: runtime::macos_base_dir(data_dir),
    })
}

/// Walk `parts` below `root`. Each folder must be a real folder and the last entry a regular file.
/// A missing entry is `Missing(missing)`; any other failure is a refusal.
fn trusted_path(
    root: &Path,
    parts: &[&str],
    missing: &'static str,
    private: bool,
) -> Result<PathBuf, RuntimeError> {
    let mut current = root.to_path_buf();
    let last = parts.len().saturating_sub(1);
    for (index, part) in parts.iter().enumerate() {
        current.push(part);
        let meta = match fs::symlink_metadata(&current) {
            Ok(meta) => meta,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(RuntimeError::Missing(missing));
            }
            Err(error) => {
                return Err(RuntimeError::Refused(format!(
                    "{} cannot be read: {error}",
                    current.display()
                )));
            }
        };
        if meta.file_type().is_symlink() {
            return Err(RuntimeError::Refused(format!(
                "{} is a link; the macOS guest refuses links",
                current.display()
            )));
        }
        if index == last {
            if !meta.is_file() {
                return Err(RuntimeError::Refused(format!(
                    "{} is not a regular file",
                    current.display()
                )));
            }
        } else if !meta.is_dir() {
            return Err(RuntimeError::Refused(format!(
                "{} is not a folder",
                current.display()
            )));
        }
        if private && !owned_by_user(&meta) {
            return Err(RuntimeError::Refused(format!(
                "{} is not owned by the current user",
                current.display()
            )));
        }
    }
    Ok(current)
}

#[cfg(unix)]
fn executable(meta: &Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable(_: &Metadata) -> bool {
    true
}

#[cfg(unix)]
fn owned_by_user(meta: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    // SAFETY: `geteuid` takes no arguments, cannot fail and has no preconditions.
    meta.uid() == unsafe { geteuid() }
}

#[cfg(not(unix))]
fn owned_by_user(_: &Metadata) -> bool {
    true
}

/// The helper's argv after the program name: `run RUN_DIR`, plus the project ISO when the user
/// asked to share the project. Each path is one argument; nothing is shell-quoted.
pub fn args(run_dir: &Path, project_iso: Option<&Path>) -> Vec<OsString> {
    let mut out = vec![OsString::from("run"), run_dir.as_os_str().to_owned()];
    if let Some(iso) = project_iso {
        out.push(iso.as_os_str().to_owned());
    }
    out
}

/// Make the run's private copies of the base files in `run_dir`. The base folder is only read.
/// On failure the run is not started; a partly made folder is removed with the run.
pub fn stage(mac: &MacRuntime, run_dir: &Path) -> Result<(), String> {
    for name in CLONED {
        clone_file(&mac.base.join(name), &run_dir.join(name)).map_err(|error| {
            format!(
                "the macOS guest could not make a copy-on-write clone of {name} ({error}); the app data folder must be on an APFS volume"
            )
        })?;
    }
    for name in COPIED {
        copy_small(&mac.base.join(name), &run_dir.join(name))?;
    }
    Ok(())
}

/// Copy a small identity file. Only regular files within [`MAC_IDENTITY_LIMIT`] are copied.
fn copy_small(src: &Path, dst: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(src).map_err(|error| format!("{}: {error}", src.display()))?;
    if !meta.is_file() || meta.len() > MAC_IDENTITY_LIMIT {
        return Err(format!(
            "{} must be a regular file of at most {MAC_IDENTITY_LIMIT} bytes",
            src.display()
        ));
    }
    fs::copy(src, dst)
        .map(|_| ())
        .map_err(|error| format!("{} could not be copied: {error}", src.display()))
}

#[cfg(target_os = "macos")]
fn clone_file(src: &Path, dst: &Path) -> io::Result<()> {
    use std::ffi::{CString, c_char, c_int};
    use std::os::unix::ffi::OsStrExt;

    unsafe extern "C" {
        fn clonefile(src: *const c_char, dst: *const c_char, flags: u32) -> c_int;
    }
    /// Refuse a link at the source: the base files are never followed.
    const CLONE_NOFOLLOW: u32 = 0x0001;
    let c_path = |path: &Path| {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
    };
    let src = c_path(src)?;
    let dst = c_path(dst)?;
    // SAFETY: both strings are NUL-terminated and live across the call, which only reads them.
    let status = unsafe { clonefile(src.as_ptr(), dst.as_ptr(), CLONE_NOFOLLOW) };
    if status == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "macos"))]
fn clone_file(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "copy-on-write clones exist only on macOS",
    ))
}

#[cfg(test)]
mod tests;
