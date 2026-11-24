//! Import the disk chosen in a native dialog. Neither the webview nor MCP can supply a host path.
//!
//! Raw disks (`.raw`, `.img`) are copied. QCOW2, VMDK and VDI disks are converted to raw with
//! `qemu-img`, found in the bundled runtime or a fixed machine-wide location (never `PATH`);
//! without it the user is told how to convert. Either way the import can be cancelled, and a
//! partial disk is removed.
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

use crate::app_runner::runtime::{
    self, GuestTarget, guest_disk_path_for, preferred_guest_target, unsupported_message,
};

/// Import progress, sent as `app_runner_import` events while a disk copies or converts.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportProgress {
    copied: u64,
    total: u64,
    /// `qemu-img` is converting: `copied` of `total` is the percentage done (out of 10 000).
    converting: bool,
}

/// Set by [`app_runner_import_cancel`]; the running import stops at its next check.
static CANCEL: AtomicBool = AtomicBool::new(false);

/// Disk formats `qemu-img` converts to raw on import.
const CONVERTIBLE: [&str; 3] = ["qcow2", "vmdk", "vdi"];

/// Stop the disk import in flight; its partial copy is removed. Harmless when none runs.
#[tauri::command]
pub(crate) fn app_runner_import_cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

fn cancelled() -> bool {
    CANCEL.load(Ordering::SeqCst)
}

fn cancel_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "the import was cancelled")
}

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
        let Some(selected) = app.dialog().file().set_title("Choose a bootable guest disk")
            .add_filter("Disk image", &["raw", "img", "qcow2", "vmdk", "vdi"]).blocking_pick_file() else { return Ok(false) };
        let path = selected.into_path().map_err(|e| e.to_string())?;
        if !fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
            return Err("Choose a regular disk image file, not a device, directory or symlink.".into());
        }
        let format = convertible_format(&path);
        let qemu_img = match format {
            Some(format) => Some(find_qemu_img(&app).ok_or_else(|| no_qemu_img_message(format))?),
            None => None,
        };
        let file = File::open(&path).map_err(|e| e.to_string())?;
        let meta = file.metadata().map_err(|e| e.to_string())?;
        let size = meta.len();
        if format.is_none() && (!meta.is_file() || size < 512 || size % 512 != 0) {
            return Err("Expected a raw disk image made up of complete 512-byte sectors. QCOW2, VMDK and VDI disks are converted on import.".into());
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let question = match format {
            Some(format) => format!("Convert {name} ({}, {}) to a raw disk in Dial’s local app data as the {} guest disk? Conversion uses qemu-img and needs room for the full disk size. Projects can boot disposable guests from this base disk. Choose a disk without private data. Nothing is downloaded; the original is not changed.", format.to_ascii_uppercase(), human_size(size), guest.label()),
            None => format!("Copy {name} ({}) to Dial’s local app data as the {} guest disk? Projects can boot disposable guests from this base disk. Choose a disk without private data. Nothing is downloaded; the original is not changed.", human_size(size), guest.label()),
        };
        if !app.dialog().message(question)
            .title("Import guest disk?").buttons(MessageDialogButtons::OkCancel).blocking_show() { return Ok(false); }
        check_project()?;
        let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let dest = guest_disk_path_for(&data, guest).ok_or_else(|| unsupported_message(guest))?;
        CANCEL.store(false, Ordering::SeqCst);
        let mut last = Instant::now();
        let mut send = |copied: u64, total: u64, converting: bool| {
            if copied >= total || last.elapsed() >= Duration::from_millis(250) {
                last = Instant::now();
                let _ = app.emit("app_runner_import", ImportProgress { copied, total, converting });
            }
        };
        let check = || check_project().map_err(io::Error::other);
        let result = match qemu_img {
            Some(tool) => {
                drop(file);
                convert_disk(&tool, &path, &dest, |percent| send((percent * 100.0) as u64, 10_000, true), cancelled, check)
            }
            None => copy_disk(file, size, &dest, |copied| send(copied, size, false), cancelled, check),
        };
        match result {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => Ok(false),
            Err(e) => Err(format!("Could not import guest disk: {e}")),
        }
    }).await.map_err(|e| e.to_string())?
}

/// The disk format `qemu-img` must convert, from the file's extension; `None` for raw disks.
fn convertible_format(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    CONVERTIBLE.into_iter().find(|format| *format == ext)
}

fn no_qemu_img_message(format: &str) -> String {
    format!(
        "This is a {} disk, and Dial needs qemu-img to convert it, which was not found in this build or in the usual install locations. Install QEMU (for example `brew install qemu`, your package manager's qemu-utils, or QEMU for Windows in Program Files), or convert it yourself: `qemu-img convert -p -O raw input.{format} guest.raw`, then import guest.raw.",
        format.to_ascii_uppercase()
    )
}

/// `qemu-img`: the bundled runtime's first, then a machine-wide install at a fixed location.
fn find_qemu_img(app: &AppHandle) -> Option<PathBuf> {
    let exe = runtime::exe_name("qemu-img");
    let mut candidates = Vec::new();
    if let Ok(resources) = app.path().resource_dir() {
        candidates.push(resources.join(runtime::RUNTIME_DIR).join("bin").join(&exe));
    }
    if let Some(system) = runtime::Arch::host().and_then(runtime::system_qemu)
        && let Some(dir) = system.qemu.parent()
    {
        candidates.push(dir.join(&exe));
    }
    candidates.extend(machine_qemu_img());
    candidates.into_iter().find(|path| crate::launcher::is_executable(path))
}

#[cfg(target_os = "macos")]
fn machine_qemu_img() -> Vec<PathBuf> {
    ["/opt/homebrew/bin/qemu-img", "/usr/local/bin/qemu-img", "/opt/local/bin/qemu-img"].map(PathBuf::from).to_vec()
}

