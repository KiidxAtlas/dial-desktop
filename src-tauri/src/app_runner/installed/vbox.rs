//! VirtualBox consoles for registered VMs, through VirtualBox's own Python SDK.
//!
//! A console is one trusted Python helper (`vbox/console.py`, embedded at build time) that Dial
//! starts with `python3 -I -c`, using only the SDK that sits beside the `VBoxManage` the inventory
//! already trusted. The helper speaks JSON lines; a frame reply's line announces `pixels`, the
//! number of raw BGRA bytes that follow it on the stream (no PNG, no base64). Input is absolute
//! pointer and PS/2 scancode events. There is no host input, no arbitrary
//! command, and no PATH lookup.
//!
//! Lifecycle: a powered-off VM is started headless; a running VM is attached with a shared session.
//! Dropping a console releases only that session. The VM is never powered off, saved, discarded or
//! reconfigured, and it keeps running after Dial disconnects.
//!
//! Every exchange has a deadline; a helper that stops answering is killed (the VM process is a
//! separate process and is untouched). Errors that reach callers are fixed Dial messages; native
//! exception text, paths and machine names are never copied out of the helper.

use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::inventory::{Machine, Provider};
use crate::app_runner::input::{Frame, Gesture, Low, Modifier, qcode_for};

#[cfg(test)]
mod tests;

/// The helper, embedded so that no file has to be installed next to the app.
const HELPER_SOURCE: &str = include_str!("vbox/console.py");
/// Opening may wait for a cold VM start (the helper bounds its own launch at 120 s).
const OPEN_TIMEOUT: Duration = Duration::from_secs(200);
const FRAME_TIMEOUT: Duration = Duration::from_secs(15);
const INPUT_TIMEOUT: Duration = Duration::from_secs(10);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(3);
/// Largest JSON line from the helper. Pixels never travel in a line.
const MAX_REPLY_BYTES: u64 = 1 << 20;
/// Largest raw frame after a line: the biggest screen at 4 bytes a pixel.
const MAX_PIXEL_BYTES: u64 = MAX_FRAME_WIDTH as u64 * MAX_FRAME_HEIGHT as u64 * 4;
/// Largest guest screen accepted from the helper (the helper checks the same bounds first).
const MAX_FRAME_WIDTH: u32 = 3840;
const MAX_FRAME_HEIGHT: u32 = 2160;

const STOPPED: &str = "The VirtualBox console stopped. Reopen the VM in App → VM.";
const STALE: &str = "The VirtualBox VM changed or stopped. Reopen it in App → VM.";
const CLOSED: &str = "The VirtualBox console is closed.";
const REFUSED: &str = "VirtualBox would not serve that request for this VM.";
const FAILED: &str = "VirtualBox reported an error.";

/// One open connection to one registered VirtualBox VM.
pub(in crate::app_runner) struct Console {
    helper: Option<Helper>,
}

struct Helper {
    child: Child,
    stdin: ChildStdin,
    /// Replies from the helper's stdout; `None` marks a reply over the size bound or the end.
    lines: Receiver<Option<Line>>,
}

/// One reply: its JSON line and, when the line announces `pixels`, the raw bytes after it.
#[derive(Debug, PartialEq, Eq)]
struct Line {
    text: String,
    payload: Vec<u8>,
}

