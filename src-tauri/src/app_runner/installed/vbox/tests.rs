use std::fs;
use std::time::{Duration, Instant};

use tempfile::TempDir;

use super::*;

const UUID: &str = "1f2e3d4c-5b6a-4789-8abc-def012345678";

/// A fake helper: a POSIX shell in place of Python, so no VirtualBox is needed. Nothing here
/// starts a VM; the scripts only print fixed JSON lines and wait.
#[cfg(unix)]
fn fake_helper(script: &str) -> Console {
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(script);
    Console::start(command).expect("sh starts")
}

fn key_codes(code: &str, modifiers: &[Modifier]) -> Result<Vec<u8>, String> {
    press_codes(code, modifiers)
}
#[test]
fn click_becomes_move_press_release_for_each_button() {
    let cases = [("left", 0x01), ("right", 0x02), ("middle", 0x04)];
    for (button, mask) in cases {
        let steps = gesture_steps(
            &Gesture::Click {
                x: 10,
                y: 20,
                button: button.to_owned(),
            },
            640,
            480,
        )
        .unwrap();
        assert_eq!(
            steps,
            vec![
                json!({ "mouse": [10, 20, 0] }),
                json!({ "mouse": [10, 20, mask] }),
                json!({ "mouse": [10, 20, 0] }),
            ],
            "button {button}"
        );
    }
}

#[test]
fn click_refusals_name_no_host_detail() {
    let click = |x, y, button: &str| Gesture::Click {
        x,
        y,
        button: button.to_owned(),
    };
    let cases = [
        (click(640, 0, "left"), 640, 480, "outside"),
        (click(0, 480, "left"), 640, 480, "outside"),
        (click(0, 0, "left"), 0, 480, "frame"),
        (click(0, 0, "back"), 640, 480, "not supported"),
    ];
    for (gesture, width, height, expected) in cases {
        let err = gesture_steps(&gesture, width, height).unwrap_err();
        assert!(err.contains(expected), "{err}");
        assert!(!err.contains('/'), "{err}");
    }
}

#[test]
fn key_presses_map_to_set1_scancodes_with_modifiers() {
    let cases: Vec<(&str, Vec<Modifier>, Vec<u8>)> = vec![
        ("KeyA", vec![], vec![0x1E, 0x9E]),
        ("KeyA", vec![Modifier::Shift], vec![0x2A, 0x1E, 0x9E, 0xAA]),
        // Extended keys carry the E0 prefix on both make and break.
        ("ArrowUp", vec![], vec![0xE0, 0x48, 0xE0, 0xC8]),
        (
            "ArrowUp",
            vec![Modifier::Control],
            vec![0x1D, 0xE0, 0x48, 0xE0, 0xC8, 0x9D],
        ),
        (
            "KeyA",
            vec![Modifier::Meta],
            vec![0xE0, 0x5B, 0x1E, 0x9E, 0xE0, 0xDB],
        ),
        // A modifier that is the key itself is not pressed twice.
        ("ShiftLeft", vec![Modifier::Shift], vec![0x2A, 0xAA]),
    ];
    for (code, modifiers, expected) in cases {
        assert_eq!(key_codes(code, &modifiers).unwrap(), expected, "{code}");
    }
}

#[test]
fn key_refusals_are_refused_before_any_send() {
    assert!(key_codes("NoSuchKey", &[]).is_err());
    assert!(key_codes("KeyA", &[Modifier::Shift, Modifier::Shift]).is_err());
}

#[test]
fn every_key_the_translator_accepts_has_a_scancode() {
    let mut codes: Vec<String> = ('A'..='Z').map(|c| format!("Key{c}")).collect();
    codes.extend(('0'..='9').map(|c| format!("Digit{c}")));
    codes.extend((1..=12).map(|n| format!("F{n}")));
    codes.extend(
        [
            "Enter",
            "NumpadEnter",
            "Escape",
            "Backspace",
            "Tab",
            "Space",
            "Minus",
            "Equal",
            "BracketLeft",
            "BracketRight",
            "Backslash",
            "Semicolon",
            "Quote",
            "Backquote",
            "Comma",
            "Period",
            "Slash",
            "ArrowUp",
            "ArrowDown",
            "ArrowLeft",
            "ArrowRight",
            "Home",
            "End",
            "PageUp",
            "PageDown",
            "Delete",
            "Insert",
            "ShiftLeft",
            "ShiftRight",
            "ControlLeft",
            "ControlRight",
            "AltLeft",
            "AltRight",
            "MetaLeft",
            "MetaRight",
            "CapsLock",
        ]
        .map(String::from),
    );
    for code in codes {
        let qcode = qcode_for(&code).unwrap_or_else(|| panic!("{code} is translated"));
        assert!(
            scancode(qcode).is_some(),
            "{code} -> {qcode} has no scancode"
        );
    }
}

