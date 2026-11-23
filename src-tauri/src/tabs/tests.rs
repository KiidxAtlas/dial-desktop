use super::*;
use crate::engine::{Command, Update};
use dial_client::{Event, Message, Ready, RequestId};
use serde_json::json;

fn request_id(n: u64) -> RequestId {
    Event {
        kind: "x".into(),
        body: json!({"request_id": n}),
    }
    .request_id()
    .unwrap()
}

/// A connected model on `session`; `busy` leaves a turn running (request id 1).
fn model(session: &str, busy: bool) -> Model {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(Ready {
        session_id: session.into(),
        engine_version: None,
        model: "m".into(),
        supported_protocol_versions: vec![1],
        methods: vec![],
        capabilities: vec![],
        raw: json!({"event": "ready", "session_id": session, "seq": 1}),
    })));
    if busy {
        model.apply(Update::Requested {
            command: Command::Send {
                type_context: None,
                text: format!("work in {session}"),
                images: vec![],
                inline_images: vec![],
            },
            id: request_id(1),
        });
    }
    model
}

fn finish(model: &mut Model) {
    model.apply(Update::Message(Message::Response {
        id: request_id(1),
        method: "send".into(),
        result: Ok(json!({"outcome": "completed"})),
    }));
}

fn tab(session: &str, busy: bool) -> Tab {
    Tab::new(model(session, busy), None)
}

#[test]
fn busy_tab_keeps_latest_model_and_effort_until_both_settle() {
    let mut tabs = Tabs::new(tab("one", true));
    let model = Command::SetModel {
        session_id: "one".into(),
        provider: "p".into(),
        model_id: "new".into(),
    };
    let thinking = |level: &str| Command::SetThinking {
        session_id: "one".into(),
        level: level.into(),
    };
    let high = thinking("high");
    for edit in [model.clone(), thinking("low"), high.clone()] {
        assert_eq!(tabs.active_mut().select_model_setting(edit).unwrap(), None);
    }
    tabs.open(tab("two", false));
    let reply = |method: &str, n| {
        Update::Message(Message::Response {
            id: request_id(n),
            method: method.into(),
            result: Ok(json!({})),
        })
    };
    assert!(tabs.tabs[0].apply(reply("send", 1)).contains(&model));
    assert!(tabs.close_idle().is_empty());
    assert_eq!(tabs.tabs[0].apply(reply("set_model", 2)), [high]);
    assert!(tabs.active().edits.is_empty() && tabs.active().editing.is_none());
    tabs.tabs[0].apply(reply("set_thinking", 3));
    assert_eq!(tabs.close_idle().len(), 1);
}

#[test]
fn an_idle_tab_opens_sessions_in_place() {
    let tabs = Tabs::new(tab("s1", false));
    assert_eq!(tabs.route_resume("s2"), Route::Here);
    assert_eq!(tabs.route_new(), Route::Here);
}

#[test]
fn a_busy_tab_keeps_working_while_another_session_opens() {
    let mut tabs = Tabs::new(tab("s1", true));
    assert_eq!(tabs.route_resume("s2"), Route::Spawn);
    assert_eq!(tabs.route_new(), Route::Spawn);

    tabs.open(tab("s2", false));
    assert_eq!((tabs.len(), tabs.active_index()), (2, 1));
    // The running session is reached through its own engine, never resumed a second time.
    assert_eq!(tabs.route_resume("s1"), Route::Switch(0));
    assert!(tabs.delete_refusal(&["s9".into(), "s1".into()]).is_some());
    assert_eq!(tabs.delete_refusal(&["s2".into()]), None);

    let activity = tabs.activity();
    assert!(activity["s1"].running);
    assert_eq!(activity["s1"].title, "work in s1");
    assert!(!activity["s2"].running);
    // A busy background tab stays open.
    assert!(tabs.close_idle().is_empty());
}

#[test]
fn saved_settings_wait_for_their_own_turn_not_another_session() {
    let mut tabs = Tabs::new(tab("first", true));
    assert_eq!(
        tabs.active_mut().saved_settings(),
        vec![Command::GetSettings]
    );
    tabs.open(tab("second", false));
    assert_eq!(
        tabs.active_mut().saved_settings(),
        vec![Command::GetSettings, Command::ApplySettings]
    );
    let first = tabs.get_mut(0).unwrap();
    let follow = first.apply(Update::Message(Message::Response {
        id: request_id(1),
        method: "send".into(),
        result: Ok(json!({"outcome":"completed"})),
    }));
    assert_eq!(follow, vec![Command::ListSessions, Command::ApplySettings]);
}

