//! Fixtures for the VMware console: a fake probe and vmrun, and a mock RFB server on loopback.
//! No VM is started and no real VNC endpoint is contacted.

use std::cell::Cell;
use std::collections::VecDeque;
use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::binding::{self, Listener, Probe, Proof};
use super::vmrun::{self, Runner};
use super::{Console, Waits, keys, vmx};
use crate::app_runner::input::{Gesture, Modifier, qcode_for};

const WAITS: Waits = Waits {
    running: Duration::from_millis(200),
    started: Duration::from_millis(200),
    poll: Duration::from_millis(5),
};

/// A VMware executable under a trusted install root, for the current platform.
fn trusted_exe() -> PathBuf {
    #[cfg(windows)]
    { super::super::inventory::vmware_install_dirs().next().unwrap().join("vmware-vmx.exe") }
    #[cfg(not(windows))]
    { PathBuf::from(if cfg!(target_os = "macos") {
        "/Applications/VMware Fusion.app/Contents/Library/vmware-vmx"
    } else {
        "/usr/lib/vmware/bin/vmware-vmx"
    }) }
}

/// A fake probe whose answers each test sets. `vm` describes a healthy VM owned by pid 7.
struct FakeProbe {
    supported: bool,
    listeners: Vec<Listener>,
    exe: Option<PathBuf>,
    open: Vec<String>,
    owner: Option<u32>,
    calls: Cell<u32>,
    appear_after: u32,
}

impl FakeProbe {
    fn vm(vmx: &str) -> Self {
        Self {
            supported: true,
            listeners: vec![Listener {
                pid: 7,
                host: "127.0.0.1".into(),
            }],
            exe: Some(trusted_exe()),
            open: vec![vmx.to_owned()],
            owner: Some(7),
            calls: Cell::new(0),
            appear_after: 0,
        }
    }
}

impl Probe for FakeProbe {
    fn supported(&self) -> Result<(), String> {
        if self.supported {
            Ok(())
        } else {
            Err("unsupported on this platform".into())
        }
    }

    fn listeners(&self, _port: u16) -> Result<Vec<Listener>, String> {
        let n = self.calls.get();
        self.calls.set(n + 1);
        Ok(if n < self.appear_after {
            Vec::new()
        } else {
            self.listeners.clone()
        })
    }

    fn executable(&self, _pid: u32) -> Result<Option<PathBuf>, String> {
        Ok(self.exe.clone())
    }

    fn has_open_file(&self, _pid: u32, path: &str) -> Result<bool, String> {
        Ok(self.open.iter().any(|open| open == path))
    }

    fn connection_owner(&self, _port: u16, _client: SocketAddr) -> Result<Option<u32>, String> {
        Ok(self.owner)
    }
}

#[derive(Default)]
struct FakeRunner {
    running: Vec<String>,
    start_error: Option<String>,
    started: Vec<String>,
}

impl Runner for FakeRunner {
    fn running(&mut self) -> Result<Vec<String>, String> {
        Ok(self.running.clone())
    }

    fn start_nogui(&mut self, vmx: &str) -> Result<(), String> {
        self.started.push(vmx.to_owned());
        match &self.start_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}

fn vmx_text(enabled: &str, port: &str, ip: Option<&str>) -> String {
    let mut text = format!(
        "displayName = \"a\"\nRemoteDisplay.vnc.enabled = \"{enabled}\"\nremotedisplay.vnc.port = \"{port}\"\n"
    );
    if let Some(ip) = ip {
        text.push_str(&format!("RemoteDisplay.VNC.IP = \"{ip}\"\n"));
    }
    text
}

/// A VM directory holding `a.vmx`; returns the guard and the `.vmx` path in the plain form the
/// console receives after `vmx::load` (verbatim `\\?\` prefix stripped on Windows).
fn write_vmx(text: &str) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.vmx");
    fs::write(&path, text).unwrap();
    let canonical = fs::canonicalize(&path).unwrap();
    (dir, vmx::normalize(&canonical).to_str().unwrap().to_owned())
}

// ---- VMX settings ----

