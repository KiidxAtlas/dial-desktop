//! Optional, user-confirmed installation of the official HF client in Desktop's own data folder.
//! Bootstrap with a checksum-pinned uv executable; never evaluate a remote installer or require
//! system Python, administrator privileges, shell startup edits or a restart.
use reqwest::blocking::Client;
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{self, Cursor, Read},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

type Error = Box<dyn std::error::Error + Send + Sync>;
const PACKAGE: &str = "huggingface_hub==2.4.0";
const MAX_WHEEL: u64 = 32 << 20;

// URLs and SHA256 from https://pypi.org/pypi/uv/0.11.8/json. Extract just the one named
// executable, not arbitrary archive paths. The bootstrap itself is removed after setup.
fn wheel() -> Result<(String, &'static str), Error> {
    let (path, digest) = match (env::consts::OS, env::consts::ARCH) {
        ("macos", "aarch64") => (
            "d5/62/82953018801a250e16b091ef4b5e95e939b2f01224363d6fc80f600b7eff/uv-0.11.8-py3-none-macosx_11_0_arm64.whl",
            "f0d402e182ab581e934c159cc9edf25ec6e08d32f29aa797980e949afefc87cd",
        ),
        ("macos", "x86_64") => (
            "86/05/557aa070fda7b8460bbbe1e867e8e5b80602c5b30ed77d1d94fc5acae518/uv-0.11.8-py3-none-macosx_10_12_x86_64.whl",
            "d414fc3795b6f56fb6b1fa359537930924fdfe857750a144d2aedf3077be3f1d",
        ),
        ("windows", "x86_64") => (
            "f9/9b/a91a9c60dcae0e1e3da06377d38f32118a523697d461fe41bc9f117ecf59/uv-0.11.8-py3-none-win_amd64.whl",
            "306c624c68d95dd7ea3647675323d72c1abc25f91c3e92ae4cd6f0f11b508726",
        ),
        ("linux", "x86_64") => (
            "5b/01/df175979018743cc5ba6e2fb9dcec916868271e8d88cf0b9df8fd805a0df/uv-0.11.8-py3-none-manylinux_2_17_x86_64.manylinux2014_x86_64.whl",
            "d97bb2920d6cddc07faa475013461294cc09b77ec8139278416c6e54b938d037",
        ),
        ("linux", "aarch64") => (
            "af/4c/477f2abe16f9a3d3c73077f15615878a303eef3760115ec946be58ecb9b2/uv-0.11.8-py3-none-manylinux_2_17_aarch64.manylinux2014_aarch64.musllinux_1_1_aarch64.whl",
            "877c9af3b3955a35ef739e5b2ba79c56dae5c4d50420a7ed908c0901e1c8c807",
        ),
        _ => return Err("Automatic model helper setup is unavailable on this platform".into()),
    };
    Ok((
        format!("https://files.pythonhosted.org/packages/{path}"),
        digest,
    ))
}

fn root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map(|base| base.join("model-helper-2.4.0"))
        .map_err(|e| e.to_string())
}
fn managed(root: &Path) -> Option<PathBuf> {
    let cli = root
        .join("bin")
        .join(format!("hf{}", env::consts::EXE_SUFFIX));
    (root.join("ready").is_file() && cli.is_file()).then_some(cli)
}
pub(super) fn hf_program(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(managed(&root(app)?).unwrap_or_else(|| "hf".into()))
}

fn extract_uv(bytes: Vec<u8>, digest: &str, work: &Path) -> Result<PathBuf, Error> {
    if format!("{:x}", Sha256::digest(&bytes)) != digest {
        return Err("Model helper bootstrap checksum mismatch; nothing was installed".into());
    }
    let exe = format!("uv{}", env::consts::EXE_SUFFIX);
    let dest = work.join(&exe);
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
    let mut entry = zip.by_name(&format!("uv-0.11.8.data/scripts/{exe}"))?;
    let mut file = fs::File::create_new(&dest)?;
    io::copy(&mut entry, &mut file)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o700))?;
    }
    Ok(dest)
}

