use super::*;
use dial_client::{Decision, Event, Outstanding};
use serde_json::json;

fn ready(session: &str) -> Ready {
    Ready {
        session_id: session.into(),
        engine_version: Some("0.3.10".into()),
        model: "claude-sonnet".into(),
        supported_protocol_versions: vec![1, 2],
        methods: vec![],
        capabilities: vec![],
        raw: json!({"event": "ready", "session_id": session, "seq": 1}),
    }
}

fn event(body: Value) -> Update {
    Update::Message(Message::Event(Event {
        kind: body["event"].as_str().unwrap().to_owned(),
        body,
    }))
}

/// Feed the worker's `Requested` for `command` and return its id.
fn requested(model: &mut Model, command: Command, id: u64) -> (RequestId, Vec<Command>) {
    let id = request_id(id);
    let follow = model.apply(Update::Requested { command, id });
    (id, follow)
}

/// `RequestId` has no public constructor; `Event::request_id` is the public way to read one
/// from wire data.
fn request_id(n: u64) -> RequestId {
    Event {
        kind: "x".into(),
        body: json!({"request_id": n}),
    }
    .request_id()
    .unwrap()
}

fn respond(model: &mut Model, id: RequestId, method: &str, result: Value) -> Vec<Command> {
    model.apply(Update::Message(Message::Response {
        id,
        method: method.into(),
        result: Ok(result),
    }))
}

fn prompt(text: &str, images: usize) -> Command {
    Command::Send {
        type_context: None,
        text: text.into(),
        images: vec!["/tmp/shot.png".into(); images],
        inline_images: vec![],
    }
}

#[test]
fn progress_belongs_only_to_the_active_prompt() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    let (id, _) = requested(&mut model, prompt("hello", 0), 7);
    let progress = |session: &str, request: u64, phase: &str| event(json!({
        "event":"model_progress", "session_id":session, "request_id":request,
        "progress":{"phase":phase,"prompt_tokens":20,"processed_tokens":5}
    }));
    model.apply(progress("s1", 7, "processing"));
    model.apply(progress("s2", 7, "generating"));
    model.apply(progress("s1", 8, "generating"));
    let view = serde_json::to_value(model.model_progress).unwrap();
    assert_eq!(view["phase"], "processing");
    assert_eq!(view["processedTokens"], 5);
    respond(&mut model, id, "send", json!({}));
    model.apply(progress("s1", 7, "generating"));
    assert!(model.model_progress.is_none());
}

