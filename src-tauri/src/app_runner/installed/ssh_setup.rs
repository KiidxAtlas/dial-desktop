//! App → VM **Set up SSH**: one press that makes a connected Linux VirtualBox VM reachable through
//! Dial's `ssh` tool.
//!
//! 1. Reads the VM's network adapters (`VBoxManage showvminfo`). A NAT adapter without an SSH port
//!    forward gets one on loopback (`controlvm natpfN`); a bridged adapter is found by its MAC.
//! 2. Creates a dedicated key, `~/.ssh/dial_<alias>` (ed25519, no passphrase), unless it exists.
//! 3. Types one command into a terminal in the guest, as the user's own input: it enables `sshd`
//!    (installing `openssh-server` with apt when missing), adds the public key to the guest user's
//!    `authorized_keys`, and pings the Mac so the guest's address appears in the host ARP table.
//!    `sudo` asks for the guest password in the VM's own screen.
//! 4. Waits until `ssh` with that key succeeds, then writes a managed `Host <alias>` block at the
//!    top of `~/.ssh/config` (other content is kept) and links the alias to the VM.
//!
//! Every program is a fixed system path, never looked up on `PATH`.

use std::ffi::OsStr;
use std::fs;
use std::io::{ErrorKind, Read};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::inventory::{self, Provider};
use super::ssh;
use crate::app_runner::actions::Action;
use crate::app_runner::{AppRunError, AppRunner};

#[cfg(test)]
mod tests;

const TOOL_TIMEOUT: Duration = Duration::from_secs(20);
const PROBE_TIMEOUT: Duration = Duration::from_secs(12);
/// How long the guest has to finish: the user may type a sudo password, apt may download.
const READY_TIMEOUT: Duration = Duration::from_secs(300);
const POLL: Duration = Duration::from_secs(3);
const MAX_OUTPUT: u64 = 256 << 10;
const MARK_OPEN: &str = "# >>> dial-desktop vm ";
const MARK_CLOSE: &str = "# <<< dial-desktop vm ";

/// What a finished setup reports to the App panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupDone {
    pub host: String,
    pub address: String,
    pub port: u16,
    pub user: String,
}

/// How the Mac reaches the guest's SSH port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reach {
    /// An existing NAT port forward: host loopback `port` → guest 22.
    Forward { port: u16 },
    /// A NAT adapter (1-based `nic`) that needs a forward.
    NatNeedsForward { nic: u8 },
    /// A bridged adapter: the guest has its own LAN address. It reports that address to a one-time
    /// listener on the Mac (see [`Report`]); the ARP entry for `mac` is the fallback. ARP alone is not
    /// enough: bridging to Wi-Fi rewrites the guest's MAC to the Mac's own on the wire.
    Lan { mac: [u8; 6] },
}

/// Where the guest connects back to say it is ready: the Mac's LAN address, a port Dial listens on,
/// and a one-time token so a stray connection from another LAN host is ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Report {
    pub addr: SocketAddrV4,
    pub token: String,
}

/// Whether bytes a peer sent are the report for `token`.
pub(crate) fn is_report(received: &[u8], token: &str) -> bool {
    !token.is_empty() && std::str::from_utf8(received).is_ok_and(|text| text.trim() == token)
}

// ----- pure parts (unit-tested) -----