fn install(root: &Path) -> Result<(), Error> {
    let (url, digest) = wheel()?;
    fs::create_dir_all(root)?;
    let lock = fs::File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("setup.lock"))?;
    lock.try_lock()
        .map_err(|_| "Model helper setup is already running in another window")?;
    if managed(root).is_some() {
        return Ok(());
    }
    let work = tempfile::tempdir_in(root)?;
    let mut bytes = Vec::new();
    // Reuse the updater's ring provider, including when setup happens before an update check.
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(180))
        .build()?
        .get(url)
        .send()?
        .error_for_status()?
        .take(MAX_WHEEL + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_WHEEL {
        return Err("Model helper bootstrap exceeds the download limit".into());
    }
    let uv = extract_uv(bytes, digest, work.path())?;
    // No shell, VIRTUAL_ENV, alternative package indexes, Python startup hooks or project config.
    let mut cmd = Command::new(uv);
    crate::launcher::without_console(&mut cmd);
    cmd.env_clear().current_dir(work.path());
    for key in [
        "HOME",
        "USERPROFILE",
        "SystemRoot",
        "LOCALAPPDATA",
        "APPDATA",
        "TMPDIR",
        "TMP",
        "TEMP",
        "HTTPS_PROXY",
        "HTTP_PROXY",
        "NO_PROXY",
    ] {
        if let Some(value) = env::var_os(key) {
            cmd.env(key, value);
        }
    }
    cmd.env("UV_TOOL_DIR", root.join("tools"))
        .env("UV_TOOL_BIN_DIR", root.join("bin"))
        .env("UV_PYTHON_INSTALL_DIR", root.join("python"))
        .env("UV_PYTHON_BIN_DIR", root.join("python-bin"))
        .env("UV_CACHE_DIR", work.path().join("cache"))
        .args([
            "tool",
            "install",
            "--no-config",
            "--managed-python",
            "--python",
            "3.12",
            "--no-build",
            "--default-index",
            "https://pypi.org/simple",
            PACKAGE,
        ]);
    let (status, _, stderr) = super::run_bounded(cmd, Duration::from_secs(600))?;
    if !status.success() {
        let text = String::from_utf8_lossy(&stderr);
        let reason = text
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("no details");
        return Err(format!("Could not install the model helper: {reason}").into());
    }
    if !root
        .join("bin")
        .join(format!("hf{}", env::consts::EXE_SUFFIX))
        .is_file()
    {
        return Err("Model helper setup ended without installing hf".into());
    }
    fs::File::create(root.join("ready"))?;
    Ok(())
}

pub(super) async fn setup(app: AppHandle) -> Result<bool, String> {
    let dir = root(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        if managed(&dir).is_some() { return Ok(true); }
        let consent = app.dialog().message("Download a private Python runtime from Astral and the Hugging Face helper and its dependencies from PyPI? They are stored in Dial Desktop’s app data without changing system software. If you just confirmed a model download, it will begin after setup.")
            .title("Set up model downloads?").buttons(MessageDialogButtons::OkCancelCustom("Download helper".into(), "Cancel".into())).blocking_show();
        if !consent { return Ok(false); }
        install(&dir).map_err(|e| e.to_string())?;
        Ok(true)
    }).await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unverified_bootstrap_is_never_materialized() {
        let work = tempfile::tempdir().unwrap();
        let (_, hash) = wheel().unwrap();
        assert!(
            extract_uv(vec![1, 2, 3], hash, work.path())
                .unwrap_err()
                .to_string()
                .contains("checksum")
        );
        assert_eq!(work.path().read_dir().unwrap().count(), 0);
    }

    /// Explicit network smoke check: all installed programs live in a disposable folder.
    #[test]
    #[ignore = "downloads a private runtime and HF client from the network"]
    fn live_private_model_helper_setup() {
        let root = tempfile::tempdir().unwrap();
        install(root.path()).unwrap();
        let mut cmd = Command::new(managed(root.path()).unwrap());
        cmd.args(["models", "list", "--help"])
            .env("HF_HOME", root.path().join("hf-cache"));
        let (exit, _, err) = super::super::run_bounded(cmd, Duration::from_secs(30)).unwrap();
        assert!(exit.success(), "{}", String::from_utf8_lossy(&err));
    }

    #[test]
    fn partial_setup_is_not_used_for_search_or_download() {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let hf = bin.join(format!("hf{}", env::consts::EXE_SUFFIX));
        fs::write(&hf, []).unwrap();
        assert!(managed(root.path()).is_none());
        fs::write(root.path().join("ready"), []).unwrap();
        assert_eq!(managed(root.path()), Some(hf.clone()));
        fs::remove_file(hf).unwrap();
        assert!(managed(root.path()).is_none());
    }
}