#[test]
fn a_prompt_with_images_shows_their_count() {
    let mut model = Model::starting();
    let mut ready = ready("s1");
    ready.raw["image_input"] = json!(true);
    model.apply(Update::Connected(Box::new(ready)));
    assert_eq!(model.ready_image_input, Some(true));
    requested(&mut model, prompt("what is this?", 2), 1);
    match model.conversation.entries() {
        [Entry::User(user)] => assert_eq!((user.text.as_str(), user.images), ("what is this?", 2)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_type_turn_shows_the_users_own_words_and_a_literal_marker_stays() {
    let briefing = "<type-context>\nBoard.\n</type-context>";
    for text in ["make it warmer", "[[type:notes]] is literal text"] {
        let mut model = Model::starting();
        model.apply(Update::Connected(Box::new(ready("s1"))));
        let command = Command::Send {
            text: text.into(),
            images: vec![],
            inline_images: vec![],
            type_context: Some(crate::engine::TypeScope::Framed(crate::engine::TypeContext {
                id: "inspiration".into(),
                briefing: briefing.into(),
            })),
        };
        requested(&mut model, command, 1);
        match model.conversation.entries() {
            [Entry::User(user)] => assert_eq!(user.text, text),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn connecting_goes_live_and_asks_for_sessions() {
    let mut model = Model::starting();
    assert_eq!(model.connection(), &Connection::Starting);
    assert!(!model.can_send());
    let follow = model.apply(Update::Connected(Box::new(ready("s1"))));
    assert_eq!(
        follow,
        vec![Command::ListSessions, Command::State, Command::GetSettings, Command::DiscoverModels]
    );
    assert!(
        matches!(model.connection(), Connection::Live { model, .. } if model == "claude-sonnet")
    );
    assert_eq!(model.conversation.session_id(), Some("s1"));
    assert!(model.can_send());
}

#[test]
fn session_listing_follows_the_engine_offset() {
    let mut model = Model::starting();
    let (id, _) = requested(&mut model, Command::ListSessions, 1);
    let follow = respond(
        &mut model,
        id,
        "list_sessions",
        json!({
            "sessions": [{"session_id": "s1"}], "next_offset": 1,
        }),
    );
    let page = follow
        .into_iter()
        .next()
        .expect("the next page must be requested");
    let (_, method, params) = page.request();
    assert_eq!(method, "list_sessions");
    assert_eq!(params["offset"], 1);
    let (page_id, _) = requested(&mut model, page, 2);
    assert!(
        respond(
            &mut model,
            page_id,
            "list_sessions",
            json!({
                "sessions": [{"session_id": "s1"}, {"session_id": "s2"}], "next_offset": null,
            })
        )
        .is_empty()
    );
    assert_eq!(
        model
            .sessions
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>(),
        ["s1", "s2"]
    );
}

#[test]
fn sessions_held_elsewhere_are_marked_and_a_forced_copy_is_explained() {
    let mut model = Model::starting();
    let (id, _) = requested(&mut model, Command::ListSessions, 1);
    respond(
        &mut model,
        id,
        "list_sessions",
        json!({"sessions": [{"session_id": "s1", "open_elsewhere": true}, {"session_id": "s2"}]}),
    );
    assert_eq!(
        model
            .sessions
            .iter()
            .map(|s| s.open_elsewhere)
            .collect::<Vec<_>>(),
        [true, false]
    );
    let (id, _) = requested(&mut model, Command::Resume("s1".into()), 2);
    respond(
        &mut model,
        id,
        "resume",
        json!({"session_id": "s9", "forked_from": "s1"}),
    );
    assert!(
        model
            .toast
            .as_deref()
            .is_some_and(|t| t.contains("opened a copy"))
    );
}

#[test]
fn stale_session_pages_cannot_replace_a_newer_refresh() {
    let mut model = Model::starting();
    let (first, _) = requested(&mut model, Command::ListSessions, 1);
    let mut follow = respond(
        &mut model,
        first,
        "list_sessions",
        json!({
            "sessions": [{"session_id": "old-first"}], "next_offset": 1,
        }),
    );
    let (refresh, _) = requested(&mut model, Command::ListSessions, 2);
    let (stale, _) = requested(&mut model, follow.remove(0), 3);
    respond(
        &mut model,
        refresh,
        "list_sessions",
        json!({"sessions": [{"session_id": "fresh"}]}),
    );
    respond(
        &mut model,
        stale,
        "list_sessions",
        json!({"sessions": [{"session_id": "old-second"}]}),
    );
    assert_eq!(model.sessions.len(), 1);
    assert_eq!(model.sessions[0].id, "fresh");
}

#[test]
fn opening_a_session_does_not_reorder_the_list() {
    let mut model = Model::starting();
    let list = |model: &mut Model, n: u64, rows: Value| {
        let (id, _) = requested(model, Command::ListSessions, n);
        respond(model, id, "list_sessions", json!({"sessions": rows}));
        model.sessions.iter().map(|s| s.id.clone()).collect::<Vec<_>>()
    };
    let row = |id: &str, messages: u64| json!({"session_id": id, "messages": messages});
    assert_eq!(list(&mut model, 1, json!([row("a", 4), row("b", 2), row("c", 9)])), ["a", "b", "c"]);
    // The engine moved `c` up only because it was opened: the order stays.
    assert_eq!(list(&mut model, 2, json!([row("c", 9), row("a", 4), row("b", 2)])), ["a", "b", "c"]);
    // A new session and one that gained messages take the engine's places.
    assert_eq!(
        list(&mut model, 3, json!([row("new", 1), row("b", 3), row("c", 9), row("a", 4)])),
        ["new", "b", "a", "c"]
    );
}

#[test]
fn switching_sessions_waits_for_the_running_turn() {
    // The app opens a busy engine's other sessions in a second engine; should one still refuse,
    // the reason reads plainly.
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    requested(&mut model, prompt("review", 0), 1);
    let (id, _) = requested(&mut model, Command::Resume("s2".into()), 2);
    model.apply(Update::Message(Message::Response {
        id,
        method: "resume".into(),
        result: Err(dial_client::RpcError {
            code: Some("busy".into()),
            message: "agent busy; use steer, abort or approve".into(),
        }),
    }));
    assert_eq!(
        model.toast.as_deref(),
        Some("Could not open the session: Dial is still working; stop the turn first")
    );
}

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|id| (*id).to_owned()).collect()
}

#[test]
fn deleting_other_sessions_asks_directly_and_refreshes_once() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    assert_eq!(
        model.delete_sessions(&ids(&["s2", "s3"])),
        Ok(vec![
            Command::DeleteSession("s2".into()),
            Command::DeleteSession("s3".into())
        ])
    );
    let (a, _) = requested(&mut model, Command::DeleteSession("s2".into()), 1);
    let (b, _) = requested(&mut model, Command::DeleteSession("s3".into()), 2);
    let first = respond(&mut model, a, "delete_session", json!({"session_id": "s2"}));
    assert_eq!(
        first,
        vec![],
        "the list refreshes after the last delete only"
    );
    let last = respond(&mut model, b, "delete_session", json!({"session_id": "s3"}));
    assert_eq!(last, vec![Command::ListSessions]);
}

#[test]
fn stale_history_cannot_restore_a_deleted_session() {
    let mut model = Model::starting();
    let (list, _) = requested(&mut model, Command::ListSessions, 1);
    let (del, _) = requested(&mut model, Command::DeleteSession("gone".into()), 2);
    respond(&mut model, del, "delete_session", json!({"session_id":"gone"}));
    respond(&mut model, list, "list_sessions", json!({"sessions":[{"session_id":"gone"}]}));
    assert!(model.sessions.is_empty());
}

#[test]
fn deleting_the_open_session_moves_off_it_first() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));

    let (turn, _) = requested(&mut model, prompt("review", 0), 1);
    assert!(model.delete_sessions(&ids(&["s1", "s2"])).is_err());
    // Other sessions can go while a turn runs.
    assert!(model.delete_sessions(&ids(&["s2"])).is_ok());
    respond(&mut model, turn, "send", json!({"outcome": "completed"}));

    assert_eq!(
        model.delete_sessions(&ids(&["s1", "s2"])),
        Ok(vec![
            Command::DeleteSession("s2".into()),
            Command::NewSession
        ])
    );
    let (id, _) = requested(&mut model, Command::NewSession, 2);
    let follow = respond(&mut model, id, "new_session", json!({"session_id": "s3"}));
    assert_eq!(
        follow,
        vec![
            Command::DeleteSession("s1".into()),
            Command::ListSessions,
            Command::State
        ]
    );

    // A failed delete says so.
    let (id, _) = requested(&mut model, Command::DeleteSession("s1".into()), 3);
    model.apply(Update::Message(Message::Response {
        id,
        method: "delete_session".into(),
        result: Err(dial_client::RpcError {
            code: Some("busy".into()),
            message: "session is open elsewhere".into(),
        }),
    }));
    assert_eq!(
        model.toast.as_deref(),
        Some("Could not delete the session: Dial is still working; stop the turn first")
    );
}

