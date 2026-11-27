//! Collaboration relay lifecycle (`dial relay`), owned per app instance via `RelayManager`.

use super::dial;
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

const RELAY_READY: &str = "Dial collaboration relay listening on ";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelayStatus {
    /// The address it listens on, while running.
    pub bind: Option<String>,
}

/// Owns the collaboration relay child process (`dial relay`) for one app instance,
/// stopping it cleanly when dropped or explicitly stopped.
#[derive(Default)]
pub struct RelayManager {
    slot: std::sync::Mutex<Option<(std::process::Child, String)>>,
}

impl RelayManager {
    fn slot(&self) -> std::sync::MutexGuard<'_, Option<(std::process::Child, String)>> {
        let mut slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
        // Forget a relay that exited on its own.
        if slot.as_mut().is_some_and(|(child, _)| !matches!(child.try_wait(), Ok(None))) {
            *slot = None;
        }
        slot
    }

    pub fn status(&self) -> RelayStatus {
        RelayStatus { bind: self.slot().as_ref().map(|(_, bind)| bind.clone()) }
    }

    /// `dial relay --bind <addr>`, waiting until it reports it is listening.
    pub fn start(&self, program: &Path, cwd: Option<&Path>, bind: &str) -> Result<RelayStatus, String> {
        use std::io::{BufRead, BufReader};
        let bind = bind.trim();
        bind.parse::<std::net::SocketAddr>()
            .map_err(|_| format!("`{bind}` is not an address like 127.0.0.1:7331"))?;
        let mut slot = self.slot();
        if let Some((_, running)) = slot.as_ref() {
            return Ok(RelayStatus { bind: Some(running.clone()) });
        }
        let mut command = dial(program, cwd);
        command
            .args(["relay", "--bind", bind])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        let mut child = crate::launcher::without_console(&mut command)
            .spawn()
            .map_err(|e| format!("Could not start the relay: {e}"))?;
        let stderr = child.stderr.take().ok_or("The relay has no output to watch")?;
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut said = String::new();
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if line.starts_with(RELAY_READY) {
                    let _ = tx.send(Ok(()));
                    // Keep draining so the relay never blocks on a full pipe.
                    continue;
                }
                if said.len() < 4096 {
                    said.push_str(&line);
                    said.push('\n');
                }
            }
            let _ = tx.send(Err(said));
        });
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => {
                *slot = Some((child, bind.to_owned()));
                Ok(RelayStatus { bind: Some(bind.to_owned()) })
            }
            outcome => {
                let _ = child.kill();
                let _ = child.wait();
                Err(match outcome {
                    Ok(Err(said)) if !said.trim().is_empty() => format!("The relay stopped: {}", said.trim()),
                    Ok(Err(_)) => "The relay stopped before listening".into(),
                    _ => "The relay did not report that it is listening".into(),
                })
            }
        }
    }

    /// Stop the relay Desktop started; also called when the app exits.
    pub fn stop(&self) -> RelayStatus {
        if let Some((mut child, _)) = self.slot().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        RelayStatus { bind: None }
    }
}

impl Drop for RelayManager {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
