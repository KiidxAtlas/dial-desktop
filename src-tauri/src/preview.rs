//! A built-in session for designing and reviewing the interface without an engine.
//!
//! It is fed through the same [`dial_client::Conversation`] reducer as live events, so what it
//! shows is what those events produce.

use crate::cli::Preview;
use crate::model::{ContextUsage, CurrentModel, Model, ModelOption, SessionSummary};
use crate::time;
use dial_client::Event;
use serde_json::{Value, json};

#[cfg(test)]
mod tests;

const SESSION: &str = "preview-session";

/// A model for the chosen preview. `workspace` roots the example file paths.
pub fn model(kind: Preview, workspace: &str) -> Model {
    let mut model = Model::preview();
    model.sessions = sessions(time::now());
    model.approval = Some("ask".into());
    model.current_model = Some(CurrentModel {
        provider: "anthropic".into(),
        model_id: "claude-sonnet-4-5".into(),
        thinking: None,
        default_thinking: Some("medium".into()),
        levels: ["low", "medium", "high"].map(String::from).to_vec(),
        thinking_settable: true,
    });
    model.models = models();
    model.models_changed = true;
    if kind == Preview::Empty {
        model.sessions.retain(|s| s.id != SESSION);
        return model;
    }
    model.context = Some(ContextUsage {
        tokens: 48_200,
        estimated: false,
    });
    let conversation = &mut model.conversation;
    let mut feed = |body: Value| {
        let kind = body["event"].as_str().unwrap_or_default().to_owned();
        conversation.apply_event(&Event { kind, body });
    };
    feed(json!({"event": "ready", "session_id": SESSION}));
    let path = |rel: &str| format!("{workspace}/{rel}");
    conversation_turn(&mut feed, &path);
    model.conversation.push_user(
        "Now run the limiter tests and push the branch when they pass.",
        0,
    );
    let conversation = &mut model.conversation;
    let mut feed = |body: Value| {
        let kind = body["event"].as_str().unwrap_or_default().to_owned();
        conversation.apply_event(&Event { kind, body });
    };
    second_turn(&mut feed, kind);
    model
}

fn conversation_turn(feed: &mut impl FnMut(Value), path: &impl Fn(&str) -> String) {
    feed(
        json!({"event": "reasoning", "delta": "Requests enter through the router; a middleware layer is the least invasive place for a limiter. I should check whether a clock abstraction exists so the tests stay deterministic."}),
    );
    feed(
        json!({"event": "text", "delta": "I'll trace how requests reach the handlers first, then add the limiter as middleware."}),
    );
    tool(
        feed,
        "c1",
        "read",
        "Read",
        "src/server/router.rs",
        "tools",
        Some((true, "118 lines", vec![])),
    );
    tool(
        feed,
        "c2",
        "grep",
        "Search",
        "\"Clock|Instant::now\" in src/",
        "tools",
        Some((true, "src/util/clock.rs:4: pub trait Clock", vec![])),
    );
    feed(
        json!({"event": "text", "delta": "The router builds one middleware chain in `Router::new`, and there is already a `Clock` trait in `src/util/clock.rs`. My plan:\n\n1. Add a **token bucket** per client key in `limiter.rs`\n2. Wire it into the chain before authentication\n3. Return `429` with a `Retry-After` header\n\n```rust\npub struct Limiter<C: Clock> {\n    buckets: HashMap<ClientKey, Bucket>,\n    rate: Rate,\n    clock: C,\n}\n```"}),
    );
    feed(json!({"event": "todo", "tasks": [
        {"content": "Trace request routing", "status": "completed"},
        {"content": "Add token-bucket limiter middleware", "status": "completed"},
        {"content": "Cover limits and Retry-After with tests", "status": "in_progress"},
        {"content": "Document the new `rate_limit` settings", "status": "pending"}
    ]}));
    tool(
        feed,
        "c3",
        "edit",
        "Edit",
        "src/server/limiter.rs",
        "edit",
        Some((
            true,
            "Created src/server/limiter.rs (+86)",
            vec![path("src/server/limiter.rs")],
        )),
    );
    tool(
        feed,
        "c4",
        "edit",
        "Edit",
        "src/server/router.rs",
        "edit",
        Some((
            true,
            "src/server/router.rs (+4 -1)",
            vec![path("src/server/router.rs")],
        )),
    );
    tool(
        feed,
        "c5",
        "bash",
        "Run",
        "cargo check -p server",
        "shell",
        Some((
            false,
            "error[E0599]: no method named `retry_after` found for struct `Bucket`",
            vec![],
        )),
    );
    tool(
        feed,
        "c6",
        "edit",
        "Edit",
        "src/server/limiter.rs",
        "edit",
        Some((
            true,
            "src/server/limiter.rs (+9)",
            vec![path("src/server/limiter.rs")],
        )),
    );
    // A generated image renders inline; the app icon stands in when previewing this repository.
    tool(
        feed,
        "c6g",
        "image_gen",
        "Generate image",
        "src-tauri/icons/128x128@2x.png",
        "tools",
        Some((
            true,
            "Wrote src-tauri/icons/128x128@2x.png (256x256)",
            vec![path("src-tauri/icons/128x128@2x.png")],
        )),
    );
    feed(
        json!({"event": "text", "delta": "The limiter is in place and the crate builds. Requests over the limit now get `429 Too Many Requests` with a `Retry-After` header."}),
    );
}

