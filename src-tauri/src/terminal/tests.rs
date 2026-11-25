use super::*;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn utf8_split_across_reads_is_held_back() {
    let mut pending = b"ab\xc3".to_vec();
    assert_eq!(take_utf8(&mut pending), "ab");
    assert_eq!(pending, b"\xc3");
    pending.push(0xa9);
    assert_eq!(take_utf8(&mut pending), "é");
    assert!(pending.is_empty());
}

#[test]
fn invalid_bytes_become_replacement_characters() {
    let mut pending = b"a\xffb".to_vec();
    assert_eq!(take_utf8(&mut pending), "a\u{FFFD}b");
    assert!(pending.is_empty());
}

#[test]
fn a_shell_streams_output_and_reports_its_exit() {
    let mut terminals = Terminals::default();
    let (tx, rx) = mpsc::channel();
    let (mut command, input) = if cfg!(windows) {
        let mut cmd = CommandBuilder::new("cmd.exe");
        cmd.args(["/D", "/Q", "/V:ON"]);
        (cmd, "set /a RESULT=40+2\recho dial-!RESULT!\rexit\r")
    } else {
        (CommandBuilder::new("/bin/sh"), "echo dial-$((40+2))\nexit\n")
    };
    command.cwd(std::env::temp_dir());
    let id = terminals
        .open(command, 80, 24, move |event| {
            let _ = tx.send(event);
        })
        .unwrap();
    terminals.write(id, input).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut output = String::new();
    let mut exited = false;
    while Instant::now() < deadline && !exited {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(TerminalEvent::Data { data, .. }) => {
                // Windows' ConPTY asks for the cursor position before it runs the shell; the
                // app's terminal answers, so the test must too.
                if data.contains("\u{1b}[6n") {
                    let _ = terminals.write(id, "\u{1b}[1;1R");
                }
                output.push_str(&data);
            }
            Ok(TerminalEvent::Exit { id: ended }) => exited = ended == id,
            Err(_) => {}
        }
    }
    assert!(output.contains("dial-42"), "{output:?}");
    assert!(exited);
    assert!(
        terminals.open_ids().is_empty(),
        "exited shell must be removed before Exit is reported"
    );
    assert!(terminals.write(id, "x").is_err());
    // Closing an already-exited shell remains a safe no-op.
    terminals.close(id);
}

#[test]
fn slash_command_preserves_literal_argument_and_configured_program() {
    let program = Path::new("configured engine");
    let root = Path::new("project root");
    let text = "/doctor 'quoted' $(example) & %name%";
    let child = slash_command(program, root, text).unwrap();
    assert_eq!(child.get_argv(), &[program.as_os_str(), text.as_ref()]);
    assert_eq!(child.get_cwd().map(Path::new), Some(root));
    for invalid in ["", "dial", "--help", " /doctor", "/", "/a\nb", "/a\u{1b}"] {
        assert!(
            slash_command(program, root, invalid).is_err(),
            "{invalid:?}"
        );
    }
}

#[test]
fn windows_prefers_powershell_7_when_it_is_on_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = std::env::join_paths([dir.path()]).unwrap();
    assert_eq!(windows_shell(Some(&path)), "powershell.exe");
    assert_eq!(windows_shell(None), "powershell.exe");
    std::fs::write(dir.path().join("pwsh.exe"), b"").unwrap();
    assert_eq!(windows_shell(Some(&path)), "pwsh.exe");
}
