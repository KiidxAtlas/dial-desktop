//! User-confirmed, per-user setup of the Metal fork, separate from stock llama.cpp.
use std::{env, fs, path::PathBuf, process::{Command, Stdio}};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

pub(super) async fn setup(app: tauri::AppHandle) -> Result<bool, String> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return Err("Atomic setup requires an Apple Silicon Mac".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(path) = env::var_os("DIAL_ATOMIC_SERVER").filter(|p| !p.is_empty()) {
            return PathBuf::from(path).is_file().then_some(true).ok_or_else(|| "The configured DIAL_ATOMIC_SERVER was not found".to_owned());
        }
        let home = env::var_os("DIAL_HOME").filter(|s| !s.is_empty()).map(PathBuf::from)
            .or_else(|| env::var_os("HOME").filter(|s| !s.is_empty()).map(|h| PathBuf::from(h).join(".dial")))
            .filter(|p| p.is_absolute()).ok_or("Cannot locate Dial's private tools directory")?;
        let root = home.join("tools/atomic-engine");
        let binary = root.join("atomic-llama-server");
        if binary.is_file() { return Ok(true); }
        if !app.dialog().message("Download checksum-verified Atomic Engine source from GitHub and build its Metal server into Dial’s private tools folder? Requires CMake and the macOS developer tools. This can take several minutes. No weights are downloaded and MLX is unchanged.")
            .title("Set up Atomic Engine?").buttons(MessageDialogButtons::OkCancelCustom("Build engine".into(), "Cancel".into())).blocking_show() { return Ok(false); }
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let lock = fs::File::options().create(true).truncate(false).write(true).open(root.join("setup.lock")).map_err(|e| e.to_string())?;
        lock.try_lock().map_err(|_| "Atomic setup is running in another window")?;
        if binary.is_file() { return Ok(true); }
        let work = tempfile::tempdir_in(&root).map_err(|e| e.to_string())?;
        let result = Command::new("/bin/sh").args(["-c", include_str!("build-atomic.sh")])
            .env_clear().env("HOME", work.path()).env("PATH", "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin")
            .current_dir(work.path()).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).output().map_err(|e| e.to_string())?;
        if !result.status.success() {
            let start = result.stderr.len().saturating_sub(2048);
            return Err(format!("Could not build Atomic Engine: {}", String::from_utf8_lossy(&result.stderr[start..]).trim()));
        }
        fs::rename(work.path().join("build/bin/llama-server"), binary).map_err(|e| e.to_string())?;
        Ok(true)
    }).await.map_err(|e| e.to_string())?
}