#[cfg(windows)]
fn machine_qemu_img() -> Vec<PathBuf> {
    std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .filter(|root| root.is_absolute())
        .map(|root| vec![root.join("qemu").join("qemu-img.exe")])
        .unwrap_or_default()
}

#[cfg(not(any(target_os = "macos", windows)))]
fn machine_qemu_img() -> Vec<PathBuf> {
    ["/usr/bin/qemu-img", "/usr/local/bin/qemu-img"].map(PathBuf::from).to_vec()
}

/// The percentage in a `qemu-img convert -p` progress line, like `    (42.17/100%)`.
fn qemu_progress(line: &str) -> Option<f64> {
    let inner = line.trim().strip_prefix('(')?.strip_suffix("/100%)")?;
    inner.trim().parse::<f64>().ok().filter(|p| (0.0..=100.0).contains(p))
}

/// Convert `source` to a raw disk published at `dest` with `qemu-img`. Like [`copy_disk`], the
/// disk appears only complete and never overwrites one; a cancelled or failed conversion leaves
/// nothing behind.
fn convert_disk(
    tool: &Path,
    source: &Path,
    dest: &Path,
    mut progress: impl FnMut(f64),
    cancelled: impl Fn() -> bool,
    check: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let dir = dest.parent().ok_or_else(|| io::Error::other("missing import directory"))?;
    fs::create_dir_all(dir)?;
    if dest.try_exists()? {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "a guest disk is already installed"));
    }
    // Removed on drop unless persisted, so every early return cleans up.
    let tmp = tempfile::Builder::new().prefix(".import-").suffix(".raw").tempfile_in(dir)?.into_temp_path();
    let mut command = Command::new(tool);
    command.args(["convert", "-p", "-O", "raw"]).arg(source).arg(&*tmp)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = command.spawn().map_err(|e| io::Error::other(format!("could not start qemu-img: {e}")))?;
    let (tx, rx) = std::sync::mpsc::channel::<f64>();
    let stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let Some(mut out) = stdout else { return };
        let mut buf = [0u8; 512];
        let mut line = Vec::new();
        while let Ok(n) = out.read(&mut buf) {
            if n == 0 { break; }
            for &byte in &buf[..n] {
                if byte == b'\r' || byte == b'\n' {
                    if let Some(p) = qemu_progress(&String::from_utf8_lossy(&line)) { let _ = tx.send(p); }
                    line.clear();
                } else if line.len() < 256 {
                    line.push(byte);
                }
            }
        }
    });
    let stderr = child.stderr.take();
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut err) = stderr { let _ = err.by_ref().take(64 << 10).read_to_string(&mut text); }
        text
    });
    let status = loop {
        while let Ok(p) = rx.try_recv() { progress(p); }
        if cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err(cancel_error());
        }
        if let Some(status) = child.try_wait()? { break status; }
        std::thread::sleep(Duration::from_millis(100));
    };
    let _ = reader.join();
    while let Ok(p) = rx.try_recv() { progress(p); }
    let errors = errors.join().unwrap_or_default();
    if !status.success() {
        let reason = errors.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no details");
        return Err(io::Error::other(format!("qemu-img could not convert the disk ({reason})")));
    }
    let size = fs::metadata(&tmp)?.len();
    if size < 512 || size % 512 != 0 {
        return Err(io::Error::other("the converted disk is not made up of complete 512-byte sectors"));
    }
    File::open(&tmp)?.sync_all()?;
    progress(100.0);
    check()?;
    tmp.persist_noclobber(dest).map_err(|e| e.error)?;
    Ok(())
}

/// Delete the imported base disk of `guest` after a native confirmation. `false` when cancelled
/// or when there is none. Running guests keep their own overlay; new runs need a fresh import.
#[tauri::command]
pub(crate) async fn app_runner_remove_disk(
    app: AppHandle,
    workspace_root: String,
    guest_target: GuestTarget,
) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if !crate::app::browser_project(&app, &workspace_root) {
            return Err("The open project changed.".into());
        }
        if matches!(guest_target, GuestTarget::Vm | GuestTarget::Macos) {
            return Err("Only imported QEMU guest disks can be removed here.".into());
        }
        let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let dest = guest_disk_path_for(&data, guest_target).ok_or_else(|| unsupported_message(guest_target))?;
        let Ok(meta) = fs::symlink_metadata(&dest) else { return Ok(false) };
        if !app.dialog().message(format!("Delete the imported {} guest disk ({}) from Dial’s app data? Every project on this computer uses it. You can import a disk again later; the original file you imported is not touched.", guest_target.label(), human_size(meta.len())))
            .title("Remove guest disk?").buttons(MessageDialogButtons::OkCancel).blocking_show() { return Ok(false); }
        fs::remove_file(&dest).map_err(|e| format!("Could not remove the guest disk: {e}"))?;
        Ok(true)
    }).await.map_err(|e| e.to_string())?
}

/// `bytes` for people: `512 B`, `1.5 GB`.
fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

fn copy_disk(
    input: impl Read,
    size: u64,
    dest: &Path,
    mut progress: impl FnMut(u64),
    cancelled: impl Fn() -> bool,
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
    let mut input = input.take(size.saturating_add(1));
    let mut buf = vec![0u8; 4 << 20];
    let mut n = 0u64;
    loop {
        if cancelled() {
            // `tmp` is removed when it drops.
            return Err(cancel_error());
        }
        let read = match input.read(&mut buf) {
            Ok(0) => break,
            Ok(read) => read,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        tmp.write_all(&buf[..read])?;
        n += read as u64;
        progress(n.min(size));
    }
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
