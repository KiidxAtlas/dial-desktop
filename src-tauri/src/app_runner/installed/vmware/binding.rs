//! Proof that the endpoint on a VM's VNC port belongs to that VM's VMware process.
//!
//! A port number in a `.vmx` is not evidence of anything: another program can hold it. The console
//! checks the following before sending RFB data:
//! - every listener on the port is on a loopback address and one process listens there;
//! - that process's executable is VMware's `vmware-vmx` under a trusted install root (exact file
//!   name, not a substring of a command line);
//! - the process identifies this exact `.vmx` (open file on Unix, whole argument on Windows);
//! - the client socket Dial connected is owned, on the server side, by that same process.
//!
//! macOS and Linux use system `lsof`; Windows uses system socket and process queries.
//! If ownership cannot be proven, Dial refuses the connection.

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

/// One listening socket on the VNC port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Listener {
    pub(super) pid: u32,
    pub(super) host: String,
}

/// Read-only process and socket queries. Tests substitute a fake.
pub(super) trait Probe {
    /// Refuses before anything is started when this host cannot prove socket ownership.
    fn supported(&self) -> Result<(), String>;
    fn listeners(&self, port: u16) -> Result<Vec<Listener>, String>;
    /// The executable file the process runs from, if it can be read.
    fn executable(&self, pid: u32) -> Result<Option<PathBuf>, String>;
    /// Whether this process identifies the exact VMX (open file on Unix, argument on Windows).
    fn has_open_file(&self, pid: u32, path: &str) -> Result<bool, String>;
    /// The pid owning the server end of the established connection `client` -> `port`.
    fn connection_owner(&self, port: u16, client: SocketAddr) -> Result<Option<u32>, String>;
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Proof {
    NotListening,
    /// The listener on the port is this VM's VMware process.
    Bound {
        pid: u32,
    },
}

/// Whether `pid` is the VMware VM process for the exact `.vmx` path `vmx`.
///
/// The executable must be named exactly `vmware-vmx` (or `vmware-vmx.exe`) and sit under a trusted
/// install root. The VMX must match an open file (Unix) or a whole path argument (Windows).
/// A substring match is never sufficient.
pub(super) fn owns_vm(probe: &dyn Probe, pid: u32, vmx: &str) -> Result<bool, String> {
    let Some(exe) = probe.executable(pid)? else {
        return Ok(false);
    };
    if !is_trusted_vmx_executable(&exe) {
        return Ok(false);
    }
    probe.has_open_file(pid, vmx)
}

/// Check the listeners on `port` against the VM whose `.vmx` is `vmx`.
pub(super) fn verify(probe: &dyn Probe, port: u16, vmx: &str) -> Result<Proof, String> {
    let listeners = probe.listeners(port)?;
    if listeners.is_empty() {
        return Ok(Proof::NotListening);
    }
    if let Some(listener) = listeners.iter().find(|l| !is_loopback(&l.host)) {
        return Err(format!(
            "the VM's VNC port {port} is bound to {}, not loopback; refusing to connect",
            listener.host
        ));
    }
    let mut pids: Vec<u32> = listeners.iter().map(|l| l.pid).collect();
    pids.sort_unstable();
    pids.dedup();
    let &[pid] = pids.as_slice() else {
        return Err(format!(
            "{} processes listen on the VM's VNC port {port}; refusing to guess which is the VM",
            pids.len()
        ));
    };
    if !owns_vm(probe, pid, vmx)? {
        return Err(format!(
            "the process on the VM's VNC port {port} is not the VMware process running this VM; refusing to connect"
        ));
    }
    Ok(Proof::Bound { pid })
}

/// Poll [`verify`] until the VM's listener is bound, or fail after `wait`. Returns the owner pid.
pub(super) fn wait_for_binding(
    probe: &dyn Probe,
    port: u16,
    vmx: &str,
    wait: Duration,
    poll: Duration,
) -> Result<u32, String> {
    let deadline = Instant::now() + wait;
    loop {
        if let Proof::Bound { pid } = verify(probe, port, vmx)? {
            return Ok(pid);
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "the VM's VNC console is not listening on port {port}; check that remote display (VNC) is enabled for this VM"
            ));
        }
        thread::sleep(poll);
    }
}