impl Console {
    /// Start or attach the VM by its exact UUID. The helper decides which, from the VM's state.
    pub(super) fn open(vm: &Machine) -> Result<Self, String> {
        if vm.provider != Provider::VirtualBox {
            return Err("The selected VM is not a VirtualBox machine.".to_owned());
        }
        let uuid = valid_uuid(&vm.locator)?;
        let python = trusted_python()?;
        let layout = SdkLayout::for_tool(&vm.tool)?;
        let path = helper_path(&layout.bin)?;
        let mut command = Command::new(python);
        command
            .arg("-I")
            .arg("-u")
            .arg("-c")
            .arg(HELPER_SOURCE)
            .arg(uuid)
            .current_dir(&layout.bin)
            .env_clear()
            .env("VBOX_PROGRAM_PATH", &layout.bin)
            .env("VBOX_SDK_PATH", &layout.sdk)
            .env("VBOX_APP_HOME", &layout.bin)
            .env("PATH", path);
        for key in PASSTHROUGH_ENV {
            if let Some(value) = env::var_os(key) {
                command.env(key, value);
            }
        }
        if let Some(home) = env::var_os("VBOX_USER_HOME").filter(|v| Path::new(v).is_absolute()) {
            command.env("VBOX_USER_HOME", home);
        }
        let mut console = Console::start(command)?;
        console.read_reply(OPEN_TIMEOUT)?;
        Ok(console)
    }

    pub(super) fn frame(&mut self) -> Result<Frame, String> {
        let (reply, pixels) = self.exchange_raw(json!({ "op": "frame" }), FRAME_TIMEOUT)?;
        frame_from_reply(&reply, &pixels)
    }

    pub(super) fn input(
        &mut self,
        gesture: &Gesture,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        let steps = gesture_steps(gesture, width, height)?;
        self.exchange(
            json!({ "op": "send", "width": width, "height": height, "steps": steps }),
            INPUT_TIMEOUT,
        )?;
        Ok(())
    }

    /// Stream steps (moves, held buttons and keys, wheel). The helper keeps them held until a later
    /// step lets go, and lets go of everything when it closes.
    pub(super) fn stream(&mut self, steps: &[Low], width: u32, height: u32) -> Result<(), String> {
        let steps = stream_steps(steps)?;
        if steps.is_empty() {
            return Ok(());
        }
        self.exchange(
            json!({ "op": "stream", "width": width, "height": height, "steps": steps }),
            INPUT_TIMEOUT,
        )?;
        Ok(())
    }

    fn start(mut command: Command) -> Result<Self, String> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(|_| FAILED.to_owned())?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(FAILED.to_owned());
        };
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || read_lines(stdout, &sender));
        Ok(Console {
            helper: Some(Helper {
                child,
                stdin,
                lines,
            }),
        })
    }

    /// Send one request and read its reply within `timeout`. Any transport failure, timeout or
    /// stale reply ends the console: the helper is killed and later calls report it closed.
    fn exchange(&mut self, body: Value, timeout: Duration) -> Result<Value, String> {
        self.exchange_raw(body, timeout).map(|(reply, _)| reply)
    }

    /// [`Self::exchange`], with the raw bytes that followed the reply line.
    fn exchange_raw(&mut self, body: Value, timeout: Duration) -> Result<(Value, Vec<u8>), String> {
        let Some(helper) = self.helper.as_mut() else {
            return Err(CLOSED.to_owned());
        };
        let mut line = body.to_string();
        line.push('\n');
        let sent = helper
            .stdin
            .write_all(line.as_bytes())
            .and_then(|()| helper.stdin.flush());
        if sent.is_err() {
            self.shutdown();
            return Err(STOPPED.to_owned());
        }
        self.read_reply_raw(timeout)
    }

    fn read_reply(&mut self, timeout: Duration) -> Result<Value, String> {
        self.read_reply_raw(timeout).map(|(reply, _)| reply)
    }

    fn read_reply_raw(&mut self, timeout: Duration) -> Result<(Value, Vec<u8>), String> {
        let Some(helper) = self.helper.as_ref() else {
            return Err(CLOSED.to_owned());
        };
        let Line { text: line, payload } = match helper.lines.recv_timeout(timeout) {
            Ok(Some(line)) => line,
            Ok(None) | Err(RecvTimeoutError::Disconnected) => {
                self.shutdown();
                return Err(STOPPED.to_owned());
            }
            Err(RecvTimeoutError::Timeout) => {
                self.shutdown();
                return Err(
                    "VirtualBox did not answer in time. Reopen the VM in App → VM.".to_owned(),
                );
            }
        };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            self.shutdown();
            return Err(FAILED.to_owned());
        };
        if value.get("ok") == Some(&Value::Bool(true)) {
            return Ok((value, payload));
        }
        // Only the code is trusted; the helper's text is never shown.
        match value.get("code").and_then(Value::as_str) {
            Some("stale") => {
                self.shutdown();
                Err(STALE.to_owned())
            }
            Some("refused") => Err(REFUSED.to_owned()),
            _ => Err(FAILED.to_owned()),
        }
    }

    /// Kill the helper. The VM is a separate process and keeps running.
    fn shutdown(&mut self) {
        if let Some(mut helper) = self.helper.take() {
            let _ = helper.child.kill();
            let _ = helper.child.wait();
        }
    }
}

