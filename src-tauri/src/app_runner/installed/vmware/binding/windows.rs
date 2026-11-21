//! Windows probe for the VMware console (see `binding.rs`). Only fixed system tools run, and each
//! query names one exact socket or one exact process; nothing scans the process list.
//!
//! - Sockets: `%SystemRoot%\System32\netstat.exe -ano`, one read of the TCP table. Only the English
//!   states `LISTENING` and `ESTABLISHED` are understood; localized output fails closed.
//! - Processes: `powershell.exe` runs one static script (`-EncodedCommand`) that asks CIM for
//!   `Win32_Process` by exact PID and prints JSON. The PID reaches the script as a typed `u32` in
//!   `DIAL_VMX_PID`; no other text is interpolated. `-NoProfile` keeps user profiles out.
//! - Open file: `CommandLineToArgvW` splits the process's command line, and the `.vmx` must equal
//!   one whole argument (ASCII-case-insensitive, `/` read as `\`), never a substring.
//!
//! Every child runs with a cleared environment (`SystemRoot` and a System32 `PATH` only), stdin
//! closed, a deadline and an output byte cap.

use std::ffi::c_void;
use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::Deserialize;

use super::{Listener, Lsof, Probe};

const SAFE_MESSAGE: &str = "could not query the system for the VM's VNC console";
const MISSING_TOOL: &str = "a required system tool is missing";
const NETSTAT: &str = r"System32\netstat.exe";
const POWERSHELL: &str = r"System32\WindowsPowerShell\v1.0\powershell.exe";
const NETSTAT_LIMIT: usize = 8 << 20;
const PROCESS_LIMIT: usize = 256 << 10;
const TOOL_TIMEOUT: Duration = Duration::from_secs(10);
const READ_GRACE: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(20);

/// The one script ever run. Its only input is the numeric PID in `DIAL_VMX_PID`.
const PROCESS_SCRIPT: &str = "\
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { }
$ErrorActionPreference = 'Stop'
$id = [uint32]$env:DIAL_VMX_PID
$p = Get-CimInstance -ClassName Win32_Process -Filter ('ProcessId = ' + $id)
if ($null -eq $p) { exit 0 }
[pscustomobject]@{ ExecutablePath = $p.ExecutablePath; CommandLine = $p.CommandLine } | ConvertTo-Json -Compress
";

impl Probe for Lsof {
    fn supported(&self) -> Result<(), String> {
        system_tool(NETSTAT)?;
        system_tool(POWERSHELL)?;
        Ok(())
    }

    fn listeners(&self, port: u16) -> Result<Vec<Listener>, String> {
        Ok(listeners_in(&netstat()?, port))
    }

    fn executable(&self, pid: u32) -> Result<Option<PathBuf>, String> {
        process_executable(pid)
    }

    fn has_open_file(&self, pid: u32, path: &str) -> Result<bool, String> {
        process_has_argument(pid, path)
    }

    fn connection_owner(&self, port: u16, client: SocketAddr) -> Result<Option<u32>, String> {
        Ok(owner_of(&netstat()?, port, client))
    }
}

/// One TCP row from `netstat -ano`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Socket {
    local: SocketAddr,
    /// `None` for `*:*` and other rows without a numeric peer.
    foreign: Option<SocketAddr>,
    state: String,
    pid: u32,
}

/// The fields of one process the probe needs, as CIM reports them. `None` means CIM withheld it.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
struct Process {
    executable_path: Option<String>,
    command_line: Option<String>,
}

fn netstat() -> Result<Vec<Socket>, String> {
    let output = run_bounded(&system_tool(NETSTAT)?, &["-ano"], &[], NETSTAT_LIMIT)?;
    if !output.success {
        return Err(SAFE_MESSAGE.to_owned());
    }
    parse_netstat(&output.stdout)
}

/// Parse the TCP rows of `netstat -ano`. Headers and UDP rows are skipped. A TCP row that does not
/// have exactly five fields with parsable addresses and pid is an error, never a silent skip,
/// because a dropped listener could hide a second owner of the port.
fn parse_netstat(text: &str) -> Result<Vec<Socket>, String> {
    let mut sockets = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.first() != Some(&"TCP") {
            continue;
        }
        let [_, local, foreign, state, pid] = fields.as_slice() else {
            return Err(SAFE_MESSAGE.to_owned());
        };
        let local = local.parse().map_err(|_| SAFE_MESSAGE.to_owned())?;
        let pid = pid.parse().map_err(|_| SAFE_MESSAGE.to_owned())?;
        sockets.push(Socket {
            local,
            foreign: foreign.parse().ok(),
            state: (*state).to_owned(),
            pid,
        });
    }
    Ok(sockets)
}

/// Every listener on exactly `port`, whatever its address (the caller refuses non-loopback ones).
fn listeners_in(sockets: &[Socket], port: u16) -> Vec<Listener> {
    sockets
        .iter()
        .filter(|s| s.state == "LISTENING" && s.local.port() == port)
        .map(|s| Listener {
            pid: s.pid,
            host: s.local.ip().to_string(),
        })
        .collect()
}

/// The pid owning the server end of the established loopback connection `client` -> `port`.
/// Two different owners are ambiguous and yield `None`.
fn owner_of(sockets: &[Socket], port: u16, client: SocketAddr) -> Option<u32> {
    let mut owners = sockets
        .iter()
        .filter(|s| {
            s.state == "ESTABLISHED"
                && s.local.port() == port
                && s.local.ip().is_loopback()
                && s.foreign == Some(client)
        })
        .map(|s| s.pid);
    let owner = owners.next()?;
    owners.all(|pid| pid == owner).then_some(owner)
}

