use super::*;

fn ask(message: Value, available: bool) -> Option<Value> {
    respond(
        &message,
        &mut |tool, args| {
            if tool == "browser_read" {
                Ok(Reply::text(format!("read {args}")))
            } else if tool == "browser_screenshot" {
                Ok(Reply {
                    ok: true,
                    text: "shot".into(),
                    image: Some("iVBO".into()),
                })
            } else {
                Err(format!("Unknown tool `{tool}`"))
            }
        },
        &|| available,
    )
}

#[test]
fn initialize_answers_the_clients_revision() {
    let reply = ask(
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26"}}),
        true,
    )
    .unwrap();
    assert_eq!(reply["id"], 1);
    assert_eq!(reply["result"]["protocolVersion"], "2025-03-26");
    assert!(reply["result"]["capabilities"]["tools"].is_object());
    assert_eq!(reply["result"]["serverInfo"]["name"], "dial-desktop");
    let bare = ask(
        json!({"jsonrpc": "2.0", "id": "a", "method": "initialize"}),
        true,
    )
    .unwrap();
    assert_eq!(bare["result"]["protocolVersion"], PROTOCOL);
}

#[test]
fn notifications_get_no_answer() {
    assert!(
        ask(
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            true
        )
        .is_none()
    );
    assert!(
        ask(
            json!({"jsonrpc": "2.0", "id": null, "method": "ping"}),
            true
        )
        .is_none()
    );
}

#[test]
fn tools_are_listed_only_while_the_app_runs() {
    let list = |available| {
        ask(
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            available,
        )
        .unwrap()["result"]["tools"]
            .as_array()
            .map(Vec::len)
    };
    assert_eq!(list(true), Some(12));
    assert_eq!(list(false), Some(0));
}

#[test]
fn calls_become_content_blocks() {
    let call = |name: &str| {
        ask(
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": name, "arguments": {"a": 1}}}),
            true,
        )
        .unwrap()["result"]
            .clone()
    };
    let read = call("browser_read");
    assert_eq!(read["isError"], false);
    assert_eq!(
        read["content"],
        json!([{"type": "text", "text": "read {\"a\":1}"}])
    );
    let shot = call("browser_screenshot");
    assert_eq!(
        shot["content"][1],
        json!({"type": "image", "data": "iVBO", "mimeType": "image/png"})
    );
    let bad = call("nope");
    assert_eq!(bad["isError"], true);
    assert_eq!(bad["content"][0]["text"], "Unknown tool `nope`");
}

#[test]
fn unknown_methods_are_errors() {
    let reply = ask(
        json!({"jsonrpc": "2.0", "id": 4, "method": "resources/list"}),
        true,
    )
    .unwrap();
    assert_eq!(reply["error"]["code"], -32601);
}

#[test]
fn the_bridge_talks_to_the_app_over_loopback() {
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = agent_browser::Endpoint {
        port: listener.local_addr().unwrap().port(),
        token: "tok".into(),
        pid: 0,
    };
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        let reply = agent_browser::handle(line.trim(), "tok", |tool, args| {
            Ok(Reply::text(format!("{tool} {args}")))
        });
        let mut stream = &stream;
        writeln!(stream, "{}", serde_json::to_string(&reply).unwrap()).unwrap();
    });
    let reply = exchange(
        &endpoint,
        "browser_click",
        &json!({"ref": 2}),
        Duration::from_secs(5),
    )
    .unwrap();
    server.join().unwrap();
    assert_eq!(reply, Reply::text(r#"browser_click {"ref":2}"#));
}