#[test]
fn renaming_another_session_opens_it_then_renames_it() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));

    // The open session is renamed directly, with its whitespace tidied.
    assert_eq!(
        model.rename_session("s1", "  Rate   limits \n"),
        Ok(vec![Command::Rename("Rate limits".into())])
    );
    assert!(model.rename_session("s1", "   ").is_err());

    // Another one opens first; the rename follows once the engine is on it.
    assert_eq!(
        model.rename_session("s2", "Billing"),
        Ok(vec![Command::Resume("s2".into())])
    );
    let (id, _) = requested(&mut model, Command::Resume("s2".into()), 1);
    model.apply(event(
        json!({"event": "active_session", "session_id": "s2", "seq": 2}),
    ));
    let follow = respond(&mut model, id, "resume", json!({}));
    assert_eq!(
        follow,
        vec![
            Command::Recover,
            Command::Rename("Billing".into()),
            Command::ListSessions,
            Command::State
        ]
    );
    let (id, _) = requested(&mut model, Command::Rename("Billing".into()), 2);
    assert_eq!(
        respond(&mut model, id, "rename", json!({"title": "Billing"})),
        vec![Command::ListSessions]
    );

    // Not while a turn runs here.
    requested(&mut model, prompt("go", 0), 3);
    assert!(model.rename_session("s1", "Other").is_err());
}

#[test]
fn undo_needs_an_idle_turn_and_reloads_the_conversation() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    assert!(model.undo().is_err(), "nothing to undo yet");

    let (turn, _) = requested(&mut model, prompt("edit it", 0), 1);
    assert!(model.undo().is_err(), "not mid-turn");
    respond(&mut model, turn, "send", json!({"outcome": "completed"}));
    assert_eq!(model.undo(), Ok(Command::Undo));

    let (id, _) = requested(&mut model, Command::Undo, 2);
    let follow = respond(
        &mut model,
        id,
        "undo",
        json!({"turns": 1, "files": {"restored": ["a.rs", "b.rs"], "removed": [], "unrestorable": []}}),
    );
    assert_eq!(follow, vec![Command::Recover, Command::ListSessions]);
    assert_eq!(
        model.toast.as_deref(),
        Some("Undid the last turn and restored 2 files")
    );
}

#[test]
fn context_meter_prefers_the_effective_window_with_a_catalog_fallback() {
    let mut model = Model::starting();
    let mut wire = json!({"provider":"mlx", "model_id":"m", "context_window":65536});
    model.models = vec![ModelOption::from_wire(&wire).unwrap()];
    for (reported, expected) in [(json!(8192), 8192), (Value::Null, 65536)] {
        wire["context_window"] = reported;
        model.current_model = CurrentModel::from_wire(&wire);
        assert_eq!(model.context_window(), Some(expected));
    }
}

