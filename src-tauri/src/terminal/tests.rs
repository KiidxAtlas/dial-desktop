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

#[cfg(unix)]
#[test]
fn a_shell_streams_output_and_reports_its_exit() {
    let mut terminals = Terminals::default();
    let (tx, rx) = mpsc::channel();
    let mut command = CommandBuilder::new("/bin/sh");
    command.cwd(std::env::temp_dir());
    let id = terminals
        .open(command, 80, 24, move |event| {
            let _ = tx.send(event);
        })
        .unwrap();
    terminals.write(id, "echo dial-$((40+2))\nexit\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut output = String::new();
    let mut exited = false;
    while Instant::now() < deadline && !exited {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(TerminalEvent::Data { data, .. }) => output.push_str(&data),
            Ok(TerminalEvent::Exit { id: ended }) => exited = ended == id,
            Err(_) => {}
        }
    }
    assert!(output.contains("dial-42"), "{output:?}");
    assert!(exited);
    assert!(
        !terminals.has_shells(),
        "exited shell must be removed before Exit is reported"
    );
    assert!(terminals.write(id, "x").is_err());
    // Closing an already-exited shell remains a safe no-op.
    terminals.close(id);
}
