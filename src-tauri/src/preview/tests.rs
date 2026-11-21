use super::*;
use dial_client::Entry;

#[test]
fn the_session_preview_exercises_every_row_kind() {
    let model = model(Preview::Session, "/work/app");
    let entries = model.conversation.entries();
    let has = |f: fn(&Entry) -> bool| entries.iter().any(f);
    assert!(has(|e| matches!(e, Entry::User(_))));
    assert!(has(
        |e| matches!(e, Entry::Assistant(a) if !a.reasoning.is_empty())
    ));
    assert!(has(|e| matches!(e, Entry::Tool(t) if t.result.is_none())));
    assert!(has(
        |e| matches!(e, Entry::Tool(t) if t.result.as_ref().is_some_and(|r| r.is_error))
    ));
    assert!(has(|e| matches!(e, Entry::Notice(_))));
    assert_eq!(model.conversation.pending_approvals().len(), 1);
    assert_eq!(model.conversation.todo().len(), 4);
    assert!(model.conversation.turn_running());
    assert_eq!(
        model
            .changed_paths()
            .into_iter()
            .map(|(path, _)| path)
            .collect::<Vec<_>>(),
        [
            "/work/app/src/server/limiter.rs",
            "/work/app/src/server/router.rs",
            "/work/app/src-tauri/icons/128x128@2x.png"
        ]
    );
    assert_eq!(model.title(), "Rate limiting for the public API");
}

#[test]
fn the_empty_preview_has_sessions_but_no_conversation() {
    let model = model(Preview::Empty, "/work/app");
    assert!(model.conversation.entries().is_empty());
    assert!(!model.sessions.is_empty());
    assert!(model.sessions.iter().all(|s| !s.current));
}

#[test]
fn the_question_preview_waits_on_an_answerable_question() {
    let model = model(Preview::Question, "/work/app");
    assert!(model.conversation.pending_approvals().is_empty());
    let request = model.conversation.question().expect("an open question");
    assert!(crate::engine::QuestionRef::of(request).is_some());
    assert!(model.conversation.turn_running());
}