#[test]
fn context_usage_is_kept_until_the_session_changes() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    model.apply(event(
        json!({"event": "context_usage", "tokens": 1234, "estimated": true, "seq": 2}),
    ));
    assert_eq!(
        model.context,
        Some(ContextUsage {
            tokens: 1234,
            estimated: true
        })
    );
    model.apply(event(
        json!({"event": "active_session", "session_id": "s2", "seq": 3}),
    ));
    assert_eq!(model.context, None);
}

#[test]
fn export_says_where_the_file_went() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    let (id, _) = requested(&mut model, Command::Export("notes/s1.md".into()), 1);
    respond(
        &mut model,
        id,
        "export",
        json!({"path": "notes/s1.md", "format": "markdown"}),
    );
    assert_eq!(model.toast.as_deref(), Some("Exported to notes/s1.md"));
}

#[test]
fn settings_load_and_saved_changes_apply_when_idle() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));

    let (id, _) = requested(&mut model, Command::SettingsSchema, 1);
    respond(
        &mut model,
        id,
        "settings_schema",
        json!({"settings": [{"path": "ui.theme"}]}),
    );
    assert_eq!(model.settings_schema.as_ref().map(Vec::len), Some(1));
    assert!(std::mem::take(&mut model.settings_changed));

    let (id, _) = requested(&mut model, Command::GetSettings, 2);
    respond(
        &mut model,
        id,
        "get_settings",
        json!({"settings": [
            {"key": "ui.theme", "value": "dark", "redacted": false, "scope": "project"},
            {"key": "ui.motion", "value": false, "redacted": false, "scope": "global"},
            {"key": "ui.zoom", "value": 1, "redacted": false, "scope": "elsewhere"},
            {"key": "keys.openai", "value": "***", "redacted": true}
        ]}),
    );
    let saved = model.settings.as_ref().unwrap();
    assert_eq!(saved["ui.theme"].value, json!("dark"));
    assert!(saved["keys.openai"].redacted);
    // The layer that sets each value; older engines (and unknown names) report none.
    assert_eq!(saved["ui.theme"].scope.as_deref(), Some("project"));
    assert_eq!(saved["ui.motion"].scope.as_deref(), Some("global"));
    assert_eq!(saved["ui.zoom"].scope, None);
    assert_eq!(saved["keys.openai"].scope, None);

    // Idle: a save is applied right away.
    let save = Model::save_setting("ui.theme", json!("light"), "global").unwrap();
    let (id, _) = requested(&mut model, save, 3);
    let follow = respond(&mut model, id, "set_setting", json!({"saved": true}));
    assert_eq!(follow, vec![Command::GetSettings, Command::ApplySettings]);

    // Mid-turn: the apply waits for the turn to end.
    let (turn, _) = requested(&mut model, prompt("go", 4), 4);
    let reset = Model::reset_setting("ui.theme", "project").unwrap();
    let (id, _) = requested(&mut model, reset, 5);
    let follow = respond(&mut model, id, "reset_setting", json!({"removed": true}));
    assert_eq!(follow, vec![Command::GetSettings]);
    let follow = respond(&mut model, turn, "send", json!({"outcome": "completed"}));
    assert_eq!(follow, vec![Command::ListSessions, Command::ApplySettings]);

    assert!(Model::save_setting("ui.theme", Value::Null, "global").is_err());
    assert!(Model::reset_setting("ui.theme", "user").is_err());
}

#[test]
fn a_sent_prompt_runs_until_its_own_response_and_then_refreshes_sessions() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    let (turn, _) = requested(&mut model, prompt("fix the bug", 0), 1);
    assert!(model.conversation.turn_running());
    assert!(!model.can_send());
    assert_eq!(model.title(), "fix the bug");

    model.apply(event(json!({"event": "text", "seq": 2, "delta": "Done."})));
    let follow = respond(&mut model, turn, "send", json!({"outcome": "completed"}));
    assert!(!model.conversation.turn_running());
    assert_eq!(follow, vec![Command::ListSessions]);
}

