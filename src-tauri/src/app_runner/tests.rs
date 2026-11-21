use super::input::{Frame, Gesture, frame_from_ppm, qcode_for};
use super::qemu::{Accel, Launch, args};
use super::qmp::Qmp;
use super::runtime;
use super::runtime::{
    Arch, GUEST_IMAGE, GuestTarget, Manifest, RuntimeError, discover, discover_for,
    guest_disk_path, guest_disk_path_for, host_target,
};
use super::{
    CPUS, Channel, Controller, MEMORY_MB, Origin, RunInfo, Status, capture, execute, kill,
    reveal_payload, status_payload,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A resource dir with a manifest for `target` describing the given files (path → bytes), and a
/// guest disk in the data dir. Returns (resource dir, data dir).
fn fixture(target: &str, files: &[(&str, &[u8])]) -> (tempfile::TempDir, tempfile::TempDir) {
    let resource = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let qemu = resource.path().join("qemu");
    fs::create_dir_all(qemu.join("bin")).unwrap();
    fs::create_dir_all(qemu.join("share")).unwrap();
    let mut entries = Vec::new();
    for (path, bytes) in files {
        fs::write(qemu.join(path), bytes).unwrap();
        entries.push(json!({
            "path": path, "sha256": sha_hex(bytes), "size": bytes.len(), "executable": path.starts_with("bin/")
        }));
    }
    let manifest = json!({ "schema": 1, "target": target, "binary": files[0].0, "files": entries });
    fs::write(qemu.join("runtime.json"), manifest.to_string()).unwrap();
    let disk = data.path().join("app-runner");
    fs::create_dir_all(&disk).unwrap();
    fs::write(disk.join("guest.raw"), b"disk").unwrap();
    (resource, data)
}

fn good_files(arch: Arch) -> Vec<(String, Vec<u8>)> {
    vec![
        (
            format!("bin/{}", super::runtime::exe_name(arch.qemu_name())),
            b"qemu".to_vec(),
        ),
        (format!("share/{}", arch.firmware_code()), b"code".to_vec()),
        (format!("share/{}", arch.firmware_vars()), b"vars".to_vec()),
    ]
}

fn as_refs(files: &[(String, Vec<u8>)]) -> Vec<(&str, &[u8])> {
    files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect()
}

#[test]
fn a_verified_runtime_is_found_and_a_missing_part_is_named() {
    let target = "aarch64-apple-darwin";
    let files = good_files(Arch::Aarch64);
    let (resource, data) = fixture(target, &as_refs(&files));
    let runtime = discover(resource.path(), data.path(), Arch::Aarch64, target).unwrap();
    assert!(runtime.qemu.is_file());
    assert!(
        runtime.qemu.ends_with("bin/qemu-system-aarch64")
            || runtime.qemu.ends_with("bin\\qemu-system-aarch64.exe")
    );

    // No manifest: the runtime is not packaged at all.
    let empty = tempfile::tempdir().unwrap();
    assert_eq!(
        discover(empty.path(), data.path(), Arch::Aarch64, target),
        Err(RuntimeError::Missing("qemu"))
    );
    // No guest disk imported yet.
    fs::remove_file(data.path().join("app-runner").join("guest.raw")).unwrap();
    assert_eq!(
        discover(resource.path(), data.path(), Arch::Aarch64, target),
        Err(RuntimeError::Missing("guest-image"))
    );
}

#[test]
fn a_runtime_for_another_target_or_a_changed_file_is_refused() {
    let files = good_files(Arch::X86_64);
    let (resource, data) = fixture("x86_64-unknown-linux-gnu", &as_refs(&files));
    assert!(matches!(
        discover(resource.path(), data.path(), Arch::X86_64, "aarch64-apple-darwin"),
        Err(RuntimeError::Refused(reason)) if reason.contains("x86_64-unknown-linux-gnu")
    ));

    // Same target, but the binary no longer matches its recorded sha256.
    let qemu_rel = format!("bin/{}", super::runtime::exe_name("qemu-system-x86_64"));
    fs::write(resource.path().join("qemu").join(&qemu_rel), b"tampered!").unwrap();
    assert!(matches!(
        discover(
            resource.path(),
            data.path(),
            Arch::X86_64,
            "x86_64-unknown-linux-gnu"
        ),
        Err(RuntimeError::Refused(_))
    ));
}

#[test]
fn undeclared_runtime_library_is_refused() {
    let files = good_files(Arch::X86_64);
    let (res, data) = fixture("x86_64-pc-windows-msvc", &as_refs(&files));
    fs::write(res.path().join("qemu/bin/extra.dll"), b"extra").unwrap();
    assert!(
        matches!(discover(res.path(), data.path(), Arch::X86_64, "x86_64-pc-windows-msvc"), Err(RuntimeError::Refused(reason)) if reason.contains("undeclared"))
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_binary_is_refused() {
    let files = good_files(Arch::X86_64);
    let (resource, data) = fixture("x86_64-unknown-linux-gnu", &as_refs(&files));
    let bin = resource.path().join("qemu/bin");
    let real = resource.path().join("outside-bundle");
    fs::rename(bin.join("qemu-system-x86_64"), &real).unwrap();
    std::os::unix::fs::symlink(&real, bin.join("qemu-system-x86_64")).unwrap();
    let error = discover(
        resource.path(),
        data.path(),
        Arch::X86_64,
        "x86_64-unknown-linux-gnu",
    )
    .unwrap_err();
    assert!(matches!(error, RuntimeError::Refused(reason) if reason.contains("symlink")));
}

#[test]
fn the_manifest_must_be_schema_one() {
    let text = r#"{"schema":2,"target":"x","binary":"bin/qemu","files":[]}"#;
    let manifest = Manifest::parse(text).unwrap();
    assert_eq!(manifest.schema, 2);
    assert!(Manifest::parse("not json").is_err());
}

#[test]
fn qemu_runs_with_no_defaults_no_network_and_disposable_disk() {
    let files = good_files(Arch::Aarch64);
    let (resource, data) = fixture("aarch64-apple-darwin", &as_refs(&files));
    let runtime = discover(
        resource.path(),
        data.path(),
        Arch::Aarch64,
        "aarch64-apple-darwin",
    )
    .unwrap();
    let vars = data.path().join("vars.fd");
    let share = runtime.firmware_code.parent().unwrap().to_path_buf();
    let argv = args(&Launch {
        runtime: &runtime,
        vars_copy: &vars,
        share_dir: &share,
        accel: Accel {
            name: "tcg",
            cpu: "cortex-a72",
        },
        share_iso: None,
        vnc_socket: None,
        memory_mb: 2048,
        cpus: 2,
    });
    let has = |flag: &str| argv.iter().any(|a| a == flag);
    for flag in [
        "-nodefaults",
        "-no-user-config",
        "-snapshot",
        "-no-reboot",
        "-nic",
        "-display",
    ] {
        assert!(has(flag), "missing {flag}");
    }
    assert!(argv.windows(2).any(|w| w == ["-cpu", "cortex-a72"]));
    assert!(
        argv.windows(2)
            .any(|w| w == ["-L", share.to_str().unwrap()])
    );
    assert!(argv.iter().any(|a| a.contains("virt,accel=tcg")));
    // Nothing that reaches the host, and no per-OS process option.
    for banned in [
        "-netdev",
        "-virtfs",
        "-spice",
        "-audiodev",
        "-serial",
        "-parallel",
    ] {
        assert!(!has(banned), "{banned} must not be passed");
    }
    assert!(!argv.iter().any(|a| a.contains("process=")));
    assert!(
        !argv.iter().any(|a| a.contains("media=cdrom")),
        "no ISO unless shared"
    );
}

#[test]
fn accel_keeps_existing_mac_and_unknown_host_cpu_models() {
    assert_eq!(
        Accel::for_host("other", Arch::Aarch64),
        Accel {
            name: "tcg",
            cpu: "cortex-a72"
        }
    );
    assert_eq!(
        Accel::for_host("macos", Arch::X86_64),
        Accel {
            name: "tcg",
            cpu: "qemu64"
        }
    );
    assert_eq!(
        Accel::for_host("macos", Arch::Aarch64),
        Accel {
            name: "hvf",
            cpu: "host"
        }
    );
}

#[test]
fn keys_map_by_physical_code_and_unknown_keys_are_refused() {
    let table = [
        ("KeyA", "a"),
        ("Digit7", "7"),
        ("Enter", "ret"),
        ("ArrowLeft", "left"),
        ("F12", "f12"),
    ];
    for (code, qcode) in table {
        assert_eq!(qcode_for(code), Some(qcode), "{code}");
    }
    assert_eq!(qcode_for("Unidentified"), None);
    let unknown: Gesture =
        serde_json::from_value(json!({"kind":"press", "code":"Unidentified"})).unwrap();
    assert!(unknown.command(10, 10).is_err());
}

#[test]
fn a_modified_key_is_one_balanced_qmp_gesture() {
    let key: Gesture = serde_json::from_value(
        json!({"kind":"press","code":"KeyA","modifiers":["Control","Shift"]}),
    )
    .unwrap();
    let cmd = key.command(10, 10).unwrap();
    let events = cmd.arguments["events"].as_array().unwrap();
    let keys: Vec<_> = events
        .iter()
        .map(|e| {
            (
                e["data"]["key"]["data"].as_str().unwrap(),
                e["data"]["down"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        keys,
        [
            ("ctrl", true),
            ("shift", true),
            ("a", true),
            ("a", false),
            ("shift", false),
            ("ctrl", false)
        ]
    );
    let duplicate: Gesture =
        serde_json::from_value(json!({"kind":"press","code":"KeyA","modifiers":["Shift","Shift"]}))
            .unwrap();
    assert!(duplicate.command(10, 10).is_err());
}

#[test]
fn clicks_use_frame_pixels_and_never_leave_the_button_pressed() {
    let click = |x, y| {
        Gesture::Click {
            x,
            y,
            button: "left".into(),
        }
        .command(200, 100)
    };
    let cmd = click(0, 99).unwrap();
    let events = cmd.arguments["events"].as_array().unwrap();
    assert_eq!(events.len(), 4);
    assert_eq!(events[0]["data"], json!({"axis":"x","value":0}));
    assert_eq!(events[1]["data"], json!({"axis":"y","value":32767}));
    assert_eq!(events[2]["data"], json!({"button":"left","down":true}));
    assert_eq!(events[3]["data"], json!({"button":"left","down":false}));
    assert!(click(200, 0).is_err());
    assert!(serde_json::from_value::<Gesture>(json!({"kind":"down","button":"left"})).is_err());
}

#[test]
fn qmp_skips_events_and_reports_errors() {
    let script = concat!(
        r#"{"QMP":{"version":{}}}"#,
        "\n",
        r#"{"return":{}}"#,
        "\n",
        r#"{"event":"RESET","timestamp":{}}"#,
        "\n",
        r#"{"return":{"ok":true}}"#,
        "\n",
        r#"{"error":{"class":"GenericError","desc":"no such device"}}"#,
        "\n",
    );
    let mut written = Vec::new();
    let mut qmp = Qmp::connect(Cursor::new(script.as_bytes()), &mut written).unwrap();
    assert_eq!(
        qmp.execute("query-status", json!({})).unwrap(),
        json!({"ok": true})
    );
    let error = qmp.execute("bogus", json!({})).unwrap_err();
    assert!(error.contains("no such device"));
    let sent = String::from_utf8(written).unwrap();
    assert!(sent.contains(r#""execute":"qmp_capabilities""#));
}

#[test]
fn a_closed_control_channel_fails_the_handshake() {
    let result = Qmp::connect(Cursor::new(Vec::<u8>::new()), Vec::new());
    assert!(result.is_err());
}

#[test]
fn a_screendump_becomes_an_rgba_frame_and_bad_images_are_refused() {
    let mut ppm = b"P6\n2 1\n255\n".to_vec();
    ppm.extend_from_slice(&[255, 0, 0, 0, 255, 0]);
    let frame = frame_from_ppm(&ppm).unwrap();
    assert_eq!((frame.width, frame.height), (2, 1));
    assert_eq!(&*frame.rgba, &[255, 0, 0, 255, 0, 255, 0, 255]);
    // Only the agent's screenshot is a PNG, and it decodes to the same pixels.
    let png = frame.png().unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let mut reader = png::Decoder::new(Cursor::new(png)).read_info().unwrap();
    let mut rgb = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut rgb).unwrap();
    assert_eq!(&rgb[..info.buffer_size()], &[255, 0, 0, 0, 255, 0]);
    assert!(frame_from_ppm(b"P5\n2 1\n255\n\0\0").is_err());
    assert!(
        frame_from_ppm(b"P6\n2 2\n255\n\0\0\0").is_err(),
        "truncated pixels"
    );
    assert!(frame_from_ppm(b"P6\n4294967295 4294967295\n255\n").is_err());
    assert!(Frame::from_bgrx(2, 1, &[1, 2, 3, 0, 4, 5, 6, 0]).is_ok_and(|f| &*f.rgba == [3, 2, 1, 255, 6, 5, 4, 255]));
    assert!(Frame::from_bgrx(2, 1, &[0; 7]).is_err());
}

#[test]
fn the_runtime_probe_reads_nothing_from_the_path() {
    // The check only looks under the resource dir, so a missing runtime is never satisfied by a
    // binary of the same name elsewhere on the host.
    let empty = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    assert!(matches!(
        discover(
            Path::new(empty.path()),
            data.path(),
            Arch::X86_64,
            "x86_64-unknown-linux-gnu"
        ),
        Err(RuntimeError::Missing("qemu"))
    ));
}

#[test]
#[allow(clippy::unwrap_used)]
fn a_status_event_names_the_run_and_who_started_it() {
    let info = RunInfo {
        run_id: "run-7".into(),
        workspace_root: "/work/app".into(),
        title: "app".into(),
        status: Status::Starting,
        controller: Controller::Agent,
        origin: Origin::Agent,
        guest_target: GuestTarget::Linux,
        vm_id: None,
    };
    let payload = status_payload(&info, None);
    assert_eq!(payload["kind"], "status");
    assert_eq!(payload["runId"], "run-7");
    assert_eq!(payload["status"], "starting");
    assert_eq!(payload["controller"], "agent");
    assert_eq!(payload["origin"], "agent");
    assert!(payload.get("message").is_none());
}

#[test]
fn guest_targets_use_their_wire_names_and_disk_files() {
    let table = [
        ("default", GuestTarget::Default, Some("guest.raw")),
        ("linux", GuestTarget::Linux, Some("linux.raw")),
        ("windows", GuestTarget::Windows, Some("windows.raw")),
        ("macos", GuestTarget::Macos, None),
        ("vm", GuestTarget::Vm, None),
    ];
    let data = tempfile::tempdir().unwrap();
    for (wire, guest, file) in table {
        assert_eq!(
            serde_json::from_value::<GuestTarget>(json!(wire)).unwrap(),
            guest
        );
        assert_eq!(serde_json::to_value(guest).unwrap(), json!(wire));
        assert_eq!(guest.disk_file(), file);
        assert_eq!(
            guest_disk_path_for(data.path(), guest),
            file.map(|file| data.path().join("app-runner").join(file))
        );
    }
    assert!(serde_json::from_value::<GuestTarget>(json!("arch")).is_err());
    assert!(serde_json::from_value::<GuestTarget>(json!("Linux")).is_err());
}

#[test]
fn each_guest_target_boots_only_its_own_disk() {
    let target = "x86_64-unknown-linux-gnu";
    let files = good_files(Arch::X86_64);
    let (resource, data) = fixture(target, &as_refs(&files));
    let disk_dir = data.path().join("app-runner");
    let found = |guest| discover_for(resource.path(), data.path(), Arch::X86_64, target, guest);

    // Only the legacy disk exists: it is the default guest's and never another target's fallback.
    assert_eq!(
        found(GuestTarget::Default).unwrap().guest_disk,
        guest_disk_path(data.path())
    );
    for guest in [GuestTarget::Linux, GuestTarget::Windows] {
        assert_eq!(found(guest), Err(RuntimeError::Missing(GUEST_IMAGE)));
    }
    // Each labelled disk is found under its own name.
    for (guest, file) in [
        (GuestTarget::Linux, "linux.raw"),
        (GuestTarget::Windows, "windows.raw"),
    ] {
        fs::write(disk_dir.join(file), b"disk").unwrap();
        assert_eq!(found(guest).unwrap().guest_disk, disk_dir.join(file));
    }
    // A labelled disk that is a link is refused, as the legacy disk is.
    #[cfg(unix)]
    {
        let real = data.path().join("elsewhere.raw");
        fs::rename(disk_dir.join("linux.raw"), &real).unwrap();
        std::os::unix::fs::symlink(&real, disk_dir.join("linux.raw")).unwrap();
        assert!(matches!(
            found(GuestTarget::Linux),
            Err(RuntimeError::Refused(reason)) if reason.contains("link")
        ));
    }
    // macOS has no backend: refused before any file is read, even with every disk present.
    assert_eq!(
        found(GuestTarget::Macos),
        Err(RuntimeError::Unsupported(GuestTarget::Macos))
    );
}

/// How long the whole guest may run before the watchdog kills it (handshake, screens and input).
const WATCHDOG: Duration = Duration::from_secs(240);
/// How long the firmware may show a blank screen before the smoke test fails.
const BOOT_PICTURE: Duration = Duration::from_secs(120);

/// The guest for one smoke run. Dropping it kills the process through the production `kill`, so a
/// failed assertion cannot leave QEMU running.
struct SmokeGuest {
    child: Arc<Mutex<Child>>,
    _watchdog: mpsc::Sender<()>,
}

impl Drop for SmokeGuest {
    fn drop(&mut self) {
        kill(&self.child);
    }
}

fn smoke_path(name: &str) -> PathBuf {
    match std::env::var_os(name) {
        Some(value) if !value.is_empty() => value.into(),
        _ => panic!(
            "{name} must name a staged runtime or a disposable disk; the smoke test fails rather than skipping"
        ),
    }
}

fn log_tail(dir: &Path) -> String {
    let log = fs::read_to_string(dir.join("qemu.log")).unwrap_or_default();
    let lines: Vec<&str> = log.lines().collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
}

fn stamp(path: &Path) -> (u64, std::time::SystemTime) {
    let meta = fs::metadata(path).unwrap();
    (meta.len(), meta.modified().unwrap())
}

/// Whether a decoded frame has more than one byte value, that is, is not a blank screen.
fn frame_has_content(frame: &Frame) -> bool {
    frame.rgba.iter().any(|&byte| byte != frame.rgba[0])
}

/// Screendump until the firmware draws something other than a blank screen, or the deadline passes.
fn wait_for_picture(channel: &mut Channel, dir: &Path, within: Duration) -> Frame {
    let until = Instant::now() + within;
    loop {
        let frame = capture(channel, dir)
            .unwrap_or_else(|e| panic!("screendump failed: {e}\n{}", log_tail(dir)));
        if frame_has_content(&frame) {
            return frame;
        }
        assert!(
            Instant::now() < until,
            "the guest screen stayed blank for {within:?}\n{}",
            log_tail(dir)
        );
        thread::sleep(Duration::from_millis(500));
    }
}

/// Real-guest smoke test for packagers. It is `#[ignore]`d because it boots the staged QEMU bundle
/// on a disposable copy of a disk. Run it explicitly; it fails rather than skips when a prerequisite
/// is missing:
///
/// ```sh
/// DIAL_APP_RUNNER_SMOKE_RESOURCES=<dir containing qemu/runtime.json> \
/// DIAL_APP_RUNNER_SMOKE_DISK=<disposable raw image> \
/// cargo test --locked --manifest-path src-tauri/Cargo.toml real_guest_ -- --ignored --nocapture
/// ```
///
/// Optional: `DIAL_APP_RUNNER_SMOKE_EXPECT_INPUT_EFFECT=1` requires the screen to change after the
/// click and Enter. Without it the input's visible effect is only printed, because firmware may not
/// react to it. Passing proves the firmware frame and the QMP control path, not a guest desktop.
#[test]
#[ignore = "boots the real bundled guest; set DIAL_APP_RUNNER_SMOKE_RESOURCES and DIAL_APP_RUNNER_SMOKE_DISK and run with --ignored"]
fn real_guest_shows_a_picture_takes_input_and_stops() {
    let resources = smoke_path("DIAL_APP_RUNNER_SMOKE_RESOURCES");
    let disk_src = smoke_path("DIAL_APP_RUNNER_SMOKE_DISK");
    let require_effect =
        std::env::var("DIAL_APP_RUNNER_SMOKE_EXPECT_INPUT_EFFECT").is_ok_and(|v| v == "1");
    assert!(
        disk_src.is_file(),
        "the smoke disk {} is not a file",
        disk_src.display()
    );
    let target = host_target().expect("this platform has no app runner target");
    let arch = Arch::host().expect("no bundled guest exists for this host architecture");

    // The bundle's own verifier (manifest, tree, sizes, hashes, licences). Node only reads files.
    let verifier =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../packaging/runtime/stage-runtime.mjs");
    let check = std::process::Command::new("node")
        .arg(&verifier)
        .args(["--target", &target, "--check"])
        .arg(resources.join("qemu"))
        .output()
        .expect("node is required to run the bundle verifier");
    assert!(
        check.status.success(),
        "the bundle verifier refused the runtime:\n{}",
        String::from_utf8_lossy(&check.stderr)
    );

    // The guest disk is a copy under a temporary data dir; the source is only read.
    let data = tempfile::tempdir().unwrap();
    fs::create_dir_all(data.path().join("app-runner")).unwrap();
    let disk = guest_disk_path(data.path());
    fs::copy(&disk_src, &disk).expect("copy the disposable disk");
    let disk_stamp = stamp(&disk);
    let source_stamp = stamp(&disk_src);

    // Discovery through the production reader, which checks every file against the manifest.
    let runtime = discover(&resources, data.path(), arch, &target)
        .unwrap_or_else(|e| panic!("the runtime was not discovered: {e:?}"));
    assert!(runtime.qemu.is_file() && runtime.firmware_code.is_file());

    // Launch with the production argument builder. The spawn below mirrors `start`; see the
    // follow-up in docs/app-runtime-packaging.md about moving it into one shared owner.
    let run = tempfile::Builder::new()
        .prefix("smoke-run-")
        .tempdir()
        .unwrap();
    let dir = run.path().to_path_buf();
    let vars_copy = dir.join("vars.fd");
    fs::copy(&runtime.firmware_vars, &vars_copy).unwrap();
    let share_dir = runtime.firmware_code.parent().unwrap().to_path_buf();
    let accel = Accel::for_host(std::env::consts::OS, runtime.arch);
    let argv = args(&Launch {
        runtime: &runtime,
        vars_copy: &vars_copy,
        share_dir: &share_dir,
        accel,
        share_iso: None,
        vnc_socket: None,
        memory_mb: MEMORY_MB,
        cpus: CPUS,
    });
    let log = File::create(dir.join("qemu.log")).unwrap();
    let mut command = std::process::Command::new(&runtime.qemu);
    command
        .args(&argv)
        .env_clear()
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(super::CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .unwrap_or_else(|e| panic!("could not start the guest: {e}"));
    let stdin = child.stdin.take().expect("guest stdin");
    let stdout = child.stdout.take().expect("guest stdout");
    let (watchdog, expiry) = mpsc::channel::<()>();
    let guest = SmokeGuest {
        child: Arc::new(Mutex::new(child)),
        _watchdog: watchdog,
    };
    let watched = Arc::clone(&guest.child);
    thread::spawn(move || {
        if let Err(mpsc::RecvTimeoutError::Timeout) = expiry.recv_timeout(WATCHDOG) {
            kill(&watched);
        }
    });

    // Handshake, then the firmware frame: the first non-blank screendump.
    let qmp = Qmp::connect(BufReader::new(stdout), stdin).unwrap_or_else(|e| {
        panic!(
            "the guest did not complete the QMP handshake: {e}\n{}",
            log_tail(&dir)
        )
    });
    let mut channel = Channel {
        qmp: Some(qmp),
        ..Channel::default()
    };
    let first = wait_for_picture(&mut channel, &dir, BOOT_PICTURE);
    assert!(
        first.width > 0 && first.height > 0,
        "the screendump has no size"
    );

    // Guest input through the production gesture translation and QMP executor.
    let (w, h) = (first.width, first.height);
    let click = Gesture::Click {
        x: w / 2,
        y: h / 2,
        button: "left".into(),
    }
    .command(w, h)
    .expect("a click inside the frame");
    execute(&mut channel, &click)
        .unwrap_or_else(|e| panic!("QEMU refused the click: {}\n{}", e.message, log_tail(&dir)));
    let enter = Gesture::Press {
        code: "Enter".into(),
        modifiers: Vec::new(),
    }
    .command(w, h)
    .expect("Enter is a known key");
    execute(&mut channel, &enter)
        .unwrap_or_else(|e| panic!("QEMU refused Enter: {}\n{}", e.message, log_tail(&dir)));
    let after = capture(&mut channel, &dir)
        .unwrap_or_else(|e| panic!("screendump after input failed: {e}\n{}", log_tail(&dir)));
    let changed = after.rgba != first.rgba;
    if require_effect {
        assert!(
            changed,
            "the screen did not change after the click and Enter"
        );
    } else {
        println!("input effect on the firmware frame: changed={changed} (informational)");
    }

    // Stop and cleanup through the production kill: the process exits and the disk copy is untouched.
    kill(&guest.child);
    assert!(
        guest.child.lock().unwrap().try_wait().unwrap().is_some(),
        "the guest is still running after stop"
    );
    assert_eq!(
        stamp(&disk),
        disk_stamp,
        "the guest changed its disk copy; writes must stay in snapshot mode"
    );
    assert_eq!(
        stamp(&disk_src),
        source_stamp,
        "the source disk was modified"
    );
}

#[test]
fn a_reveal_names_only_the_project() {
    assert_eq!(
        reveal_payload("/work/app", GuestTarget::Windows),
        json!({"kind": "reveal", "workspaceRoot": "/work/app", "guestTarget": "windows"})
    );
}

#[test]
fn a_mismatched_guest_names_the_open_target_and_keeps_the_requested_status() {
    use super::{StateView, state_view};
    // (requested, open run's target, runtime check, expected status, expected open target)
    type Case = (
        GuestTarget,
        Option<GuestTarget>,
        Result<(), RuntimeError>,
        &'static str,
        Option<GuestTarget>,
    );
    let table: Vec<Case> = vec![
        // Nothing open: the requested target alone decides, and nothing is named as open.
        (GuestTarget::Linux, None, Ok(()), "idle", None),
        // A Linux run is open; a Windows request still reports its own readiness.
        (
            GuestTarget::Windows,
            Some(GuestTarget::Linux),
            Ok(()),
            "idle",
            Some(GuestTarget::Linux),
        ),
        (
            GuestTarget::Windows,
            Some(GuestTarget::Linux),
            Err(RuntimeError::Missing(GUEST_IMAGE)),
            "missing_runtime",
            Some(GuestTarget::Linux),
        ),
        (
            GuestTarget::Windows,
            Some(GuestTarget::Linux),
            Err(RuntimeError::Refused("bad".into())),
            "failed",
            Some(GuestTarget::Linux),
        ),
        // Unsupported stays the requested status, even when another guest is open.
        (
            GuestTarget::Macos,
            Some(GuestTarget::Linux),
            Err(RuntimeError::Unsupported(GuestTarget::Macos)),
            "unsupported",
            Some(GuestTarget::Linux),
        ),
    ];
    for (guest, open, check, status, open_guest) in table {
        let view: StateView = state_view(guest, open, check);
        assert_eq!(view.status, status, "requested {guest:?}, open {open:?}");
        assert_eq!(view.guest_target, guest);
        assert_eq!(view.open_guest_target, open_guest);
        assert!(view.run.is_none());
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json.get("openGuestTarget").is_some(), open_guest.is_some());
    }
}

#[test]
fn the_status_event_carries_the_guest_target_of_the_run() {
    let info = RunInfo {
        run_id: "run-8".into(),
        workspace_root: "/work/app".into(),
        title: "app".into(),
        status: Status::Running,
        controller: Controller::User,
        origin: Origin::User,
        guest_target: GuestTarget::Windows,
        vm_id: None,
    };
    assert_eq!(status_payload(&info, None)["guestTarget"], "windows");
}

#[test]
fn guest_targets_route_to_one_backend_with_no_fallback() {
    use super::runtime::{Backend, backend_for};
    let cases: [(GuestTarget, bool, Result<Backend, RuntimeError>); 6] = [
        (GuestTarget::Default, true, Ok(Backend::Qemu)),
        (GuestTarget::Default, false, Ok(Backend::Qemu)),
        (GuestTarget::Linux, false, Ok(Backend::Qemu)),
        (GuestTarget::Windows, false, Ok(Backend::Qemu)),
        (GuestTarget::Macos, true, Ok(Backend::MacOs)),
        (
            GuestTarget::Macos,
            false,
            Err(RuntimeError::Unsupported(GuestTarget::Macos)),
        ),
    ];
    for (guest, apple_silicon, want) in cases {
        assert_eq!(backend_for(guest, apple_silicon), want, "{guest:?}");
    }
}

#[test]
fn the_preferred_guest_matches_the_host_without_repurposing_the_unlabelled_disk() {
    use super::runtime::preferred_for;
    let cases = [
        (("macos", "aarch64"), GuestTarget::Macos),
        (("macos", "x86_64"), GuestTarget::Default),
        (("linux", "x86_64"), GuestTarget::Linux),
        (("linux", "aarch64"), GuestTarget::Linux),
        (("windows", "x86_64"), GuestTarget::Windows),
        (("other", "x86_64"), GuestTarget::Default),
    ];
    for ((os, arch), want) in cases {
        assert_eq!(preferred_for(os, arch), want, "{os}/{arch}");
    }
}

#[test]
fn missing_components_name_the_macos_helper_and_base_files() {
    let helper = runtime::missing_message(runtime::MACOS_HELPER);
    assert!(helper.contains("macos-guest-helper") && helper.contains("dial-mac-guest"));
    let image = runtime::missing_message(runtime::MACOS_IMAGE);
    assert!(image.contains("macos-guest-image"));
    for name in runtime::MAC_BASE_FILES {
        assert!(image.contains(name), "{name} missing from {image}");
    }
    assert_eq!(
        runtime::missing_message(runtime::QEMU),
        "The app runner needs the bundled qemu."
    );
}

mod live_input {
    use super::super::input::{Gesture, Held, Low, Modifier, atomic, stream_command};
    use super::super::agent;
    use super::super::input::Frame;
    use super::super::screen::{FLAG_FULL, PACKET_HEADER, Screen, TILE};
    use std::sync::Arc;
    use serde_json::json;

    fn g(value: serde_json::Value) -> Gesture {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn stream_gestures_parse_and_only_click_and_press_are_atomic() {
        assert!(!g(json!({"kind":"pointer","x":1,"y":2,"buttons":1})).is_atomic());
        assert!(!g(json!({"kind":"wheel","x":1,"y":2,"dy":-3})).is_atomic());
        assert!(!g(json!({"kind":"key","code":"KeyA","down":true})).is_atomic());
        assert!(!g(json!({"kind":"release"})).is_atomic());
        assert!(g(json!({"kind":"click","x":1,"y":2,"button":"left"})).is_atomic());
        assert!(serde_json::from_value::<Gesture>(json!({"kind":"key","code":"KeyA"})).is_err());
    }

    #[test]
    fn the_agent_cannot_send_live_input() {
        let args = json!({"runId":"run-1","frameSeq":1,"token":"00000000000000000000000000000001",
            "event":{"kind":"key","code":"KeyA","down":true}});
        assert_eq!(agent::parse_input(&args).err().as_deref(), Some("the event must be a click or a press"));
        // A stream gesture has no atomic QMP command either.
        assert!(g(json!({"kind":"release"})).command(10, 10).is_err());
    }

    #[test]
    fn a_drag_holds_the_button_and_qmp_gets_only_the_changes() {
        let mut held = Held::default();
        let down = held.apply(&g(json!({"kind":"pointer","x":0,"y":99,"buttons":1})), 100, 100).unwrap();
        assert_eq!(down, vec![Low::Mouse { x: 0, y: 99, buttons: 1, prev: 0 }]);
        let cmd = stream_command(&down, 100, 100).unwrap();
        let events = &cmd.arguments["events"];
        assert_eq!(events[0]["data"], json!({"axis":"x","value":0}));
        assert_eq!(events[1]["data"], json!({"axis":"y","value":32767}));
        assert_eq!(events[2]["data"], json!({"button":"left","down":true}));
        assert_eq!(events.as_array().unwrap().len(), 3);
        // Dragging outside the frame pins to the edge; no button event while it stays held.
        let drag = held.apply(&g(json!({"kind":"pointer","x":500,"y":5,"buttons":1})), 100, 100).unwrap();
        assert_eq!(drag, vec![Low::Mouse { x: 99, y: 5, buttons: 1, prev: 1 }]);
        assert_eq!(stream_command(&drag, 100, 100).unwrap().arguments["events"].as_array().unwrap().len(), 2);
        assert!(held.any());
        let up = held.apply(&g(json!({"kind":"pointer","x":10,"y":5,"buttons":0})), 100, 100).unwrap();
        let events = stream_command(&up, 100, 100).unwrap().arguments["events"].clone();
        assert_eq!(events[2]["data"], json!({"button":"left","down":false}));
        assert!(!held.any());
    }

    #[test]
    fn wheel_notches_are_clamped_and_sent_as_wheel_buttons() {
        let mut held = Held::default();
        let steps = held.apply(&g(json!({"kind":"wheel","x":1,"y":1,"dy":50})), 10, 10).unwrap();
        assert_eq!(steps, vec![Low::Wheel { x: 1, y: 1, buttons: 0, dy: 10 }]);
        let events = stream_command(&steps, 10, 10).unwrap().arguments["events"].clone();
        assert_eq!(events.as_array().unwrap().len(), 2 + 20);
        assert_eq!(events[2]["data"], json!({"button":"wheel-down","down":true}));
        let up = held.apply(&g(json!({"kind":"wheel","x":1,"y":1,"dy":-1})), 10, 10).unwrap();
        assert_eq!(stream_command(&up, 10, 10).unwrap().arguments["events"][2]["data"],
            json!({"button":"wheel-up","down":true}));
        assert!(held.apply(&g(json!({"kind":"wheel","x":1,"y":1,"dy":0})), 10, 10).unwrap().is_empty());
    }

    #[test]
    fn keys_hold_until_up_ignore_autorepeat_and_release_lets_go_of_everything() {
        let mut held = Held::default();
        let shift = held.apply(&g(json!({"kind":"key","code":"ShiftLeft","down":true})), 10, 10).unwrap();
        assert_eq!(stream_command(&shift, 10, 10).unwrap().arguments["events"][0]["data"],
            json!({"down":true,"key":{"type":"qcode","data":"shift"}}));
        held.apply(&g(json!({"kind":"key","code":"KeyA","down":true})), 10, 10).unwrap();
        assert!(held.apply(&g(json!({"kind":"key","code":"KeyA","down":true})), 10, 10).unwrap().is_empty());
        held.apply(&g(json!({"kind":"pointer","x":3,"y":4,"buttons":2})), 10, 10).unwrap();
        // An up for a key never pressed here sends nothing; an unknown key is refused.
        assert!(held.apply(&g(json!({"kind":"key","code":"KeyB","down":false})), 10, 10).unwrap().is_empty());
        assert!(held.apply(&g(json!({"kind":"key","code":"Lang9","down":true})), 10, 10).is_err());
        let all = held.apply(&g(json!({"kind":"release"})), 10, 10).unwrap();
        assert_eq!(all, vec![
            Low::Key { code: "KeyA".into(), qcode: "a", down: false },
            Low::Key { code: "ShiftLeft".into(), qcode: "shift", down: false },
            Low::Mouse { x: 3, y: 4, buttons: 0, prev: 2 },
        ]);
        assert!(!held.any());
        assert!(held.release().is_empty());
    }

    #[test]
    fn a_whole_gesture_guest_gets_clicks_on_release_and_presses_with_held_modifiers() {
        let mut held = Held::default();
        let down = held.apply(&g(json!({"kind":"pointer","x":5,"y":6,"buttons":1})), 10, 10).unwrap();
        assert!(atomic(&down, &held).is_empty());
        let up = held.apply(&g(json!({"kind":"pointer","x":7,"y":8,"buttons":0})), 10, 10).unwrap();
        let clicks = atomic(&up, &held);
        assert!(matches!(&clicks[..], [Gesture::Click { x: 7, y: 8, button }] if button == "left"));
        let ctrl = held.apply(&g(json!({"kind":"key","code":"ControlLeft","down":true})), 10, 10).unwrap();
        assert!(atomic(&ctrl, &held).is_empty());
        let c = held.apply(&g(json!({"kind":"key","code":"KeyC","down":true})), 10, 10).unwrap();
        let presses = atomic(&c, &held);
        assert!(matches!(&presses[..], [Gesture::Press { code, modifiers }]
            if code == "KeyC" && modifiers == &vec![Modifier::Control]));
    }

    fn solid(w: u32, h: u32, v: u8) -> Vec<u8> {
        vec![v; (w * h * 4) as usize]
    }

    struct Rect {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        data: Vec<u8>,
    }

    /// Read a packet: header (seq, width, height, flags) and its rectangles.
    fn read(packet: &[u8]) -> (u64, u32, u32, u32, Vec<Rect>) {
        let u32_at = |at: usize| u32::from_le_bytes(packet[at..at + 4].try_into().unwrap());
        let seq = u64::from_le_bytes(packet[..8].try_into().unwrap());
        let (width, height, count, flags) = (u32_at(8), u32_at(12), u32_at(16), u32_at(20));
        let mut at = PACKET_HEADER;
        let mut rects = Vec::new();
        for _ in 0..count {
            let (x, y, w, h) = (u32_at(at), u32_at(at + 4), u32_at(at + 8), u32_at(at + 12));
            at += 16;
            let len = (w * h * 4) as usize;
            rects.push(Rect { x, y, w, h, data: packet[at..at + len].to_vec() });
            at += len;
        }
        assert_eq!(at, packet.len(), "nothing after the last rectangle");
        (seq, width, height, flags, rects)
    }

    /// A frame with one pixel changed at (x, y).
    fn poke(frame: &Frame, x: u32, y: u32, v: u8) -> Frame {
        let mut rgba = frame.rgba.to_vec();
        let at = ((y * frame.width + x) * 4) as usize;
        rgba[at..at + 4].copy_from_slice(&[v, v, v, 255]);
        Frame { rgba: rgba.into(), ..frame.clone() }
    }

    #[test]
    fn the_first_frame_and_a_window_with_nothing_get_the_whole_screen() {
        let mut screen = Screen::default();
        let mut seq = 0;
        assert!(screen.plan(None, 0).is_none());
        let frame = Frame { width: 130, height: 70, rgba: solid(130, 70, 9).into() };
        assert_eq!(screen.record(&frame, || { seq += 1; seq }), Some(1));
        let (s, w, h, flags, rects) = read(&screen.plan(None, 1).unwrap().packet());
        assert_eq!((s, w, h, flags, rects.len()), (1, 130, 70, FLAG_FULL, 1));
        assert_eq!((rects[0].x, rects[0].y, rects[0].w, rects[0].h), (0, 0, 130, 70));
        assert_eq!(rects[0].data, &*frame.rgba);
        // The same pixels again are not a new frame; one already shown gets an empty delta.
        let same = Frame { rgba: frame.rgba.to_vec().into(), ..frame.clone() };
        assert_eq!(screen.record(&same, || unreachable!()), None);
        assert!(screen.record(&frame, || unreachable!()).is_none(), "shared pixels short-circuit");
        let (_, _, _, flags, rects) = read(&screen.plan(Some(1), 1).unwrap().packet());
        assert_eq!((flags, rects.len()), (0, 0));
    }

    #[test]
    fn a_delta_carries_only_the_changed_tiles_clipped_to_the_frame() {
        let mut screen = Screen::default();
        let mut seq = 0;
        let mut next = || { seq += 1; seq };
        let base = Frame { width: 130, height: 70, rgba: solid(130, 70, 0).into() };
        screen.record(&base, &mut next);
        // A pixel in the last column of tiles (x 128..130, y 64..70), clipped at the edges.
        let a = poke(&base, 129, 69, 7);
        assert_eq!(screen.record(&a, &mut next), Some(2));
        let (s, _, _, flags, rects) = read(&screen.plan(Some(1), 2).unwrap().packet());
        assert_eq!((s, flags, rects.len()), (2, 0, 1));
        let r = &rects[0];
        assert_eq!((r.x, r.y, r.w, r.h), (2 * TILE, TILE, 2, 6));
        assert_eq!(&r.data[r.data.len() - 4..], &[7, 7, 7, 255]);
        // Two more changes in the first row of tiles, next to each other: one rectangle.
        let b = poke(&poke(&a, 0, 0, 5), 70, 3, 5);
        screen.record(&b, &mut next);
        let (_, _, _, _, rects) = read(&screen.plan(Some(2), 3).unwrap().packet());
        assert_eq!(rects.iter().map(|r| (r.x, r.y, r.w, r.h)).collect::<Vec<_>>(), vec![(0, 0, 128, 64)]);
        // Since frame 1: everything that changed in 2 and 3, at its newest pixels.
        let (_, _, _, flags, rects) = read(&screen.plan(Some(1), 3).unwrap().packet());
        assert_eq!(flags, 0);
        assert_eq!(rects.iter().map(|r| (r.x, r.y, r.w, r.h)).collect::<Vec<_>>(),
            vec![(0, 0, 128, 64), (128, 64, 2, 6)]);
        assert_eq!(&rects[0].data[..4], &[5, 5, 5, 255]);
    }

    #[test]
    fn deltas_applied_in_order_rebuild_the_newest_frame() {
        let mut screen = Screen::default();
        let mut seq = 0;
        let mut frame = Frame { width: 200, height: 150, rgba: solid(200, 150, 1).into() };
        screen.record(&frame, || { seq += 1; seq });
        let mut canvas = frame.rgba.to_vec();
        let mut shown = 1;
        for i in 0..40u32 {
            frame = poke(&frame, (i * 37) % 200, (i * 53) % 150, i as u8 + 2);
            screen.record(&frame, || { seq += 1; seq });
            // The window skips some frames, as a busy one does.
            if i % 3 != 0 {
                continue;
            }
            let (s, _, _, _, rects) = read(&screen.plan(Some(shown), seq).unwrap().packet());
            for r in rects {
                for row in 0..r.h {
                    let dst = (((r.y + row) * 200 + r.x) * 4) as usize;
                    let src = (row * r.w * 4) as usize;
                    canvas[dst..dst + (r.w * 4) as usize].copy_from_slice(&r.data[src..src + (r.w * 4) as usize]);
                }
            }
            shown = s;
            assert_eq!(canvas, &*frame.rgba, "after frame {s}");
        }
    }

    #[test]
    fn a_window_too_far_behind_or_after_a_resize_gets_the_whole_frame() {
        let mut screen = Screen::default();
        let mut seq = 0;
        let mut frame = Frame { width: 64, height: 64, rgba: solid(64, 64, 0).into() };
        screen.record(&frame, || { seq += 1; seq });
        for i in 0..130u32 {
            frame = poke(&frame, i % 64, 0, (i % 250) as u8 + 1);
            screen.record(&frame, || { seq += 1; seq });
        }
        assert_eq!(read(&screen.plan(Some(1), seq).unwrap().packet()).3, FLAG_FULL, "history dropped");
        assert_eq!(read(&screen.plan(Some(seq - 5), seq).unwrap().packet()).3, 0);
        assert_eq!(read(&screen.plan(Some(seq + 1), seq).unwrap().packet()).3, FLAG_FULL, "a future frame");
        let bigger = Frame { width: 80, height: 64, rgba: Arc::from(solid(80, 64, 3)) };
        screen.record(&bigger, || { seq += 1; seq });
        let (_, w, _, flags, _) = read(&screen.plan(Some(seq - 1), seq).unwrap().packet());
        assert_eq!((w, flags), (80, FLAG_FULL));
    }
}


#[cfg(unix)]
mod qemu_display {
    use super::super::rfb::tests::{bgrx, server, update};
    use super::super::rfb::{Session, Viewer};
    use super::super::{Channel, attach_vnc, capture, vnc_frame, vnc_socket_for};
    use std::os::unix::net::UnixStream;
    use std::path::Path;
    use std::sync::mpsc;
    use std::thread;

    fn channel_with_display(replies: Vec<Vec<u8>>, close_when_done: bool) -> Channel {
        let (client, server_end) = UnixStream::pair().unwrap();
        let (tx, _rx) = mpsc::channel();
        thread::spawn(move || server(server_end, 2, 1, replies, tx, close_when_done));
        Channel {
            vnc: Some(Viewer::new(Session::connect(client).unwrap())),
            ..Channel::default()
        }
    }

    #[test]
    fn capture_reads_the_display_without_a_screendump() {
        // No QMP at all: a capture that tried a screendump would fail.
        let mut channel = channel_with_display(vec![update(&[(0, 0, 2, 1, bgrx(2, 3, 2, 1))])], false);
        let dir = tempfile::tempdir().unwrap();
        let frame = capture(&mut channel, dir.path()).unwrap();
        assert_eq!((frame.width, frame.height), (2, 1));
        assert_eq!(&frame.rgba[..4], &[1, 2, 3, 255]);
        assert_eq!(channel.screen_size, Some((2, 1)));
        assert!(!dir.path().join("screen.ppm").exists());
        assert!(channel.vnc.is_some());
    }

    #[test]
    fn a_failed_display_is_dropped_and_the_capture_falls_back() {
        let mut channel = channel_with_display(vec![update(&[(0, 0, 2, 1, bgrx(2, 0, 0, 0))])], true);
        assert!(vnc_frame(&mut channel).is_some());
        // The display closes; the next capture drops it and goes to QMP, absent here.
        let error = loop {
            match capture(&mut channel, Path::new("/nonexistent")) {
                Ok(_) => thread::sleep(std::time::Duration::from_millis(5)),
                Err(error) => break error,
            }
        };
        assert!(channel.vnc.is_none(), "a failed display must not be polled again");
        assert_eq!(error, "the guest is not connected");
    }

    #[test]
    fn a_missing_socket_leaves_the_run_on_screendumps() {
        let dir = tempfile::tempdir().unwrap();
        let mut channel = Channel::default();
        attach_vnc(&mut channel, Some(&dir.path().join("vnc.sock")));
        assert!(channel.vnc.is_none());
        attach_vnc(&mut channel, None);
        assert!(channel.vnc.is_none());
    }

    #[test]
    fn the_socket_lives_in_the_run_folder_and_fits_a_socket_address() {
        let short = Path::new("/tmp/run-abc");
        assert_eq!(vnc_socket_for(short).unwrap(), short.join("vnc.sock"));
        let long = Path::new("/").join("x".repeat(120));
        assert!(vnc_socket_for(&long).is_none(), "too long for sun_path: screendumps instead");
    }
}