/// A guest account name: letters, digits, `.`, `_`, `-`; not starting with `-`; at most 32.
pub(crate) fn valid_user(user: &str) -> bool {
    !user.is_empty()
        && user.len() <= 32
        && !user.starts_with('-')
        && user.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// The SSH alias for a VM name: lowercase, other characters as `-`, `vm` when nothing is left.
pub(crate) fn alias_for(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').chars().take(48).collect::<String>();
    if out.is_empty() { "vm".into() } else { out }
}

pub(crate) fn parse_mac(text: &str) -> Option<[u8; 6]> {
    let text = text.trim();
    let parts: Vec<&str> = if text.contains(':') || text.contains('-') {
        text.split([':', '-']).collect()
    } else if text.len() == 12 {
        (0..6).map(|i| &text[i * 2..i * 2 + 2]).collect()
    } else {
        return None;
    };
    if parts.len() != 6 {
        return None;
    }
    let mut mac = [0u8; 6];
    for (slot, part) in mac.iter_mut().zip(parts) {
        if part.is_empty() || part.len() > 2 {
            return None;
        }
        *slot = u8::from_str_radix(part, 16).ok()?;
    }
    Some(mac)
}

/// The VM's adapters and loopback SSH forwards, from `showvminfo --machinereadable`.
pub(crate) fn reach_from_vminfo(text: &str) -> Result<Reach, String> {
    let mut nics: Vec<(u8, String, Option<[u8; 6]>)> = Vec::new();
    let mut macs: Vec<(u8, [u8; 6])> = Vec::new();
    let mut forward = None;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim().trim_matches('"');
        if let Some(n) = key.strip_prefix("nic").and_then(|n| n.parse::<u8>().ok()) {
            nics.push((n, value.to_owned(), None));
        } else if let Some(n) = key.strip_prefix("macaddress").and_then(|n| n.parse::<u8>().ok()) {
            if let Some(mac) = parse_mac(value) {
                macs.push((n, mac));
            }
        } else if key.starts_with("Forwarding(") {
            // name,proto,hostip,hostport,guestip,guestport
            let f: Vec<&str> = value.split(',').collect();
            if f.len() == 6
                && f[1] == "tcp"
                && f[5] == "22"
                && matches!(f[2], "" | "127.0.0.1" | "0.0.0.0")
                && forward.is_none()
            {
                forward = f[3].parse::<u16>().ok().filter(|p| *p > 0);
            }
        }
    }
    for nic in &mut nics {
        nic.2 = macs.iter().find(|(n, _)| *n == nic.0).map(|(_, m)| *m);
    }
    if let Some(port) = forward {
        return Ok(Reach::Forward { port });
    }
    if let Some(mac) = nics.iter().find(|n| n.1 == "bridged").and_then(|n| n.2) {
        return Ok(Reach::Lan { mac });
    }
    if let Some(nic) = nics.iter().find(|n| n.1 == "nat") {
        return Ok(Reach::NatNeedsForward { nic: nic.0 });
    }
    Err("This VM has no NAT or bridged network adapter, so the Mac cannot reach it over SSH. Add one in VirtualBox, then try again.".into())
}

/// The IPv4 address for `mac` in ARP output (`arp -an`, `arp -a` or `/proc/net/arp`).
pub(crate) fn ip_for_mac(arp: &str, mac: [u8; 6]) -> Option<Ipv4Addr> {
    arp.lines().find_map(|line| {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let has_mac = tokens.iter().any(|t| parse_mac(t) == Some(mac) && (t.contains(':') || t.contains('-')));
        if !has_mac {
            return None;
        }
        tokens
            .iter()
            .find_map(|t| t.trim_matches(|c| c == '(' || c == ')').parse::<Ipv4Addr>().ok())
    })
}

/// The one-line command typed in the guest terminal. `pub_key` is an OpenSSH public key line.
pub(crate) fn guest_command(pub_key: &str, ping: Option<Ipv4Addr>, report: Option<&Report>) -> Result<String, String> {
    let key = pub_key.trim();
    if key.is_empty()
        || !key.starts_with("ssh-ed25519 ")
        || !key.bytes().all(|b| (0x20..0x7f).contains(&b) && b != b'\'')
    {
        return Err("The new SSH key is not in the expected form.".into());
    }
    // Bash's /dev/tcp works whatever the login shell is; `timeout` keeps a blocked port from hanging.
    let report = match report {
        Some(Report { token, .. }) if token.is_empty() || !token.bytes().all(|b| b.is_ascii_alphanumeric()) => {
            return Err("The setup token is not in the expected form.".into());
        }
        Some(Report { addr, token }) => format!(
            " && (timeout 5 bash -c \"echo {token} >/dev/tcp/{}/{}\") >/dev/null 2>&1",
            addr.ip(),
            addr.port()
        ),
        None => String::new(),
    };
    let ping = ping
        .map(|ip| format!("; ping -c 3 -W 1 {ip} >/dev/null 2>&1"))
        .unwrap_or_default();
    Ok(format!(
        "sudo sh -c 'command -v sshd >/dev/null 2>&1 || (\
if command -v apt-get >/dev/null 2>&1; then apt-get update && apt-get install -y openssh-server; \
elif command -v dnf >/dev/null 2>&1; then dnf install -y openssh-server; \
elif command -v yum >/dev/null 2>&1; then yum install -y openssh-server; \
elif command -v pacman >/dev/null 2>&1; then pacman -Sy --noconfirm openssh; \
elif command -v zypper >/dev/null 2>&1; then zypper --non-interactive install openssh; \
elif command -v apk >/dev/null 2>&1; then apk add --no-cache openssh; \
else exit 1; fi); \
systemctl enable --now ssh 2>/dev/null || systemctl enable --now sshd 2>/dev/null || rc-service sshd start 2>/dev/null || service ssh start' && \
mkdir -p ~/.ssh && chmod 700 ~/.ssh && \
(grep -qxF '{key}' ~/.ssh/authorized_keys 2>/dev/null || echo '{key}' >> ~/.ssh/authorized_keys) && \
chmod 600 ~/.ssh/authorized_keys{report}{ping}; echo DIAL-SSH-READY\n"
    ))
}

