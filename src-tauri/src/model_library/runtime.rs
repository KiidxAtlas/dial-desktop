//! User-confirmed on-select download/setup for every local inference runtime Dial supports.
//! Delegates recipe-backed runtimes (`llama-cpp`, `mlx-serve`, `vllm-mlx`, `mistral-rs`, `vllm`,
//! `sglang`, `exl3`, `mlx`) to `dial doctor --fix <id>`, Atomic Engine to its verified Metal
//! build, and `ik-llama` to a private build under `~/.dial/tools/ik-llama`.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

/// Metadata for a Desktop-installable local runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub recipe_id: &'static str,
    pub env_var: &'static str,
    pub install_dir: &'static str,
    pub binary: &'static str,
}

const SPECS: &[RuntimeSpec] = &[
    RuntimeSpec {
        id: "mlx",
        label: "MLX",
        recipe_id: "mlx-runtime",
        env_var: "",
        install_dir: "",
        binary: "",
    },
    RuntimeSpec {
        id: "mlx-serve",
        label: "MLX-Serve",
        recipe_id: "mlx-serve",
        env_var: "DIAL_MLX_SERVE",
        install_dir: "mlx-serve",
        binary: "mlx-serve",
    },
    RuntimeSpec {
        id: "vllm-mlx",
        label: "vLLM-MLX",
        recipe_id: "vllm-mlx",
        env_var: "DIAL_VLLM_MLX_SERVER",
        install_dir: "vllm-mlx",
        binary: "vllm-mlx",
    },
    RuntimeSpec {
        id: "llama-cpp",
        label: "llama.cpp",
        recipe_id: "llama-cpp",
        env_var: "DIAL_LLAMA_SERVER",
        install_dir: "llama-cpp",
        binary: "llama-server",
    },
    RuntimeSpec {
        id: "atomic",
        label: "Atomic Engine",
        recipe_id: "atomic-engine",
        env_var: "DIAL_ATOMIC_SERVER",
        install_dir: "atomic-engine",
        binary: "atomic-llama-server",
    },
    RuntimeSpec {
        id: "ik-llama",
        label: "ik_llama.cpp",
        recipe_id: "ik-llama",
        env_var: "DIAL_IK_LLAMA_SERVER",
        install_dir: "ik-llama",
        binary: "ik-llama-server",
    },
    RuntimeSpec {
        id: "mistral-rs",
        label: "mistral.rs",
        recipe_id: "mistral-rs",
        env_var: "DIAL_MISTRALRS_SERVER",
        install_dir: "mistral-rs",
        binary: "mistralrs-server",
    },
    RuntimeSpec {
        id: "vllm",
        label: "vLLM",
        recipe_id: "vllm",
        env_var: "DIAL_VLLM_SERVER",
        install_dir: "vllm",
        binary: "vllm",
    },
    RuntimeSpec {
        id: "sglang",
        label: "SGLang",
        recipe_id: "sglang",
        env_var: "DIAL_SGLANG_SERVER",
        install_dir: "sglang",
        binary: "sglang",
    },
    RuntimeSpec {
        id: "exl3",
        label: "ExLlamaV3",
        recipe_id: "exl3",
        env_var: "DIAL_EXL3_SERVER",
        install_dir: "exl3",
        binary: "exllamav3-server",
    },
];

pub(crate) fn spec(runtime: &str) -> Option<RuntimeSpec> {
    SPECS.iter().copied().find(|s| s.id == runtime)
}

fn dial_home() -> Option<PathBuf> {
    env::var_os("DIAL_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
            env::var_os(key)
                .filter(|s| !s.is_empty())
                .map(|h| PathBuf::from(h).join(".dial"))
        })
        .filter(|p| p.is_absolute())
}

