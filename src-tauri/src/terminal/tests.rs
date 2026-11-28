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
fn attach_help_must_be_for_the_subcommand_not_the_legacy_prompt_cli() {
    assert!(attach_usage("View a running session\n\nUsage: dial attach <SESSION>"));
    assert!(!attach_usage("Usage: dial [OPTIONS] [PROMPT]... [COMMAND]\n  --image   Attach an image"));
}

#[test]
fn attached_chat_targets_a_literal_session_in_the_selected_project() {
    let child = Task::Attach { session: "abcd1234".into() }.child(Path::new("dial"), Path::new("project")).unwrap();
    assert_eq!(child.get_argv(), &[OsStr::new("dial"), OsStr::new("attach"), OsStr::new("--"), OsStr::new("abcd1234")]);
    assert_eq!(child.get_cwd().map(Path::new), Some(Path::new("project")));
    assert!(Task::Attach { session: "--help".into() }.child(Path::new("dial"), Path::new("project")).is_err());
}

#[test]
fn windows_honors_resolvable_shells_and_falls_back_from_msys_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = std::env::join_paths([dir.path()]).unwrap();
    let resolve = |shell| windows_shell(shell, Some(&path));
    assert_eq!(resolve(None), Path::new("powershell.exe"));
    let pwsh = dir.path().join("pwsh.exe");
    let bash = dir.path().join("custom bash.exe");
    std::fs::write(&pwsh, b"").unwrap();
    std::fs::write(&bash, b"").unwrap();
    for invalid in [None, Some(OsStr::new("/usr/bin/bash")), Some(OsStr::new("script.sh")), Some(dir.path().as_os_str())] {
        assert_eq!(resolve(invalid), pwsh);
    }
    for valid in [OsStr::new("custom bash"), OsStr::new("custom bash.exe"), bash.as_os_str()] {
        assert_eq!(resolve(Some(valid)), bash);
    }
}
