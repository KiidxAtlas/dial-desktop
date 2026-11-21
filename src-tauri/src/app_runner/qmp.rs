//! A minimal QMP client over any reader/writer pair: the guest's stdio in the app, byte buffers in
//! tests. One command is in flight at a time; asynchronous events QEMU emits in between are skipped.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

pub struct Qmp<R, W> {
    reader: R,
    writer: W,
}

impl<R: BufRead, W: Write> Qmp<R, W> {
    /// Read QEMU's greeting and switch to command mode.
    pub fn connect(reader: R, writer: W) -> Result<Self, String> {
        let mut qmp = Qmp { reader, writer };
        let greeting = qmp.read_message()?;
        if greeting.get("QMP").is_none() {
            return Err("QEMU did not send a QMP greeting".into());
        }
        qmp.execute("qmp_capabilities", json!({}))?;
        Ok(qmp)
    }

    /// Run one command and return its `return` value.
    pub fn execute(&mut self, command: &str, arguments: Value) -> Result<Value, String> {
        let request = json!({ "execute": command, "arguments": arguments });
        let mut line = request.to_string();
        line.push('\n');
        self.writer
            .write_all(line.as_bytes())
            .and_then(|()| self.writer.flush())
            .map_err(|error| format!("could not reach the guest: {error}"))?;
        loop {
            let message = self.read_message()?;
            if let Some(value) = message.get("return") {
                return Ok(value.clone());
            }
            if let Some(error) = message.get("error") {
                let desc = error
                    .get("desc")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error");
                return Err(format!("QEMU refused {command}: {desc}"));
            }
            // Anything else is an asynchronous event; skip it.
        }
    }

    fn read_message(&mut self) -> Result<Value, String> {
        loop {
            let mut line = String::new();
            let read = self
                .reader
                .read_line(&mut line)
                .map_err(|error| format!("could not read from the guest: {error}"))?;
            if read == 0 {
                return Err("the guest closed its control channel".into());
            }
            if line.trim().is_empty() {
                continue;
            }
            return serde_json::from_str(&line)
                .map_err(|error| format!("QEMU sent a message that is not JSON: {error}"));
        }
    }
}
