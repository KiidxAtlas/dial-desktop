use super::*;

#[test]
fn commands_map_to_documented_wire_requests() {
    let (purpose, method, params) = Command::Send {
        type_context: None,
        text: "hi".into(),
        images: vec![],
        inline_images: vec![],
    }
    .request();
    assert_eq!((purpose, method), (Purpose::Turn, "send"));
    assert_eq!(
        params,
        json!({"text": "hi"}),
        "a plain prompt names no attachments"
    );

    let (_, _, params) = Command::Send {
        type_context: None,
        text: "  \n".into(),
        images: vec!["/tmp/a.png".into()],
        inline_images: vec!["iVBORw==".into()],
    }
    .request();
    let prompt = params["text"].as_str().unwrap();
    for instruction in ["infer my intent", "take the relevant action", "ask a focused question"] {
        assert!(prompt.contains(instruction), "missing instruction: {instruction}");
    }
    assert_eq!(params["images"], json!(["/tmp/a.png"]));
    assert_eq!(params["inline_images"], json!(["iVBORw=="]));

    let (_, _, params) = Command::Send {
        type_context: None,
        text: "Only describe the layout.".into(),
        images: vec!["/tmp/a.png".into()],
        inline_images: vec![],
    }
    .request();
    assert_eq!(params["text"], "Only describe the layout.");

    let (purpose, method, params) = Command::Approve {
        approval_id: "a1".into(),
        decision: Decision::Always,
    }
    .request();
    assert_eq!((purpose, method), (Purpose::Approve, "approve"));
    assert_eq!(params, json!({"approval_id": "a1", "decision": "always"}));

    let (_, method, params) = Command::Resume("s9".into()).request();
    assert_eq!(method, "resume");
    assert_eq!(params, json!({"session_id": "s9"}));

    let (purpose, method, params) = Command::DeleteSession("s4".into()).request();
    assert_eq!(
        (purpose, method),
        (Purpose::DeleteSession, "delete_session")
    );
    assert_eq!(params, json!({"session_id": "s4"}));

    assert_eq!(Command::ListSessions.request().1, "list_sessions");
    assert_eq!(Command::ListSessions.request().2["include_empty"], true);
    assert_eq!(Command::NewSession.request().1, "new_session");
    assert_eq!(Command::Abort.request().1, "abort");
    assert_eq!(Command::Recover.request().1, "recovery_snapshot");

    let (purpose, method, params) = Command::Steer { text: "use the v2 API".into(), scope: None }.request();
    assert_eq!((purpose, method), (Purpose::Steer, "steer"));
    assert_eq!(params, json!({"text": "use the v2 API"}));
    // A retry is a turn, settled by its own reply.
    assert_eq!(
        Command::Retry.request(),
        (Purpose::Turn, "retry", json!({}))
    );
    assert_eq!(
        Command::Compact.request(),
        (Purpose::Compact, "compact", json!({"reason": "manual"}))
    );

    // The file name picks the export format; the path goes through unchanged.
    for (path, format) in [("notes/s1.md", "markdown"), ("s1.html", "html"), ("out/S1.HTM", "html"), ("s1", "markdown")] {
        let (purpose, method, params) = Command::Export(path.into()).request();
        assert_eq!((purpose, method), (Purpose::Export, "export"));
        assert_eq!(params, json!({"format": format, "path": path}));
    }
}

#[test]
fn question_commands_carry_the_correlation() {
    let question =
        QuestionRef::of(&json!({"question_id": "q1", "generation": 3, "session_id": "s1"}))
            .unwrap();
    let correlation = json!({"question_id": "q1", "generation": 3, "session_id": "s1"});
    let (purpose, method, params) = Command::QuestionPresented(question.clone()).request();
    assert_eq!(
        (purpose, method),
        (Purpose::QuestionPresented, "question_presented")
    );
    assert_eq!(params, correlation);

    let result = json!({"id": "a", "selectedOptions": ["origin"]});
    let (purpose, method, params) = Command::AnswerQuestion {
        question: question.clone(),
        kind: "answers".into(),
        results: vec![result.clone()],
    }
    .request();
    assert_eq!((purpose, method), (Purpose::Answer, "question_answer"));
    assert_eq!(
        params,
        json!({"question_id": "q1", "generation": 3, "session_id": "s1", "kind": "answers", "results": [result]})
    );
    // The engine rejects results on cancel, so none are sent.
    let (_, _, params) = Command::AnswerQuestion {
        question,
        kind: "cancel".into(),
        results: vec![],
    }
    .request();
    assert_eq!(params.get("results"), None);

    assert_eq!(
        QuestionRef::of(&json!({"question_id": "q1", "session_id": "s1"})),
        None
    );
}