impl Drop for Console {
    /// Ask the helper to release its session, then end it if it does not exit in time.
    fn drop(&mut self) {
        let Some(helper) = self.helper.take() else {
            return;
        };
        let Helper {
            mut child,
            mut stdin,
            ..
        } = helper;
        let _ = stdin.write_all(b"{\"op\":\"close\"}\n");
        let _ = stdin.flush();
        drop(stdin);
        let deadline = Instant::now() + CLOSE_TIMEOUT;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
            }
        }
    }
}

/// Forward replies until the stream ends: each JSON line bounded in size, and the raw bytes it
/// announces (`pixels`) read exactly, within their own bound.
fn read_lines(stdout: impl Read, sender: &mpsc::Sender<Option<Line>>) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut buf = Vec::new();
        let read = (&mut reader)
            .take(MAX_REPLY_BYTES)
            .read_until(b'\n', &mut buf);
        let line = match read {
            Ok(0) => break,
            Ok(_) if buf.last() == Some(&b'\n') => {
                buf.pop();
                // A helper that wrote text-mode lines ends them with CRLF.
                if buf.last() == Some(&b'\r') {
                    buf.pop();
                }
                String::from_utf8(buf).ok().and_then(|text| {
                    let payload = match announced_pixels(&text) {
                        Some(len) if len <= MAX_PIXEL_BYTES => {
                            let mut payload = vec![0; len as usize];
                            reader.read_exact(&mut payload).ok()?;
                            payload
                        }
                        Some(_) => return None,
                        None => Vec::new(),
                    };
                    Some(Line { text, payload })
                })
            }
            // Over the bound or cut off.
            _ => None,
        };
        let end = line.is_none();
        if sender.send(line).is_err() || end {
            break;
        }
    }
}

/// The raw byte count a reply line announces, if any.
fn announced_pixels(text: &str) -> Option<u64> {
    serde_json::from_str::<Value>(text).ok()?.get("pixels")?.as_u64()
}

fn valid_uuid(text: &str) -> Result<&str, String> {
    let bytes = text.as_bytes();
    let well_formed = bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                *b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        });
    if well_formed {
        Ok(text)
    } else {
        Err("The VM does not have a valid VirtualBox id.".to_owned())
    }
}

/// Environment names the helper inherits from Dial, beyond the fixed PATH and VirtualBox paths.
#[cfg(windows)]
const PASSTHROUGH_ENV: &[&str] = &[
    "SystemRoot",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "LOCALAPPDATA",
    "APPDATA",
    "USERNAME",
];
#[cfg(not(windows))]
const PASSTHROUGH_ENV: &[&str] = &["HOME", "TMPDIR", "USER", "LANG"];

/// The helper's PATH. Windows uses only the VirtualBox install and the system directories.
#[cfg(windows)]
fn helper_path(bin: &Path) -> Result<String, String> {
    let root = env::var_os("SystemRoot")
        .map(PathBuf::from)
        .filter(|root| root.is_absolute())
        .ok_or_else(|| FAILED.to_owned())?;
    Ok(windows_path(bin, &root))
}