#[test]
fn state_and_models_load_the_model_controls() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    let (state, _) = requested(&mut model, Command::State, 1);
    respond(
        &mut model,
        state,
        "state",
        json!({
            "approval": "ask",
            "current_model": {
                "provider": "anthropic",
                "model_id": "claude-opus",
                "thinking": {"level": null, "default_level": "high", "levels": ["low", "high"], "settable": true}
            }
        }),
    );
    let current = model.current_model.clone().unwrap();
    assert_eq!(current.model_id, "claude-opus");
    assert_eq!(current.thinking, None);
    assert_eq!(current.default_thinking.as_deref(), Some("high"));
    assert_eq!(current.levels, ["low", "high"]);
    assert!(current.thinking_settable);
    assert_eq!(model.approval.as_deref(), Some("ask"));

    let (models, _) = requested(&mut model, Command::DiscoverModels, 2);
    respond(
        &mut model,
        models,
        "list_models",
        json!({"models": [
            {"provider": "anthropic", "model_id": "claude-opus", "label": "Claude Opus", "sources": ["main"], "reasoning": true, "context_window": 1000000},
            {"provider": "mlx", "model_id": "/models/test", "sources": ["local", "favorite"]},
            {"provider": "openai", "model_id": "gpt", "sources": ["catalog"]},
            {"provider": "", "model_id": "broken"}
        ]}),
    );
    assert!(model.models_changed);
    assert_eq!(model.models.len(), 3);
    assert_eq!(model.models[0].label, "Claude Opus");
    assert!(model.models[0].configured);
    assert!(!model.models[0].local);
    assert!(model.models[1].local && model.models[1].configured);
    assert_eq!(model.models[2].label, "gpt");
    assert!(!model.models[2].configured && !model.models[2].local);
}

#[test]
fn discovering_models_asks_for_a_refresh_and_replaces_the_list() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    assert_eq!(
        Command::DiscoverModels.request().1,
        "list_models",
        "discovery uses the same RPC as the offline listing"
    );
    assert_eq!(
        Command::DiscoverModels.request().2,
        json!({"refresh": true})
    );
    let (id, _) = requested(&mut model, Command::DiscoverModels, 1);
    respond(
        &mut model,
        id,
        "list_models",
        json!({"models": [
            {"provider": "google-antigravity", "model_id": "gemini-a", "sources": ["listing"]},
            {"provider": "google-antigravity", "model_id": "gemini-b", "sources": ["listing"]}
        ], "checked": {"providers": 1}}),
    );
    assert!(model.models_changed);
    assert_eq!(model.models.len(), 2);
    assert!(model.models.iter().all(|m| m.listed && !m.catalog_only));
}

#[test]
fn automatic_scans_are_live_only_and_coalesced_until_the_next_interval() {
    let mut model = Model::starting();
    let interval = Duration::from_secs(60);
    assert!(model.discover_if_due(Instant::now(), interval).is_none());
    model.apply(Update::Connected(Box::new(ready("s1"))));
    let first = model.last_discovery.unwrap();
    assert!(model.discover_if_due(first + interval, Duration::ZERO).is_none());
    let (id, _) = requested(&mut model, Command::DiscoverModels, 1);
    respond(&mut model, id, "list_models", json!({"models": []}));
    assert!(model.discover_if_due(first, interval).is_none());
    assert_eq!(model.discover_if_due(first + interval, interval), Some(Command::DiscoverModels));
}

#[test]
fn switching_model_rereads_state_and_failures_toast() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    let switch = Command::SetModel {
        session_id: "s1".into(),
        provider: "openai".into(),
        model_id: "gpt".into(),
    };
    let (id, _) = requested(&mut model, switch, 1);
    let follow = respond(&mut model, id, "set_model", json!({"model": "gpt"}));
    assert_eq!(follow, vec![Command::State]);

    let (id, _) = requested(&mut model, Command::SetThinking { session_id: "s1".into(), level: "max".into() }, 2);
    model.apply(Update::Message(Message::Response {
        id,
        method: "set_thinking".into(),
        result: Err(dial_client::RpcError {
            code: None,
            message: "unknown reasoning level".into(),
        }),
    }));
    assert_eq!(
        model.toast.as_deref(),
        Some("Could not set the reasoning level: unknown reasoning level")
    );

    let (id, _) = requested(&mut model, Command::Resume("s2".into()), 3);
    let follow = respond(&mut model, id, "resume", json!({}));
    assert_eq!(
        follow,
        vec![Command::Recover, Command::ListSessions, Command::State]
    );
}

#[test]
fn resumed_history_is_loading_until_recovery_finishes() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    let loading = |m: &Model| {
        let view = crate::view::Snapshot::new(m, None, 0);
        serde_json::to_value(view).unwrap()["historyLoading"] == true
    };
    assert!(!loading(&model));
    let (id, _) = requested(&mut model, Command::Resume("s2".into()), 1);
    respond(&mut model, id, "resume", json!({"session_id":"s2"}));
    assert!(loading(&model), "an opened session is not empty before recovery arrives");
    let (id, _) = requested(&mut model, Command::Recover, 2);
    assert!(loading(&model));
    respond(&mut model, id, "recovery_snapshot", json!({"session_id":"s2", "seq":2, "messages":[], "busy":false, "pending_approvals":[], "question":null, "todo":[]}));
    assert!(!loading(&model));
}

