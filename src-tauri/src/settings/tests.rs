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
        json!({"path": "retry.max_delay_secs", "label": "Longest retry wait (s)", "tab": "Model",
               "kind": "Int { min: 1, max: 3600, step: 1 }",
               "constraints": {"maximum": 3600, "minimum": 1, "type": "integer"}, "default": 30}),
        json!({"path": "shell.idle_secs", "label": "Idle limit", "tab": "Tools",
               "kind": "Int { min: 1, max: 3600, step: 1 }",
               "constraints": {"maximum": 3600, "minimum": 1, "type": "integer"}, "default": 30}),
        json!({"path": "image.label_test", "label": "Images (beta)", "tab": "Tools", "kind": "Text",
               "constraints": {"type": "string"}, "default": ""}),
        json!({"path": "ui.theme", "label": "Theme", "tab": "Common", "kind": "Theme",
               "constraints": {"type": "string"}, "default": "auto"}),
        json!({"path": "weights.model", "tab": "Learning", "kind": "Text", "constraints": {"type": "string"}, "default": ""}),
        json!({"path": "lsp.pyright.program", "tab": "Tools", "kind": "Text", "constraints": {"type": "string"}, "default": ""}),
        json!({"path": "memory.hindsight.api_key_env", "tab": "Memory", "kind": "Text", "constraints": {"type": "string"}, "default": ""}),
        json!({"path": "image.base_url", "tab": "Tools", "kind": "Text", "constraints": {"type": "string"}, "default": ""}),
        json!({"path": "ida.idadir", "tab": "Tools", "kind": "Text", "constraints": {"type": "string"}, "default": ""}),
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
    assert_eq!(
        page.settings.len(),
        18,
        "an entry without a path is skipped"
    );
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
            auto: true,
            unit: None,
        }
    );
    assert_eq!(
        control("harness.temperature"),
        Control::Number {
            min: Some(0.0),
            max: Some(2.0),
            step: Some(0.1),
            integer: false,
            auto: false,
            unit: None,
        }
    );
    assert!(
        matches!(control("assistance.lean_local"), Control::Select { options } if options.len() == 3)
    );
    assert_eq!(
        control("memory.semantic.embedding_model"),
        Control::Text {
            suggestions: vec![choice("nomic-embed-text", "nomic · 768 dims")],
            hint: TextHint::Plain,
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
            "Tools",
            "Learning",
            "Memory",
            "Connections"
        ]
    );
}

#[test]
fn text_settings_name_what_they_hold_so_the_page_can_offer_a_picker() {
    let page = view(&schema(), &HashMap::new());
    let row = |key: &str| page.settings.iter().find(|s| s.key == key).unwrap();
    let hint = |key: &str| match &row(key).control {
        Control::Text { hint, .. } => *hint,
        other => panic!("{key} is not text: {other:?}"),
    };
    assert_eq!(hint("weights.model"), TextHint::Model);
    assert_eq!(hint("lsp.pyright.program"), TextHint::Program);
    assert_eq!(hint("memory.hindsight.api_key_env"), TextHint::EnvVar);
    assert_eq!(hint("image.base_url"), TextHint::Url);
    assert_eq!(hint("ida.idadir"), TextHint::Folder);
    // Terminal themes are a dropdown, with "auto" first for the main theme.
    let Control::Select { options } = &row("ui.theme").control else {
        panic!("theme is a dropdown")
    };
    assert_eq!(options[0].value, "auto");
    assert!(options.iter().any(|s| s.value == "dial-dark"));
}

#[test]
fn the_image_model_accepts_text_and_suggests_image_models_not_chat_models() {
    let entry = json!({"path": "image.model", "label": "Image model", "tab": "Tools", "kind": "Text",
                       "constraints": {"type": "string"}, "default": "gpt-image-1.5"});
    let page = view(&[entry], &HashMap::new());
    let Control::Text { suggestions: options, hint: TextHint::Plain } = &page.settings[0].control else {
        panic!("image.model accepts custom text without chat model suggestions")
    };
    assert!(options.iter().any(|s| s.value == "gpt-image-1.5"));
    assert!(
        options
            .iter()
            .any(|s| s.value.starts_with("gemini-") && s.value.contains("image"))
    );
}

#[test]
fn engine_image_models_replace_the_built_in_list_and_bring_their_endpoint() {
    let entry = json!({"path": "image.model", "label": "Image model", "tab": "Tools", "kind": "Text",
                       "constraints": {"type": "string"}, "default": "gpt-image-1.5"});
    let mut page = view(&[entry], &HashMap::new());
    let built_in = page.settings[0].control.clone();
    // An engine without the listing sends none: the built-in list stays.
    offer_image_models(&mut page, &[]);
    assert_eq!(page.settings[0].control, built_in);
    offer_image_models(
        &mut page,
        &[
            json!({"provider": "gemini", "model_id": "gemini-3-pro-image", "label": "Gemini 3 Pro Image",
                   "base_url": "https://generativelanguage.googleapis.com/v1beta", "image_api": "gemini"}),
            json!({"provider": "mlx", "model_id": "/models/flux", "label": "", "base_url": null, "image_api": null}),
            json!({"provider": "gateway", "model_id": "gemini-3-pro-image", "label": "Gemini via gateway",
                   "base_url": "https://gateway.example/v1", "image_api": "openai"}),
        ],
    );
    let Control::Text { suggestions: options, .. } = &page.settings[0].control else {
        panic!("image.model still accepts custom text")
    };
    assert_eq!(options.len(), 3);
    assert_eq!(options[0].value, options[2].value);
    assert_ne!(options[0].id, options[2].id);
    assert_eq!(options[2].with["image.base_url"], "https://gateway.example/v1");
    assert_eq!(options[0].value, "gemini-3-pro-image");
    assert_eq!(options[0].label, "Gemini 3 Pro Image · gemini");
    assert_eq!(
        options[0].with,
        json!({"image.base_url": "https://generativelanguage.googleapis.com/v1beta", "image.api": "gemini"})
            .as_object()
            .unwrap()
            .clone()
    );
    assert_eq!(options[1].label, "/models/flux · mlx");
    assert!(
        options[1].with.is_empty(),
        "nothing unknown is saved alongside"
    );
}

#[test]
fn numbers_carry_their_unit_out_of_the_label_or_key() {
    let page = view(&schema(), &HashMap::new());
    let row = |key: &str| page.settings.iter().find(|s| s.key == key).unwrap();
    let unit = |key: &str| match &row(key).control {
        Control::Number { unit, .. } => unit.clone(),
        other => panic!("{key} is not a number: {other:?}"),
    };
    assert_eq!(row("retry.max_delay_secs").label, "Longest retry wait");
    assert_eq!(unit("retry.max_delay_secs").as_deref(), Some("s"));
    assert_eq!(unit("shell.idle_secs").as_deref(), Some("s"));
    assert_eq!(unit("context_window"), None);
    // Only numbers lose a parenthetical; for other settings it is part of the name.
    assert_eq!(row("image.label_test").label, "Images (beta)");
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
