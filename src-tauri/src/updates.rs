//! Keeping Dial current from the desktop app: the engine (`dial`) and the app itself.
//!
//! The engine owns its own updates. The app asks it with `dial update --check --json`, installs
//! with `dial update` (which follows however Dial was installed: the signed release installer,
//! Homebrew, Cargo or a source checkout). For missing or externally managed engines the user
//! can choose a separate signed release installed into Desktop's own directory.
//!
//! The app updates through Tauri's updater plugin against this repository's signed releases. The
//! verifying public key lives in `tauri.release.conf.json`, so development and local builds,
//! which lack it, never check.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests;

/// Whether the updater plugin's config (`plugins.updater`) carries a public key, which only
/// release builds (`--config tauri.release.conf.json`) have.
pub fn updater_configured(updater: Option<&serde_json::Value>) -> bool {
    updater
        .and_then(|config| config.get("pubkey"))
        .and_then(serde_json::Value::as_str)
        .is_some_and(|key| !key.trim().is_empty())
}

/// Unix installer: Dial's short address, which redirects to the latest release's signed
/// `install.sh`.
const INSTALL_UNIX: &str = "curl -fsSL https://dial.rootatlas.dev/install | sh";
/// Windows installer: the same address serves `install.ps1` to PowerShell.
const INSTALL_WINDOWS: &str = "irm https://dial.rootatlas.dev/install | iex";

/// GitHub may be slow; the engine waits up to 15 s itself.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// A Cargo or source-checkout update rebuilds Dial, which takes minutes.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Output kept for the UI from a long install or update.
const OUTPUT_TAIL: usize = 4000;

/// `dial update --check --json`, as the engine reports it. Unknown fields are ignored and missing
/// ones default, so newer and older engines both parse.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct EngineStatus {
    pub current: String,
    pub latest: Option<String>,
    pub update_available: bool,
    pub installation: String,
    pub method: String,
    pub can_update: bool,
    pub update_command: String,
    pub error: Option<String>,
}

/// What the app knows about the engine's updates, as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum EngineUpdateView {
    /// The engine answered.
    Checked { status: EngineStatus },
    /// The engine predates `--check`; `dial update` still works, so offer it.
    Unsupported { detail: String },
    /// No engine could be run.
    Missing { error: String },
    /// The check ran but failed.
    Failed { error: String },
}

/// How the engine failed to connect, so the UI can offer the right fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineProblem {
    /// No `dial` executable was found or it could not be started: offer to install it.
    Missing,
    /// The engine speaks no protocol version this app does: offer to update one of them.
    Incompatible,
    /// Anything else: the error text says what.
    Other,
}

/// Classify a connect error from `dial_client` (`ClientError`'s display text).
pub fn classify_failure(error: &str) -> EngineProblem {
    let lower = error.to_ascii_lowercase();
    let not_found = lower.contains("not found")
        || lower.contains("no such file")
        || lower.contains("os error 2")
        || lower.contains("cannot find the file");
    if lower.starts_with("could not start") && not_found {
        EngineProblem::Missing
    } else if lower.contains("engine serves protocol") {
        EngineProblem::Incompatible
    } else {
        EngineProblem::Other
    }
}

/// Fold the outcome of `dial update --check --json` into what the UI shows.
pub fn parse_check(outcome: Result<RunOutput, String>) -> EngineUpdateView {
    let output = match outcome {
        Ok(output) => output,
        Err(error) if classify_failure(&error) == EngineProblem::Missing => {
            return EngineUpdateView::Missing { error };
        }
        Err(error) => return EngineUpdateView::Failed { error },
    };
    if output.success {
        return match serde_json::from_str::<EngineStatus>(output.stdout.trim()) {
            Ok(status) => EngineUpdateView::Checked { status },
            Err(error) => EngineUpdateView::Failed {
                error: format!("unreadable answer from `dial update --check`: {error}"),
            },
        };
    }
    // clap refuses the unknown flag on engines from before `--check`.
    let stderr = output.stderr.trim();
    if stderr.contains("--check") || stderr.contains("unexpected argument") {
        EngineUpdateView::Unsupported {
            detail: "This Dial engine predates update checks; updating it adds them.".into(),
        }
    } else {
        EngineUpdateView::Failed {
            error: last_lines(stderr, OUTPUT_TAIL),
        }
    }
}