#[test]
fn opened_and_forked_sessions_load_their_history() {
    // `dial rpc` replays nothing on a switch; the transcript comes from `recovery_snapshot`.
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    let (id, _) = requested(&mut model, Command::ForkSession("s1".into()), 1);
    let follow = respond(&mut model, id, "fork_session", json!({"session_id": "s3"}));
    assert_eq!(
        follow,
        vec![Command::Recover, Command::ListSessions, Command::State]
    );

    // A fresh session has no history to load.
    let (id, _) = requested(&mut model, Command::NewSession, 2);
    let follow = respond(&mut model, id, "new_session", json!({}));
    assert_eq!(follow, vec![Command::ListSessions, Command::State]);
}

#[test]
fn steering_retrying_and_compacting_follow_the_turn() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    assert!(model.retry().is_err(), "no prompt yet");
    assert!(model.compact().is_err(), "nothing to compact");
    assert!(model.steer("more", None).is_err(), "no turn to steer");

    // Guidance goes to the running turn only.
    let (turn, _) = requested(&mut model, prompt("refactor", 0), 1);
    assert_eq!(
        model.steer("  keep the API  ", None),
        Ok(Command::Steer { text: "keep the API".into(), scope: None })
    );
    assert!(model.steer("   ", None).is_err());
    assert!(
        model.retry().is_err() && model.compact().is_err(),
        "not mid-turn"
    );
    let rows = model.conversation.entries().len();
    let note = model.steer("also check tests", None).unwrap();
    let (accepted, _) = requested(&mut model, note, 4);
    respond(&mut model, accepted, "steer", json!({"queued": true}));
    assert!(matches!(model.conversation.entries().last(), Some(Entry::Notice(note))
        if note.kind.as_deref() == Some("steer") && note.message == "also check tests"));
    assert!(model.conversation.turn_running());
    let note = model.steer("keep the API", None).unwrap();
    let (id, _) = requested(&mut model, note, 2);
    model.apply(Update::Message(Message::Response {
        id,
        method: "steer".into(),
        result: Err(dial_client::RpcError {
            code: Some("not_pending".into()),
            message: "no active turn".into(),
        }),
    }));
    assert_eq!(
        model.toast.take().as_deref(),
        Some("Dial finished before your note arrived; send it as a new prompt")
    );
    assert_eq!(model.conversation.entries().len(), rows + 1, "only accepted guidance is shown");
    respond(&mut model, turn, "send", json!({"outcome": "aborted"}));

    // A retry runs as a turn until its own reply arrives.
    assert_eq!(model.retry(), Ok(Command::Retry));
    let (retry, _) = requested(&mut model, Command::Retry, 3);
    assert!(model.conversation.turn_running());
    respond(&mut model, retry, "retry", json!({"outcome": "completed"}));
    assert!(!model.conversation.turn_running());

    // Compaction reloads the rewritten history.
    assert_eq!(model.compact(), Ok(Command::Compact));
    let (id, _) = requested(&mut model, Command::Compact, 4);
    assert_eq!(
        respond(&mut model, id, "compact", json!({})),
        vec![Command::Recover, Command::ListSessions]
    );
}

#[test]
fn session_pages_parse_with_fallback_titles() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    let (id, _) = requested(&mut model, Command::ListSessions, 1);
    respond(
        &mut model,
        id,
        "list_sessions",
        json!({"sessions": [
            {"session_id": "s1", "title": "Refactor parser", "updated": "2026-11-18T10:00:00Z", "messages": 12, "current": true},
            {"session_id": "s2", "title": null, "first_message": "add tests\nfor x", "updated": 1700000000, "pinned": true},
            {"title": "no id is dropped"}
        ], "offset": 0, "total": 3}),
    );
    assert_eq!(model.sessions.len(), 2);
    assert_eq!(model.sessions[0].title, "Refactor parser");
    assert!(model.sessions[0].current);
    assert_eq!(model.sessions[1].title, "add tests");
    assert_eq!(model.sessions[1].updated, "1700000000");
    assert!(model.sessions[1].pinned);
    assert_eq!(model.title(), "Refactor parser");
}

#[test]
fn refused_requests_become_toasts_not_rows() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    let (id, _) = requested(
        &mut model,
        Command::Approve {
            approval_id: "a1".into(),
            decision: Decision::Allow,
        },
        4,
    );
    model.apply(Update::Message(Message::Response {
        id,
        method: "approve".into(),
        result: Err(dial_client::RpcError {
            code: Some("not_pending".into()),
            message: "already answered".into(),
        }),
    }));
    assert_eq!(
        model.toast.as_deref(),
        Some("Approval not applied: not_pending: already answered")
    );
    assert!(model.conversation.entries().is_empty());

    model.apply(Update::RequestFailed {
        command: Command::NewSession,
        error: "queue full".into(),
    });
    assert_eq!(
        model.toast.as_deref(),
        Some("Could not start a session: queue full")
    );
}