#[test]
fn vmx_settings_table() {
    let cases: &[(&str, &str, bool)] = &[
        (
            "minimal loopback",
            "RemoteDisplay.vnc.enabled = \"TRUE\"\nRemoteDisplay.vnc.port = \"5901\"\nRemoteDisplay.vnc.ip = \"127.0.0.1\"\n",
            true,
        ),
        (
            "mixed case, unquoted, v6 loopback",
            "remotedisplay.VNC.Enabled = true\nremotedisplay.vnc.port = 5902\nremotedisplay.vnc.ip = \"::1\"\n",
            true,
        ),
        (
            "wildcard listen address",
            "RemoteDisplay.vnc.enabled = \"TRUE\"\nRemoteDisplay.vnc.port = \"5901\"\nRemoteDisplay.vnc.ip = \"0.0.0.0\"\n",
            false,
        ),
        (
            "LAN listen address",
            "RemoteDisplay.vnc.enabled = \"TRUE\"\nRemoteDisplay.vnc.port = \"5901\"\nRemoteDisplay.vnc.ip = \"192.168.1.5\"\n",
            false,
        ),
        (
            "hostname is not proof",
            "RemoteDisplay.vnc.enabled = \"TRUE\"\nRemoteDisplay.vnc.port = \"5901\"\nRemoteDisplay.vnc.ip = \"localhost\"\n",
            false,
        ),
        (
            "listen address missing",
            "RemoteDisplay.vnc.enabled = \"TRUE\"\nRemoteDisplay.vnc.port = \"5901\"\n",
            false,
        ),
        (
            "disabled",
            "RemoteDisplay.vnc.enabled = \"FALSE\"\nRemoteDisplay.vnc.port = \"5901\"\nRemoteDisplay.vnc.ip = \"127.0.0.1\"\n",
            false,
        ),
        (
            "port zero",
            "RemoteDisplay.vnc.enabled = \"TRUE\"\nRemoteDisplay.vnc.port = \"0\"\nRemoteDisplay.vnc.ip = \"127.0.0.1\"\n",
            false,
        ),
        (
            "port out of range",
            "RemoteDisplay.vnc.enabled = \"TRUE\"\nRemoteDisplay.vnc.port = \"70000\"\nRemoteDisplay.vnc.ip = \"127.0.0.1\"\n",
            false,
        ),
        (
            "duplicate ip",
            "RemoteDisplay.vnc.enabled = \"TRUE\"\nRemoteDisplay.vnc.port = \"5901\"\nRemoteDisplay.vnc.ip = \"127.0.0.1\"\nremotedisplay.vnc.ip = \"0.0.0.0\"\n",
            false,
        ),
    ];
    for (name, text, ok) in cases {
        assert_eq!(vmx::settings(text).is_ok(), *ok, "case: {name}");
    }
}

#[test]
fn vmx_wildcard_refusal_names_loopback() {
    let text = vmx_text("TRUE", "5901", Some("0.0.0.0"));
    let error = vmx::settings(&text).unwrap_err();
    assert!(error.contains("loopback"), "{error}");
}

#[test]
fn vmx_load_refuses_non_canonical_and_wrong_files() {
    let (dir, canonical) = write_vmx(&vmx_text("TRUE", "5901", Some("127.0.0.1")));
    let parent = Path::new(&canonical)
        .parent()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let name = Path::new(&canonical)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let detour = format!(
        "{parent}/../{}/{name}",
        Path::new(&parent).file_name().unwrap().to_str().unwrap()
    );
    let other = dir.path().join("b.txt");
    fs::write(&other, "x").unwrap();
    let cases = [
        ("relative", "a.vmx".to_owned()),
        ("detour not canonical", detour),
        ("wrong extension", other.to_str().unwrap().to_owned()),
        ("missing", format!("{parent}/missing.vmx")),
    ];
    for (name, path) in cases {
        assert!(vmx::load(Path::new(&path)).is_err(), "case: {name}");
    }
    let (loaded, vnc) = vmx::load(Path::new(&canonical)).unwrap();
    assert!(vmx::same_location(&loaded, Path::new(&canonical)));
    assert_eq!(vnc.port, 5901);
    let ipv6 = vmx::settings(&vmx_text("TRUE", "5901", Some("::1"))).unwrap();
    assert_eq!(ipv6.ip, std::net::Ipv6Addr::LOCALHOST);
}

// ---- vmrun tool ----

#[test]
fn vmrun_kind_and_tool_checks() {
    assert_eq!(
        vmrun::kind_for(Path::new(
            "/Applications/VMware Fusion.app/Contents/Library/vmrun"
        )),
        "fusion"
    );
    assert_eq!(
        vmrun::kind_for(Path::new("/Applications/VMWARE FUSION.app/vmrun")),
        "fusion"
    );
    assert_eq!(
        vmrun::kind_for(Path::new(
            r"C:\Program Files (x86)\VMware\VMware Workstation\vmrun.exe"
        )),
        "ws"
    );
    assert!(vmrun::Tool::new(Path::new("vmrun")).is_err());
    assert!(vmrun::Tool::new(Path::new("/definitely/missing/vmrun")).is_err());
}

