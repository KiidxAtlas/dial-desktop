//! The agent's eyes and hands on a Type's app while it is open in the window: open it, wait until
//! it has loaded, run a script inside it and read what it logged.
//!
//! The app runs in a sandboxed frame the main webview hosts (`AppFrame.vue`), which keeps a small
//! registry, `window.__dialTypes`: each call here is a fixed script evaluated in the main webview
//! that asks that registry, which talks to the frame by `postMessage`.

use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use super::Store;

const POLL: Duration = Duration::from_millis(100);
/// How long a script inside the app may run.
const RUN_LIMIT: Duration = Duration::from_secs(15);
/// How long an app may take to load after it is opened or changed.
const LOAD_LIMIT: Duration = Duration::from_secs(10);

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Status {
    /// The app's frame is mounted and visible.
    pub open: bool,
    /// Its page finished loading (and the runtime said so).
    pub loaded: bool,
    /// The app version (`Full::app_version`) the frame shows.
    pub version: String,
}

#[derive(Debug, Deserialize)]
struct Log {
    level: String,
    text: String,
}

/// Evaluate `call` (an expression on `h`, the registry, giving a JSON string) in the main webview.
fn host<R: Runtime>(app: &AppHandle<R>, call: &str) -> Result<Value, String> {
    let view = app.get_webview("main").ok_or("The Dial window is not open")?;
    let script = format!(
        "(() => {{ const h = window.__dialTypes; if (!h) return JSON.stringify({{error: 'No Type is open in Dial Desktop; open it with types_open'}}); return {call}; }})()"
    );
    let raw = crate::browser::eval_view_raw(&view, &script, Duration::from_secs(5))?;
    let value = crate::browser::unwrap_json(&raw)?;
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(error.to_owned());
    }
    Ok(value)
}

pub fn status<R: Runtime>(app: &AppHandle<R>, id: &str) -> Result<Status, String> {
    let value = host(app, &format!("h.status({})", json!(id)))?;
    serde_json::from_value(value).map_err(|e| format!("Unreadable status: {e}"))
}

/// The type's current app version, which the frame must show before it counts as loaded, or
/// `None` when the type has no app.
fn version<R: Runtime>(app: &AppHandle<R>, id: &str) -> Result<Option<String>, String> {
    let full = app.state::<Store>().get(id)?;
    Ok(full.has_app.then_some(full.app_version))
}

/// Wait until type `id`'s app shows its current version, or say why not.
pub fn wait_loaded<R: Runtime>(app: &AppHandle<R>, id: &str, limit: Duration) -> Result<Status, String> {
    let want = version(app, id)?.ok_or("This Type has no app yet; write one with types_update")?;
    let start = Instant::now();
    loop {
        let now = status(app, id)?;
        if now.open && now.loaded && now.version == want {
            return Ok(now);
        }
        if start.elapsed() >= limit {
            return Err(if now.open {
                "The Type's app did not finish loading; check its console with types_run, or its source".into()
            } else {
                "The Type is not on screen; open it with types_open (and close any overlay)".into()
            });
        }
        thread::sleep(POLL);
    }
}

/// What the app logged since it loaded (errors and warnings, or everything when `all`).
pub fn console<R: Runtime>(app: &AppHandle<R>, id: &str, all: bool) -> String {
    let logs: Vec<Log> = host(app, &format!("h.logs({})", json!(id)))
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    format_logs(&logs, all)
}

fn format_logs(logs: &[Log], all: bool) -> String {
    let shown: Vec<String> = logs
        .iter()
        .filter(|l| all || matches!(l.level.as_str(), "error" | "warn"))
        .map(|l| format!("[{}] {}", l.level, l.text))
        .collect();
    if shown.is_empty() {
        return if all { "Console: nothing logged.".into() } else { "Console: no errors or warnings.".into() };
    }
    let skipped = shown.len().saturating_sub(30);
    let mut out = String::from("Console:\n");
    if skipped > 0 {
        out.push_str(&format!("({skipped} earlier lines left out)\n"));
    }
    out.push_str(&shown[skipped..].join("\n"));
    out
}

/// Show type `id` in the window and wait for its app.
pub fn open<R: Runtime>(app: &AppHandle<R>, id: &str) -> Result<String, String> {
    app.emit("type-open", id).map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        match wait_loaded(app, id, Duration::from_millis(500)) {
            Ok(_) => {
                return Ok(format!(
                    "Opened it; the app has loaded. {}",
                    console(app, id, false)
                ));
            }
            Err(e) if e.contains("no app yet") => return Ok(format!("Opened it. {e}.")),
            Err(e) if start.elapsed() >= LOAD_LIMIT => return Err(e),
            Err(_) => {}
        }
    }
}

/// Run `script` (an async function body) inside type `id`'s app.
pub fn run<R: Runtime>(app: &AppHandle<R>, id: &str, script: &str) -> Result<String, String> {
    wait_loaded(app, id, LOAD_LIMIT)?;
    let started = host(app, &format!("h.run({}, {})", json!(id), json!(script)))?;
    let key = started
        .get("key")
        .and_then(Value::as_u64)
        .ok_or("The app did not take the script")?;
    let start = Instant::now();
    let done = loop {
        let answer = host(app, &format!("h.result({key})"))?;
        if answer.get("pending").is_none() {
            break answer;
        }
        if start.elapsed() > RUN_LIMIT {
            return Err(format!(
                "The script did not finish within {} s. {}",
                RUN_LIMIT.as_secs(),
                console(app, id, false)
            ));
        }
        thread::sleep(POLL);
    };
    let logs: Vec<Log> = done
        .get("logs")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let console = format_logs(&logs, true).replacen("Console:", "Console while it ran:", 1);
    if done.get("ok").and_then(Value::as_bool) != Some(true) {
        let error = done.get("error").and_then(Value::as_str).unwrap_or("The script failed");
        return Err(format!("{error}\n{console}"));
    }
    let value = done.get("value").cloned().unwrap_or(Value::Null);
    let mut text = serde_json::to_string_pretty(&value).unwrap_or_default();
    if text.len() > 20_000 {
        let mut end = 20_000;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n[clipped]");
    }
    Ok(format!("Result:\n{text}\n{console}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_shows_problems_first_and_keeps_the_latest() {
        let log = |level: &str, text: &str| Log { level: level.into(), text: text.into() };
        assert_eq!(format_logs(&[log("log", "hi")], false), "Console: no errors or warnings.");
        assert_eq!(format_logs(&[log("log", "hi"), log("error", "boom")], false), "Console:\n[error] boom");
        let many: Vec<Log> = (0..35).map(|n| log("warn", &n.to_string())).collect();
        let out = format_logs(&many, true);
        assert!(out.starts_with("Console:\n(5 earlier lines left out)\n[warn] 5\n"));
        assert!(out.ends_with("[warn] 34"));
    }
}