fn is_installed_on_disk(spec: RuntimeSpec) -> bool {
    if spec.id == "mlx" {
        return dial_home().is_some_and(|home| {
            let venv = home.join("python").join(".venv");
            let py = if cfg!(windows) {
                venv.join("Scripts").join("python.exe")
            } else {
                venv.join("bin").join("python")
            };
            py.is_file()
        });
    }
    if !spec.env_var.is_empty()
        && let Some(path) = env::var_os(spec.env_var).filter(|v| !v.is_empty())
    {
        return PathBuf::from(path).is_file();
    }
    let name = format!("{}{}", spec.binary, std::env::consts::EXE_SUFFIX);
    if let Some(home) = dial_home() {
        let root = home.join("tools").join(spec.install_dir);
        for candidate in [
            root.join(&name),
            root.join("bin").join(&name),
            root.join("Scripts").join(&name),
        ] {
            if candidate.is_file() {
                return true;
            }
        }
    }
    if let Some(paths) = env::var_os("PATH") {
        for dir in env::split_paths(&paths) {
            if dir.join(&name).is_file() {
                return true;
            }
        }
    }
    for dir in ["/opt/homebrew/bin", "/usr/local/bin"] {
        if Path::new(dir).join(&name).is_file() {
            return true;
        }
    }
    false
}

/// Returns `true` if the preview describes an automatic install (`Method:` line present) or
/// `false` when it is manual-only on this host.
pub(crate) fn preview_is_automatic(preview: &str) -> bool {
    preview.lines().any(|line| line.trim_start().starts_with("Method:"))
}