/// Ask the engine at `program` whether a newer release exists.
pub fn check_engine(program: &Path) -> EngineUpdateView {
    let mut command = Command::new(program);
    command.args(["update", "--check", "--json"]);
    parse_check(run_bounded(&mut command, CHECK_TIMEOUT))
}

/// Run `dial update` with the engine at `program`; returns the tail of its output.
pub fn update_engine(program: &Path) -> Result<String, String> {
    let mut command = Command::new(program);
    command.arg("update");
    finished(run_bounded(&mut command, UPDATE_TIMEOUT)?, "dial update")
}

/// The shell command that installs the engine on this platform.
pub fn install_command(windows: bool) -> (&'static str, Vec<&'static str>) {
    if windows {
        (
            "powershell",
            vec![
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                INSTALL_WINDOWS,
            ],
        )
    } else {
        ("sh", vec!["-c", INSTALL_UNIX])
    }
}

/// Reserve a fresh destination: never let the installer overwrite an existing binary or backup.
pub fn install_engine(app_dir: &Path) -> Result<(PathBuf, String), String> {
    install_separate(app_dir, |directory| {
        let (program, args) = install_command(cfg!(windows));
        let mut command = Command::new(program);
        command
            .args(args)
            .env("DIAL_INSTALL_DIR", directory)
            .env("DIAL_NO_MODIFY_PATH", "1")
            .env_remove("DIAL_ALLOW_UNSIGNED_INSTALL");
        finished(
            run_bounded(&mut command, INSTALL_TIMEOUT)?,
            "the Dial installer",
        )
    })
}

fn install_separate(
    app_dir: &Path,
    install: impl FnOnce(&Path) -> Result<String, String>,
) -> Result<(PathBuf, String), String> {
    use crate::launcher::{DIAL_NAME, ENGINE_SELECTION};
    use std::io::Write;
    std::fs::create_dir_all(app_dir).map_err(|e| e.to_string())?;
    let directory = tempfile::Builder::new()
        .prefix("engine-")
        .tempdir_in(app_dir)
        .map_err(|e| e.to_string())?;
    let log = install(directory.path())?;
    let binary = directory.path().join(DIAL_NAME);
    if !binary.is_file() {
        return Err(format!(
            "The installer did not create {}.\n{log}",
            binary.display()
        ));
    }
    let mut selection = tempfile::NamedTempFile::new_in(app_dir).map_err(|e| e.to_string())?;
    let name = directory
        .path()
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("Invalid engine directory")?;
    selection
        .write_all(name.as_bytes())
        .map_err(|e| e.to_string())?;
    selection
        .persist(app_dir.join(ENGINE_SELECTION))
        .map_err(|e| e.to_string())?;
    let _ = directory.keep();
    Ok((binary, log))
}

fn finished(output: RunOutput, what: &str) -> Result<String, String> {
    let text = last_lines(
        &format!("{}\n{}", output.stdout.trim(), output.stderr.trim()),
        OUTPUT_TAIL,
    );
    if output.success {
        Ok(text)
    } else {
        Err(if text.is_empty() {
            format!("{what} failed")
        } else {
            format!("{what} failed:\n{text}")
        })
    }
}

/// The end of `text`, at most `limit` bytes, cut at a line start.
pub fn last_lines(text: &str, limit: usize) -> String {
    let text = text.trim();
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut start = text.len() - limit;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let tail = &text[start..];
    tail.split_once('\n')
        .map_or(tail, |(_, rest)| rest)
        .to_owned()
}

/// What a finished command printed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Run `command` without stdin, capturing both streams; killed after `timeout`. `Err` when it
/// cannot be started or runs out of time.
pub fn run_bounded(command: &mut Command, timeout: Duration) -> Result<RunOutput, String> {
    let shown = format!("{:?}", command.get_program());
    let mut child = crate::launcher::without_console(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("could not start {shown}: {error}"))?;
    let read = |stream: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = Vec::new();
            if let Some(mut stream) = stream {
                let _ = stream.read_to_end(&mut text);
            }
            String::from_utf8_lossy(&text).into_owned()
        })
    };
    let stdout = read(
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let stderr = read(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{shown} did not finish within {timeout:?}"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(format!("waiting for {shown}: {error}")),
        }
    };
    Ok(RunOutput {
        success: status.success(),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}
