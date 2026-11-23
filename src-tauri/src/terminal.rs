//! Interactive shells for the Terminal panel: one pseudo-terminal per shell, its output streamed
//! to a sink (the app forwards it as `terminal` events), keystrokes and resizes sent back in.

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

/// What a shell reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TerminalEvent {
    /// Output text; split multi-byte characters are held back until complete.
    Data { id: u32, data: String },
    /// The shell ended.
    Exit { id: u32 },
}

struct Shell {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

/// The open shells, by id. Shared with reader threads so naturally-exited shells are reaped and
/// removed as soon as their output closes.
#[derive(Default)]
pub struct Terminals {
    next: u32,
    shells: Arc<Mutex<HashMap<u32, Shell>>>,
}
/// The user's login shell, run in `cwd`.
pub fn login_shell(cwd: &Path) -> CommandBuilder {
    let program = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if cfg!(windows) {
                "powershell.exe".into()
            } else {
                "/bin/sh".into()
            }
        });
    let mut command = CommandBuilder::new(program);
    if !cfg!(windows) {
        command.arg("-l");
    }
    command.cwd(cwd);
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    command
}

#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Task {
    Slash { command: String },
    Scheduler,
}

impl Task {
    pub fn child(self, program: &Path, cwd: &Path) -> Result<CommandBuilder, String> {
        match self {
            Self::Slash { command } => slash_command(program, cwd, &command),
            Self::Scheduler => {
                let mut child = CommandBuilder::new(program);
                child.args(["schedule", "run"]);
                child.cwd(cwd);
                Ok(child)
            }
        }
    }
}

/// A separate interactive engine with one literal startup argument; never execute shell syntax.
pub fn slash_command(program: &Path, cwd: &Path, text: &str) -> Result<CommandBuilder, String> {
    let name = text
        .split_ascii_whitespace()
        .next()
        .and_then(|s| s.strip_prefix('/'));
    if !text.starts_with('/')
        || text.chars().any(char::is_control)
        || !name.is_some_and(|s| {
            s.starts_with(|c: char| c.is_ascii_alphabetic())
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
        })
    {
        return Err("Expected a single-line slash command".into());
    }
    let mut child = CommandBuilder::new(program);
    child.arg(text);
    child.cwd(cwd);
    child.env("TERM", "xterm-256color");
    child.env("COLORTERM", "truecolor");
    Ok(child)
}

impl Terminals {
    fn lock_shells(&self) -> std::sync::MutexGuard<'_, HashMap<u32, Shell>> {
        self.shells.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn has_shells(&self) -> bool {
        !self.lock_shells().is_empty()
    }

    pub fn open_ids(&self) -> Vec<u32> {
        self.lock_shells().keys().copied().collect()
    }
    /// Start `command` in a terminal of `cols`×`rows`; its output goes to `sink` from a reader
    /// thread until it ends. Returns the shell's id.
    pub fn open(
        &mut self,
        command: CommandBuilder,
        cols: u16,
        rows: u16,
        sink: impl Fn(TerminalEvent) + Send + 'static,
    ) -> Result<u32, String> {
        let pair = native_pty_system()
            .openpty(size(cols, rows))
            .map_err(|e| format!("Could not open a terminal: {e}"))?;
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| format!("Could not start the shell: {e}"))?;
        // The child holds its own handle; ours would keep the terminal open after it exits.
        drop(pair.slave);
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("Could not read the terminal: {e}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("Could not write to the terminal: {e}"))?;
        self.next += 1;
        let id = self.next;
        self.lock_shells().insert(
            id,
            Shell {
                master: pair.master,
                writer,
                child,
            },
        );
        let shells = Arc::clone(&self.shells);
        if let Err(e) = thread::Builder::new()
            .name(format!("dial-terminal-{id}"))
            .spawn(move || {
                let mut buf = [0u8; 8192];
                let mut pending = Vec::new();
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            pending.extend_from_slice(&buf[..n]);
                            let data = take_utf8(&mut pending);
                            if !data.is_empty() {
                                sink(TerminalEvent::Data { id, data });
                            }
                        }
                    }
                }
                if !pending.is_empty() {
                    let data = String::from_utf8_lossy(&pending).into_owned();
                    sink(TerminalEvent::Data { id, data });
                }
                // Reap and remove the finished shell before notifying the UI so FDs and
                // `has_shells()` are already clean when `Exit` arrives.
                let ended = shells
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&id);
                if let Some(mut shell) = ended {
                    let _ = shell.child.wait();
                }
                sink(TerminalEvent::Exit { id });
            })
        {
            self.close(id);
            return Err(format!("Could not start the terminal reader: {e}"));
        }
        Ok(id)
    }

    /// Type `data` into shell `id`.
    pub fn write(&mut self, id: u32, data: &str) -> Result<(), String> {
        let mut shells = self.lock_shells();
        let shell = shells.get_mut(&id).ok_or("That terminal is closed")?;
        shell
            .writer
            .write_all(data.as_bytes())
            .and_then(|()| shell.writer.flush())
            .map_err(|e| format!("Could not write to the terminal: {e}"))
    }

    pub fn resize(&mut self, id: u32, cols: u16, rows: u16) -> Result<(), String> {
        let shells = self.lock_shells();
        let shell = shells.get(&id).ok_or("That terminal is closed")?;
        shell
            .master
            .resize(size(cols, rows))
            .map_err(|e| format!("Could not resize the terminal: {e}"))
    }

    /// End shell `id`; closing one that already ended is fine.
    pub fn close(&mut self, id: u32) {
        let removed = self.lock_shells().remove(&id);
        if let Some(mut shell) = removed {
            let _ = shell.child.kill();
            let _ = shell.child.wait();
        }
    }

    /// End every shell (the workspace closed or the app quits).
    pub fn close_all(&mut self) {
        let removed: Vec<Shell> = {
            let mut shells = self.lock_shells();
            shells.drain().map(|(_, shell)| shell).collect()
        };
        for mut shell in removed {
            let _ = shell.child.kill();
            let _ = shell.child.wait();
        }
    }
}

impl Drop for Terminals {
    fn drop(&mut self) {
        self.close_all();
    }
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.max(2),
        cols: cols.max(10),
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Take the complete UTF-8 text from the front of `pending`, leaving an unfinished multi-byte
/// character there for the next read. Invalid bytes become U+FFFD.
pub fn take_utf8(pending: &mut Vec<u8>) -> String {
    let mut out = String::new();
    let mut rest: &[u8] = pending;
    loop {
        match std::str::from_utf8(rest) {
            Ok(text) => {
                out.push_str(text);
                rest = &[];
                break;
            }
            Err(e) => {
                let (valid, after) = rest.split_at(e.valid_up_to());
                out.push_str(std::str::from_utf8(valid).unwrap_or_default());
                match e.error_len() {
                    // Incomplete character at the end: keep it.
                    None => {
                        rest = after;
                        break;
                    }
                    Some(len) => {
                        out.push('\u{FFFD}');
                        rest = &after[len..];
                    }
                }
            }
        }
    }
    *pending = rest.to_vec();
    out
}

#[cfg(test)]
mod tests;
