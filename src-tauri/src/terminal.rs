//! Interactive shells for the Terminal panel: one pseudo-terminal per shell, its output streamed
//! to a sink (the app forwards it as `terminal` events), keystrokes and resizes sent back in.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::thread;

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;

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

/// The open shells, by id.
#[derive(Default)]
pub struct Terminals {
    next: u32,
    shells: HashMap<u32, Shell>,
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

impl Terminals {
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
        thread::Builder::new()
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
                sink(TerminalEvent::Exit { id });
            })
            .map_err(|e| format!("Could not start the terminal reader: {e}"))?;
        self.shells.insert(
            id,
            Shell {
                master: pair.master,
                writer,
                child,
            },
        );
        Ok(id)
    }

    /// Type `data` into shell `id`.
    pub fn write(&mut self, id: u32, data: &str) -> Result<(), String> {
        let shell = self.shells.get_mut(&id).ok_or("That terminal is closed")?;
        shell
            .writer
            .write_all(data.as_bytes())
            .and_then(|()| shell.writer.flush())
            .map_err(|e| format!("Could not write to the terminal: {e}"))
    }

    pub fn resize(&mut self, id: u32, cols: u16, rows: u16) -> Result<(), String> {
        let shell = self.shells.get(&id).ok_or("That terminal is closed")?;
        shell
            .master
            .resize(size(cols, rows))
            .map_err(|e| format!("Could not resize the terminal: {e}"))
    }

    /// End shell `id`; closing one that already ended is fine.
    pub fn close(&mut self, id: u32) {
        if let Some(mut shell) = self.shells.remove(&id) {
            let _ = shell.child.kill();
            let _ = shell.child.wait();
        }
    }

    /// End every shell (the workspace closed or the app quits).
    pub fn close_all(&mut self) {
        let ids: Vec<u32> = self.shells.keys().copied().collect();
        for id in ids {
            self.close(id);
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