#[test]
fn scancode_table_has_no_duplicate_names() {
    for (i, (name, _)) in SCANCODES.iter().enumerate() {
        assert!(
            SCANCODES[..i].iter().all(|(other, _)| other != name),
            "{name} is listed twice"
        );
    }
}

#[test]
fn only_well_formed_uuids_reach_the_helper() {
    assert_eq!(valid_uuid(UUID), Ok(UUID));
    for bad in [
        "",
        "Windows 11",
        "1f2e3d4c5b6a47898abcdef012345678",
        "1f2e3d4c-5b6a-4789-8abc-def01234567g",
        "../../etc/passwd",
        "1f2e3d4c-5b6a-4789-8abc-def0123456789",
    ] {
        assert!(valid_uuid(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn frame_replies_are_bounded_before_they_become_frames() {
    let pixels = [1u8, 2, 3, 0, 4, 5, 6, 0];
    let ok = json!({ "ok": true, "width": 2, "height": 1, "format": "bgra", "pixels": 8 });
    let frame = frame_from_reply(&ok, &pixels).unwrap();
    assert_eq!((frame.width, frame.height), (2, 1));
    assert_eq!(&*frame.rgba, &[3, 2, 1, 255, 6, 5, 4, 255]);

    let cases = [
        (json!({ "ok": true, "width": 0, "height": 1, "format": "bgra" }), &pixels[..]),
        (json!({ "ok": true, "width": 3841, "height": 1, "format": "bgra" }), &pixels[..]),
        (json!({ "ok": true, "width": 2, "height": 2161, "format": "bgra" }), &pixels[..]),
        (json!({ "ok": true, "width": 2, "height": 1, "format": "png" }), &pixels[..]),
        (json!({ "ok": true, "width": 2, "height": 1 }), &pixels[..]),
        (json!({ "ok": true, "width": 2, "height": 1, "format": "bgra" }), &pixels[..7]),
        (json!({ "ok": true, "width": 2, "height": 1, "format": "bgra" }), &[][..]),
    ];
    for (reply, bytes) in cases {
        assert!(frame_from_reply(&reply, bytes).is_err(), "{reply} with {} bytes", bytes.len());
    }
}

#[test]
fn reply_lines_carry_exactly_the_pixels_they_announce() {
    let mut stream = b"{\"ok\":true}\n".to_vec();
    stream.extend_from_slice(b"{\"ok\":true,\"pixels\":3}\n\n\x01\x02");
    stream.extend_from_slice(b"{\"ok\":false}\r\n");
    let (sender, lines) = mpsc::channel();
    read_lines(std::io::Cursor::new(stream), &sender);
    let got: Vec<_> = lines.try_iter().collect();
    assert_eq!(got, vec![
        Some(Line { text: "{\"ok\":true}".into(), payload: vec![] }),
        // A newline inside the pixels is a pixel byte, not the end of a line.
        Some(Line { text: "{\"ok\":true,\"pixels\":3}".into(), payload: vec![b'\n', 1, 2] }),
        Some(Line { text: "{\"ok\":false}".into(), payload: vec![] }),
    ]);

    // Pixels cut short, or more than the largest screen, end the console.
    for stream in [b"{\"pixels\":4}\n\x01\x02".to_vec(), format!("{{\"pixels\":{}}}\n", MAX_PIXEL_BYTES + 1).into_bytes()] {
        let (sender, lines) = mpsc::channel();
        read_lines(std::io::Cursor::new(stream), &sender);
        assert_eq!(lines.try_iter().collect::<Vec<_>>(), vec![None]);
    }
}

#[test]
fn sdk_must_sit_beside_the_trusted_tool() {
    let dir = TempDir::new().unwrap();
    let bin = dir.path().join("VirtualBox.app/Contents/MacOS");
    fs::create_dir_all(&bin).unwrap();
    let tool = bin.join("VBoxManage");
    fs::write(&tool, "tool").unwrap();

    let missing = SdkLayout::for_tool(&tool).unwrap_err();
    assert!(!missing.contains(dir.path().to_string_lossy().as_ref()));

    let xpcom = bin.join("sdk/bindings/xpcom/python/xpcom");
    let api = bin.join("sdk/installer/python/vboxapi/src/vboxapi");
    fs::create_dir_all(&xpcom).unwrap();
    fs::create_dir_all(&api).unwrap();
    fs::write(xpcom.join("vboxxpcom.py"), "").unwrap();
    fs::write(api.join("__init__.py"), "").unwrap();
    let layout = SdkLayout::for_tool(&tool).unwrap();
    assert_eq!(layout.bin, fs::canonicalize(&bin).unwrap());
    assert_eq!(layout.sdk, layout.bin.join("sdk"));
    assert!(SdkLayout::for_tool(&bin.join("absent")).is_err());
}

#[test]
fn replies_map_to_fixed_messages_and_stale_ends_the_console() {
    #[cfg(unix)]
    {
        // The helper's own text ("secret path") must never reach the caller.
        let mut console = fake_helper(
            "printf '{\"ok\":true}\\n'; read _l; \
             printf '{\"ok\":false,\"code\":\"stale\",\"error\":\"secret path\"}\\n'; \
             exec sleep 30",
        );
        console.read_reply(Duration::from_secs(5)).unwrap();
        let err = console
            .exchange(json!({"op":"frame"}), Duration::from_secs(5))
            .unwrap_err();
        assert_eq!(err, STALE);
        assert_eq!(
            console
                .exchange(json!({"op":"frame"}), Duration::from_secs(5))
                .unwrap_err(),
            CLOSED
        );
    }
}

#[test]
fn refused_and_unknown_codes_keep_the_console_open() {
    #[cfg(unix)]
    {
        let mut console = fake_helper(
            "printf '{\"ok\":true}\\n'; read _l; \
             printf '{\"ok\":false,\"code\":\"refused\",\"error\":\"x\"}\\n'; \
             read _l; \
             printf '{\"ok\":false,\"code\":\"weird\"}\\n'; \
             read _l; printf '{\"ok\":true}\\n'; exec sleep 30",
        );
        console.read_reply(Duration::from_secs(5)).unwrap();
        let refused = console
            .exchange(json!({"op":"frame"}), Duration::from_secs(5))
            .unwrap_err();
        assert_eq!(refused, REFUSED);
        let unknown = console
            .exchange(json!({"op":"frame"}), Duration::from_secs(5))
            .unwrap_err();
        assert_eq!(unknown, FAILED);
        assert!(
            console
                .exchange(json!({"op":"send"}), Duration::from_secs(5))
                .is_ok()
        );
    }
}

#[test]
fn a_silent_helper_is_killed_within_the_deadline() {
    #[cfg(unix)]
    {
        let mut console = fake_helper("printf '{\"ok\":true}\\n'; read _l; exec sleep 30");
        console.read_reply(Duration::from_secs(5)).unwrap();
        let started = Instant::now();
        let err = console
            .exchange(json!({"op":"frame"}), Duration::from_millis(200))
            .unwrap_err();
        assert!(err.contains("did not answer"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(
            console
                .exchange(json!({"op":"frame"}), Duration::from_secs(1))
                .unwrap_err(),
            CLOSED
        );
    }
}

#[test]
fn oversized_reply_lines_end_the_console() {
    #[cfg(unix)]
    {
        let (sender, lines) = mpsc::channel();
        let reader = std::io::repeat(b'x');
        thread::spawn(move || read_lines(reader.take(MAX_REPLY_BYTES + 16), &sender));
        assert_eq!(lines.recv_timeout(Duration::from_secs(10)).unwrap(), None);
    }
}

/// The helper may start a headless VM and may attach to one; it must never stop, save, discard
/// or reconfigure a machine. Checked against the embedded source that Dial actually runs.
#[test]
fn helper_never_calls_destructive_methods() {
    for forbidden in [
        "powerDown",
        "poweroff",
        "discardSavedState",
        "saveState",
        "saveSettings",
        "setExtraData",
        "unregister",
        "removeMachine",
        "delete",
        "forget",
        "takeSnapshot",
        "restoreSnapshot",
        "\"gui\"",
    ] {
        assert!(
            !HELPER_SOURCE.contains(forbidden),
            "helper mentions {forbidden}"
        );
    }
    assert!(HELPER_SOURCE.contains("launchVMProcess(session, \"headless\", [])"));
    assert!(HELPER_SOURCE.contains("LockType_Shared"));
}

#[test]
fn console_can_move_between_threads() {
    fn assert_send<T: Send>() {}
    assert_send::<Console>();
}

#[test]
fn sdk_files_are_required_per_platform() {
    let dir = TempDir::new().unwrap();
    let sdk = dir.path().join("sdk");
    let api = sdk.join("installer/python/vboxapi/src/vboxapi");
    fs::create_dir_all(&api).unwrap();
    fs::write(api.join("__init__.py"), "").unwrap();
    // Windows needs only the vboxapi package; other hosts also need the XPCOM bindings.
    assert!(sdk_files_present(&sdk, true));
    assert!(!sdk_files_present(&sdk, false));
    let xpcom = sdk.join("bindings/xpcom/python/xpcom");
    fs::create_dir_all(&xpcom).unwrap();
    fs::write(xpcom.join("vboxxpcom.py"), "").unwrap();
    assert!(sdk_files_present(&sdk, false));
    assert!(!sdk_files_present(&dir.path().join("absent"), true));
}

#[test]
fn machine_python_needs_pywin32_and_takes_the_newest_install() {
    let dir = TempDir::new().unwrap();
    // (directory, has python.exe, has pywin32's win32com)
    let installs = [
        ("Python39", true, true),
        ("Python312", true, true),
        ("Python313", true, false),
        ("Python314", false, true),
        ("Other", true, true),
    ];
    for (name, exe, pywin32) in installs {
        let root = dir.path().join(name);
        fs::create_dir_all(root.join("Lib/site-packages/win32com")).unwrap();
        if exe {
            fs::write(root.join("python.exe"), "").unwrap();
        }
        if pywin32 {
            fs::write(root.join("Lib/site-packages/win32com/__init__.py"), "").unwrap();
        }
    }
    assert_eq!(
        machine_python(dir.path()),
        Some(dir.path().join("Python312/python.exe"))
    );
    let empty = TempDir::new().unwrap();
    assert_eq!(machine_python(empty.path()), None);
    assert_eq!(machine_python(&dir.path().join("absent")), None);
}

#[test]
fn verbatim_windows_paths_are_given_to_the_helper_plain() {
    let cases = [
        (
            r"\\?\C:\Program Files\Oracle\VirtualBox\VBoxManage.exe",
            r"C:\Program Files\Oracle\VirtualBox\VBoxManage.exe",
        ),
        (
            r"\\?\UNC\server\share\VBoxManage.exe",
            r"\\?\UNC\server\share\VBoxManage.exe",
        ),
        (
            "/Applications/VirtualBox.app/Contents/MacOS/VBoxManage",
            "/Applications/VirtualBox.app/Contents/MacOS/VBoxManage",
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(strip_verbatim(Path::new(input)), PathBuf::from(expected));
    }
}

#[test]
fn helper_path_lists_only_the_install_and_system_directories() {
    assert_eq!(
        windows_path(
            Path::new(r"C:\Program Files\Oracle\VirtualBox"),
            Path::new(r"C:\Windows")
        ),
        r"C:\Program Files\Oracle\VirtualBox;C:\Windows\System32;C:\Windows"
    );
}

#[test]
fn stream_steps_hold_buttons_and_send_make_codes() {
    use crate::app_runner::input::Low;
    let steps = stream_steps(&[
        Low::Mouse { x: 3, y: 4, buttons: 5, prev: 0 },
        Low::Wheel { x: 3, y: 4, buttons: 1, dy: -2 },
        Low::Key { code: "ArrowRight".into(), qcode: "right", down: true },
        Low::Key { code: "KeyA".into(), qcode: "a", down: false },
    ])
    .unwrap();
    assert_eq!(steps[0], json!({ "mouse": [3, 4, 5] }));
    assert_eq!(steps[1], json!({ "wheel": [3, 4, -2, 1] }));
    assert_eq!(steps[2], json!({ "key": { "make": [0xE0, 0x4D], "down": true } }));
    assert_eq!(steps[3], json!({ "key": { "make": [0x1E], "down": false } }));
}