#[cfg(not(windows))]
fn helper_path(_bin: &Path) -> Result<String, String> {
    Ok("/usr/bin:/bin".to_owned())
}

/// `<bin>;<SystemRoot>\System32;<SystemRoot>`. Plain text, so it can be checked on any host.
#[cfg(any(windows, test))]
fn windows_path(bin: &Path, system_root: &Path) -> String {
    format!(
        "{};{}\\System32;{}",
        bin.display(),
        system_root.display(),
        system_root.display()
    )
}

/// Windows: a machine-wide Python 3 that carries pywin32, found only under `%ProgramFiles%`.
#[cfg(windows)]
fn trusted_python() -> Result<PathBuf, String> {
    env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .filter(|root| root.is_absolute())
        .and_then(|root| machine_python(&root))
        .ok_or_else(|| {
            "VirtualBox consoles need a machine-wide Python 3 with pywin32 in Program Files."
                .to_owned()
        })
}

#[cfg(not(windows))]
fn trusted_python() -> Result<PathBuf, String> {
    const PYTHON: &str = "/usr/bin/python3";
    if Path::new(PYTHON).is_file() {
        Ok(PathBuf::from(PYTHON))
    } else {
        Err("VirtualBox consoles need the system Python 3, which was not found.".to_owned())
    }
}

/// The newest `Python3*` directory directly under `program_files` that has `python.exe` and
/// pywin32's `win32com` package in its own site-packages. Only fixed names are checked; no PATH.
#[cfg(any(windows, test))]
fn machine_python(program_files: &Path) -> Option<PathBuf> {
    let mut installs: Vec<PathBuf> = fs::read_dir(program_files)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|dir| {
            dir.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("Python3"))
        })
        .filter(|dir| {
            dir.join("python.exe").is_file()
                && dir
                    .join("Lib")
                    .join("site-packages")
                    .join("win32com")
                    .join("__init__.py")
                    .is_file()
        })
        .collect();
    // Compare by (length, name) so that Python312 sorts after Python39.
    installs.sort_by_key(|dir| {
        let name = dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        (name.len(), name)
    });
    installs.pop().map(|dir| dir.join("python.exe"))
}

/// The VirtualBox install that owns the trusted `VBoxManage`, and its Python SDK.
#[derive(Debug, PartialEq, Eq)]
struct SdkLayout {
    bin: PathBuf,
    sdk: PathBuf,
}

impl SdkLayout {
    fn for_tool(tool: &Path) -> Result<Self, String> {
        let tool = strip_verbatim(
            &fs::canonicalize(tool).map_err(|_| "VBoxManage is not available.".to_owned())?,
        );
        let bin = tool
            .parent()
            .ok_or_else(|| "VBoxManage is not available.".to_owned())?
            .to_path_buf();
        let sdk = bin.join("sdk");
        if !sdk_files_present(&sdk, cfg!(windows)) {
            return Err("VirtualBox's Python SDK was not found beside VBoxManage.".to_owned());
        }
        Ok(SdkLayout { bin, sdk })
    }
}

/// The SDK files the helper imports. Windows needs only the vboxapi package (COM through pywin32);
/// macOS and Linux also need the XPCOM bindings.
fn sdk_files_present(sdk: &Path, windows: bool) -> bool {
    let api = sdk.join("installer/python/vboxapi/src/vboxapi/__init__.py");
    let xpcom = sdk.join("bindings/xpcom/python/xpcom/vboxxpcom.py");
    api.is_file() && (windows || xpcom.is_file())
}

