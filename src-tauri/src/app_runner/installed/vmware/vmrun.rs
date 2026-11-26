//! The trusted `vmrun` tool: list running VMs and start one without a GUI. It is the only lifecycle
//! command this console runs. Nothing here stops, suspends or resets a VM.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(180);
const POLL: Duration = Duration::from_millis(50);
const MAX_OUTPUT: u64 = 64 * 1024;

/// The lifecycle operations the console may request.
pub(super) trait Runner {
    /// The `.vmx` paths of running VMs, as vmrun prints them.
    fn running(&mut self) -> Result<Vec<String>, String>;
    /// Start a VM with no GUI. The VM is never stopped by this console.
    fn start_nogui(&mut self, vmx: &str) -> Result<(), String>;
}

/// A vmrun at a recorded absolute path, used with the product's `-T` type.
pub(super) struct Tool {
    path: PathBuf,
    kind: &'static str,
}

/// The outcome of one vmrun call. Its output is parsed, never echoed into an error.
struct Output {
    success: bool,
    code: Option<i32>,
    text: String,
}

impl Tool {
    pub(super) fn new(path: &Path) -> Result<Self, String> {
        if !path.is_absolute() || !path.is_file() {
            return Err("the VMware vmrun tool is not installed where this VM expects it".into());
        }
        Ok(Self {
            path: path.to_path_buf(),
            kind: kind_for(path),
        })
    }

    fn run(&self, args: &[&str]) -> Result<Output, String> {
        let mut child = crate::launcher::without_console(&mut Command::new(&self.path))
            .arg("-T")
            .arg(self.kind)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("could not run vmrun: {error}"))?;
        let stdout = drain(child.stdout.take());
        let stderr = drain(child.stderr.take());
        let deadline = Instant::now() + COMMAND_TIMEOUT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => thread::sleep(POLL),
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("vmrun did not finish in time".into());
                }
                Err(error) => return Err(format!("could not wait for vmrun: {error}")),
            }
        };
        let mut text = stdout.join().unwrap_or_default();
        text.push_str(&stderr.join().unwrap_or_default());
        Ok(Output {
            success: status.success(),
            code: status.code(),
            text,
        })
    }
}

impl Runner for Tool {
    fn running(&mut self) -> Result<Vec<String>, String> {
        let output = self.run(&["list"])?;
        if !output.success {
            return Err(failure("could not list running VMs", output.code));
        }
        Ok(output
            .text
            .lines()
            .map(str::trim)
            .filter(|line| line.to_ascii_lowercase().ends_with(".vmx"))
            .map(str::to_owned)
            .collect())
    }

    fn start_nogui(&mut self, vmx: &str) -> Result<(), String> {
        let output = self.run(&["start", vmx, "nogui"])?;
        if output.success {
            Ok(())
        } else {
            Err(failure("could not start the VM", output.code))
        }
    }
}

/// Workstation and Fusion ship differently named vmrun tools; Fusion's lives in its app bundle.
pub(in crate::app_runner::installed) fn kind_for(path: &Path) -> &'static str {
    if path
        .to_string_lossy()
        .to_ascii_lowercase()
        .contains("fusion")
    {
        "fusion"
    } else {
        "ws"
    }
}

/// A fixed sentence plus the exit code. vmrun's own text can carry paths and other host detail, so
/// it stays out of errors that reach the UI.
fn failure(what: &str, code: Option<i32>) -> String {
    match code {
        Some(code) => format!("vmrun {what} (exit code {code})"),
        None => format!("vmrun {what}"),
    }
}

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(pipe) = pipe {
            let _ = pipe.take(MAX_OUTPUT).read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}
