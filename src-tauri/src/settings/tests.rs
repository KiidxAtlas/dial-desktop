use super::*;
use serde_json::json;

fn saved(entries: &[(&str, Value, bool)]) -> HashMap<String, SavedSetting> {
    entries
        .iter()
        .map(|(key, value, redacted)| {
            (
                (*key).to_owned(),
                SavedSetting {
                    value: value.clone(),
                    redacted: *redacted,
                },
            )
        })
        .collect()
}

/// Entries shaped like the engine's `settings_schema` reply.
fn schema() -> Vec<Value> {
    vec![
        json!({"path": "sandbox.mode", "label": "Sandbox", "tab": "Safety", "section": "Sandbox",
               "kind": "Enum([(\"off\", \"Off\"), (\"dev\", \"Dev\")])", "restart": true,
               "constraints": {"enum": ["off", "dev"], "labels": [["off", "Off"], ["dev", "Dev"]], "type": "string"},
               "default": "off", "description": "Confine commands."}),
        json!({"path": "ui.motion", "label": "Motion", "tab": "Common", "section": "Common",
               "kind": "Bool", "constraints": {"type": "boolean"}, "default": true, "restart": false}),
        json!({"path": "context_window", "label": "Context window", "tab": "Model", "section": "Context",
               "kind": "AutoInt { min: 2048, max: 2097152, step: 1024 }",
               "constraints": {"anyOf": [{"maximum": 2097152, "minimum": 2048, "type": "integer"}, {"const": "auto"}], "step": 1024},
               "default": "auto"}),
        json!({"path": "harness.temperature", "tab": "Model", "kind": "Float { min: 0.0, max: 2.0, step: 0.1 }",
               "constraints": {"maximum": 2.0, "minimum": 0.0, "step": 0.1, "type": "number"}, "default": 0.2}),
        json!({"path": "assistance.lean_local", "tab": "Learning", "kind": "AutoBool",
               "constraints": {"anyOf": [{"type": "boolean"}, {"const": "auto"}]}, "default": true}),
        json!({"path": "memory.semantic.embedding_model", "tab": "Memory", "kind": "Choice([...])",
               "constraints": {"suggestions": [["nomic-embed-text", "nomic · 768 dims"]]}, "default": ""}),
        json!({"path": "sandbox.extra_write", "tab": "Safety", "kind": "StringList",
               "constraints": {"items": {"type": "string"}, "type": "array"}, "default": []}),
        json!({"path": "providers", "tab": "Connections", "kind": "Toml",
               "constraints": {"format": "toml"}, "default": {}}),
        json!({"path": "ssh", "tab": "Connections", "kind": "Toml",
               "constraints": {"format": "toml"}, "default": {"connect_timeout_secs": 15}}),
        json!({"label": "no path, skipped"}),
    ]
}

#[test]
fn every_kind_gets_its_control() {
    let page = view(&schema(), &HashMap::new());
    let control = |key: &str| {
        page.settings
            .iter()
            .find(|s| s.key == key)
            .map(|s| s.control.clone())
            .unwrap()
    };
    assert_eq!(page.settings.len(), 9, "an entry without a path is skipped");
    assert_eq!(
        control("sandbox.mode"),
        Control::Select {
            options: vec![choice("off", "Off"), choice("dev", "Dev")]
        }
    );
    assert_eq!(control("ui.motion"), Control::Toggle);
    assert_eq!(
        control("context_window"),
        Control::Number {
            min: Some(2048.0),
            max: Some(2_097_152.0),
            step: Some(1024.0),
            integer: true,
            auto: true
        }
    );
    assert_eq!(
        control("harness.temperature"),
        Control::Number {
            min: Some(0.0),
            max: Some(2.0),
            step: Some(0.1),
            integer: false,
            auto: false
        }
    );
    assert!(
        matches!(control("assistance.lean_local"), Control::Select { options } if options.len() == 3)
    );
    assert_eq!(
        control("memory.semantic.embedding_model"),
        Control::Text {
            suggestions: vec![choice("nomic-embed-text", "nomic · 768 dims")]
        }
    );
    assert_eq!(control("sandbox.extra_write"), Control::List);
    assert_eq!(control("providers"), Control::Table);
    // Common first, then the reference's order.
    assert_eq!(
        page.tabs,
        [
            "Common",
            "Safety",
            "Model",
            "Learning",
            "Memory",
            "Connections"
        ]
    );
}

#[test]
fn values_fall_back_to_defaults_and_mark_changes() {
    let values = saved(&[
        ("sandbox.mode", json!("dev"), false),
        ("ui.motion", json!(true), false),
        ("harness.temperature", json!(0.2), false),
    ]);
    let page = view(&schema(), &values);
    let row = |key: &str| page.settings.iter().find(|s| s.key == key).unwrap();
    assert_eq!(row("sandbox.mode").value, json!("dev"));
    assert!(row("sandbox.mode").modified);
    assert!(row("sandbox.mode").restart);
    assert_eq!(row("sandbox.mode").label, "Sandbox");
    assert!(!row("ui.motion").modified, "saved but equal to the default");
    assert!(!row("harness.temperature").modified);
    assert_eq!(row("context_window").value, json!("auto"));
    assert_eq!(row("harness.temperature").label, "harness.temperature");
}

#[test]
fn tables_fold_leaf_keys_and_hide_credentials() {
    let values = saved(&[
        ("providers.openai.base_url", json!("https://x"), false),
        ("providers.openai.api_key", json!("<redacted>"), true),
        ("ssh.connect_timeout_secs", json!(30), false),
        ("ssh.hosts.a.port", json!(22), false),
    ]);
    let page = view(&schema(), &values);
    let row = |key: &str| page.settings.iter().find(|s| s.key == key).unwrap();
    let providers = row("providers");
    assert_eq!(
        providers.value,
        json!({"openai": {"api_key": "<redacted>", "base_url": "https://x"}})
    );
    assert!(providers.redacted && !providers.modified);
    let ssh = row("ssh");
    assert_eq!(
        ssh.value,
        json!({"connect_timeout_secs": 30, "hosts": {"a": {"port": 22}}})
    );
    assert!(!ssh.redacted && ssh.modified);
}