#[test]
fn a_background_tab_reports_when_done_and_then_closes() {
    let mut tabs = Tabs::new(tab("s1", true));
    tabs.open(tab("s2", false));
    let before = tabs.busy_states();
    for tab in tabs.iter_mut() {
        if tab.model.conversation.session_id() == Some("s1") {
            finish(&mut tab.model);
        }
    }
    assert_eq!(tabs.background_news(&before), ["“work in s1” finished"]);
    let closed = tabs.close_idle();
    assert_eq!(closed.len(), 1);
    assert_eq!((tabs.len(), tabs.active_index()), (1, 0));
    assert_eq!(tabs.active().model.conversation.session_id(), Some("s2"));
}

#[test]
fn session_notices_target_the_waiting_session_and_do_not_repeat() {
    let mut tabs = Tabs::new(tab("s1", true));
    tabs.open(tab("s2", false));
    for tab in tabs.iter_mut().filter(|t| t.session() == Some("s1")) {
        tab.apply(Update::Message(Message::Event(Event {
            kind: "question".into(),
            body: json!({"event": "question", "seq": 2, "request": {
                "question_id": "q1", "generation": 1, "session_id": "s1",
                "questions": [{"id": "a", "question": "Remote?", "options": [{"label": "origin"}]}]
            }}),
        })));
    }
    assert_eq!(
        tabs.session_notices(),
        [SessionNotice {
            session_id: "s1".into(),
            kind: NoticeKind::Question,
        }]
    );
    assert!(tabs.session_notices().is_empty());
}

#[test]
fn a_completed_turn_notifies_once_but_an_engine_exit_does_not() {
    let mut tabs = Tabs::new(tab("s1", false));
    tabs.active_mut().apply(Update::Requested {
        command: Command::Send {
            type_context: None,
            text: "quick turn".into(),
            images: vec![],
            inline_images: vec![],
        },
        id: request_id(1),
    });
    // Both updates may have arrived in the same frame.
    tabs.active_mut().apply(Update::Message(Message::Response {
        id: request_id(1),
        method: "send".into(),
        result: Ok(json!({"outcome": "completed"})),
    }));
    assert_eq!(
        tabs.session_notices(),
        [SessionNotice {
            session_id: "s1".into(),
            kind: NoticeKind::Finished,
        }]
    );
    assert!(tabs.session_notices().is_empty());
    let mut disconnected = Tabs::new(tab("s2", true));
    disconnected
        .active_mut()
        .apply(Update::ConnectFailed("test exit".into()));
    assert!(disconnected.session_notices().is_empty());
}

#[test]
fn switching_back_leaves_the_idle_tab_to_close() {
    let mut tabs = Tabs::new(tab("s1", true));
    tabs.open(tab("s2", false));
    tabs.switch(0);
    assert!(tabs.active().model.models_changed);
    assert_eq!(tabs.close_idle().len(), 1);
    assert_eq!(tabs.active().model.conversation.session_id(), Some("s1"));
    assert_eq!(tabs.active_index(), 0);
}

#[test]
fn a_new_tab_starts_with_the_sidebar_on_screen() {
    let mut first = tab("s1", true);
    first.model.sessions = vec![crate::model::SessionSummary {
        id: "s1".into(),
        title: "One".into(),
        updated: String::new(),
        messages: 1,
        pinned: false,
        current: true,
        open_elsewhere: false,
    }];
    let mut tabs = Tabs::new(first);
    tabs.open(Tab::new(Model::starting(), None));
    assert_eq!(tabs.active().model.sessions.len(), 1);
}

#[test]
fn a_session_cannot_be_resumed_while_its_previous_engine_shuts_down() {
    let mut tabs = Tabs::new(tab("s1", true));
    tabs.open(tab("s2", false));
    finish(&mut tabs.tabs[0].model);
    let closed = tabs.close_idle();
    assert_eq!(closed.len(), 1);
    assert_eq!(tabs.route_resume("s1"), Route::Closing);
    drop(closed);
    assert_eq!(tabs.route_resume("s1"), Route::Here);
}