/// After connecting: the server end of our socket must belong to the process proven above.
pub(super) fn check_connection(
    probe: &dyn Probe,
    port: u16,
    client: SocketAddr,
    proven_pid: u32,
) -> Result<(), String> {
    match probe.connection_owner(port, client)? {
        Some(pid) if pid == proven_pid => Ok(()),
        Some(_) => Err(
            "the connection to the VM's console belongs to another process; refusing to use it"
                .into(),
        ),
        None => Err(
            "could not confirm the owner of the connection to the VM's console; refusing to use it"
                .into(),
        ),
    }
}

/// Install roots whose `vmware-vmx` may run a VM. Executables elsewhere are refused even when named
/// correctly.
#[cfg(not(windows))]
fn trusted_roots() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    {
        &["/Applications/VMware Fusion.app/"]
    }
    #[cfg(target_os = "linux")]
    {
        &["/usr/lib/vmware/"]
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        &[]
    }
}

/// The executable must be named exactly `vmware-vmx` (`.exe` on Windows) and lie under a trusted root.
#[cfg(not(windows))]
pub(super) fn is_trusted_vmx_executable(exe: &Path) -> bool {
    let name_ok = exe
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "vmware-vmx" || n == "vmware-vmx.exe");
    let text = exe.to_string_lossy();
    name_ok
        && trusted_roots().iter().any(|root| text.starts_with(root))
}

#[cfg(windows)]
pub(super) fn is_trusted_vmx_executable(exe: &Path) -> bool {
    windows::trusted_executable(exe, super::super::inventory::vmware_install_dirs())
}

/// A loopback IP literal. Hostnames such as `localhost` are not accepted: they are not proof.
pub(super) fn is_loopback(host: &str) -> bool {
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
}

#[cfg(unix)]
/// The server end is the socket named `<host>:<port>-><client>`, shown under its owner's pid.
pub(super) fn owner_of(sockets: &[(u32, String)], port: u16, client: SocketAddr) -> Option<u32> {
    let client = client.to_string();
    let mut owners = sockets
        .iter()
        .filter(|(_, name)| {
            name.split_once("->").is_some_and(|(server, peer)| {
                peer == client && port_of(server) == Some(port) && is_loopback_socket(server)
            })
        })
        .map(|(pid, _)| *pid);
    let owner = owners.next()?;
    owners.all(|pid| pid == owner).then_some(owner)
}

#[cfg(unix)]
/// Parse `lsof -Fpn` output into `(pid, name)` pairs. `p<pid>` starts a process; `n<name>`
/// names one of its sockets or files.
pub(super) fn parse_sockets(text: &str) -> Vec<(u32, String)> {
    let mut pid = None;
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(field) = line.strip_prefix('p') {
            pid = field.parse().ok();
        } else if let Some(name) = line.strip_prefix('n')
            && let Some(pid) = pid
        {
            out.push((pid, name.to_owned()));
        }
    }
    out
}

#[cfg(unix)]
fn port_of(name: &str) -> Option<u16> {
    let (_, port) = name.rsplit_once(':')?;
    port.parse().ok()
}

#[cfg(unix)]
fn is_loopback_socket(server: &str) -> bool {
    server
        .rsplit_once(':')
        .is_some_and(|(host, _)| is_loopback(host))
}

/// The system probe: `lsof` for sockets, executables and open files.
pub(super) struct Lsof;

#[cfg(unix)]
mod unix {
    use std::net::SocketAddr;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use super::{Listener, Lsof, Probe, owner_of, parse_sockets, port_of};

    const LSOF: [&str; 2] = ["/usr/sbin/lsof", "/usr/bin/lsof"];
    const SAFE_MESSAGE: &str = "could not query the system for the VM's VNC console";