/// The managed config block for `alias`.
pub(crate) fn config_block(alias: &str, host: &str, port: u16, user: &str, key: &Path) -> String {
    format!(
        "{MARK_OPEN}{alias} (App → VM → Set up SSH; Dial rewrites this block)\n\
Host {alias}\n    HostName {host}\n    Port {port}\n    User {user}\n    IdentityFile \"{}\"\n    \
IdentitiesOnly yes\n    StrictHostKeyChecking accept-new\n{MARK_CLOSE}{alias}\n",
        key.display()
    )
}

/// `text` without the managed block for `alias`.
pub(crate) fn without_block(text: &str, alias: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end();
        let names = |mark: &str| {
            bare.strip_prefix(mark)
                .is_some_and(|rest| rest == alias || rest.starts_with(&format!("{alias} ")))
        };
        if !inside && names(MARK_OPEN) {
            inside = true;
            continue;
        }
        if inside {
            if names(MARK_CLOSE) {
                inside = false;
            }
            continue;
        }
        out.push_str(line);
    }
    out
}

/// `text` with the managed block for `alias` replaced by `block`, at the top (SSH uses the first
/// value it finds, so a later `Host *` cannot override it).
pub(crate) fn upsert_block(text: &str, alias: &str, block: &str) -> String {
    let rest = without_block(text, alias);
    let rest = rest.trim_start_matches('\n');
    if rest.is_empty() { block.to_owned() } else { format!("{block}\n{rest}") }
}

/// The alias to write: the VM's own, or `dial-<alias>` when the user's config already has a host
/// of that name outside Dial's block.
pub(crate) fn free_alias(config: &str, base: &str) -> String {
    let taken = |alias: &str| {
        ssh::parse_aliases(&without_block(config, alias))
            .iter()
            .any(|a| a.eq_ignore_ascii_case(alias))
    };
    if !taken(base) {
        return base.to_owned();
    }
    let dial = format!("dial-{base}");
    if !taken(&dial) { dial } else { format!("dial-vm-{base}") }
}

// ----- the host side -----

fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| "Dial could not find your home folder.".to_owned())
}

#[cfg(windows)]
fn system_tool(name: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    match name {
        "arp" => root.join("System32").join("arp.exe"),
        _ => root.join("System32").join("OpenSSH").join(format!("{name}.exe")),
    }
}

#[cfg(not(windows))]
fn system_tool(name: &str) -> PathBuf {
    match name {
        "arp" => PathBuf::from("/usr/sbin/arp"),
        _ => PathBuf::from("/usr/bin").join(name),
    }
}

/// Run `program` with a deadline; returns (success, stdout). The child is killed on timeout.
fn run<S: AsRef<OsStr>>(program: &Path, args: &[S], timeout: Duration) -> Result<(bool, String), String> {
    let mut command = Command::new(program);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| format!("Dial could not run {}.", program.display()))?;
    let pipe = child.stdout.take();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(pipe) = pipe {
            let _ = pipe.take(MAX_OUTPUT).read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
                return Ok((status.success(), String::from_utf8_lossy(&out).into_owned()));
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok((false, String::new()));
            }
        }
    }
}

/// The Mac's address on its default route. A UDP `connect` sends nothing.
fn host_ip() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

fn arp_table() -> String {
    if cfg!(target_os = "linux") {
        return fs::read_to_string("/proc/net/arp").unwrap_or_default();
    }
    let args: &[&str] = if cfg!(windows) { &["-a"] } else { &["-an"] };
    run(&system_tool("arp"), args, TOOL_TIMEOUT).map(|(_, out)| out).unwrap_or_default()
}

/// The Mac side of [`Report`]: a non-blocking listener on the Mac's LAN address.
struct ReportListener {
    listener: TcpListener,
    report: Report,
}

impl ReportListener {
    fn open(host: Ipv4Addr) -> Option<Self> {
        let listener = TcpListener::bind(SocketAddrV4::new(host, 0)).ok()?;
        listener.set_nonblocking(true).ok()?;
        let SocketAddr::V4(addr) = listener.local_addr().ok()? else { return None };
        let token = crate::agent_browser::new_token().chars().take(32).collect();
        Some(Self { listener, report: Report { addr, token } })
    }

