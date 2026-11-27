//! The agent's view of and hands on Dial Desktop itself: the `desktop_*` tools.
//!
//! What the window shows and the palette's commands come from the frontend (`src/agentDesktop.ts`)
//! through a fixed script in the main webview; the agent passes JSON arguments, never code.
//! Engine settings are read and saved here, as the settings page does. Settings that govern the
//! agent's own permissions or credentials are visible but never changed by these tools
//! ([`protected`]); approvals, Agent access and VM grants are not reachable at all.

use std::time::Duration;

use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use super::{Reply, SCRIPT_TIMEOUT, TEXT_LIMIT, clip, parse_outcome, string_arg};
use crate::settings::SettingView;

/// The tools' names, routed here by `agent_browser::serve`.
pub const TOOLS: [&str; 5] = [
    "desktop_view",
    "desktop_commands",
    "desktop_run",
    "desktop_settings",
    "desktop_set_setting",
];

/// How long `desktop_settings` waits for an engine that has not sent its settings yet.
const SETTINGS_WAIT: Duration = Duration::from_secs(10);
/// Rows `desktop_settings` lists without a filter.
const SETTINGS_LIMIT: usize = 120;

/// Settings (and everything under them) that decide what the agent may do, where it reaches, or
/// hold credentials. The agent reads them but only the user changes them.
const PROTECTED: &[&str] = &[
    "agents",
    "approval",
    "bash_interceptor",
    "browser.allow_attach",
    "browser.allow_install",
    "budget",
    "collab",
    "endpoint",
    "escalation",
    "foreign_sources",
    "gate",
    "hooks",
    "image.allow_weight_edits",
    "integrations",
    "isolation",
    "kernel",
    "keys",
    "mcp",
    "mcp_settings",
    "provider",
    "providers",
    "sandbox",
    "secrets",
    "share",
    "shell",
    "ssh",
    "ssh_hosts",
    "subagents",
    "telemetry",
    "tools",
];

/// Whether the agent must leave setting `key` to the user: a protected key, one inside it, a
/// table that contains one, or anything naming a credential.
pub fn protected(key: &str) -> bool {
    let key = key.trim().to_ascii_lowercase();
    let credential = |part: &str| {
        part.contains("api_key")
            || part.contains("password")
            || part == "token"
            || part.ends_with("_token")
    };
    if key.is_empty() || key.split('.').any(credential) {
        return true;
    }
    PROTECTED.iter().any(|p| {
        key == *p
            || key
                .strip_prefix(p)
                .is_some_and(|rest| rest.starts_with('.'))
            || p.strip_prefix(key.as_str())
                .is_some_and(|rest| rest.starts_with('.'))
    })
}

