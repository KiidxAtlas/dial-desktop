use super::*;
use dial_client::{Effect, Event, Outstanding};

const SESSION: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PEER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn ready() -> Ready {
    Ready {
        session_id: SESSION.into(), engine_version: None, model: String::new(),
        supported_protocol_versions: vec![2],
        methods: ["task_status", "task_message", "list_peers", "send_peer_message"].map(String::from).into(),
        capabilities: vec!["peer_coordination".into()], raw: Value::Null,
    }
}

fn event(kind: &str, body: Value) -> Message {
    Message::Event(Event { kind: kind.into(), body })
}

fn pending(method: &str) -> Outstanding {
    let id = Event { kind: "x".into(), body: json!({"request_id":1}) }.request_id().unwrap();
    Outstanding { id, method: method.into(), effect: Effect::MayHaveApplied }
}

fn list(running: bool) -> Request { Request::Workers { id: None, running } }

fn send(row: &Value) -> Request {
    Request::Message { id: row["id"].as_str().unwrap().into(), text: "Hello".into(), token: row["watchToken"].as_str().unwrap().into() }
}

fn worker(node: &str, id: &str, started: u64) -> Value {
    json!({"runtime_node":node, "id":id, "status":"running", "name":"Review", "started_ms":started})
}

fn rpc(action: Result<Action, String>) -> (&'static str, Value) {
    match action.unwrap() { Action::Rpc(method, params) => (method, params), _ => panic!("expected engine RPC") }
}

#[test]
fn busy_board_merges_streamed_workers_and_the_last_idle_roster() {
    let ready = ready();
    let mut board = Board::new(SESSION.into());
    assert_eq!(rpc(board.action(&ready, SESSION, &[], list(false))), ("task_status", json!({})));
    let node = |session: &str, key: &str, data: Value| event("state_node", json!({"session_id":session, "node":{
        "kind":"worker", "id":key, "label":"Research", "result":{"phase":"running", "data":data}
    }}));
    board.observe(&node(SESSION, "sync", Value::Null), 0);
    board.observe(&node(SESSION, "one", Value::Null), 0); // start before the mailbox was registered
    board.observe(&node(PEER, "foreign", Value::Null), 0);
    let mut one = worker("one", "task-1", 12);
    one["instruction"] = json!("private context");
    let roster = board.status_view(&json!([one])).unwrap();
    assert_eq!(roster.as_array().unwrap().len(), 2, "sync node preserved; provisional node replaced");
    let selected = &roster[0];
    assert!(selected.get("instruction").is_none());
    assert_eq!(roster[1]["id"], "node:sync");
    assert!(board.action(&ready, SESSION, &[], send(&roster[1])).is_err());
    for (running, pending) in [(true, vec![]), (false, vec![pending("send")])] {
        let Action::Cached(rows) = board.action(&ready, SESSION, &pending, list(running)).unwrap() else { panic!("busy actor must not get task_status") };
        let rows = rows.as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.contains(selected));
    }
    assert_eq!(rpc(board.action(&ready, SESSION, &[], send(selected))), ("task_message", json!({"id":"task-1","text":"Hello"})));
    one["status"] = json!("completed");
    board.observe(&node(SESSION, "one", one), 0);
    assert!(board.action(&ready, SESSION, &[], send(selected)).is_err());
    let after = board.status_view(&json!([])).unwrap();
    assert_eq!(after, json!([roster[1]]));
}

#[test]
fn old_selection_cannot_message_a_resumed_worker_or_replacement_engine() {
    let ready = ready();
    let mut board = Board::new(SESSION.into());
    let data = worker("one", "task-1", 10);
    let initial = board.status_view(&data).unwrap();
    for replacement in [worker("two", "task-1", 10), worker("one", "task-1", 11)] {
        let selected = board.status_view(&replacement).unwrap();
        assert!(board.action(&ready, SESSION, &[], send(&initial)).is_err());
        assert!(board.action(&ready, SESSION, &[], send(&selected)).is_ok());
    }
    let mut reopened = Board::new(SESSION.into());
    reopened.status_view(&data).unwrap();
    assert!(reopened.action(&ready, SESSION, &[], send(&initial)).is_err());
    board.observe(&event("state_reset", json!({"session_id":SESSION})), 0);
    board.status_view(&data).unwrap();
    assert!(board.action(&ready, SESSION, &[], send(&initial)).is_err());
}

#[test]
fn stale_sender_switch_in_flight_and_event_gap_block_dispatch() {
    let ready = ready();
    let mut board = Board::new(SESSION.into());
    let selected = board.status_view(&worker("one", "task-1", 12)).unwrap();
    assert!(board.action(&ready, PEER, &[], send(&selected)).is_err());
    for method in ["new_session", "resume", "fork_session", "branch_session", "apply_settings"] {
        assert!(board.action(&ready, SESSION, &[pending(method)], send(&selected)).is_err(), "{method}");
    }
    board.observe(&event("state_node", json!({"session_id":SESSION})), 1);
    assert!(board.action(&ready, SESSION, &[], Request::Peers).is_err());
    board.observe(&Message::Response { id: pending("state").id, method: "state".into(), result: Ok(json!({"session_id":PEER})) }, 1);
    assert!(board.action(&ready, SESSION, &[], Request::Peers).is_err());
    assert_eq!(rpc(board.action(&ready, PEER, &[], Request::Peers)), ("list_peers", json!({"session_id":PEER})));
}

#[test]
fn peers_require_the_negotiated_transport_and_an_exact_nonself_address() {
    let board = Board::new(SESSION.into());
    let mut ready = ready();
    let send = |id: &str, text: &str| Request::PeerMessage { id: id.into(), text: text.into() };
    assert_eq!(rpc(board.action(&ready, SESSION, &[], send(PEER, "Hi"))), ("send_peer_message", json!({"session_id":SESSION,"to":PEER,"text":"Hi"})));
    for (id, text) in [("bbbb", "Hi"), (SESSION, "Hi"), (PEER, " \n "), (PEER, &"🖋".repeat(1025))] {
        assert!(board.action(&ready, SESSION, &[], send(id, text)).is_err());
    }
    ready.methods.retain(|m| m != "send_peer_message");
    assert!(board.action(&ready, SESSION, &[], send(PEER, "Hi")).is_err());
    ready.capabilities.clear();
    assert!(board.action(&ready, SESSION, &[], Request::Peers).is_err());
    assert!(board.action(&ready, SESSION, &[], list(false)).is_ok(), "legacy worker support is independent");
}
