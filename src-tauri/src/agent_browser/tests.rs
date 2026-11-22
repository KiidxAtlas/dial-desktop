use super::*;

#[test]
#[allow(clippy::unwrap_used)]
fn tool_list_is_well_formed() {
    let tools = tools();
    let tools = tools.as_array().unwrap();
    assert_eq!(tools.len(), 20);
    let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for tool in tools {
        assert_eq!(tool["inputSchema"]["type"], "object");
        assert!(tool["description"].as_str().unwrap().len() > 10);
    }
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), tools.len(), "tool names are unique");
    let guest = tools.iter().find(|t| t["name"] == "app_desktop").unwrap();
    assert_eq!(guest["annotations"]["readOnlyHint"], false);
    assert_eq!(
        guest["inputSchema"]["properties"]["action"]["enum"],
        json!(["state", "start", "frame", "input"])
    );
    assert_eq!(
        guest["inputSchema"]["properties"]["guestTarget"]["enum"],
        json!(["default", "linux", "windows", "macos", "vm"])
    );
    let type_tool = tools.iter().find(|t| t["name"] == "browser_type").unwrap();
    assert_eq!(type_tool["inputSchema"]["required"], json!(["text"]));
    assert!(type_tool["inputSchema"]["properties"]["ref"].is_object());
    let saved: Vec<&str> = names.iter().copied().filter(|n| n.starts_with("types_")).collect();
    assert_eq!(
        saved,
        [
            "types_add_item", "types_create", "types_items", "types_list", "types_screenshot",
            "types_update", "types_update_item",
        ]
    );
    let screenshot = tools.iter().find(|t| t["name"] == "types_screenshot").unwrap();
    assert_eq!(
        screenshot["annotations"]["readOnlyHint"], false,
        "capture of a private Type requires approval"
    );
    assert_eq!(screenshot["inputSchema"]["required"], json!(["type"]));
}

#[test]
fn targets_are_read_from_ref_selector_or_name() {
    assert_eq!(target(&json!({"ref": 3})).unwrap(), Some(json!({"ref": 3})));
    assert_eq!(
        target(&json!({"ref": "[7]"})).unwrap(),
        Some(json!({"ref": 7}))
    );
    assert_eq!(
        target(&json!({"selector": " #go "})).unwrap(),
        Some(json!({"selector": "#go"}))
    );
    assert_eq!(
        target(&json!({"name": "Sign in", "selector": ""})).unwrap(),
        Some(json!({"name": "Sign in"}))
    );
    assert_eq!(target(&json!({})).unwrap(), None);
    assert!(target(&json!({"ref": 0})).is_err());
    assert!(target(&json!({"ref": "x"})).is_err());
    assert!(required_target(&json!({})).is_err());
}