/// `fs::canonicalize` yields `\\?\C:\...` on Windows; the helper is given the plain form.
fn strip_verbatim(path: &Path) -> PathBuf {
    match path.to_str().and_then(|text| text.strip_prefix(r"\\?\")) {
        Some(plain) if !plain.starts_with(r"UNC\") => PathBuf::from(plain),
        _ => path.to_path_buf(),
    }
}

/// A frame reply: its size from the line, its BGRA pixels from the bytes after it.
fn frame_from_reply(reply: &Value, pixels: &[u8]) -> Result<Frame, String> {
    let width = dimension(reply, "width", MAX_FRAME_WIDTH)?;
    let height = dimension(reply, "height", MAX_FRAME_HEIGHT)?;
    if reply.get("format").and_then(Value::as_str) != Some("bgra") {
        return Err(FAILED.to_owned());
    }
    Frame::from_bgrx(width, height, pixels).map_err(|_| FAILED.to_owned())
}

fn dimension(reply: &Value, key: &str, max: u32) -> Result<u32, String> {
    reply
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v > 0 && *v <= max)
        .ok_or_else(|| "The guest display is not a supported size.".to_owned())
}

/// Translate one gesture into helper steps. Coordinates are guest pixels of the frame the caller
/// saw; the helper refuses if the guest display has changed size since then.
fn gesture_steps(gesture: &Gesture, width: u32, height: u32) -> Result<Vec<Value>, String> {
    if width == 0 || height == 0 {
        return Err("No guest frame is available yet.".to_owned());
    }
    match gesture {
        Gesture::Click { x, y, button } => {
            if *x >= width || *y >= height {
                return Err("The click is outside the guest screen.".to_owned());
            }
            let mask = button_mask(button)?;
            Ok(vec![
                json!({ "mouse": [x, y, 0] }),
                json!({ "mouse": [x, y, mask] }),
                json!({ "mouse": [x, y, 0] }),
            ])
        }
        Gesture::Press { code, modifiers } => {
            Ok(vec![json!({ "keys": press_codes(code, modifiers)? })])
        }
        _ => Err("That input needs the live screen.".to_owned()),
    }
}

/// Stream steps as helper steps: `mouse [x, y, buttons]` with VirtualBox's mask (1 left, 2 right,
/// 4 middle, the same bits as the DOM), `wheel [x, y, dz, buttons]` (positive dz scrolls down), and
/// `key {make, down}` with the key's make code.
fn stream_steps(steps: &[Low]) -> Result<Vec<Value>, String> {
    steps
        .iter()
        .map(|step| match step {
            Low::Mouse { x, y, buttons, .. } => Ok(json!({ "mouse": [x, y, buttons] })),
            Low::Wheel { x, y, buttons, dy } => Ok(json!({ "wheel": [x, y, dy, buttons] })),
            Low::Key { qcode, down, .. } => {
                let mut make = Vec::new();
                push_scancode(&mut make, qcode, false)?;
                Ok(json!({ "key": { "make": make, "down": down } }))
            }
        })
        .collect()
}

fn button_mask(button: &str) -> Result<u8, String> {
    match button {
        "left" => Ok(0x01),
        "right" => Ok(0x02),
        "middle" => Ok(0x04),
        _ => Err("That mouse button is not supported.".to_owned()),
    }
}

/// Make codes, then break codes in reverse; modifiers already pressed by the key are skipped
/// (the same rule the QEMU path uses).
fn press_codes(code: &str, modifiers: &[Modifier]) -> Result<Vec<u8>, String> {
    let key = qcode_for(code).ok_or_else(|| "That key is not supported.".to_owned())?;
    if modifiers.len() > 4
        || modifiers
            .iter()
            .enumerate()
            .any(|(i, m)| modifiers[..i].contains(m))
    {
        return Err("Duplicate key modifiers are not supported.".to_owned());
    }
    let mods: Vec<&str> = modifiers
        .iter()
        .map(|m| modifier_qcode(*m))
        .filter(|&m| m != key)
        .collect();
    let mut codes = Vec::new();
    for m in &mods {
        push_scancode(&mut codes, m, false)?;
    }
    push_scancode(&mut codes, key, false)?;
    push_scancode(&mut codes, key, true)?;
    for m in mods.iter().rev() {
        push_scancode(&mut codes, m, true)?;
    }
    Ok(codes)
}

fn modifier_qcode(modifier: Modifier) -> &'static str {
    match modifier {
        Modifier::Shift => "shift",
        Modifier::Control => "ctrl",
        Modifier::Alt => "alt",
        Modifier::Meta => "meta_l",
    }
}

