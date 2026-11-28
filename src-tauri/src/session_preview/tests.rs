use super::*;

fn messages(text: &str) -> Vec<Value> {
    vec![json!({"role": "user", "content": text}), json!({"role": "assistant", "content": format!("re: {text}")})]
}

#[test]
fn a_kept_copy_shows_at_once_and_the_engine_copy_replaces_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut previews = Previews::new(Some(dir.path().to_owned()));
    previews.begin("/p", "s1");
    assert!(previews.shown("/p", Some("s1")).is_none(), "nothing kept yet");

    previews.update("/p", "s1", &messages("old"));
    let mut later = Previews::new(Some(dir.path().to_owned()));
    later.begin("/p", "s1");
    assert_eq!(later.shown("/p", Some("s1")).unwrap().model.conversation.entries().len(), 2);

    later.update("/p", "s1", &[messages("new"), messages("more")].concat());
    assert_eq!(later.shown("/p", Some("s1")).unwrap().model.conversation.entries().len(), 4);
    // Another session being opened, or another project, clears the preview.
    assert!(later.shown("/p", Some("s2")).is_none());
    assert!(later.shown("/p", Some("s1")).is_none());
}

#[test]
fn copies_stay_within_their_project_and_go_with_permanent_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let mut previews = Previews::new(Some(dir.path().to_owned()));
    previews.update("/a", "s1", &messages("a"));
    previews.begin("/b", "s1");
    assert!(previews.shown("/b", Some("s1")).is_none());
    previews.begin("/a", "s1");
    assert!(previews.shown("/a", Some("s1")).is_some());
    previews.forget("/a", &["s1".into()]);
    assert!(previews.shown("/a", Some("s1")).is_none());
    previews.begin("/a", "s1");
    assert!(previews.shown("/a", Some("s1")).is_none());
    // Ids that could name another path are never used as file names.
    previews.update("/a", "../x", &messages("x"));
    assert!(previews.file("/a", "../x").is_none());
}

#[test]
fn compacted_previews_are_neither_saved_nor_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let mut previews = Previews::new(Some(dir.path().to_owned()));
    let compacted = vec![
        json!({"role": "user", "dial_compaction_summary": true, "content": "summary"}),
        json!({"role": "user", "content": "recent"}),
    ];
    previews.update("/p", "s1", &compacted);
    previews.begin("/p", "s1");
    assert!(previews.shown("/p", Some("s1")).is_none(), "truncated history must not be cached");

    // Even a stale truncated file already on disk is ignored.
    let file = previews.file("/p", "s1").unwrap();
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, serde_json::to_vec(&json!({"root": "/p", "session_id": "s1", "messages": compacted})).unwrap()).unwrap();
    previews.begin("/p", "s1");
    assert!(previews.shown("/p", Some("s1")).is_none(), "stale compacted disk preview must be ignored");
}