/// The tools' MCP descriptions.
pub fn tools() -> Vec<Value> {
    let tool = |name: &str, title: &str, read_only: bool, description: &str, schema: Value| {
        json!({
            "name": name,
            "title": title,
            "description": description,
            "inputSchema": schema,
            "annotations": {"title": title, "readOnlyHint": read_only, "openWorldHint": false}
        })
    };
    vec![
        tool(
            "desktop_view",
            "See Dial Desktop",
            true,
            "What the Dial Desktop window shows now, as JSON: project, connection, session, model, \
             approval mode, pending approvals and questions, plan, changed files, the session \
             list, which panels and settings page are open, appearance, and recent notices. \
             `screenshot: true` adds a picture of the window (macOS; the Browser panel's page is \
             drawn separately, use browser_screenshot for it).",
            json!({"type": "object", "properties": {"screenshot": {"type": "boolean"}}}),
        ),
        tool(
            "desktop_commands",
            "List Dial Desktop actions",
            true,
            "Every action of Dial Desktop's command palette available right now (sessions, \
             panels, settings pages, theme, zoom, projects, updates…) with its id. `query` \
             filters and ranks like the palette. Run one with desktop_run.",
            json!({"type": "object", "properties": {"query": {"type": "string"}}}),
        ),
        tool(
            "desktop_run",
            "Run a Dial Desktop action",
            false,
            "Run a palette action by id from desktop_commands, as if the user picked it; the user \
             sees a notice. Returns the window's state afterwards.",
            json!({"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}),
        ),
        tool(
            "desktop_settings",
            "Read Dial settings",
            true,
            "Dial's settings as the Settings page shows them: key, label, tab, value, default, \
             where it is saved, and the allowed choices. Filter by `query` (key, label or \
             description words), `tab`, or `changed: true`. Credentials stay hidden; rows marked \
             `user only` can be read but only the user changes them.",
            json!({"type": "object", "properties": {
                "query": {"type": "string"},
                "tab": {"type": "string"},
                "changed": {"type": "boolean"}
            }}),
        ),
        tool(
            "desktop_set_setting",
            "Change a Dial setting",
            false,
            "Change one setting from desktop_settings, or `reset: true` to restore its default. \
             `scope` is `project` (this project only) or `global`; by default the layer that \
             already sets it, else `project`. Permission, sandbox, shell, MCP, provider and \
             credential settings are the user's and are refused.",
            json!({"type": "object", "properties": {
                "key": {"type": "string"},
                "value": {"description": "The new value, typed as the setting's control expects"},
                "reset": {"type": "boolean"},
                "scope": {"type": "string", "enum": ["project", "global"]}
            }, "required": ["key"]}),
        ),
    ]
}

/// Ask the frontend's `window.__dialDesktop` (see `src/agentDesktop.ts`).
fn frontend(app: &AppHandle, call: &str, args: &Value) -> Result<Value, String> {
    let view = app
        .get_webview("main")
        .ok_or("The Dial Desktop window is not open")?;
    let script = format!(
        "(() => window.__dialDesktop ? window.__dialDesktop.call({}, {}) : \
         JSON.stringify({{ok: false, error: 'The Dial Desktop window is still loading'}}))()",
        json!(call),
        args
    );
    parse_outcome(&crate::browser::eval_view_raw(
        &view,
        &script,
        SCRIPT_TIMEOUT,
    )?)
}

fn pretty(value: &Value) -> String {
    clip(
        &serde_json::to_string_pretty(value).unwrap_or_default(),
        TEXT_LIMIT,
    )
}

/// Run one `desktop_*` tool.
pub fn call(app: &AppHandle, tool: &str, args: &Value) -> Result<Reply, String> {
    match tool {
        "desktop_view" => {
            let text = pretty(&frontend(app, "view", &json!({}))?);
            if args.get("screenshot").and_then(Value::as_bool) == Some(true) {
                return screenshot(app, text);
            }
            Ok(Reply::text(text))
        }
        "desktop_commands" => {
            let query = args.get("query").and_then(Value::as_str).unwrap_or("");
            Ok(Reply::text(pretty(&frontend(
                app,
                "commands",
                &json!({"query": query}),
            )?)))
        }
        "desktop_run" => {
            let id = string_arg(args, "id")?;
            let ran = frontend(app, "run", &json!({"id": id}))?;
            // Let the action land before describing the window.
            std::thread::sleep(Duration::from_millis(300));
            let after = frontend(app, "view", &json!({}))?;
            Ok(Reply::text(format!(
                "{}\nNow:\n{}",
                ran.as_str().unwrap_or("Done"),
                pretty(&after)
            )))
        }
        "desktop_settings" => {
            let view = crate::app::agent_settings(app, SETTINGS_WAIT)?;
            Ok(Reply::text(list_settings(&view.settings, args)))
        }
        "desktop_set_setting" => {
            let key = string_arg(args, "key")?.trim();
            let view = crate::app::agent_settings(app, SETTINGS_WAIT)?;
            let reset = args.get("reset").and_then(Value::as_bool) == Some(true);
            let (scope, value) = plan_change(&view.settings, key, args, reset)?;
            crate::app::agent_save_setting(app, key, value.clone(), scope)?;
            let what = match &value {
                Some(value) => format!("Set {key} to {value} ({scope})"),
                None => format!("Reset {key} to its default ({scope})"),
            };
            let _ = frontend(app, "notice", &json!({"text": format!("Dial: {what}")}));
            Ok(Reply::text(format!(
                "{what}. Read it back with desktop_settings to confirm the engine applied it."
            )))
        }
        other => Err(format!("Unknown tool `{other}`")),
    }
}

/// Check a requested change against the page's row: the scope to save in and the value
/// (`None` resets).
pub fn plan_change(
    rows: &[SettingView],
    key: &str,
    args: &Value,
    reset: bool,
) -> Result<(&'static str, Option<Value>), String> {
    if protected(key) {
        return Err(format!(
            "`{key}` decides what Dial may do or holds credentials; only the user changes it, in Settings"
        ));
    }
    let row = rows
        .iter()
        .find(|r| r.key == key)
        .ok_or_else(|| format!("No setting `{key}`; find keys with desktop_settings"))?;
    if row.redacted {
        return Err(format!(
            "`{key}` holds a credential; only the user changes it"
        ));
    }
    let scope = match args.get("scope").and_then(Value::as_str) {
        Some("global") => "global",
        Some("project") => "project",
        Some(other) => return Err(format!("`scope` is project or global, not {other}")),
        None if row.source.as_deref() == Some("global") => "global",
        None => "project",
    };
    if reset {
        return Ok((scope, None));
    }
    match args.get("value") {
        None | Some(Value::Null) => Err("Give a `value`, or `reset: true`".into()),
        Some(value) => Ok((scope, Some(value.clone()))),
    }
}

/// The rows `args` asks for, one line each.
pub fn list_settings(rows: &[SettingView], args: &Value) -> String {
    let text = |key: &str| {
        args.get(key)
            .and_then(Value::as_str)
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
    };
    let query = text("query");
    let tab = text("tab");
    let changed = args.get("changed").and_then(Value::as_bool) == Some(true);
    let matching: Vec<&SettingView> = rows
        .iter()
        .filter(|r| tab.as_ref().is_none_or(|t| r.tab.to_lowercase() == *t))
        .filter(|r| !changed || r.modified)
        .filter(|r| {
            query.as_ref().is_none_or(|q| {
                let hay = format!("{} {} {}", r.key, r.label, r.description).to_lowercase();
                q.split_whitespace().all(|word| hay.contains(word))
            })
        })
        .collect();
    let filtered = query.is_some() || tab.is_some() || changed;
    let limit = if filtered {
        matching.len()
    } else {
        SETTINGS_LIMIT
    };
    let mut out = String::new();
    for row in matching.iter().take(limit) {
        let value = if row.redacted {
            "(hidden)".to_owned()
        } else {
            row.value.to_string()
        };
        out.push_str(&format!("{} = {value}", row.key));
        if row.modified {
            out.push_str(&format!(
                " (default {}{})",
                row.default,
                row.source
                    .as_deref()
                    .map(|s| format!(", saved in {s}"))
                    .unwrap_or_default()
            ));
        }
        out.push_str(&format!(" [{} › {}] {}", row.tab, row.section, row.label));
        if let Ok(Value::Object(control)) = serde_json::to_value(&row.control)
            && let Some(Value::Array(options)) = control.get("options")
        {
            let values: Vec<String> = options
                .iter()
                .filter_map(|o| o.get("value").map(Value::to_string))
                .collect();
            out.push_str(&format!(" — one of {}", values.join(", ")));
        }
        if row.restart {
            out.push_str(" — applies on engine restart");
        }
        if protected(&row.key) || row.redacted {
            out.push_str(" — user only");
        }
        out.push('\n');
    }
    if matching.len() > limit {
        out.push_str(&format!(
            "…and {} more; filter with query, tab or changed. Tabs: {}\n",
            matching.len() - limit,
            tabs(rows)
        ));
    }
    if out.is_empty() {
        out = format!("No setting matches. Tabs: {}", tabs(rows));
    }
    clip(&out, TEXT_LIMIT)
}

fn tabs(rows: &[SettingView]) -> String {
    let mut seen: Vec<&str> = Vec::new();
    for row in rows {
        if !seen.contains(&row.tab.as_str()) {
            seen.push(&row.tab);
        }
    }
    seen.join(", ")
}

#[cfg(target_os = "macos")]
fn screenshot(app: &AppHandle, text: String) -> Result<Reply, String> {
    let view = app
        .get_webview("main")
        .ok_or("The Dial Desktop window is not open")?;
    let png = super::screenshot::capture(&view, None)?;
    super::screenshot_wire::reply(text, png)
}

#[cfg(not(target_os = "macos"))]
fn screenshot(_app: &AppHandle, text: String) -> Result<Reply, String> {
    Ok(Reply::text(format!(
        "{text}\n(Window screenshots are available on macOS only for now.)"
    )))
}