/// Append one key's make (`release == false`) or break code, with the E0 prefix when extended.
fn push_scancode(codes: &mut Vec<u8>, qcode: &str, release: bool) -> Result<(), String> {
    let value = scancode(qcode).ok_or_else(|| "That key is not supported.".to_owned())?;
    let (prefix, code) = (value >> 8, value as u8);
    if prefix == 0xE0 {
        codes.push(0xE0);
    }
    codes.push(if release { code | 0x80 } else { code });
    Ok(())
}

/// PS/2 set 1 make codes, keyed by QEMU's qcode names. Values above 0xFF carry the E0 prefix.
const SCANCODES: &[(&str, u16)] = &[
    ("a", 0x1E),
    ("b", 0x30),
    ("c", 0x2E),
    ("d", 0x20),
    ("e", 0x12),
    ("f", 0x21),
    ("g", 0x22),
    ("h", 0x23),
    ("i", 0x17),
    ("j", 0x24),
    ("k", 0x25),
    ("l", 0x26),
    ("m", 0x32),
    ("n", 0x31),
    ("o", 0x18),
    ("p", 0x19),
    ("q", 0x10),
    ("r", 0x13),
    ("s", 0x1F),
    ("t", 0x14),
    ("u", 0x16),
    ("v", 0x2F),
    ("w", 0x11),
    ("x", 0x2D),
    ("y", 0x15),
    ("z", 0x2C),
    ("1", 0x02),
    ("2", 0x03),
    ("3", 0x04),
    ("4", 0x05),
    ("5", 0x06),
    ("6", 0x07),
    ("7", 0x08),
    ("8", 0x09),
    ("9", 0x0A),
    ("0", 0x0B),
    ("f1", 0x3B),
    ("f2", 0x3C),
    ("f3", 0x3D),
    ("f4", 0x3E),
    ("f5", 0x3F),
    ("f6", 0x40),
    ("f7", 0x41),
    ("f8", 0x42),
    ("f9", 0x43),
    ("f10", 0x44),
    ("f11", 0x57),
    ("f12", 0x58),
    ("ret", 0x1C),
    ("esc", 0x01),
    ("backspace", 0x0E),
    ("tab", 0x0F),
    ("spc", 0x39),
    ("minus", 0x0C),
    ("equal", 0x0D),
    ("bracket_left", 0x1A),
    ("bracket_right", 0x1B),
    ("backslash", 0x2B),
    ("semicolon", 0x27),
    ("apostrophe", 0x28),
    ("grave_accent", 0x29),
    ("comma", 0x33),
    ("dot", 0x34),
    ("slash", 0x35),
    ("caps_lock", 0x3A),
    ("up", 0xE048),
    ("down", 0xE050),
    ("left", 0xE04B),
    ("right", 0xE04D),
    ("home", 0xE047),
    ("end", 0xE04F),
    ("pgup", 0xE049),
    ("pgdn", 0xE051),
    ("delete", 0xE053),
    ("insert", 0xE052),
    ("shift", 0x2A),
    ("shift_r", 0x36),
    ("ctrl", 0x1D),
    ("ctrl_r", 0xE01D),
    ("alt", 0x38),
    ("alt_r", 0xE038),
    ("meta_l", 0xE05B),
    ("meta_r", 0xE05C),
];

fn scancode(qcode: &str) -> Option<u16> {
    SCANCODES
        .iter()
        .find(|(name, _)| *name == qcode)
        .map(|(_, code)| *code)
}
