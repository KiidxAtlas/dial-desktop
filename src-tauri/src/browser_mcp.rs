//! `dial-desktop --browser-mcp`: the MCP server (stdio, one JSON message per line) through which
//! Dial's engine drives the Browser panel. It holds no browser itself: each tool call goes to the
//! running app over its loopback port (`agent_browser`). Without a running app it lists no tools,
//! so Dial sessions outside the app see nothing.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
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
    let home = crate::launcher::home_dir();
    let directory =
        crate::launcher::app_dir(home.as_deref()).map(|dir| dir.join("browser-endpoints"));
    let workspace = std::env::current_dir().and_then(|p| p.canonicalize()).ok();
    let endpoint = || find_window(directory.as_deref(), workspace.as_deref());
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
                &mut |tool, args| {
                    exchange(
                        &endpoint()?,
                        workspace.as_deref().ok_or(NOT_RUNNING)?,
                        tool,
                        args,
                        CALL_TIMEOUT,
                    )
                },
                &|| endpoint().is_ok(),
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
    workspace: &Path,
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
        workspace: workspace.to_string_lossy().into_owned(),
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

/// Never route a call to an arbitrary window when two windows have the same workspace.
fn find_window(dir: Option<&Path>, root: Option<&Path>) -> Result<agent_browser::Endpoint, String> {
    let root = root.ok_or(NOT_RUNNING)?;
    let entries = std::fs::read_dir(dir.ok_or(NOT_RUNNING)?).map_err(|_| NOT_RUNNING)?;
    let mut found = None;
    for file in entries.flatten().map(|e| e.path()) {
        if file.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let Some(endpoint) = agent_browser::read_endpoint(&file) else {
            continue;
        };
        if exchange(&endpoint, root, PING, &Value::Null, Duration::from_secs(2)).is_ok_and(|r| r.ok)
            && found.replace(endpoint).is_some()
        {
            return Err("More than one window has this project open".into());
        }
    }
    found.ok_or_else(|| "No Dial Desktop window for this project".into())
}