    /// The address of a guest that sent the token, from connections waiting now.
    fn poll(&self) -> Option<Ipv4Addr> {
        loop {
            let (stream, peer) = match self.listener.accept() {
                Ok(pair) => pair,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => return None,
            };
            let SocketAddr::V4(peer) = peer else { continue };
            if stream.set_nonblocking(false).is_err() || stream.set_read_timeout(Some(Duration::from_secs(2))).is_err() {
                continue;
            }
            let mut received = Vec::new();
            let _ = stream.take(128).read_to_end(&mut received);
            if is_report(&received, &self.report.token) {
                return Some(*peer.ip());
            }
        }
    }
}

fn free_port() -> Option<u16> {
    TcpListener::bind("127.0.0.1:0").ok()?.local_addr().ok().map(|a| a.port())
}

/// The public key path `ssh-keygen -f <key>` writes: `<key>.pub`, preserving dots in `<key>`.
pub(crate) fn public_key_path(key: &Path) -> PathBuf {
    let mut os = key.as_os_str().to_os_string();
    os.push(".pub");
    PathBuf::from(os)
}

/// The dedicated key pair, created on first use. Returns (private key path, public key line).
fn ensure_key(ssh_dir: &Path, alias: &str) -> Result<(PathBuf, String), String> {
    fs::create_dir_all(ssh_dir).map_err(|_| "Dial could not create ~/.ssh.".to_owned())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(ssh_dir, fs::Permissions::from_mode(0o700));
    }
    let key = ssh_dir.join(format!("dial_{alias}"));
    let public = public_key_path(&key);
    if !key.is_file() || !public.is_file() {
        let (ok, _) = run(
            &system_tool("ssh-keygen"),
            &[OsStr::new("-q"), OsStr::new("-t"), OsStr::new("ed25519"), OsStr::new("-N"), OsStr::new(""),
              OsStr::new("-C"), OsStr::new("dial"), OsStr::new("-f"), key.as_os_str()],
            TOOL_TIMEOUT,
        )?;
        if !ok {
            return Err("ssh-keygen could not create the key for this VM.".into());
        }
    }
    let line = fs::read_to_string(&public).map_err(|_| "Dial could not read the new public key.".to_owned())?;
    Ok((key, line.trim().to_owned()))
}

fn probe(key: &Path, user: &str, host: &str, port: u16) -> bool {
    let port = port.to_string();
    let target = format!("{user}@{host}");
    let args: Vec<&OsStr> = vec![
        OsStr::new("-F"), OsStr::new(if cfg!(windows) { "NUL" } else { "/dev/null" }),
        OsStr::new("-i"), key.as_os_str(),
        OsStr::new("-p"), OsStr::new(&port),
        OsStr::new("-o"), OsStr::new("BatchMode=yes"),
        OsStr::new("-o"), OsStr::new("IdentitiesOnly=yes"),
        OsStr::new("-o"), OsStr::new("ConnectTimeout=4"),
        OsStr::new("-o"), OsStr::new("StrictHostKeyChecking=accept-new"),
        OsStr::new("--"), OsStr::new(&target), OsStr::new("true"),
    ];
    run(&system_tool("ssh"), &args, PROBE_TIMEOUT).is_ok_and(|(ok, _)| ok)
}

fn write_config(path: &Path, alias: &str, block: &str) -> Result<(), String> {
    let old = fs::read_to_string(path).unwrap_or_default();
    let new = upsert_block(&old, alias, block);
    let tmp = path.with_extension("dial-tmp");
    fs::write(&tmp, new).map_err(|_| "Dial could not write ~/.ssh/config.".to_owned())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    }
    fs::rename(&tmp, path).map_err(|_| "Dial could not write ~/.ssh/config.".to_owned())
}