fn second_turn(feed: &mut impl FnMut(Value), kind: Preview) {
    feed(
        json!({"event": "notice", "message": "Context compacted · 42k tokens freed", "lane": "divider"}),
    );
    tool(
        feed,
        "c7",
        "bash",
        "Run",
        "cargo test -p server limiter",
        "shell",
        None,
    );
    feed(
        json!({"event": "tool_output", "call_id": "c7", "name": "bash", "delta": "running 12 tests\ntest limiter::allows_burst ... ok\ntest limiter::refills_over_time ... ok\n"}),
    );
    feed(
        json!({"event": "tool_progress", "call_id": "c7", "name": "bash", "text": "8 of 12 tests passed"}),
    );
    if kind == Preview::Question {
        feed(
            json!({"event": "question", "presentation_ack_required": true, "request": {
                "question_id": "preview-question", "generation": 1, "session_id": SESSION,
                "timeout_seconds": 0, "notify": true,
                "questions": [
                    {"id": "remote", "header": "Push", "question": "Which remote should the branch go to?",
                     "options": [
                         {"label": "origin", "description": "github.com/acme/api (your fork)"},
                         {"label": "upstream", "description": "github.com/acme-org/api"}
                     ], "recommended": 0},
                    {"id": "follow-up", "header": "After pushing", "question": "Anything else once the branch is up?",
                     "options": [
                         {"label": "Open a draft PR"},
                         {"label": "Request review from @platform"},
                         {"label": "Update CHANGELOG.md"}
                     ], "multi": true}
                ]
            }}),
        );
        return;
    }
    feed(
        json!({"event": "approval", "approval_id": "preview-approval", "tool": "bash", "summary": "git push -u origin feature/rate-limit", "call_id": "c8"}),
    );
}

/// One tool call; `result` is `(succeeded, text, changed paths)` or `None` while it runs.
fn tool(
    feed: &mut impl FnMut(Value),
    id: &str,
    name: &str,
    label: &str,
    target: &str,
    tone: &str,
    result: Option<(bool, &str, Vec<String>)>,
) {
    feed(json!({"event": "tool_call", "call_id": id, "name": name, "summary": target}));
    feed(
        json!({"event": "tool_card", "call_id": id, "label": label, "target": target, "tone": tone}),
    );
    if let Some((ok, text, changed)) = result {
        feed(json!({
            "event": "tool_result", "call_id": id, "name": name, "text": text,
            "is_error": !ok, "status": if ok { "ok" } else { "error" }, "changed_paths": changed
        }));
    }
}

fn sessions(now: i64) -> Vec<SessionSummary> {
    let at = |ago: i64| {
        let t = now - ago;
        // RFC 3339 is what the engine sends; a plain number parses the same way.
        t.to_string()
    };
    let row = |id: &str, title: &str, ago: i64, messages: u64, pinned: bool| SessionSummary {
        id: id.to_owned(),
        title: title.to_owned(),
        updated: at(ago),
        messages,
        pinned,
        current: id == SESSION,
    };
    vec![
        row(
            "preview-pinned",
            "Release checklist for 0.4",
            3 * 86_400,
            31,
            true,
        ),
        row(SESSION, "Rate limiting for the public API", 40, 24, false),
        row(
            "preview-2",
            "Fix flaky websocket reconnect test",
            2 * 3600,
            18,
            false,
        ),
        row(
            "preview-3",
            "Explain the billing reconciliation job",
            26 * 3600,
            9,
            false,
        ),
        row(
            "preview-4",
            "Migrate settings to the new schema",
            4 * 86_400,
            42,
            false,
        ),
        row(
            "preview-5",
            "Review PR #212: search indexing",
            12 * 86_400,
            15,
            false,
        ),
        row(
            "preview-6",
            "Profile slow startup on Windows",
            40 * 86_400,
            27,
            false,
        ),
    ]
}

/// Models for the picker: a few configured ones and a catalog.
fn models() -> Vec<ModelOption> {
    let row = |provider: &str, id: &str, label: &str, window: u64, configured: bool| ModelOption {
        provider: provider.into(),
        model_id: id.into(),
        label: label.into(),
        context_window: Some(window),
        reasoning: true,
        vision: true,
        configured,
    };
    vec![
        row(
            "anthropic",
            "claude-sonnet-4-5",
            "Claude Sonnet 4.5",
            200_000,
            true,
        ),
        row(
            "anthropic",
            "claude-opus-4-1",
            "Claude Opus 4.1",
            200_000,
            true,
        ),
        row("openai", "gpt-5", "GPT-5", 400_000, true),
        row(
            "anthropic",
            "claude-haiku-4-5",
            "Claude Haiku 4.5",
            200_000,
            false,
        ),
        row("openai", "gpt-5-mini", "GPT-5 mini", 400_000, false),
        row(
            "google",
            "gemini-2.5-pro",
            "Gemini 2.5 Pro",
            1_000_000,
            false,
        ),
        row(
            "ollama",
            "qwen3-coder:30b",
            "qwen3-coder:30b",
            262_144,
            false,
        ),
    ]
}
