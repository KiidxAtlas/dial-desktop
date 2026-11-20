use dial_client::{Client, Launch};
use dial_desktop::engine::fork_before;
use serde_json::json;
use std::time::Duration;

/// Run with an isolated temporary HOME and DIAL_TEST_ROOT; needs an installed Dial.
#[test]
#[ignore = "requires isolated Dial installation and scratch project"]
fn branch_preserves_source_and_files() {
    let root = std::path::PathBuf::from(std::env::var_os("DIAL_TEST_ROOT").unwrap());
    let marker = root.join("marker");
    std::fs::write(&marker, "current bytes").unwrap();
    let launch = Launch::new("dial")
        .args(["--model", "test", "--endpoint", "http://127.0.0.1:1/v1"])
        .current_dir(&root);
    let wait = Duration::from_secs(30);
    let mut original = Client::connect(&launch, wait).unwrap();
    let source = original.ready().session_id.clone();
    original
        .call(
            "seed_context",
            json!({"messages":["first","selected","later"]}),
            wait,
        )
        .unwrap();
    let id = fork_before(&launch, &source, "selected").unwrap();
    assert_ne!(id, source);
    let mut copy = Client::connect(&launch, wait).unwrap();
    copy.call("resume", json!({"session_id":id}), wait).unwrap();
    let history = |client: &mut Client| {
        client.call("messages_page", json!({}), wait).unwrap()["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["role"] == "user")
            .map(|m| m["content"].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(history(&mut copy), vec![json!("first")]);
    assert_eq!(
        history(&mut original),
        vec![json!("first"), json!("selected"), json!("later")]
    );
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "current bytes");
}