#[test]
fn every_decision_has_its_wire_name() {
    let names: Vec<_> = [
        Decision::Allow,
        Decision::Always,
        Decision::Project,
        Decision::Deny,
    ]
    .into_iter()
    .map(decision_str)
    .collect();
    assert_eq!(names, ["allow", "always", "project", "deny"]);
    for name in names {
        assert_eq!(parse_decision(name).map(decision_str), Some(name));
    }
    assert_eq!(parse_decision("maybe"), None);
}

#[test]
fn a_missing_engine_reports_connect_failure_and_wakes_the_ui() {
    let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = Arc::clone(&woken);
    let launch = Launch::new("/nonexistent/dial-engine-for-tests");
    let mut engine = Engine::spawn(
        launch,
        Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
    );
    let update = engine
        .updates
        .recv_timeout(Duration::from_secs(10))
        .expect("an update");
    assert!(matches!(update, Update::ConnectFailed(_)), "{update:?}");
    engine.shutdown();
    assert!(woken.load(Ordering::SeqCst) >= 1);
}

#[test]
fn presets_do_not_write_to_a_changed_session_or_continue_after_model_failure() {
    let main = || Some(PresetModel { provider: "p".into(), model: "m".into(), thinking: Some("high".into()) });
    let mut calls = Vec::new();
    let changed = apply_preset(&mut |method, _| {
        calls.push(method.to_owned());
        Ok(json!({"session_id":"other"}))
    }, "target", json!({}), main());
    assert!(changed.is_err());
    assert_eq!(calls, ["get_session_stats"]);

    calls.clear();
    let rejected = apply_preset(&mut |method, _| {
        calls.push(method.to_owned());
        if method == "set_model" { Err("Unknown model".into()) }
        else { Ok(json!({"session_id":"target"})) }
    }, "target", json!({}), main());
    assert_eq!(rejected, Err("Unknown model".into()));
    assert_eq!(calls, ["get_session_stats", "set_setting", "apply_settings", "set_model"]);
}

#[test]
fn a_type_context_an_older_engine_cannot_take_is_framed_in_the_text() {
    let briefing = "<type-context>\nBoard.\n</type-context>";
    let (_, _, params) = Command::Send {
        text: "make it warmer".into(),
        images: vec![],
        inline_images: vec![],
        type_context: Some(TypeScope::Framed(TypeContext {
            id: "inspiration".into(),
            briefing: briefing.into(),
        })),
    }
    .request();
    assert_eq!(params["text"], format!("[[type:inspiration]]\n{briefing}\nmake it warmer"));
    assert!(params.get("type_context").is_none());
}

#[test]
fn a_modern_plain_turn_sends_null_and_keeps_an_exact_legacy_literal_as_typed() {
    // Ordinary modern turn: its multiline text is exactly the old framing shape, and it must reach the engine intact.
    let literal = "[[type:notes]]\n<type-context>\nlooks like framing\n</type-context>\nliteral";
    let (_, _, params) = Command::Send {
        text: literal.into(),
        images: vec![],
        inline_images: vec![],
        type_context: Some(TypeScope::Plain),
    }
    .request();
    assert_eq!(params["text"], literal);
    assert_eq!(params["type_context"], Value::Null);
}

#[test]
fn an_engine_that_predates_the_field_gets_no_type_key_for_a_plain_turn() {
    let (_, _, params) = Command::Send {
        text: "hi".into(),
        images: vec![],
        inline_images: vec![],
        type_context: None,
    }
    .request();
    assert_eq!(params["text"], "hi");
    assert!(params.get("type_context").is_none());
}
#[test]
fn a_structured_type_context_rides_beside_the_unchanged_text() {
    let briefing = "<type-context>\nBoard.\n</type-context>";
    let (purpose, method, params) = Command::Send {
        text: "make it warmer".into(),
        images: vec![],
        inline_images: vec![],
        type_context: Some(TypeScope::Typed(TypeContext {
            id: "inspiration".into(),
            briefing: briefing.into(),
        })),
    }
    .request();
    assert_eq!((purpose, method), (Purpose::Turn, "send"));
    assert_eq!(params["text"], "make it warmer");
    assert_eq!(
        params["type_context"],
        json!({"id": "inspiration", "briefing": briefing})
    );
    let (_, _, plain) = Command::Send {
        text: "hi".into(),
        images: vec![],
        inline_images: vec![],
        type_context: None,
    }
    .request();
    assert!(plain.get("type_context").is_none(), "a plain prompt names no Type");
}