#[cfg(unix)]
fn script(dir: &Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("vmrun");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
#[test]
fn vmrun_errors_are_sanitized() {
    let dir = tempfile::tempdir().unwrap();
    let tool = script(
        dir.path(),
        r#"if [ "$3" = list ]; then echo "Total running VMs: 1"; echo "/vm/a.vmx"; exit 0; fi
echo "secret-leak /Users/someone/private.vmx"; exit 3"#,
    );
    let mut runner = vmrun::Tool::new(&tool).unwrap();
    assert_eq!(runner.running().unwrap(), vec!["/vm/a.vmx".to_owned()]);
    let error = runner.start_nogui("/vm/a.vmx").unwrap_err();
    assert!(error.contains("exit code 3"), "{error}");
    assert!(
        !error.contains("secret-leak") && !error.contains("private.vmx"),
        "{error}"
    );
}

// ---- process ownership ----

#[test]
fn owns_vm_matches_exact_executable_and_exact_vmx() {
    let vmx = "/vm/a.vmx";
    let trusted = trusted_exe();
    let cases: Vec<(&str, Option<PathBuf>, Vec<&str>, bool)> = vec![
        (
            "trusted exe, exact vmx open",
            Some(trusted.clone()),
            vec![vmx],
            true,
        ),
        (
            "trusted exe, sibling backup open",
            Some(trusted.clone()),
            vec!["/vm/a.vmx.bak"],
            false,
        ),
        (
            "trusted exe, vmx only as a substring of another path",
            Some(trusted.clone()),
            vec!["/other/vm/a.vmx"],
            false,
        ),
        (
            "trusted exe, nothing open",
            Some(trusted.clone()),
            vec![],
            false,
        ),
        (
            "executable named as a prefix",
            Some(trusted.with_file_name("vmware-vmx-helper")),
            vec![vmx],
            false,
        ),
        (
            "right name, untrusted root",
            Some(PathBuf::from("/tmp/evil/vmware-vmx")),
            vec![vmx],
            false,
        ),
        (
            "unrelated program",
            Some(PathBuf::from("/usr/bin/python3")),
            vec![vmx],
            false,
        ),
        ("executable unreadable", None, vec![vmx], false),
    ];
    for (name, exe, open, expected) in cases {
        let probe = FakeProbe {
            exe,
            open: open.iter().map(|s| (*s).to_owned()).collect(),
            ..FakeProbe::vm(vmx)
        };
        assert_eq!(
            binding::owns_vm(&probe, 7, vmx).unwrap(),
            expected,
            "case: {name}"
        );
    }
}

#[test]
fn verify_listener_table() {
    let vmx = "/vm/a.vmx";
    let owned = |host: &str, pid: u32| Listener {
        pid,
        host: host.into(),
    };
    let cases: Vec<(&str, Vec<Listener>, bool)> = vec![
        ("no listener", vec![], false),
        ("loopback owned", vec![owned("127.0.0.1", 7)], true),
        ("v6 loopback owned", vec![owned("[::1]", 7)], true),
        ("wildcard listener", vec![owned("*", 7)], false),
        ("LAN listener", vec![owned("192.168.1.5", 7)], false),
        (
            "two processes",
            vec![owned("127.0.0.1", 7), owned("127.0.0.1", 8)],
            false,
        ),
    ];
    for (name, listeners, expect_bound) in cases {
        let probe = FakeProbe {
            listeners,
            ..FakeProbe::vm(vmx)
        };
        let result = binding::verify(&probe, 5901, vmx);
        match (expect_bound, result) {
            (true, Ok(Proof::Bound { pid: 7 })) => {}
            (false, Ok(Proof::NotListening)) if name == "no listener" => {}
            (false, Err(_)) => {}
            (expected, other) => panic!("case {name}: expected bound={expected}, got {other:?}"),
        }
    }
}

#[test]
fn connection_owner_must_match_proven_pid() {
    let client = SocketAddr::from((Ipv4Addr::LOCALHOST, 53000));
    let probe = FakeProbe {
        owner: Some(8),
        ..FakeProbe::vm("/vm/a.vmx")
    };
    assert!(binding::check_connection(&probe, 5901, client, 7).is_err());
    let probe = FakeProbe {
        owner: None,
        ..FakeProbe::vm("/vm/a.vmx")
    };
    assert!(binding::check_connection(&probe, 5901, client, 7).is_err());
    let probe = FakeProbe {
        owner: Some(7),
        ..FakeProbe::vm("/vm/a.vmx")
    };
    assert!(binding::check_connection(&probe, 5901, client, 7).is_ok());
}

#[cfg(unix)]
#[test]
fn lsof_parsing_and_connection_owner() {
    let text = "p7\nn127.0.0.1:5901\np8\nn127.0.0.1:5901->127.0.0.1:53000\n";
    let sockets = binding::parse_sockets(text);
    assert_eq!(sockets.len(), 2);
    let client = SocketAddr::from((Ipv4Addr::LOCALHOST, 53000));
    assert_eq!(binding::owner_of(&sockets, 5901, client), Some(8));
    let other = SocketAddr::from((Ipv4Addr::LOCALHOST, 53001));
    assert_eq!(binding::owner_of(&sockets, 5901, other), None);
    // Two different owners claiming the same connection is ambiguous, so it is refused.
    let ambiguous = "p8\nn127.0.0.1:5901->127.0.0.1:53000\np9\nn127.0.0.1:5901->127.0.0.1:53000\n";
    assert_eq!(
        binding::owner_of(&binding::parse_sockets(ambiguous), 5901, client),
        None
    );
}

#[test]
fn wait_for_binding_polls_then_gives_up() {
    let vmx = "/vm/a.vmx";
    let late = FakeProbe {
        appear_after: 2,
        ..FakeProbe::vm(vmx)
    };
    assert_eq!(
        binding::wait_for_binding(
            &late,
            5901,
            vmx,
            Duration::from_secs(2),
            Duration::from_millis(1)
        ),
        Ok(7)
    );
    let never = FakeProbe {
        listeners: Vec::new(),
        ..FakeProbe::vm(vmx)
    };
    assert!(
        binding::wait_for_binding(&never, 5901, vmx, Duration::ZERO, Duration::from_millis(1))
            .is_err()
    );
}

#[test]
fn trusted_root_is_checked_by_name_and_root() {
    let cases = [
        ("exact name under root", trusted_exe(), true),
        (
            "bak suffix",
            trusted_exe().with_file_name("vmware-vmx.bak"),
            false,
        ),
        (
            "prefixed name",
            trusted_exe().with_file_name("xvmware-vmx"),
            false,
        ),
    ];
    for (name, path, expected) in cases {
        assert_eq!(
            binding::is_trusted_vmx_executable(&path),
            expected,
            "case: {name}"
        );
    }
}

// ---- open refusals before any launch ----

#[test]
fn open_refuses_before_starting_anything() {
    let (_dir, vmx) = write_vmx(&vmx_text("TRUE", "5901", Some("0.0.0.0")));
    let mut runner = FakeRunner::default();
    let probe = FakeProbe::vm(&vmx);
    assert!(Console::open_with(&vmx, &mut runner, &probe, WAITS).is_err());
    assert!(
        runner.started.is_empty(),
        "wildcard VNC must not start the VM"
    );

    let (_dir, vmx) = write_vmx(&vmx_text("TRUE", "5901", Some("127.0.0.1")));
    let probe = FakeProbe {
        supported: false,
        ..FakeProbe::vm(&vmx)
    };
    assert!(Console::open_with(&vmx, &mut runner, &probe, WAITS).is_err());
    assert!(
        runner.started.is_empty(),
        "unsupported host must not start the VM"
    );

    let occupied = FakeProbe::vm(&vmx);
    assert!(Console::open_with(&vmx, &mut runner, &occupied, WAITS).is_err());
    assert!(
        runner.started.is_empty(),
        "occupied port must not start the VM"
    );
}

#[test]
fn stopped_vm_starts_once_then_uses_its_proven_console() {
    let m = mock(Plan::default());
    let (_dir, vmx) = write_vmx(&vmx_text("TRUE", &m.port.to_string(), Some("127.0.0.1")));
    let mut runner = FakeRunner::default();
    let probe = FakeProbe {
        appear_after: 1,
        ..FakeProbe::vm(&vmx)
    };
    let console = Console::open_with(&vmx, &mut runner, &probe, WAITS).unwrap();
    drop(console);
    assert_eq!(runner.started, [vmx]);
}

// ---- mock RFB server ----

#[derive(Debug, Clone, PartialEq, Eq)]
enum Sent {
    PixelFormat,
    Encodings(Vec<i32>),
    Request {
        incremental: bool,
        width: u16,
        height: u16,
    },
    Key {
        down: bool,
        keysym: u32,
    },
    Pointer {
        buttons: u8,
        x: u16,
        y: u16,
    },
}

struct Plan {
    version: &'static str,
    security: Vec<u8>,
    refusal: Option<String>,
    width: u16,
    height: u16,
    updates: Vec<Vec<u8>>,
    /// Bytes sent right after the handshake, before any update (used for cut-text fixtures).
    preamble: Vec<u8>,
}

impl Default for Plan {
    fn default() -> Self {
        Self {
            version: "RFB 003.008\n",
            security: vec![1],
            refusal: None,
            width: 4,
            height: 2,
            updates: Vec::new(),
            preamble: Vec::new(),
        }
    }
}

enum Rect {
    Raw {
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        data: Vec<u8>,
    },
    Desktop {
        w: u16,
        h: u16,
    },
}

fn update(rects: &[Rect]) -> Vec<u8> {
    let mut msg = vec![0, 0];
    msg.extend_from_slice(&(rects.len() as u16).to_be_bytes());
    for rect in rects {
        match rect {
            Rect::Raw { x, y, w, h, data } => {
                for v in [x, y, w, h] {
                    msg.extend_from_slice(&v.to_be_bytes());
                }
                msg.extend_from_slice(&0i32.to_be_bytes());
                msg.extend_from_slice(data);
            }
            Rect::Desktop { w, h } => {
                msg.extend_from_slice(&[0, 0, 0, 0]);
                msg.extend_from_slice(&w.to_be_bytes());
                msg.extend_from_slice(&h.to_be_bytes());
                msg.extend_from_slice(&(-223i32).to_be_bytes());
            }
        }
    }
    msg
}

/// A `w`x`h` BGRX screen filled with one colour.
fn solid(w: u16, h: u16, b: u8, g: u8, r: u8) -> Vec<u8> {
    (0..usize::from(w) * usize::from(h))
        .flat_map(|_| [b, g, r, 0])
        .collect()
}

fn serve(listener: TcpListener, plan: Plan, seen: Arc<Mutex<Vec<Sent>>>) {
    let Ok((mut s, _)) = listener.accept() else {
        return;
    };
    let _ = s.write_all(plan.version.as_bytes());
    let mut client_version = [0u8; 12];
    if s.read_exact(&mut client_version).is_err() {
        return;
    }
    if plan.version == "RFB 003.003\n" {
        let kind = plan.security.first().copied().unwrap_or(0);
        let _ = s.write_all(&u32::from(kind).to_be_bytes());
        if kind != 1 {
            return;
        }
    } else {
        let _ = s.write_all(&[plan.security.len() as u8]);
        let _ = s.write_all(&plan.security);
        if plan.security.is_empty() || !plan.security.contains(&1) {
            return;
        }
        let mut chosen = [0u8; 1];
        if s.read_exact(&mut chosen).is_err() {
            return;
        }
        let result: u32 = u32::from(plan.refusal.is_some());
        let _ = s.write_all(&result.to_be_bytes());
        if let Some(reason) = &plan.refusal {
            let _ = s.write_all(&(reason.len() as u32).to_be_bytes());
            let _ = s.write_all(reason.as_bytes());
            return;
        }
    }
    let mut client_init = [0u8; 1];
    if s.read_exact(&mut client_init).is_err() {
        return;
    }
    let mut init = Vec::new();
    init.extend_from_slice(&plan.width.to_be_bytes());
    init.extend_from_slice(&plan.height.to_be_bytes());
    init.extend_from_slice(&[32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0]);
    init.extend_from_slice(&4u32.to_be_bytes());
    init.extend_from_slice(b"mock");
    let _ = s.write_all(&init);
    let _ = s.write_all(&plan.preamble);
    let mut updates: VecDeque<Vec<u8>> = plan.updates.into();
    loop {
        let mut kind = [0u8; 1];
        if s.read_exact(&mut kind).is_err() {
            return;
        }
        let ok = match kind[0] {
            0 => s
                .read_exact(&mut [0u8; 19])
                .map(|_| seen.lock().unwrap().push(Sent::PixelFormat)),
            2 => {
                let mut head = [0u8; 3];
                s.read_exact(&mut head).and_then(|_| {
                    let mut e = vec![0u8; usize::from(u16::from_be_bytes([head[1], head[2]])) * 4];
                    s.read_exact(&mut e)?;
                    let list = e
                        .chunks(4)
                        .map(|c| i32::from_be_bytes([c[0], c[1], c[2], c[3]]))
                        .collect();
                    seen.lock().unwrap().push(Sent::Encodings(list));
                    Ok(())
                })
            }
            3 => {
                let mut b = [0u8; 9];
                s.read_exact(&mut b).map(|_| {
                    seen.lock().unwrap().push(Sent::Request {
                        incremental: b[0] != 0,
                        width: u16::from_be_bytes([b[5], b[6]]),
                        height: u16::from_be_bytes([b[7], b[8]]),
                    });
                    if let Some(msg) = updates.pop_front() {
                        let _ = s.write_all(&msg);
                    }
                })
            }
            4 => {
                let mut b = [0u8; 7];
                s.read_exact(&mut b).map(|_| {
                    seen.lock().unwrap().push(Sent::Key {
                        down: b[0] != 0,
                        keysym: u32::from_be_bytes([b[3], b[4], b[5], b[6]]),
                    })
                })
            }
            5 => {
                let mut b = [0u8; 5];
                s.read_exact(&mut b).map(|_| {
                    seen.lock().unwrap().push(Sent::Pointer {
                        buttons: b[0],
                        x: u16::from_be_bytes([b[1], b[2]]),
                        y: u16::from_be_bytes([b[3], b[4]]),
                    })
                })
            }
            _ => return,
        };
        if ok.is_err() {
            return;
        }
    }
}

struct Mock {
    port: u16,
    seen: Arc<Mutex<Vec<Sent>>>,
}

fn mock(plan: Plan) -> Mock {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let thread_seen = Arc::clone(&seen);
    std::thread::spawn(move || serve(listener, plan, thread_seen));
    Mock { port, seen }
}

/// Open a console against `mock`, with its VMX and a healthy fake probe.
fn open_mock(mock: &Mock) -> (Result<Console, String>, FakeRunner, tempfile::TempDir) {
    let (dir, vmx) = write_vmx(&vmx_text("TRUE", &mock.port.to_string(), Some("127.0.0.1")));
    let mut runner = FakeRunner {
        running: vec![vmx.clone()],
        ..FakeRunner::default()
    };
    let probe = FakeProbe::vm(&vmx);
    let result = Console::open_with(&vmx, &mut runner, &probe, WAITS);
    (result, runner, dir)
}

fn wait_seen(seen: &Arc<Mutex<Vec<Sent>>>, count: usize) -> Vec<Sent> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let now = seen.lock().unwrap().clone();
        if now.len() >= count || Instant::now() > deadline {
            return now;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn png_pixels(frame: &crate::app_runner::input::Frame) -> (u32, u32, Vec<u8>) {
    (frame.width, frame.height, frame.rgba.to_vec())
}

#[test]
fn handshake_accepts_none_and_refuses_the_rest() {
    let good: Vec<(&str, Plan)> = vec![
        ("3.8 none", Plan::default()),
        (
            "3.7 none",
            Plan {
                version: "RFB 003.007\n",
                ..Plan::default()
            },
        ),
        (
            "3.3 none",
            Plan {
                version: "RFB 003.003\n",
                security: vec![1],
                ..Plan::default()
            },
        ),
    ];
    for (name, plan) in good {
        let m = mock(plan);
        let (result, _, _dir) = open_mock(&m);
        assert!(result.is_ok(), "case {name}: {:?}", result.err());
    }
    let refused: Vec<(&str, Plan, &str)> = vec![
        (
            "password only",
            Plan {
                security: vec![2],
                ..Plan::default()
            },
            "password authentication",
        ),
        (
            "3.3 password",
            Plan {
                version: "RFB 003.003\n",
                security: vec![2],
                ..Plan::default()
            },
            "password authentication",
        ),
        (
            "no security types",
            Plan {
                security: vec![],
                ..Plan::default()
            },
            "",
        ),
        (
            "server refusal",
            Plan {
                refusal: Some("denied by policy".into()),
                ..Plan::default()
            },
            "denied by policy",
        ),
    ];
    for (name, plan, needle) in refused {
        let m = mock(plan);
        let (result, _, _dir) = open_mock(&m);
        let error = result
            .err()
            .unwrap_or_else(|| panic!("case {name} connected"));
        assert!(error.contains(needle), "case {name}: {error}");
        assert!(
            !error.contains("remove"),
            "case {name} gives removal advice: {error}"
        );
    }
}

#[test]
fn frame_decodes_bgrx_to_rgba_and_reuses_idle_frames() {
    let plan = Plan {
        updates: vec![update(&[Rect::Raw {
            x: 0,
            y: 0,
            w: 4,
            h: 2,
            data: solid(4, 2, 10, 20, 200),
        }])],
        ..Plan::default()
    };
    let m = mock(plan);
    let (result, _, _dir) = open_mock(&m);
    let mut console = result.unwrap();
    let first = console.frame().unwrap();
    let (w, h, pixels) = png_pixels(&first);
    assert_eq!((w, h), (4, 2));
    assert_eq!(&pixels[..4], &[200, 20, 10, 255], "BGRX must decode to RGBA");
    // No further update: the request is incremental and the very same frame comes back.
    let again = console.frame().unwrap();
    assert!(std::sync::Arc::ptr_eq(&again.rgba, &first.rgba));
    let seen = wait_seen(&m.seen, 4);
    assert!(seen.contains(&Sent::Request {
        incremental: false,
        width: 4,
        height: 2
    }));
    assert!(seen.contains(&Sent::Request {
        incremental: true,
        width: 4,
        height: 2
    }));
}

#[test]
fn cut_text_is_discarded_with_its_padding() {
    // Type 3, three padding bytes, length 5, then the text; the screen that follows must still decode.
    let mut preamble = vec![3, 0, 0, 0, 0, 0, 0, 5];
    preamble.extend_from_slice(b"clip!");
    let plan = Plan {
        preamble,
        updates: vec![update(&[Rect::Raw {
            x: 0,
            y: 0,
            w: 4,
            h: 2,
            data: solid(4, 2, 1, 2, 3),
        }])],
        ..Plan::default()
    };
    let m = mock(plan);
    let (result, _, _dir) = open_mock(&m);
    let mut console = result.unwrap();
    let frame = console.frame().unwrap();
    assert_eq!(png_pixels(&frame).2[..3], [3, 2, 1]);
}

#[test]
fn spoofed_rectangles_hit_the_update_budget_before_allocating() {
    // Two full-screen-half rects use the whole 64 MiB per-update budget; the 1x1 rect after them is
    // refused from its header, before any buffer is allocated for it.
    let half = || Rect::Raw {
        x: 0,
        y: 0,
        w: 4096,
        h: 2048,
        data: vec![0; 4096 * 2048 * 4],
    };
    let plan = Plan {
        width: 4096,
        height: 4096,
        updates: vec![update(&[
            half(),
            half(),
            Rect::Raw {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
                data: vec![0; 4],
            },
        ])],
        ..Plan::default()
    };
    let m = mock(plan);
    let (result, _, _dir) = open_mock(&m);
    let mut console = result.unwrap();
    let error = console.frame().expect_err("the extra rect must be refused");
    assert!(error.contains("more screen data"), "{error}");
}

#[test]
fn desktop_resize_reallocates_and_asks_for_a_full_update() {
    let plan = Plan {
        updates: vec![
            update(&[Rect::Raw {
                x: 0,
                y: 0,
                w: 4,
                h: 2,
                data: solid(4, 2, 0, 0, 0),
            }]),
            update(&[
                Rect::Desktop { w: 2, h: 1 },
                Rect::Raw {
                    x: 0,
                    y: 0,
                    w: 2,
                    h: 1,
                    data: solid(2, 1, 9, 9, 9),
                },
            ]),
        ],
        ..Plan::default()
    };
    let m = mock(plan);
    let (result, _, _dir) = open_mock(&m);
    let mut console = result.unwrap();
    console.frame().unwrap();
    // The second update answers the request sent with the first frame; wait for it.
    let resized = console.frame_within(Duration::from_secs(2)).unwrap();
    assert_eq!((resized.width, resized.height), (2, 1));
    console.frame_within(Duration::from_millis(50)).unwrap();
    let seen = wait_seen(&m.seen, 4);
    let requests: Vec<bool> = seen
        .iter()
        .filter_map(|s| match s {
            Sent::Request { incremental, .. } => Some(*incremental),
            _ => None,
        })
        .collect();
    assert_eq!(requests.first(), Some(&false));
    assert!(
        requests.contains(&false),
        "a resize must request the whole screen again: {requests:?}"
    );
}

#[test]
fn unsupported_encoding_and_out_of_range_rect_are_refused() {
    let mut bad_encoding = update(&[Rect::Raw {
        x: 0,
        y: 0,
        w: 1,
        h: 1,
        data: vec![0; 4],
    }]);
    let len = bad_encoding.len();
    // The encoding field sits just before the 4 pixel bytes at the end of the one rect.
    bad_encoding[len - 8..len - 4].copy_from_slice(&5i32.to_be_bytes());
    let out_of_range = update(&[Rect::Raw {
        x: 3,
        y: 0,
        w: 2,
        h: 1,
        data: vec![0; 8],
    }]);
    for (name, msg) in [
        ("encoding 5", bad_encoding),
        ("rect outside screen", out_of_range),
    ] {
        let m = mock(Plan {
            updates: vec![msg],
            ..Plan::default()
        });
        let (result, _, _dir) = open_mock(&m);
        let mut console = result.unwrap();
        assert!(console.frame().is_err(), "case {name} was accepted");
    }
}

#[test]
fn input_maps_clicks_and_keys_and_refuses_bad_gestures_without_sending() {
    let m = mock(Plan {
        updates: vec![update(&[Rect::Raw {
            x: 0,
            y: 0,
            w: 4,
            h: 2,
            data: solid(4, 2, 0, 0, 0),
        }])],
        ..Plan::default()
    });
    let (result, _, _dir) = open_mock(&m);
    let mut console = result.unwrap();
    console.frame().unwrap();
    let click = Gesture::Click {
        x: 3,
        y: 1,
        button: "left".into(),
    };
    console.input(&click, 4, 2).unwrap();
    console
        .input(
            &Gesture::Press {
                code: "KeyA".into(),
                modifiers: vec![Modifier::Shift],
            },
            4,
            2,
        )
        .unwrap();
    // Refusals: nothing may be sent for any of them.
    let before = wait_seen(&m.seen, 0).len();
    assert!(
        console
            .input(
                &Gesture::Click {
                    x: 4,
                    y: 0,
                    button: "left".into()
                },
                4,
                2
            )
            .is_err()
    );
    assert!(
        console
            .input(
                &Gesture::Click {
                    x: 0,
                    y: 0,
                    button: "back".into()
                },
                4,
                2
            )
            .is_err()
    );
    assert!(
        console
            .input(
                &Gesture::Press {
                    code: "Fn".into(),
                    modifiers: vec![]
                },
                4,
                2
            )
            .is_err()
    );
    assert!(
        console
            .input(
                &Gesture::Press {
                    code: "KeyA".into(),
                    modifiers: vec![Modifier::Shift, Modifier::Shift]
                },
                4,
                2
            )
            .is_err()
    );
    assert!(console.input(&click, 0, 0).is_err());
    let seen = wait_seen(&m.seen, before + 6);
    let input: Vec<Sent> = seen
        .into_iter()
        .filter(|s| matches!(s, Sent::Pointer { .. } | Sent::Key { .. }))
        .collect();
    assert_eq!(
        input,
        vec![
            Sent::Pointer {
                buttons: 1,
                x: 3,
                y: 1
            },
            Sent::Pointer {
                buttons: 0,
                x: 3,
                y: 1
            },
            Sent::Key {
                down: true,
                keysym: 0xffe1
            },
            Sent::Key {
                down: true,
                keysym: 0x61
            },
            Sent::Key {
                down: false,
                keysym: 0x61
            },
            Sent::Key {
                down: false,
                keysym: 0xffe1
            },
        ]
    );
}

#[test]
fn qcode_keysyms_cover_every_mapped_physical_key() {
    let table: &[(&str, u32)] = &[
        ("KeyA", 0x61),
        ("Digit5", 0x35),
        ("F12", 0xffc9),
        ("Enter", 0xff0d),
        ("ShiftLeft", 0xffe1),
        ("ControlRight", 0xffe4),
        ("ArrowUp", 0xff52),
        ("Backquote", 0x60),
        ("MetaLeft", 0xffeb),
    ];
    for (code, expected) in table {
        let qcode = qcode_for(code).unwrap_or_else(|| panic!("no qcode for {code}"));
        assert_eq!(keys::keysym(qcode).unwrap(), *expected, "case {code}");
    }
}

#[test]
fn dom_buttons_map_to_rfb_bits() {
    assert_eq!(super::rfb_buttons(1), 1);
    assert_eq!(super::rfb_buttons(2), 4);
    assert_eq!(super::rfb_buttons(4), 2);
    assert_eq!(super::rfb_buttons(7), 7);
    assert_eq!(super::rfb_buttons(0), 0);
}
