//! `dial-desktop --browser-mcp`: the MCP server (stdio, one JSON message per line) through which
//! Dial's engine drives the Browser panel. It holds no browser itself: each tool call goes to the
//! running app over its loopback port (`agent_browser`). Without a running app it lists no tools,
//! so Dial sessions outside the app see nothing.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::agent_browser::{self, INSTRUCTIONS, PING, Reply, Request};

#[cfg(test)]
mod tests;

/// The MCP revision answered when the client names none.
const PROTOCOL: &str = "2025-06-18";
/// The longest a tool call may take (waits and page loads are bounded well below it).
const CALL_TIMEOUT: Duration = Duration::from_secs(90);

/// Serve stdin/stdout until stdin closes.
pub fn run() -> i32 {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let file = agent_browser::endpoint_file(home.as_deref());
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => respond(
                &message,
                &mut |tool, args| forward(file.as_deref(), tool, args),
                &|| reachable(file.as_deref()),
            ),
            Err(error) => Some(json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": {"code": -32700, "message": format!("Parse error: {error}")}
            })),
        };
        if let Some(reply) = reply
            && (writeln!(stdout, "{reply}").is_err() || stdout.flush().is_err())
        {
            break;
        }
    }
    0
}

/// The answer to one message, or `None` for notifications and responses.
pub fn respond(
    message: &Value,
    call: &mut dyn FnMut(&str, &Value) -> Result<Reply, String>,
    available: &dyn Fn() -> bool,
) -> Option<Value> {
    let id = message.get("id").filter(|id| !id.is_null())?.clone();
    let method = message.get("method").and_then(Value::as_str)?;
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "dial-desktop", "title": "Dial Desktop browser", "version": env!("CARGO_PKG_VERSION")},
            "instructions": INSTRUCTIONS,
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({
            "tools": if available() { agent_browser::tools() } else { json!([]) }
        })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            Ok(content(call(name, &args).unwrap_or_else(Reply::error)))
        }
        other => Err((-32601, format!("Unknown method `{other}`"))),
    };
    Some(match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err((code, message)) => {
            json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
        }
    })
}

/// A tool's reply as an MCP `tools/call` result.
pub fn content(reply: Reply) -> Value {
    let mut blocks = vec![json!({"type": "text", "text": reply.text})];
    if let Some(data) = reply.image {
        blocks.push(json!({"type": "image", "data": data, "mimeType": "image/png"}));
    }
    json!({"content": blocks, "isError": !reply.ok})
}

/// Send one call to the app and read its reply.
pub fn exchange(
    endpoint: &agent_browser::Endpoint,
    tool: &str,
    args: &Value,
    timeout: Duration,
) -> Result<Reply, String> {
    let address = SocketAddr::from(([127, 0, 0, 1], endpoint.port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .map_err(|_| NOT_RUNNING.to_owned())?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    let request = Request {
        token: endpoint.token.clone(),
        tool: tool.to_owned(),
        arguments: args.clone(),
    };
    let mut line = serde_json::to_string(&request).map_err(|e| e.to_string())?;
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut answer = String::new();
    BufReader::new(stream)
        .read_line(&mut answer)
        .map_err(|e| format!("Dial Desktop did not answer: {e}"))?;
    serde_json::from_str(&answer).map_err(|_| "Dial Desktop gave no answer".to_owned())
}

const NOT_RUNNING: &str = "Dial Desktop is not running, so its Browser panel is not available";

fn forward(file: Option<&Path>, tool: &str, args: &Value) -> Result<Reply, String> {
    let endpoint = file
        .and_then(agent_browser::read_endpoint)
        .ok_or(NOT_RUNNING)?;
    exchange(&endpoint, tool, args, CALL_TIMEOUT)
}

/// Whether the app answers with the token on file.
fn reachable(file: Option<&Path>) -> bool {
    file.and_then(agent_browser::read_endpoint)
        .and_then(|endpoint| exchange(&endpoint, PING, &Value::Null, Duration::from_secs(2)).ok())
        .is_some_and(|reply| reply.ok)
}
