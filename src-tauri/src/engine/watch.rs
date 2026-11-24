//! A narrow Activity adapter over the engine that actually owns the session. `task_status`
//! requires the idle actor, so during a turn use the worker nodes already streamed by RPC.
use dial_client::{Message, Ready};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(test)]
mod tests;

static EPOCH: AtomicU64 = AtomicU64::new(1);

pub(crate) enum Request {
    Workers { id: Option<String>, running: bool },
    Message { id: String, text: String, token: String },
    Peers,
    PeerMessage { id: String, text: String },
}

pub(super) enum Action {
    Cached(Value),
    Rpc(&'static str, Value),
}

fn worker_number(id: &str) -> Option<u64> {
    let digits = id.strip_prefix("task-")?;
    if digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn check_text(text: &str) -> Result<(), String> {
    if text.trim().is_empty() || text.len() > 4096 {
        return Err("Enter a message between 1 and 4096 bytes".into());
    }
    Ok(())
}

/// Copy just the Activity fields, not model context, journals or arbitrarily long reports.
fn row(id: &str, data: &Value, epoch: u64, node: Option<&str>) -> Value {
    // A resumed worker may keep its mailbox id even on the same board. Bind the selection
    // to its actual execution, not just to the session/engine that owns that mailbox.
    let started = data["started_ms"].as_u64();
    let token = if node.is_some() || started.is_some() {
        format!("{epoch}:{id}:{}:{}", node.unwrap_or_default(), started.unwrap_or_default())
    } else {
        String::new() // An older report without any execution identity is read only.
    };
    let mut row = json!({"id":id, "status":data["status"].as_str().or_else(|| data["state"].as_str()).unwrap_or("unknown"), "watchToken":token});
    for (key, limit) in [("name", 256), ("phase", 256), ("health", 256), ("model", 256), ("agent", 256), ("role", 256), ("task", 4096), ("result", 4096)] {
        if let Some(text) = data[key].as_str() {
            let clipped = match text.char_indices().nth(limit) {
                Some((end, _)) => format!("{}…", &text[..end]),
                None => text.to_owned(),
            };
            row[key] = clipped.into();
        }
    }
    for key in ["steps", "tools", "started_ms", "updated_ms", "elapsed_ms"] {
        if let Some(n) = data[key].as_u64() { row[key] = n.into(); }
    }
    row
}

pub(super) struct Board {
    session: Option<String>,
    // Public ids: mailbox id for asynchronous work, node:<id> for read-only stream rows.
    workers: BTreeMap<String, Value>,
    pub(super) epoch: u64,
    lost: u64,
}

impl Board {
    pub(super) fn new(session: String) -> Self {
        Self { session: Some(session), workers: BTreeMap::new(), epoch: EPOCH.fetch_add(1, Ordering::Relaxed), lost: 0 }
    }

    fn reset(&mut self) {
        self.epoch = EPOCH.fetch_add(1, Ordering::Relaxed);
        self.workers.clear();
    }

    pub(super) fn status_view(&mut self, status: &Value) -> Result<Value, String> {
        if let Some(rows) = status.as_array() {
            // The idle report is authoritative for mailboxes and pools; synchronous workers
            // only exist in the tree. Keep the merged view for when the actor gets busy.
            self.workers.retain(|id, _| id.starts_with("node:"));
            let mut rows: Vec<_> = rows.iter().filter_map(|data| self.status_row(data)).collect();
            rows.extend(self.workers.iter().filter(|(id, _)| id.starts_with("node:")).map(|(_, row)| row.clone()));
            Ok(Value::Array(rows))
        } else {
            self.status_row(status).ok_or_else(|| "Dial returned an unknown worker".into())
        }
    }

    fn status_row(&mut self, data: &Value) -> Option<Value> {
        let id = data["id"].as_str()?;
        let node = data["runtime_node"].as_str();
        if let Some(key) = node { self.workers.remove(&format!("node:{key}")); }
        let view = row(id, data, self.epoch, node);
        self.workers.insert(id.into(), view.clone());
        Some(view)
    }

    pub(super) fn observe(&mut self, message: &Message, lost: u64) {
        if self.lost != lost {
            self.lost = lost;
            self.session = None;
            self.reset();
        }
        // A state response is authoritative after a gap. Don't accept identity from arbitrary nodes.
        if let Message::Response { method, result: Ok(state), .. } = message
            && matches!(method.as_str(), "state" | "recovery_snapshot") && state["session_id"].is_string()
            && (self.session.is_none() || (self.workers.is_empty() && self.session.as_deref() == state["session_id"].as_str())) {
            self.session = state["session_id"].as_str().map(str::to_owned);
            for node in state["tree"].as_array().into_iter().flatten() { self.worker(node); }
        }
        let Message::Event(event) = message else { return };
        let session = event.body["session_id"].as_str();
        match event.kind.as_str() {
            "ready" | "active_session" => {
                self.session = session.map(str::to_owned);
                self.reset();
            }
            "state_node" if session.is_some() && session == self.session.as_deref() => self.worker(&event.body["node"]),
            "state_reset" if session == self.session.as_deref() => self.reset(),
            _ => {}
        }
    }

    fn worker(&mut self, node: &Value) {
        if node["kind"] != "worker" { return; }
        let Some(key) = node["id"].as_str() else { return };
        let data = &node["result"]["data"];
        let opaque = format!("node:{key}");
        let id = data["id"].as_str().filter(|s| worker_number(s).is_some()).unwrap_or(&opaque);
        let mut view = row(id, data, self.epoch, Some(key));
        if view["status"] == "unknown" { view["status"] = node["result"]["phase"].clone(); }
        if view.get("name").is_none() { view["name"] = node["label"].clone(); }
        if id != opaque { self.workers.remove(&opaque); }
        self.workers.insert(id.into(), view);
    }

    pub(super) fn action(&self, ready: &Ready, session: &str, pending: &[dial_client::Outstanding], req: Request) -> Result<Action, String> {
        if self.session.as_deref() != Some(session) || pending.iter().any(|p| matches!(p.method.as_str(), "new_session" | "resume" | "fork_session" | "branch_session" | "apply_settings")) {
            return Err("The active session changed or is reopening; refresh Activity".into());
        }
        let peers = matches!(&req, Request::Peers | Request::PeerMessage { .. });
        let method = match &req {
            Request::Workers { .. } => "task_status",
            Request::Message { .. } => "task_message",
            Request::Peers => "list_peers",
            Request::PeerMessage { .. } => "send_peer_message",
        };
        if !ready.methods.iter().any(|m| m == method) || (peers && !ready.capabilities.iter().any(|c| c == "peer_coordination")) {
            return Err(if peers { "This Dial engine does not support project peer coordination. Update the engine to watch and message other sessions.".into() } else { format!("This Dial engine does not support `{method}`. Update the engine to watch and message workers.") });
        }
        let params = match req {
            Request::Workers { id, running } => {
                if running || id.as_deref().is_some_and(|s| s.starts_with("node:")) || pending.iter().any(|p| matches!(p.method.as_str(), "send" | "retry" | "live_listen")) {
                    let view = match id {
                        Some(id) => self.workers.get(&id).cloned().ok_or("That worker is not on the live board")?,
                        None => Value::Array(self.workers.values().cloned().collect()),
                    };
                    return Ok(Action::Cached(view));
                }
                id.map_or_else(|| json!({}), |id| json!({"id": id}))
            }
            Request::Message { id, text, token } => {
                worker_number(&id).ok_or("Choose one worker by its task ID")?;
                if token.is_empty() || self.workers.get(&id).is_none_or(|row| row["watchToken"] != token) {
                    return Err("That worker was replaced; select the worker again".into());
                }
                if self.workers[&id]["status"] != "running" || self.workers.iter().any(|(other, row)| other != &id && row["status"] == "running" && row["name"] == id) {
                    return Err("That worker is no longer an unambiguous live recipient; refresh Activity".into());
                }
                check_text(&text)?;
                json!({"id":id,"text":text})
            }
            Request::Peers => json!({"session_id":session}),
            Request::PeerMessage { id, text } => {
                if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) || id == session {
                    return Err("Choose another session by its full peer ID".into());
                }
                check_text(&text)?;
                json!({"session_id":session,"to":id,"text":text})
            }
        };
        Ok(Action::Rpc(method, params))
    }
}