/// Query CIM for one exact PID. `Ok(None)` when no such process exists.
fn process(pid: u32) -> Result<Option<Process>, String> {
    let script = base64::engine::general_purpose::STANDARD.encode(utf16le(PROCESS_SCRIPT));
    let output = run_bounded(
        &system_tool(POWERSHELL)?,
        &[
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            script.as_str(),
        ],
        &[("DIAL_VMX_PID", pid.to_string())],
        PROCESS_LIMIT,
    )?;
    if !output.success {
        return Err(SAFE_MESSAGE.to_owned());
    }
    parse_process(&output.stdout)
}

/// Parse the script's JSON. Empty output means the process has exited.
fn parse_process(text: &str) -> Result<Option<Process>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    serde_json::from_str(text)
        .map(Some)
        .map_err(|_| SAFE_MESSAGE.to_owned())
}

/// The executable CIM reports for `pid`. A path with a `..` segment is refused outright.
fn process_executable(pid: u32) -> Result<Option<PathBuf>, String> {
    let Some(process) = process(pid)? else {
        return Ok(None);
    };
    Ok(process
        .executable_path
        .filter(|path| !has_parent_segment(path))
        .map(PathBuf::from))
}

/// Whether the process's command line has `path` as one whole argument.
fn process_has_argument(pid: u32, path: &str) -> Result<bool, String> {
    let Some(command_line) = process(pid)?.and_then(|process| process.command_line) else {
        return Ok(false);
    };
    Ok(argv(&command_line)?.iter().any(|arg| same_path(arg, path)))
}

/// Exact path equality for Windows: `/` and `\` are the same separator and case is ignored.
fn same_path(a: &str, b: &str) -> bool {
    normalize(a) == normalize(b)
}

fn normalize(path: &str) -> String {
    path.replace('/', r"\").to_lowercase()
}

fn has_parent_segment(path: &str) -> bool {
    path.split(['\\', '/']).any(|segment| segment == "..")
}

fn utf16le(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn CommandLineToArgvW(command_line: *const u16, num_args: *mut i32) -> *mut *mut u16;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LocalFree(memory: *mut c_void) -> *mut c_void;
}

/// Split a command line the way the Windows C runtime does, through `CommandLineToArgvW`.
fn argv(command_line: &str) -> Result<Vec<String>, String> {
    let wide: Vec<u16> = command_line
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut count = 0i32;
    // SAFETY: `wide` is NUL-terminated and lives across the call; `count` is a valid out-pointer.
    let args = unsafe { CommandLineToArgvW(wide.as_ptr(), &mut count) };
    if args.is_null() || count <= 0 {
        return Err(SAFE_MESSAGE.to_owned());
    }
    let mut out = Vec::with_capacity(count as usize);
    for index in 0..count as usize {
        // SAFETY: CommandLineToArgvW returned `count` NUL-terminated strings in `args`.
        out.push(unsafe { wide_string(*args.add(index)) });
    }
    // SAFETY: `args` came from CommandLineToArgvW and is freed exactly once, here.
    unsafe { LocalFree(args.cast()) };
    Ok(out)
}

/// # Safety
/// `ptr` must point to a NUL-terminated UTF-16 string that stays valid for the call.
unsafe fn wide_string(ptr: *const u16) -> String {
    let mut len = 0;
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
    }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, len) })
}

/// `%SystemRoot%`, which must be an absolute path. Tools are never looked up on `PATH`.
fn system_root() -> Result<PathBuf, String> {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .filter(|root| root.is_absolute())
        .ok_or_else(|| MISSING_TOOL.to_owned())
}

fn system_tool(relative: &str) -> Result<PathBuf, String> {
    let path = system_root()?.join(relative);
    if path.is_file() {
        Ok(path)
    } else {
        Err(MISSING_TOOL.to_owned())
    }
}

struct Output {
    success: bool,
    stdout: String,
}

/// Run a fixed tool with a cleared environment, closed stdin and no stderr. Stops it after
/// [`TOOL_TIMEOUT`] and fails when stdout exceeds `limit` bytes. Output is never echoed in errors.
fn run_bounded(
    exe: &Path,
    args: &[&str],
    env: &[(&str, String)],
    limit: usize,
) -> Result<Output, String> {
    let root = system_root()?;
    let mut child = Command::new(exe)
        .args(args)
        .env_clear()
        .env("SystemRoot", &root)
        .env("PATH", root.join("System32"))
        .envs(env.iter().map(|(key, value)| (*key, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| SAFE_MESSAGE.to_owned())?;
    let mut stdout = child.stdout.take().ok_or_else(|| SAFE_MESSAGE.to_owned())?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let read = (&mut stdout).take(limit as u64 + 1).read_to_end(&mut bytes);
        let _ = sender.send(read.map(|_| bytes));
    });
    let deadline = Instant::now() + TOOL_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(SAFE_MESSAGE.to_owned());
            }
        }
    };
    let bytes = receiver
        .recv_timeout(READ_GRACE)
        .map_err(|_| SAFE_MESSAGE.to_owned())?
        .map_err(|_| SAFE_MESSAGE.to_owned())?;
    if bytes.len() > limit {
        return Err(SAFE_MESSAGE.to_owned());
    }
    Ok(Output {
        success: status.success(),
        stdout: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

#[cfg(test)]
mod tests;