#[test]
fn questions_are_acknowledged_once_then_answered() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    let answers = [Answer {
        id: "a".into(),
        selected: vec!["origin".into()],
        custom: Some("  ".into()),
        ..Default::default()
    }];
    assert!(
        model
            .answer_question("q1", Reply::Answers, &answers)
            .is_err()
    );
    model.apply(event(json!({"event": "question", "seq": 2, "request": {
        "question_id": "q1", "generation": 1, "session_id": "s1",
        "questions": [{"id": "a", "question": "Remote?", "options": [{"label": "origin"}]}]
    }})));

    // Answering before the question is on screen would be refused by the engine.
    assert!(
        model
            .answer_question("q1", Reply::Answers, &answers)
            .is_err()
    );
    assert_eq!(model.present_question("other"), None);
    let ack = model.present_question("q1").expect("first acknowledgement");
    assert!(matches!(&ack, Command::QuestionPresented(q) if q.question_id == "q1"));
    assert_eq!(model.present_question("q1"), None, "acknowledged once");

    assert!(
        model
            .answer_question("stale", Reply::Answers, &answers)
            .is_err()
    );
    assert_eq!(
        model
            .answer_question("q1", Reply::Answers, &[])
            .unwrap_err(),
        "Answer every question (missing `a`)"
    );
    let Command::AnswerQuestion { kind, results, .. } = model
        .answer_question("q1", Reply::Answers, &answers)
        .unwrap()
    else {
        panic!("an answer");
    };
    assert_eq!(kind, "answers");
    // Blank free text is not sent.
    assert_eq!(results, [json!({"id": "a", "selectedOptions": ["origin"]})]);
    // Attached images go by path with the answer's free text.
    let pictured = [Answer { images: vec!["/tmp/a.png".into()], ..answers[0].clone() }];
    let Command::AnswerQuestion { results, .. } = model
        .answer_question("q1", Reply::Answers, &pictured)
        .unwrap()
    else {
        panic!("an answer");
    };
    assert_eq!(results, [json!({"id": "a", "selectedOptions": ["origin"], "customInputImages": ["/tmp/a.png"]})]);
    let Command::AnswerQuestion { kind, results, .. } =
        model.answer_question("q1", Reply::Cancel, &[]).unwrap()
    else {
        panic!("a cancel");
    };
    assert_eq!((kind.as_str(), results.len()), ("cancel", 0));

    model.apply(event(
        json!({"event": "question_closed", "seq": 3, "question_id": "q1", "outcome": "answered"}),
    ));
    assert!(model.conversation.question().is_none());
    assert!(model.answer_question("q1", Reply::Cancel, &[]).is_err());
}

#[test]
fn received_mail_preserves_provenance_through_replay_recovery_and_session_switch() {
    use crate::view::{EntryViews, Snapshot, Workspace};
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    let w = Workspace::new("/project", None);
    let mut views = EntryViews::default();
    let mut rows = |model: &Model| serde_json::to_value(Snapshot::reusing(model, Some(&w), 0, &mut views).entries).unwrap();
    let text = "[Message from reviewer]\n<script>untrusted</script>\nlast line";
    let child = json!({"role":"user", "content":text, "dial_child_coordination":true});
    let prompt = json!({"role":"user", "content":text});
    let result = json!({"role":"user", "content":text, "dial_worker_result":"task-1"});
    for message in [&child, &prompt, &result] {
        model.apply(event(json!({"event":"replay_message", "message":message})));
    }
    let journal = |sender: &str| json!({"kind":"received", "source":"journal", "sender":sender, "text":text});
    let user = json!({"kind":"user", "text":text, "images":0});
    assert_eq!(rows(&model), json!([journal("reviewer"), user, journal("task-1")]));

    // Identical client entries at the same indices, but opposite provenance: cached rows
    // may not retain either a worker label or prompt actions after a recovery.
    let (id, _) = requested(&mut model, Command::Recover, 8);
    respond(&mut model, id, "recovery_snapshot", json!({"session_id":"s1", "seq":10,
        "messages":[prompt,result,child], "busy":false, "pending_approvals":[], "question":null,"todo":[]}));
    assert_eq!(rows(&model), json!([user, journal("task-1"), journal("reviewer")]));
    model.apply(event(json!({"event":"active_session", "session_id":"s2"})));
    model.apply(event(json!({"event":"replay_message", "message":prompt})));
    assert_eq!(rows(&model), json!([user]));
    model.apply(event(json!({"event":"notice", "message":"Project peer older-label sent a coordination message.",
        "received_message":{"from":"session-42", "text":text}})));
    assert_eq!(rows(&model)[1], json!({"kind":"received", "source":"live", "sender":"session-42", "text":text}));
}