impl AppRunner {
    /// The whole **Set up SSH** flow for the connected VM run `run_id`.
    pub(crate) fn ssh_setup(
        &self,
        workspace: &str,
        run_id: &str,
        user: &str,
        open_terminal: bool,
    ) -> Result<SetupDone, AppRunError> {
        let fail = |m: &str| AppRunError::new("ssh_setup_failed", m);
        // Forget a cancel left from an earlier setup of this run.
        crate::app::ssh_setup_cancelled(run_id);
        if !valid_user(user) {
            return Err(fail("Enter the user name you log in with inside the VM (letters, digits, . _ -)."));
        }
        let vm_id = {
            let mut table = self.table();
            table.exact(workspace, run_id)?.vm_id.clone()
        }
        .ok_or_else(|| fail("Connect to a VM in App → VM first."))?;
        let vm = inventory::discover()
            .machines
            .into_iter()
            .find(|vm| vm.id == vm_id)
            .ok_or_else(|| fail("That VM is no longer in the provider inventory."))?;
        if vm.provider != Provider::VirtualBox {
            return Err(fail("Set up SSH supports VirtualBox VMs for now. For VMware, enable sshd in the guest and link a host from ~/.ssh/config."));
        }

        // 1. How the Mac reaches the guest.
        let (ok, info) = run(&vm.tool, &["showvminfo", vm.locator.as_str(), "--machinereadable"], TOOL_TIMEOUT)
            .map_err(|e| fail(&e))?;
        if !ok {
            return Err(fail("VirtualBox did not describe the VM."));
        }
        let mut reach = reach_from_vminfo(&info).map_err(|e| fail(&e))?;
        if let Reach::NatNeedsForward { nic } = reach {
            let port = free_port().ok_or_else(|| fail("No free local port for the SSH forward."))?;
            let rule = format!("dial-ssh,tcp,127.0.0.1,{port},,22");
            let (ok, _) = run(&vm.tool, &["controlvm", vm.locator.as_str(), &format!("natpf{nic}"), &rule], TOOL_TIMEOUT)
                .map_err(|e| fail(&e))?;
            if !ok {
                return Err(fail("VirtualBox would not add the SSH port forward to the VM's NAT adapter."));
            }
            reach = Reach::Forward { port };
        }

        // 2. Key and alias.
        let home = home().map_err(|e| fail(&e))?;
        let ssh_dir = home.join(".ssh");
        let config_path = ssh_dir.join("config");
        let config = fs::read_to_string(&config_path).unwrap_or_default();
        let alias = free_alias(&config, &alias_for(&vm.name));
        let (key, pub_key) = ensure_key(&ssh_dir, &alias).map_err(|e| fail(&e))?;

        // 3. The guest command, typed as the user's own input.
        // 3. The guest command, typed as the user's own input.
        let (ping, listener) = match reach {
            Reach::Lan { .. } => {
                let host = host_ip();
                (host, host.and_then(ReportListener::open))
            }
            _ => (None, None),
        };
        let command = guest_command(&pub_key, ping, listener.as_ref().map(|l| &l.report)).map_err(|e| fail(&e))?;
        let mut actions = Vec::new();
        if open_terminal {
            actions.push(Action::Press { keys: Some("ctrl+alt+t".into()), code: None, modifiers: Vec::new(), repeat: 1 });
            actions.push(Action::Wait { ms: 2500 });
        }
        actions.push(Action::Type { text: command });
        self.user_actions(workspace, run_id, &actions)?;

        // 4. Wait until SSH answers with the key, or the user cancels or disconnects.
        let deadline = Instant::now() + READY_TIMEOUT;
        let mut reported = None;
        let (address, port) = loop {
            if crate::app::ssh_setup_cancelled(run_id) || self.table().exact(workspace, run_id).is_err() {
                return Err(AppRunError::new(
                    "ssh_setup_cancelled",
                    "SSH setup was cancelled. The command may still finish in the VM; press Set up SSH again to link it.",
                ));
            }
            if reported.is_none() {
                reported = listener.as_ref().and_then(ReportListener::poll);
            }
            let target = match reach {
                Reach::Forward { port } => Some(("127.0.0.1".to_owned(), port)),
                Reach::Lan { mac } => reported
                    .or_else(|| ip_for_mac(&arp_table(), mac))
                    .map(|ip| (ip.to_string(), 22)),
                Reach::NatNeedsForward { .. } => None,
            };
            let found = target.is_some();
            if let Some((host, port)) = target
                && probe(&key, user, &host, port)
            {
                break (host, port);
            }
            if Instant::now() >= deadline {
                return Err(fail(match reach {
                    Reach::Lan { .. } if !found => "The VM did not report its network address. Check that the command finished in the VM's terminal (it prints DIAL-SSH-READY) and that the VM and this computer are on the same network, then try again.",
                    _ => "SSH did not accept the key in time. Check the VM's terminal for errors (a wrong sudo password, no network for apt, or a different user name), then try again.",
                }));
            }
            thread::sleep(POLL);
        };

        // 5. Config and link.
        let block = config_block(&alias, &address, port, user, &key);
        write_config(&config_path, &alias, &block).map_err(|e| fail(&e))?;
        ssh::save(&self.inner.data_dir, &vm_id, Some(&alias)).map_err(|e| fail(&e))?;
        Ok(SetupDone { host: alias, address, port, user: user.to_owned() })
    }
}
