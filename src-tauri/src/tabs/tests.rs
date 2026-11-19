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
    assert_ne!(tabs.route_resume("s1"), Route::Here, "the old engine can still own s1");
    drop(closed);
    assert_eq!(tabs.route_resume("s1"), Route::Here);
}
