//! Running code on MicroPython and CircuitPython boards through their raw REPL: the prompt takes
//! a whole program, runs it, and returns its output and any error separately. This needs no tool
//! on the computer, only the serial console.
use serde::Serialize;
use std::time::{Duration, Instant};

/// The serial link to the board.
pub trait Wire {
    fn send(&mut self, bytes: &[u8]) -> Result<(), String>;
    /// Whatever arrived since the last call; empty when nothing did.
    fn recv(&mut self) -> Result<Vec<u8>, String>;
}

/// What a program printed. `finished` is false when it was still running at the time limit; it
/// keeps running on the board.
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub finished: bool,
}

const BANNER: &[u8] = b"raw REPL; CTRL-B to exit\r\n>";
const NOT_PYTHON: &str = "The device did not answer as a MicroPython or CircuitPython prompt. Check that it runs one of them and that the baud rate is right.";

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Collect input into `seen` until `done` accepts it or `limit` passes.
fn wait(wire: &mut impl Wire, seen: &mut Vec<u8>, limit: Duration, done: impl Fn(&[u8]) -> bool) -> Result<bool, String> {
    let start = Instant::now();
    loop {
        seen.extend(wire.recv()?);
        if done(seen) {
            return Ok(true);
        }
        if start.elapsed() >= limit {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Stop whatever runs and switch the prompt to raw mode.
fn enter(wire: &mut impl Wire) -> Result<(), String> {
    wire.send(b"\r\x03\x03")?;
    std::thread::sleep(Duration::from_millis(100));
    wire.recv()?;
    wire.send(b"\r\x01")?;
    let mut seen = Vec::new();
    if wait(wire, &mut seen, Duration::from_secs(2), |s| find(s, BANNER).is_some())? {
        Ok(())
    } else {
        Err(NOT_PYTHON.into())
    }
}

/// Run `code` on the board and wait up to `limit` for it to finish.
pub fn exec(wire: &mut impl Wire, code: &str, limit: Duration) -> Result<Output, String> {
    enter(wire)?;
    // The raw prompt has no flow control: feed it in pieces it can keep up with.
    for chunk in code.as_bytes().chunks(256) {
        wire.send(chunk)?;
        std::thread::sleep(Duration::from_millis(10));
    }
    wire.send(b"\x04")?;
    let mut seen = Vec::new();
    if !wait(wire, &mut seen, Duration::from_secs(3), |s| find(s, b"OK").is_some())? {
        return Err("The device did not accept the code".into());
    }
    let start = find(&seen, b"OK").unwrap_or(0) + 2;
    // The reply is the output, an end mark, the error text, and another end mark.
    let finished = wait(wire, &mut seen, limit, |s| s[start..].iter().filter(|&&b| b == 4).count() >= 2)?;
    let mut parts = seen[start..].split(|&b| b == 4).map(|part| String::from_utf8_lossy(part).replace("\r\n", "\n"));
    let output = Output { stdout: parts.next().unwrap_or_default(), stderr: parts.next().unwrap_or_default(), finished };
    if finished {
        wire.send(b"\x02")?;
    }
    Ok(output)
}

const MAX_FILE: usize = 256 * 1024;

/// Save `data` as file `name` on the board's own filesystem.
pub fn put(wire: &mut impl Wire, name: &str, data: &[u8]) -> Result<(), String> {
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/');
    if name.is_empty() || name.len() > 96 || !name.chars().all(plain) || name.split('/').any(|part| part.is_empty() || part.starts_with('.')) {
        return Err("Use a plain file name such as main.py".into());
    }
    if data.len() > MAX_FILE {
        return Err("Files saved this way can be at most 256 KiB".into());
    }
    let mut code = format!("f=open('{name}','wb')\nw=f.write\n");
    for chunk in data.chunks(64) {
        code.push_str("w(b'");
        for byte in chunk {
            code.push_str(&format!("\\x{byte:02x}"));
        }
        code.push_str("')\n");
    }
    code.push_str("f.close()\n");
    let result = exec(wire, &code, Duration::from_secs(20 + data.len() as u64 / 2048))?;
    if !result.finished {
        return Err("The device did not finish saving the file".into());
    }
    match result.stderr.trim().lines().last() {
        None => Ok(()),
        // CircuitPython's drive belongs to the computer while it is plugged in.
        Some(error) if error.contains("Errno 30") || error.contains("EROFS") => Err(format!(
            "The device's storage is read-only from its prompt ({error}). On CircuitPython, copy the file to the CIRCUITPY drive instead."
        )),
        Some(error) => Err(format!("The device could not save {name}: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A board at its raw prompt: answers the mode switch, then runs what it is sent on Ctrl-D.
    struct Board {
        sent: Vec<u8>,
        out: Vec<u8>,
        reply: fn(&str) -> Option<(&'static str, &'static str)>,
        programs: Vec<String>,
    }
    impl Board {
        fn new(reply: fn(&str) -> Option<(&'static str, &'static str)>) -> Self {
            Self { sent: Vec::new(), out: Vec::new(), reply, programs: Vec::new() }
        }
    }
    impl Wire for Board {
        fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
            for &byte in bytes {
                match byte {
                    1 => {
                        self.sent.clear();
                        self.out.extend_from_slice(BANNER);
                    }
                    4 => {
                        let code = String::from_utf8(std::mem::take(&mut self.sent)).unwrap();
                        self.out.extend_from_slice(b"OK");
                        if let Some((stdout, stderr)) = (self.reply)(&code) {
                            self.out.extend_from_slice(format!("{stdout}\x04{stderr}\x04>").as_bytes());
                        }
                        self.programs.push(code);
                    }
                    2 | 3 | b'\r' if self.sent.is_empty() => {}
                    _ => self.sent.push(byte),
                }
            }
            Ok(())
        }
        fn recv(&mut self) -> Result<Vec<u8>, String> {
            Ok(std::mem::take(&mut self.out))
        }
    }

    #[test]
    fn runs_code_and_separates_output_from_errors() {
        let mut board = Board::new(|_| Some(("hi\r\n", "Traceback\r\nNameError: x\r\n")));
        let code = "print('hi')\n".repeat(40);
        let output = exec(&mut board, &code, Duration::from_secs(1)).unwrap();
        assert_eq!(output, Output { stdout: "hi\n".into(), stderr: "Traceback\nNameError: x\n".into(), finished: true });
        assert_eq!(board.programs, [code], "long code arrives whole");
    }

    #[test]
    fn a_program_that_keeps_running_reports_what_it_printed_so_far() {
        let mut board = Board::new(|_| None);
        board.reply = |_| None;
        let output = exec(&mut board, "while True: pass", Duration::from_millis(30)).unwrap();
        assert_eq!(output, Output { stdout: String::new(), stderr: String::new(), finished: false });
    }

    #[test]
    fn a_device_without_a_python_prompt_is_named() {
        struct Silent;
        impl Wire for Silent {
            fn send(&mut self, _: &[u8]) -> Result<(), String> {
                Ok(())
            }
            fn recv(&mut self) -> Result<Vec<u8>, String> {
                Ok(b"ets Jun  8 2016".to_vec())
            }
        }
        // Shortened by the device never matching: the two-second wait is the cost of this test.
        assert_eq!(exec(&mut Silent, "1", Duration::from_secs(1)).unwrap_err(), NOT_PYTHON);
    }

    #[test]
    fn saves_a_file_as_bytes_and_explains_refusals() {
        let mut board = Board::new(|_| Some(("", "")));
        put(&mut board, "lib/main.py", b"a='\n\xff").unwrap();
        assert_eq!(board.programs, ["f=open('lib/main.py','wb')\nw=f.write\nw(b'\\x61\\x3d\\x27\\x0a\\xff')\nf.close()\n"]);
        for name in ["", "a b.py", "../x", "a'.py", "/main.py", ".hidden"] {
            assert!(put(&mut board, name, b"").is_err(), "{name}");
        }
        let mut locked = Board::new(|_| Some(("", "Traceback\r\nOSError: [Errno 30] Read-only filesystem\r\n")));
        assert!(put(&mut locked, "code.py", b"x").unwrap_err().contains("CIRCUITPY"));
    }
}