async fn setup_ik_llama(app: tauri::AppHandle) -> Result<bool, String> {
    if cfg!(windows) {
        return Err(
            "Automatic ik_llama.cpp builds are supported on macOS and Linux. Build https://github.com/ikawrakow/ik_llama.cpp and set DIAL_IK_LLAMA_SERVER."
                .into(),
        );
    }
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(path) = env::var_os("DIAL_IK_LLAMA_SERVER").filter(|p| !p.is_empty()) {
            return PathBuf::from(path)
                .is_file()
                .then_some(true)
                .ok_or_else(|| "The configured DIAL_IK_LLAMA_SERVER was not found".to_owned());
        }
        let home = dial_home().ok_or("Cannot locate Dial's private tools directory")?;
        let root = home.join("tools/ik-llama");
        let binary = root.join("ik-llama-server");
        if binary.is_file() {
            return Ok(true);
        }
        if !app
            .dialog()
            .message(
                "Clone ik_llama.cpp from GitHub and build ik-llama-server into Dial's private tools folder (~/.dial/tools/ik-llama)? Requires git, CMake, and a C/C++ compiler. This can take a few minutes. No model weights are downloaded.",
            )
            .title("Build ik_llama.cpp?")
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Download & build".into(),
                "Cancel".into(),
            ))
            .blocking_show()
        {
            return Ok(false);
        }
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let lock = fs::File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(root.join("setup.lock"))
            .map_err(|e| e.to_string())?;
        lock.try_lock()
            .map_err(|_| "ik_llama.cpp setup is already running in another window")?;
        if binary.is_file() {
            return Ok(true);
        }
        let work = tempfile::tempdir_in(&root).map_err(|e| e.to_string())?;
        let result = Command::new("/bin/sh")
            .args(["-c", include_str!("build-ik-llama.sh")])
            .env_clear()
            .env("HOME", work.path())
            .env(
                "PATH",
                "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
            )
            .current_dir(work.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| e.to_string())?;
        if !result.status.success() {
            let start = result.stderr.len().saturating_sub(2048);
            return Err(format!(
                "Could not build ik_llama.cpp: {}",
                String::from_utf8_lossy(&result.stderr[start..]).trim()
            ));
        }
        fs::rename(work.path().join("build/bin/llama-server"), binary)
            .map_err(|e| e.to_string())?;
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Ensure `runtime` is installed locally, prompting the user with an explicit confirmation dialog
/// showing the exact download/install plan before changing disk. Returns `Ok(true)` when the
/// runtime is ready to use, `Ok(false)` when the user cancelled.
pub(super) async fn setup(
    app: tauri::AppHandle,
    program: PathBuf,
    cwd: Option<PathBuf>,
    runtime: String,
) -> Result<bool, String> {
    if runtime == "auto" {
        return Ok(true);
    }
    let Some(spec) = spec(&runtime) else {
        return Err(format!("Unknown local inference runtime `{runtime}`"));
    };
    if spec.id == "atomic" {
        return super::atomic::setup(app).await;
    }
    if spec.id == "ik-llama" {
        return setup_ik_llama(app).await;
    }
    if is_installed_on_disk(spec) {
        return Ok(true);
    }
    let preview = {
        let program = program.clone();
        let cwd = cwd.clone();
        let recipe_id = spec.recipe_id;
        tauri::async_runtime::spawn_blocking(move || {
            crate::engine_cli::doctor_fix(&program, cwd.as_deref(), recipe_id, false)
        })
        .await
        .map_err(|e| e.to_string())??
    };
    if preview.contains("is already set up.") {
        return Ok(true);
    }
    if !preview_is_automatic(&preview) {
        return Err(format!(
            "{} cannot be downloaded automatically on this system:\n{}",
            spec.label,
            preview.trim()
        ));
    }
    let label = spec.label;
    let body = format!(
        "{label} is not installed yet. Download and set it up now?\n\n{}\n\nNo model weights are downloaded and your existing runtimes stay unchanged.",
        preview.trim()
    );
    let confirmed = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .message(body)
            .title(format!("Download {label}?"))
            .buttons(MessageDialogButtons::OkCancelCustom(
                format!("Download {label}"),
                "Cancel".into(),
            ))
            .blocking_show()
    })
    .await
    .map_err(|e| e.to_string())?;
    if !confirmed {
        return Ok(false);
    }
    let recipe_id = spec.recipe_id;
    tauri::async_runtime::spawn_blocking(move || {
        crate::engine_cli::doctor_fix(&program, cwd.as_deref(), recipe_id, true)
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_covers_every_supported_local_runtime() {
        for id in [
            "mlx",
            "mlx-serve",
            "vllm-mlx",
            "llama-cpp",
            "atomic",
            "ik-llama",
            "mistral-rs",
            "vllm",
            "sglang",
            "exl3",
        ] {
            let s = spec(id).unwrap_or_else(|| panic!("missing RuntimeSpec for {id}"));
            assert_eq!(s.id, id);
            assert!(!s.label.is_empty());
            assert!(!s.recipe_id.is_empty());
        }
        assert!(spec("unknown-runtime").is_none());
    }

    #[test]
    fn preview_is_automatic_detects_method_lines() {
        assert!(preview_is_automatic(
            "MLX-Serve:\nMethod: direct download\nInstalls into: /x\nConfirm with: /setup mlx-serve yes"
        ));
        assert!(!preview_is_automatic(
            "mistral.rs:\ninstall mistralrs-server via cargo install"
        ));
    }

    #[test]
    fn engine_choice_deserializes_with_and_without_installed_map() {
        let legacy: crate::model::EngineChoice = serde_json::from_value(serde_json::json!({
            "selected": "auto",
            "choices": ["auto", "llama-cpp"],
            "setting": "local_model_engines.abc"
        }))
        .unwrap();
        assert_eq!(legacy.default, None);
        assert!(legacy.installed.is_empty());

        let full: crate::model::EngineChoice = serde_json::from_value(serde_json::json!({
            "selected": "auto",
            "choices": ["auto", "mlx", "mlx-serve", "vllm-mlx", "mistral-rs"],
            "setting": "local_model_engines.def",
            "default": "mlx",
            "installed": { "mlx": true, "mlx-serve": false, "vllm-mlx": false, "mistral-rs": false }
        }))
        .unwrap();
        assert_eq!(full.default.as_deref(), Some("mlx"));
        assert_eq!(full.installed.get("mlx"), Some(&true));
        assert_eq!(full.installed.get("mlx-serve"), Some(&false));
    }
}
