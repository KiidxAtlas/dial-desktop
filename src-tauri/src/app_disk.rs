//! Import the disk chosen in a native dialog. Neither the webview nor MCP can supply a host path.
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

use crate::app_runner::runtime::{
    GuestTarget, guest_disk_path_for, preferred_guest_target, unsupported_message,
};

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod macos;

/// Import a virtual Mac that Dial's own prep tool just made in `folder` (App panel install).
/// Same checks and atomic publish as the dialog import, without the folder picker.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub(crate) fn import_prepared_mac(data_dir: &Path, folder: &Path) -> Result<(), String> {
    macos::import_folder(data_dir, folder)
}

#[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
pub(crate) fn import_prepared_mac(_: &Path, _: &Path) -> Result<(), String> {
    Err(unsupported_message(GuestTarget::Macos))
}

/// Import a base for the requested guest; omission uses the host's preferred guest.
#[tauri::command]
pub(crate) async fn app_runner_import(
    app: AppHandle,
    workspace_root: String,
    guest_target: Option<GuestTarget>,
) -> Result<bool, String> {
    let guest = guest_target.unwrap_or_else(preferred_guest_target);
    tauri::async_runtime::spawn_blocking(move || {
        let check_project = || {
            crate::app::browser_project(&app, &workspace_root)
                .then_some(())
                .ok_or("The open project changed. Choose the disk again.")
        };
        check_project()?;
        if guest == GuestTarget::Vm { return Err("Registered VMs do not use disk import.".into()); }
        if guest == GuestTarget::Macos {
            #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
            return macos::import(&app, || check_project().map_err(io::Error::other));
            #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
            return Err(unsupported_message(guest));
        }
        let Some(selected) = app.dialog().file().set_title("Choose a bootable raw guest disk")
            .add_filter("Raw disk image", &["raw", "img"]).blocking_pick_file() else { return Ok(false) };
        let path = selected.into_path().map_err(|e| e.to_string())?;
        if !fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
            return Err("Choose a regular raw disk image, not a device, directory or symlink.".into());
        }
        let file = File::open(&path).map_err(|e| e.to_string())?;
        let meta = file.metadata().map_err(|e| e.to_string())?;
        let size = meta.len();
        if !meta.is_file() || size < 512 || size % 512 != 0 {
            return Err("Expected a raw disk image made up of complete 512-byte sectors.".into());
        }
        if !app.dialog().message(format!("Copy {} ({} bytes) to Dial’s local app data as the {} guest disk? Projects can boot disposable guests from this base disk. Choose a disk without private data. Nothing is downloaded; the original is not changed.", path.file_name().unwrap_or_default().to_string_lossy(), size, guest.label()))
            .title("Import guest disk?").buttons(MessageDialogButtons::OkCancel).blocking_show() { return Ok(false); }
        check_project()?;
        let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let dest = guest_disk_path_for(&data, guest).ok_or_else(|| unsupported_message(guest))?;
        copy_disk(file, size, &dest, || check_project().map_err(io::Error::other)).map_err(|e| format!("Could not import guest disk: {e}"))?;
        Ok(true)
    }).await.map_err(|e| e.to_string())?
}

fn copy_disk(
    input: impl Read,
    size: u64,
    dest: &Path,
    check: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let dir = dest
        .parent()
        .ok_or_else(|| io::Error::other("missing import directory"))?;
    fs::create_dir_all(dir)?;
    if dest.try_exists()? {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "a guest disk is already installed",
        ));
    }
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    let n = io::copy(&mut input.take(size.saturating_add(1)), &mut tmp)?;
    if n != size {
        return Err(io::Error::other("source size changed during import"));
    }
    tmp.as_file().sync_all()?;
    check()?;
    tmp.persist_noclobber(dest)?;
    Ok(())
}

#[cfg(test)]
mod tests;