#[test]
fn a_session_held_by_another_dial_process_is_not_resumed_into_a_copy() {
    let mut here = tab("s1", false);
    here.model.sessions = ["s2", "s3"]
        .into_iter()
        .map(|id| crate::model::SessionSummary {
            id: id.into(),
            title: "Same title".into(),
            updated: String::new(),
            messages: 4,
            pinned: false,
            current: false,
            open_elsewhere: id == "s2",
        })
        .collect();
    let mut tabs = Tabs::new(here);
    // Resuming would make the engine fork a sibling with the same title on every click.
    assert_eq!(tabs.route_resume("s2"), Route::Elsewhere);
    assert_eq!(tabs.route_resume("s3"), Route::Here);
    // A session another tab of this window runs is still reached through that tab.
    tabs.open(tab("s2", false));
    assert_eq!(tabs.route_resume("s2"), Route::Switch(1));
}

#[test]
fn repeated_opens_use_the_same_engine_before_resume_completes() {
    let mut tabs = Tabs::new(tab("s1", true));
    let mut opening = Tab::new(Model::starting(), None);
    opening.opening = Some(("s2".into(), None));
    tabs.open(opening);
    assert_eq!(tabs.route_resume("s2"), Route::Switch(1));
    assert_eq!(tabs.route_resume("s1"), Route::Switch(0));
    assert_eq!(tabs.route_resume("s3"), Route::Spawn);
    tabs.switch(0);
    assert!(tabs.close_idle().is_empty());

    let id = request_id(2);
    tabs.tabs[1].apply(Update::Requested {
        command: Command::Resume("s2".into()),
        id,
    });
    assert_eq!(tabs.route_resume("s2"), Route::Switch(1));
    tabs.tabs[1].apply(Update::Message(Message::Response {
        id,
        method: "resume".into(),
        result: Ok(json!({})),
    }));
    assert_eq!(tabs.route_resume("s2"), Route::Spawn);
}

#[test]
fn switching_away_from_a_queued_send_preserves_its_engine() {
    let mut tabs = Tabs::new(tab("s1", false));
    tabs.active_mut().turn_queued = true;
    assert_eq!(tabs.route_resume("s2"), Route::Spawn);
    tabs.open(tab("s2", false));
    assert!(tabs.close_idle().is_empty());
    assert_eq!(tabs.route_resume("s1"), Route::Switch(0));
}

#[test]
fn reconnect_keeps_its_target_after_a_failed_engine_start() {
    let mut restoring = Tab::new(Model::starting(), None);
    restoring.opening = Some(("original".into(), None));
    restoring.restore_target = Some("original".into());
    assert!(restoring.protects_work());
    restoring.apply(Update::ConnectFailed("engine unavailable".into()));
    assert!(!restoring.protects_work());
    assert_eq!(restoring.reconnect_target(), Some("original"));
}

#[test]
fn model_removal_is_refused_while_background_work_is_running() {
    let mut tabs = Tabs::new(tab("working", true));
    tabs.open(tab("foreground", false));
    assert!(tabs.check_model_removal("unused").is_err());
    finish(&mut tabs.tabs[0].model);
    assert!(tabs.check_model_removal("unused").is_ok());
    tabs.active_mut().model.current_model = Some(crate::model::CurrentModel {
        model_id: "selected".into(),
        ..Default::default()
    });
    assert!(tabs.check_model_removal("selected").is_err());
}

#[test]
fn restart_reservation_rechecks_terminals_and_all_tabs_before_blocking_work() {
    use RestartDecision::*;
    let mut tabs = Tabs::new(tab("running", true));
    tabs.open(tab("idle", false));
    assert_eq!(tabs.reserve_restart(vec![], &[]), Ok(Busy));
    finish(&mut tabs.tabs[0].model);
    // Queued before an engine request ID (or session id in the first place) exists.
    let mut queued = Tab::new(Model::starting(), None);
    queued.turn_queued = true;
    tabs.add_background(queued);
    assert_eq!(tabs.reserve_restart(vec![], &[]), Ok(Busy));
    tabs.tabs.last_mut().unwrap().turn_queued = false;
    assert_eq!(tabs.reserve_restart(vec![1], &[]), Ok(ConfirmTerminals { ids: vec![1] }));
    assert_eq!(tabs.reserve_restart(vec![2], &[1]), Ok(ConfirmTerminals { ids: vec![2] }));
    tabs.active_mut().model.voice.start(None);
    assert_eq!(tabs.reserve_restart(vec![2], &[2]), Ok(Busy));
    tabs.active_mut().model.voice.stop();
    assert_eq!(tabs.reserve_restart(vec![2], &[2]), Ok(Busy));
    tabs.active_mut().model.voice.stopped();
    assert_eq!(tabs.reserve_restart(vec![2], &[2]), Ok(Reserved));
    assert!(tabs.allow_work().is_err());
    tabs.release_restart();
    assert!(tabs.allow_work().is_ok());
}
