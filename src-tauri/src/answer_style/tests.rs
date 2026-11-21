use super::*;

#[test]
fn guidance_follows_the_switches() {
    let both = AnswerStyle::default().guidance().unwrap();
    assert!(both.contains("mermaid") && both.contains("dial-app"));
    let diagrams = AnswerStyle { cards: false, diagrams: true }.guidance().unwrap();
    assert!(diagrams.contains("mermaid") && !diagrams.contains("dial-app"));
    let cards = AnswerStyle { cards: true, diagrams: false }.guidance().unwrap();
    assert!(cards.contains("dial-app") && !cards.contains("xychart"));
    assert_eq!(AnswerStyle { cards: false, diagrams: false }.guidance(), None);
}

#[test]
fn explore_cards_must_explain_every_node() {
    let cards = AnswerStyle { cards: true, diagrams: false }.guidance().unwrap();
    let explore = cards.lines().find(|l| l.starts_with("- \"explore\"")).unwrap();
    assert!(explore.contains("every node and subgraph"));
    assert!(explore.contains("flowchart or stateDiagram-v2"));
}

#[test]
fn saved_switches_round_trip_and_bad_files_fall_back() {
    let dir = std::env::temp_dir().join(format!("dial-answer-style-{}", std::process::id()));
    let path = dir.join("answer-style.json");
    assert_eq!(load(Some(&path)), AnswerStyle::default());
    let off = AnswerStyle { cards: false, diagrams: true };
    save(&path, off).unwrap();
    assert_eq!(load(Some(&path)), off);
    std::fs::write(&path, "not json").unwrap();
    assert_eq!(load(Some(&path)), AnswerStyle::default());
    // A file from a build that only knew one switch keeps the other's default.
    std::fs::write(&path, r#"{"cards":false}"#).unwrap();
    assert_eq!(load(Some(&path)), AnswerStyle { cards: false, diagrams: true });
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn only_engines_that_know_the_flag_get_it() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("dial-answer-probe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let engine = |name: &str, script: &str| {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    };
    let new = engine("new", r#"[ "$1 $2" = "rpc --append-system-prompt" ] && exit 0; exit 2"#);
    let old = engine("old", "exit 2");
    let support = Support::default();
    assert!(support.accepts(&new));
    assert!(!support.accepts(&old));
    assert!(!support.accepts(&dir.join("missing")));
    let _ = std::fs::remove_dir_all(dir);
}