    fn fixed_tool<'a>(candidates: &[&'a str]) -> Result<&'a str, String> {
        candidates
            .iter()
            .copied()
            .find(|path| Path::new(path).is_file())
            .ok_or_else(|| "a required system tool is missing".to_owned())
    }

    /// Run `lsof`. It exits 1 when nothing matches, which is not an error. Output is never echoed
    /// into an error message.
    fn lsof(args: &[&str]) -> Result<String, String> {
        let output = Command::new(fixed_tool(&LSOF)?)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(|_| SAFE_MESSAGE.to_owned())?;
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        if !output.status.success() && !(output.status.code() == Some(1) && text.trim().is_empty())
        {
            return Err(SAFE_MESSAGE.to_owned());
        }
        Ok(text)
    }

    pub(super) fn supported() -> Result<(), String> {
        fixed_tool(&LSOF).map(|_| ())
    }

    pub(super) fn listeners(port: u16) -> Result<Vec<Listener>, String> {
        let text = lsof(&["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-Fpn"])?;
        Ok(parse_sockets(&text)
            .into_iter()
            .filter(|(_, name)| port_of(name) == Some(port))
            .filter_map(|(pid, name)| {
                name.rsplit_once(':').map(|(host, _)| Listener {
                    pid,
                    host: host.to_owned(),
                })
            })
            .collect())
    }

    /// The executable file: the process's `txt` descriptor.
    pub(super) fn executable(pid: u32) -> Result<Option<PathBuf>, String> {
        let text = lsof(&["-nP", "-a", "-p", &pid.to_string(), "-d", "txt", "-Fn"])?;
        Ok(text
            .lines()
            .find_map(|line| line.strip_prefix('n'))
            .map(PathBuf::from))
    }

    pub(super) fn has_open_file(pid: u32, path: &str) -> Result<bool, String> {
        let text = lsof(&["-nP", "-p", &pid.to_string(), "-Fn"])?;
        Ok(text
            .lines()
            .any(|line| line.strip_prefix('n') == Some(path)))
    }

    pub(super) fn connection_owner(port: u16, client: SocketAddr) -> Result<Option<u32>, String> {
        let text = lsof(&["-nP", &format!("-iTCP:{port}"), "-sTCP:ESTABLISHED", "-Fpn"])?;
        Ok(owner_of(&parse_sockets(&text), port, client))
    }

    impl Probe for Lsof {
        fn supported(&self) -> Result<(), String> {
            supported()
        }

        fn listeners(&self, port: u16) -> Result<Vec<Listener>, String> {
            listeners(port)
        }

        fn executable(&self, pid: u32) -> Result<Option<PathBuf>, String> {
            executable(pid)
        }

        fn has_open_file(&self, pid: u32, path: &str) -> Result<bool, String> {
            has_open_file(pid, path)
        }

        fn connection_owner(&self, port: u16, client: SocketAddr) -> Result<Option<u32>, String> {
            connection_owner(port, client)
        }
    }
}

#[cfg(windows)]
mod windows;

#[cfg(not(any(unix, windows)))]
impl Probe for Lsof {
    fn supported(&self) -> Result<(), String> {
        Err("the VMware console is supported on macOS, Linux and Windows only".into())
    }

    fn listeners(&self, _port: u16) -> Result<Vec<Listener>, String> {
        Err("the VMware console is supported on macOS, Linux and Windows only".into())
    }

    fn executable(&self, _pid: u32) -> Result<Option<PathBuf>, String> {
        Err("the VMware console is supported on macOS, Linux and Windows only".into())
    }

    fn has_open_file(&self, _pid: u32, _path: &str) -> Result<bool, String> {
        Err("the VMware console is supported on macOS, Linux and Windows only".into())
    }

    fn connection_owner(&self, _port: u16, _client: SocketAddr) -> Result<Option<u32>, String> {
        Err("the VMware console is supported on macOS, Linux and Windows only".into())
    }
}