#[test]
fn arguments_are_embedded_as_json_literals() {
    let script = fill(
        TYPE,
        &[
            ("QUERY", json!({"name": "a\"b"})),
            ("TEXT", json!("</script>\n'\"")),
            ("CLEAR", json!(true)),
            ("SUBMIT", json!(false)),
        ],
    );
    assert!(script.contains(r#"A.find({"name":"a\"b"})"#));
    assert!(script.contains(r#"const text = "</script>\n'\"";"#));
    assert!(!script.contains("QUERY") && !script.contains("CLEAR") && !script.contains("SUBMIT"));
    let full = action(&script);
    assert!(full.starts_with(HELPERS));
    assert!(full.ends_with("});"));
}

#[test]
fn fill_does_not_corrupt_values_containing_other_placeholder_names() {
    let script = fill(
        TYPE,
        &[
            ("QUERY", json!({"name": "TEXT CLEAR SUBMIT"})),
            ("TEXT", json!("CLEAR and SUBMIT and QUERY")),
            ("CLEAR", json!(true)),
            ("SUBMIT", json!(false)),
        ],
    );
    assert!(
        script.contains(r#"A.find({"name":"TEXT CLEAR SUBMIT"})"#),
        "QUERY was corrupted: {script}"
    );
    assert!(
        script.contains(r#"const text = "CLEAR and SUBMIT and QUERY";"#),
        "TEXT was corrupted: {script}"
    );
    assert!(script.contains("if (true) document.execCommand('selectAll');"));
    assert!(script.contains("(true ? '' : el.value)"));
    assert!(script.contains("return false ?"));
    assert!(EVAL.contains("setTimeout(() => { delete A.done[id]; }"));
}

#[test]
fn outcomes_are_unwrapped() {
    assert_eq!(parse_outcome(r#"{"ok":true,"value":3}"#).unwrap(), json!(3));
    let wrapped = serde_json::to_string(r#"{"ok":true,"value":"done"}"#).unwrap();
    assert_eq!(parse_outcome(&wrapped).unwrap(), json!("done"));
    assert_eq!(parse_outcome(r#"{"ok":true}"#).unwrap(), Value::Null);
    assert_eq!(
        parse_outcome(r#"{"ok":false,"error":"No element [9]"}"#).unwrap_err(),
        "No element [9]"
    );
    assert!(parse_outcome("null").is_err());
    assert!(parse_outcome("not json").is_err());
    assert!(parse_outcome("").unwrap_err().contains("syntax"));
}

#[test]
fn snapshots_read_as_text() {
    let text = format_snapshot(&json!({
        "url": "http://localhost:5173/",
        "title": "",
        "scrollY": 0,
        "height": 2000,
        "viewport": 800,
        "headings": ["h1 Welcome"],
        "elements": ["[1] link \"Docs\" -> /docs", "[2] textbox \"Search\""],
        "total": 2
    }));
    assert!(text.starts_with("Page: (untitled)\nURL: http://localhost:5173/\n"));
    assert!(text.contains("Headings:\n  h1 Welcome\n"));
    assert!(text.contains("Elements (2 of 2 visible):\n[1] link \"Docs\" -> /docs\n[2] textbox"));
    let empty = format_snapshot(&json!({"url": "about:blank", "title": "X"}));
    assert!(empty.contains("No visible interactive elements."));
}

#[test]
fn requests_need_the_token() {
    let line = |token: &str, tool: &str| {
        serde_json::to_string(&Request {
            token: token.into(),
            workspace: "a".into(),
            tool: tool.into(),
            arguments: json!({}),
        })
        .unwrap()
    };
    let mut ran = false;
    let denied = handle(
        &line("wrong", "browser_read"),
        "secret",
        |_| true,
        |_| {
            ran = true;
            Ok(Reply::text("x"))
        },
    );
    assert_eq!(denied, Reply::error("Not authorized"));
    assert!(!ran);
    assert_eq!(
        handle("{", "secret", |_| true, |_| unreachable!()),
        Reply::error("Bad request")
    );
    assert_eq!(
        handle(
            &line("secret", PING),
            "secret",
            |_| true,
            |_| unreachable!()
        ),
        Reply::text("pong")
    );
    assert!(
        !handle(
            &line("secret", "browser_click"),
            "secret",
            |root| root == "b",
            |_| unreachable!()
        )
        .ok
    );
    let failed = handle(
        &line("secret", "browser_read"),
        "secret",
        |_| true,
        |request| Err(format!("{} failed", request.tool)),
    );
    assert_eq!(failed, Reply::error("browser_read failed"));
}

#[test]
fn tokens_are_long_and_fresh() {
    let (a, b) = (new_token(), new_token());
    assert_eq!(a.len(), 64);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(a, b);
    assert!(same(&a, &a.clone()) && !same(&a, &b) && !same(&a, "short"));
}

#[test]
fn the_endpoint_file_round_trips_privately() {
    let dir = std::env::temp_dir().join(format!("dial-agent-browser-{}", std::process::id()));
    let file = dir.join("sub/endpoint.json");
    let endpoint = Endpoint {
        port: 4321,
        token: "t".into(),
        pid: std::process::id(),
    };
    write_endpoint(&file, &endpoint).unwrap();
    assert_eq!(read_endpoint(&file), Some(endpoint.clone()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // Another process's file stays.
    write_endpoint(&file, &Endpoint { pid: 1, ..endpoint }).unwrap();
    remove_endpoint(&file);
    assert!(file.exists());
    write_endpoint(
        &file,
        &Endpoint {
            pid: std::process::id(),
            port: 1,
            token: "t".into(),
        },
    )
    .unwrap();
    remove_endpoint(&file);
    assert!(!file.exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_mcp_entry_runs_this_binary_as_the_bridge() {
    let exe = Path::new("/Applications/Dial \"Desktop\".app/x\\y");
    let entry = mcp_entry(exe);
    assert!(entry.contains(r#"program = "/Applications/Dial \"Desktop\".app/x\\y""#));
    assert!(entry.contains(r#"args = ["--browser-mcp"]"#));
    assert!(entry.contains("trust_read_only_hint = true"));

    let path = Path::new("/bin/dial-desktop");
    let current =
        json!({"program": "/bin/dial-desktop", "args": ["--browser-mcp"], "enabled": true});
    assert!(needs_registration(None, path));
    assert!(!needs_registration(Some(&current), path));
    assert!(needs_registration(
        Some(&current),
        Path::new("/new/dial-desktop")
    ));
    assert!(needs_registration(Some(&json!({"url": "http://x"})), path));
    let off = json!({"program": "/old", "enabled": false});
    assert!(
        !needs_registration(Some(&off), path),
        "the user's choice to turn it off stays"
    );
}

#[test]
fn calls_are_described_for_the_user() {
    assert_eq!(
        describe_call("browser_open", &json!({"url": "localhost:3000"})),
        "Opening localhost:3000"
    );
    assert_eq!(
        describe_call("browser_click", &json!({"ref": 4})),
        "Clicking [4]"
    );
    assert_eq!(
        describe_call("browser_type", &json!({"name": "Email"})),
        "Typing into \"Email\""
    );
}