#[test]
fn lost_events_trigger_one_recovery_once_idle() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    // ready seq is 1; seq 5 is a gap.
    let follow = model.apply(event(json!({"event": "notice", "seq": 5, "message": "hi"})));
    assert_eq!(follow, vec![Command::Recover]);
    let (id, follow) = requested(&mut model, Command::Recover, 9);
    assert!(follow.is_empty(), "no second recovery while one is pending");
    respond(
        &mut model,
        id,
        "recovery_snapshot",
        json!({"session_id": "s1", "seq": 5, "messages": [{"role": "user", "content": "hello"}],
               "busy": false, "pending_approvals": [], "question": null, "todo": []}),
    );
    assert!(!model.conversation.needs_reload());
    assert_eq!(model.conversation.entries().len(), 1);
    assert!(model.toast.is_none());
}

#[test]
fn disconnect_ends_the_connection_and_marks_unknown_outcomes() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    let (id, _) = requested(&mut model, prompt("go", 0), 1);
    model.apply(Update::Message(Message::Disconnected {
        outstanding: vec![Outstanding {
            id,
            method: "send".into(),
            effect: dial_client::Effect::MayHaveApplied,
        }],
    }));
    assert_eq!(model.connection(), &Connection::Ended);
    assert!(!model.can_send());
    assert!(matches!(
        model.conversation.entries().last(),
        Some(Entry::Error(_))
    ));
}

#[test]
fn changed_paths_and_tool_stats_summarize_tool_rows() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    for (seq, body) in [
        json!({"event": "tool_call", "call_id": "c1", "name": "edit", "summary": "src/a.rs"}),
        json!({"event": "tool_result", "call_id": "c1", "text": "ok", "is_error": false, "changed_paths": ["/w/src/a.rs"]}),
        json!({"event": "tool_call", "call_id": "c2", "name": "edit", "summary": "src/a.rs"}),
        json!({"event": "tool_result", "call_id": "c2", "text": "bad", "is_error": true, "changed_paths": ["/w/src/a.rs", "/w/src/b.rs"]}),
        json!({"event": "tool_call", "call_id": "c3", "name": "bash", "summary": "cargo test"}),
    ]
    .into_iter()
    .enumerate()
    {
        let mut body = body;
        body["seq"] = json!(seq as u64 + 2);
        model.apply(event(body));
    }
    assert_eq!(
        model.changed_paths(),
        [("/w/src/a.rs".into(), 2), ("/w/src/b.rs".into(), 1)]
    );
    assert_eq!(
        model.tool_stats(),
        ToolStats {
            running: 1,
            succeeded: 1,
            failed: 1
        }
    );
}

#[test]
fn first_line_clips_by_characters() {
    assert_eq!(first_line("  héllo\nworld", 10), "héllo");
    assert_eq!(first_line("abcdef", 4), "abc…");
    assert_eq!(first_line("", 4), "");
}

#[test]
fn the_structured_type_context_is_negotiated_from_ready_features_on_every_connection() {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(ready("s1"))));
    assert!(!model.type_context_supported, "an engine without the feature gets the legacy framing");
    // A reconnect to an upgraded engine re-negotiates; the flag follows the latest ready frame.
    let mut upgraded = ready("s2");
    upgraded.raw["features"] = json!({"type_context": true});
    model.apply(Update::Connected(Box::new(upgraded)));
    assert!(model.type_context_supported);
    model.apply(Update::Connected(Box::new(ready("s3"))));
    assert!(!model.type_context_supported);
}

#[test]
fn model_switch_only_requests_preparation_when_the_engine_offers_it() {
    let mut model = Model::starting();
    let choice = |provider: &str| Command::SetModel {
        session_id: "s1".into(),
        provider: provider.into(),
        model_id: "model".into(),
    };
    for supported in [true, false] {
        let mut frame = ready("s1");
        if supported {
            frame.raw["features"] = json!({"prepare_local_model": true});
        }
        model.apply(Update::Connected(Box::new(frame)));
        for (provider, should_prepare) in [("mlx", supported), ("openai", false)] {
            let (_, _, params) = choice(provider).negotiated_request(model.prepare_local_model_supported);
            assert_eq!(params.get("prepare_local_model"), should_prepare.then_some(&Value::Bool(true)));
        }
        if supported {
            let (id, _) = requested(&mut model, choice("mlx"), 1);
            let follow = model.apply(Update::Message(Message::Response {
                id,
                method: "set_model".into(),
                result: Err(dial_client::RpcError { code: None, message: "llama-server exited".into() }),
            }));
            assert_eq!(follow, [Command::State]);
            assert!(model.toast.as_deref().unwrap().contains("llama-server exited"));
        }
    }
}
