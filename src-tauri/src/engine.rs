//! Bridge between the blocking [`dial_client::Client`] and the UI thread.
//!
//! One worker thread owns the client. It forwards UI [`Command`]s as non-blocking requests and
//! every polled [`Message`] back as an [`Update`], then wakes the UI. The worker never answers
//! anything on the user's behalf: approvals, aborts and prompts only come from commands.

use dial_client::{Client, Decision, Launch, Message, Ready, RequestId};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Weak};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[cfg(test)]
mod tests;

/// Engine startup can include model discovery; give it room before calling it failed.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
/// Poll interval while idle; short enough that commands feel immediate.
const POLL: Duration = Duration::from_millis(25);
/// Grace for a graceful `shutdown` before the engine is killed.
const SHUTDOWN: Duration = Duration::from_secs(5);
/// Sessions fetched for the sidebar.
const SESSION_PAGE: u64 = 100;

/// Wakes the UI after an update is queued (the Tauri pump thread).
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Something the user asked the engine to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Start a turn with this prompt and its images: file paths and pasted base64 bytes.
    Send {
        text: String,
        images: Vec<String>,
        inline_images: Vec<String>,
    },
    /// Answer a pending approval.
    Approve {
        approval_id: String,
        decision: Decision,
    },
    /// Stop the running turn.
    Abort,
    /// Refresh the sidebar's session list.
    ListSessions,
    ListSessionsPage {
        offset: u64,
        after: RequestId,
    },
    /// Start a fresh session.
    NewSession,
    /// Switch to a persisted session.
    Resume(String),
    /// Delete another session of this project; the engine refuses the active one.
    DeleteSession(String),
    /// Load the settings reference: every setting's kind, constraints and default.
    SettingsSchema,
    /// Read every saved setting's effective value.
    GetSettings,
    /// Save one setting in `scope` (`global` or `project`).
    SetSetting {
        key: String,
        value: Value,
        scope: String,
    },
    /// Remove one setting from `scope`, so the default (or the other scope) applies again.
    ResetSetting {
        key: String,
        scope: String,
    },
    /// Reload the saved settings into the running session.
    ApplySettings,
    /// Replace the view with an authoritative snapshot after lost events.
    Recover,
    /// Read the model, thinking level and approval mode in force.
    State,
    /// Load the models `SetModel` accepts.
    ListModels,
    /// Load the models that generate images (`list_models` with `purpose: "image"`).
    ListImageModels,
    /// Switch this session's model.
    SetModel {
        provider: String,
        model_id: String,
    },
    /// Set the reasoning level; `default` restores the model's default.
    SetThinking(String),
    /// Set the approval mode: `ask`, `write`, `auto` or `readonly`.
    SetApproval(String),
    /// Tell the engine the open question is on screen; its answer timeout starts now.
    QuestionPresented(QuestionRef),
    /// Close the open question: `kind` is `answers` (with one result per question), `chat` or
    /// `cancel` (without results).
    AnswerQuestion {
        question: QuestionRef,
        kind: String,
        results: Vec<Value>,
    },
    /// Rename the active session.
    Rename(String),
    /// Pin or unpin a session of this project.
    PinSession {
        session_id: String,
        pinned: bool,
    },
    /// Copy a session of this project into a new one and switch to the copy.
    ForkSession(String),
    /// Write the active session as Markdown to `path`, relative to the project root.
    Export(String),
    /// Undo the last turn: restore the files it changed and rewind the conversation.
    Undo,
    /// Add guidance to the running turn; the engine takes it at its next step.
    Steer(String),
    /// Send the last unanswered prompt (a failed or stopped turn) again.
    Retry,
    /// Summarize the conversation so far to free context.
    Compact,
    /// Start voice mode: the engine reads each final answer aloud.
    LiveStart {
        instructions: Option<String>,
    },
    /// Record one utterance and run it as a turn.
    LiveListen,
    /// Silence (or unsilence) spoken replies.
    LiveMute(bool),
    /// End voice mode.
    LiveStop,
    /// Read text aloud (`None` stops reading), in or out of voice mode.
    LiveSpeak(Option<String>),
}

/// Identity of one open question; the engine rejects answers for any other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionRef {
    pub question_id: String,
    pub generation: u64,
    pub session_id: String,
}

impl QuestionRef {
    /// Read the identity from a `question` event's `request`; `None` if any part is missing.
    pub fn of(request: &Value) -> Option<Self> {
        Some(Self {
            question_id: request["question_id"].as_str()?.to_owned(),
            generation: request["generation"].as_u64()?,
            session_id: request["session_id"].as_str()?.to_owned(),
        })
    }

    fn params(&self) -> Value {
        json!({
            "question_id": self.question_id,
            "generation": self.generation,
            "session_id": self.session_id,
        })
    }
}

/// Approval modes the engine accepts, most cautious first.
pub const APPROVAL_MODES: [&str; 4] = ["readonly", "ask", "write", "auto"];

/// Why a request was sent, so its response can be routed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Purpose {
    Turn,
    Approve,
    Abort,
    Sessions,
    NewSession,
    Resume,
    DeleteSession,
    SettingsSchema,
    Settings,
    SetSetting,
    ResetSetting,
    ApplySettings,
    Recover,
    State,
    Models,
    ImageModels,
    SetModel,
    SetThinking,
    SetApproval,
    QuestionPresented,
    Answer,
    Rename,
    Pin,
    Fork,
    Export,
    Undo,
    Steer,
    Compact,
    LiveStart,
    Listen,
    LiveMute,
    LiveStop,
    Speak,
}

impl Command {
    /// The wire request for this command.
    pub fn request(&self) -> (Purpose, &'static str, Value) {
        match self {
            Self::Send {
                text,
                images,
                inline_images,
            } => {
                let mut params = json!({ "text": text });
                // A plain prompt stays a plain `send`; attachments are only named when present.
                if !images.is_empty() {
                    params["images"] = json!(images);
                }
                if !inline_images.is_empty() {
                    params["inline_images"] = json!(inline_images);
                }
                (Purpose::Turn, "send", params)
            }
            Self::Approve {
                approval_id,
                decision,
            } => (
                Purpose::Approve,
                "approve",
                json!({ "approval_id": approval_id, "decision": decision_str(*decision) }),
            ),
            Self::Abort => (Purpose::Abort, "abort", json!({})),
            Self::ListSessions => (
                Purpose::Sessions,
                "list_sessions",
                json!({ "limit": SESSION_PAGE }),
            ),
            Self::ListSessionsPage { offset, .. } => (
                Purpose::Sessions,
                "list_sessions",
                json!({ "limit": SESSION_PAGE, "offset": offset }),
            ),
            Self::NewSession => (Purpose::NewSession, "new_session", json!({})),
            Self::Resume(id) => (Purpose::Resume, "resume", json!({ "session_id": id })),
            Self::DeleteSession(id) => (
                Purpose::DeleteSession,
                "delete_session",
                json!({ "session_id": id }),
            ),
            Self::SettingsSchema => (Purpose::SettingsSchema, "settings_schema", json!({})),
            Self::GetSettings => (Purpose::Settings, "get_settings", json!({})),
            Self::SetSetting { key, value, scope } => (
                Purpose::SetSetting,
                "set_setting",
                json!({ "key": key, "value": value, "scope": scope }),
            ),
            Self::ResetSetting { key, scope } => (
                Purpose::ResetSetting,
                "reset_setting",
                json!({ "key": key, "scope": scope }),
            ),
            Self::ApplySettings => (Purpose::ApplySettings, "apply_settings", json!({})),
            Self::Recover => (Purpose::Recover, "recovery_snapshot", json!({})),
            Self::State => (Purpose::State, "state", json!({})),
            Self::ListModels => (Purpose::Models, "list_models", json!({})),
            Self::ListImageModels => (
                Purpose::ImageModels,
                "list_models",
                json!({ "purpose": "image" }),
            ),
            Self::SetModel { provider, model_id } => (
                Purpose::SetModel,
                "set_model",
                json!({ "provider": provider, "model_id": model_id }),
            ),
            Self::SetThinking(level) => (
                Purpose::SetThinking,
                "set_thinking",
                json!({ "level": level }),
            ),
            Self::SetApproval(mode) => (
                Purpose::SetApproval,
                "set_approval",
                json!({ "mode": mode }),
            ),
            Self::QuestionPresented(question) => (
                Purpose::QuestionPresented,
                "question_presented",
                question.params(),
            ),
            Self::AnswerQuestion {
                question,
                kind,
                results,
            } => {
                let mut params = question.params();
                params["kind"] = json!(kind);
                if !results.is_empty() {
                    params["results"] = json!(results);
                }
                (Purpose::Answer, "question_answer", params)
            }
            Self::Rename(title) => (Purpose::Rename, "rename", json!({ "title": title })),
            Self::PinSession { session_id, pinned } => (
                Purpose::Pin,
                "pin_session",
                json!({ "session_id": session_id, "pinned": pinned }),
            ),
            Self::ForkSession(id) => (Purpose::Fork, "fork_session", json!({ "session_id": id })),
            Self::Export(path) => (
                Purpose::Export,
                "export",
                json!({ "format": "markdown", "path": path }),
            ),
            Self::Undo => (Purpose::Undo, "undo", json!({ "turns": 1 })),
            Self::Steer(text) => (Purpose::Steer, "steer", json!({ "text": text })),
            // A retry starts a turn, so its reply settles the turn like `send`'s.
            Self::Retry => (Purpose::Turn, "retry", json!({})),
            Self::Compact => (Purpose::Compact, "compact", json!({ "reason": "manual" })),
            Self::LiveStart { instructions } => {
                let mut params = json!({});
                if let Some(instructions) = instructions {
                    params["instructions"] = json!(instructions);
                }
                (Purpose::LiveStart, "live_start", params)
            }
            Self::LiveListen => (Purpose::Listen, "live_listen", json!({})),
            Self::LiveMute(muted) => (Purpose::LiveMute, "live_mute", json!({ "muted": muted })),
            Self::LiveStop => (Purpose::LiveStop, "live_stop", json!({})),
            Self::LiveSpeak(Some(text)) => (Purpose::Speak, "live_speak", json!({ "text": text })),
            Self::LiveSpeak(None) => (Purpose::Speak, "live_speak", json!({})),
        }
    }
}

fn decision_str(decision: Decision) -> &'static str {
    match decision {
        Decision::Allow => "allow",
        Decision::Always => "always",
        Decision::Project => "project",
        Decision::Deny => "deny",
    }
}

/// The decision for a wire name (`allow`, `always`, `project`, `deny`).
pub fn parse_decision(name: &str) -> Option<Decision> {
    [
        Decision::Allow,
        Decision::Always,
        Decision::Project,
        Decision::Deny,
    ]
    .into_iter()
    .find(|d| decision_str(*d) == name)
}

/// Something the worker reports, in the order it happened.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// The handshake finished.
    Connected(Box<Ready>),
    /// The engine could not be started or negotiated with.
    ConnectFailed(String),
    /// A command went out as request `id`. Sent before any message that request produces.
    Requested { command: Command, id: RequestId },
    /// A command could not be sent.
    RequestFailed { command: Command, error: String },
    /// A message from the engine.
    Message(Message),
}

type ReplySender = Sender<Result<Value, String>>;
pub type RpcReply = Receiver<Result<Value, String>>;

struct Work {
    call: Call,
    reply: Option<ReplySender>,
}

enum Call {
    Command(Command),
    Rpc(&'static str, Value),
    Preset { session: String, roles: Value, main: Option<PresetModel> },
}

#[derive(serde::Deserialize)]
pub struct PresetModel {
    provider: String,
    model: String,
    thinking: Option<String>,
}

fn apply_preset(
    rpc: &mut impl FnMut(&str, Value) -> Result<Value, String>,
    session: &str,
    roles: Value,
    main: Option<PresetModel>,
) -> Result<(), String> {
    if rpc("get_session_stats", json!({}))?["session_id"].as_str() != Some(session) {
        return Err("The active session changed; try again from that session".into());
    }
    rpc("set_setting", json!({"key":"model_roles", "value":roles, "scope":"global"}))?;
    rpc("apply_settings", json!({}))?;
    if let Some(main) = main {
        rpc("set_model", json!({"provider":main.provider,"model_id":main.model}))?;
        rpc("set_thinking", json!({"level":main.thinking.as_deref().unwrap_or("default")}))?;
    }
    Ok(())
}

/// Handle to the worker thread.
pub struct Engine {
    commands: Sender<Work>,
    updates: Receiver<Update>,
    stop: Arc<AtomicBool>,
    done: Receiver<()>,
    worker: Option<JoinHandle<()>>,
}

impl Engine {
    /// Start the engine described by `launch` on a worker thread.
    pub fn spawn(launch: Launch, waker: Waker) -> Self {
        Self::spawn_after(launch, waker, None)
    }

    pub fn spawn_after(launch: Launch, waker: Waker, previous: Option<Weak<AtomicBool>>) -> Self {
        let (commands, command_rx) = mpsc::channel();
        let (update_tx, updates) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("dial-engine".into())
            .spawn(move || {
                if let Some(previous) = previous {
                    let deadline = std::time::Instant::now() + SHUTDOWN + Duration::from_secs(2);
                    while previous.strong_count() > 0 {
                        if worker_stop.load(Ordering::SeqCst) {
                            return;
                        }
                        if std::time::Instant::now() >= deadline {
                            let _ = update_tx.send(Update::ConnectFailed("The previous engine is still shutting down. Try reconnecting again.".into()));
                            waker();
                            return;
                        }
                        thread::sleep(POLL);
                    }
                }
                run(&launch, &command_rx, &update_tx, &worker_stop, &waker);
                let _ = done_tx.send(());
                waker();
            })
            .expect("spawn engine worker thread");
        Self {
            commands,
            updates,
            stop,
            done,
            worker: Some(worker),
        }
    }

    /// The worker keeps this alive until its client is gone, even if shutdown timed out.
    pub fn lifetime(&self) -> Weak<AtomicBool> {
        Arc::downgrade(&self.stop)
    }

    /// Queue a command. Dropped silently once the worker has ended; the model already shows that.
    pub fn send(&self, command: Command) {
        let _ = self.enqueue(command);
    }

    /// Report when the worker is no longer available to accept a user draft.
    pub fn enqueue(&self, command: Command) -> Result<(), String> {
        self.queue(Work {
            call: Call::Command(command),
            reply: None,
        })
    }

    pub fn submit(&self, command: Command) -> Result<RpcReply, String> {
        self.with_reply(Call::Command(command))
    }

    pub fn rpc(&self, method: &'static str, params: Value) -> Result<RpcReply, String> {
        self.with_reply(Call::Rpc(method, params))
    }

    /// Run the steps consecutively on this worker; switching tabs cannot redirect a later step.
    pub fn apply_preset(&self, session: String, roles: Value, main: Option<PresetModel>) -> Result<RpcReply, String> {
        self.with_reply(Call::Preset { session, roles, main })
    }

    fn with_reply(&self, call: Call) -> Result<RpcReply, String> {
        let (tx, rx) = mpsc::channel();
        self.queue(Work {
            call,
            reply: Some(tx),
        })?;
        Ok(rx)
    }

    fn queue(&self, work: Work) -> Result<(), String> {
        self.commands
            .send(work)
            .map_err(|_| "Dial has stopped".into())
    }

    /// Updates queued since the last call.
    pub fn drain(&self) -> Vec<Update> {
        self.updates.try_iter().collect()
    }

    /// Ask the engine to shut down gracefully and wait a bounded time for it.
    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // A worker still inside the connect handshake cannot see the flag; do not block the UI on
        // it. Dropping the client later closes the engine's stdin, which ends it.
        if self
            .done
            .recv_timeout(SHUTDOWN + Duration::from_secs(1))
            .is_ok()
            && let Some(worker) = self.worker.take()
        {
            let _ = worker.join();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if self.worker.is_some() {
            self.shutdown();
        }
    }
}

pub fn fork_before(launch: &Launch, source: &str, prompt: &str) -> Result<String, String> {
    let mut client = Client::connect(launch, CONNECT_TIMEOUT).map_err(|e| e.to_string())?;
    let result = (|| {
        if !client.supports("branch_session") {
            return Err("This engine does not support branching".into());
        }
        let mut rpc = |method: &str, params| {
            client
                .call(method, params, Duration::from_secs(120))
                .map_err(|e| e.to_string())
        };
        let fork = rpc("fork_session", json!({"session_id":source}))?;
        let id = fork["session_id"]
            .as_str()
            .filter(|s| !s.is_empty() && *s != source)
            .ok_or("Dial did not confirm a new session")?;
        let trim = (|| {
            let mut offset = 0;
            let mut turns = None;
            loop {
                let page = rpc("messages_page", json!({"offset":offset,"limit":32}))?;
                for message in page["messages"]
                    .as_array()
                    .ok_or("Invalid conversation page")?
                {
                    if message["role"] != "user" {
                        continue;
                    }
                    let content = &message["content"];
                    let text = content.as_str().map(str::to_owned).unwrap_or_else(|| {
                        content
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|p| p["type"] == "text")
                            .filter_map(|p| p["text"].as_str())
                            .collect()
                    });
                    if text == prompt {
                        if turns.is_some() {
                            return Err("This prompt occurs more than once".to_string());
                        }
                        turns = Some(0usize);
                    }
                    if let Some(n) = &mut turns {
                        *n += 1;
                    }
                }
                if page["next_offset"].is_null() {
                    break;
                }
                offset = page["next_offset"]
                    .as_u64()
                    .filter(|n| *n > offset)
                    .ok_or("Invalid conversation cursor")?;
            }
            let turns = turns.ok_or("Cannot identify this prompt in the saved conversation")?;
            rpc("branch_session", json!({"turns":turns,"confirm":true}))?;
            Ok(())
        })();
        trim.map_err(|e| {
            format!(
                "Could not confirm branch point in copy {id}: {e}. The original was not changed."
            )
        })?;
        Ok(id.to_owned())
    })();
    let _ = client.shutdown(SHUTDOWN);
    result
}

fn run(
    launch: &Launch,
    commands: &Receiver<Work>,
    updates: &Sender<Update>,
    stop: &AtomicBool,
    waker: &Waker,
) {
    let emit = |update: Update| {
        if updates.send(update).is_ok() {
            waker();
        }
    };
    let mut client = match Client::connect(launch, CONNECT_TIMEOUT) {
        Ok(client) => client,
        Err(error) => {
            emit(Update::ConnectFailed(error.to_string()));
            return;
        }
    };
    emit(Update::Connected(Box::new(client.ready().clone())));
    let mut replies = HashMap::new();
    loop {
        if stop.load(Ordering::SeqCst) {
            let _ = client.shutdown(SHUTDOWN);
            return;
        }
        loop {
            match commands.try_recv() {
                Ok(Work { call, reply }) => {
                    let (command, method, params) = match call {
                        Call::Command(command) => {
                            let (_, method, params) = command.request();
                            (Some(command), method, params)
                        }
                        Call::Rpc(method, params) => (None, method, params),
                        Call::Preset { session, roles, main } => {
                            let result = apply_preset(&mut |method, params| {
                                client.call(method, params, Duration::from_secs(30)).map_err(|e| e.to_string())
                            }, &session, roles, main);
                            // Even a partial failure can have saved settings or changed the model.
                            for command in [Command::GetSettings, Command::State] {
                                let (_, method, params) = command.request();
                                if let Ok(id) = client.request(method, params) {
                                    emit(Update::Requested { command, id });
                                }
                            }
                            if let Some(reply) = reply {
                                let _ = reply.send(result.map(|()| Value::Null));
                            }
                            continue;
                        }
                    };
                    match client.request(method, params) {
                        Ok(id) => {
                            if let Some(reply) = reply {
                                replies.insert(id, (method, reply));
                            }
                            if let Some(command) = command {
                                emit(Update::Requested { command, id });
                            }
                        }
                        Err(error) => {
                            if let Some(reply) = reply {
                                let _ = reply.send(Err(error.to_string()));
                            }
                            if let Some(command) = command {
                                emit(Update::RequestFailed {
                                    command,
                                    error: error.to_string(),
                                });
                            }
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let _ = client.shutdown(SHUTDOWN);
                    return;
                }
            }
        }
        if let Some(message) = client.poll(POLL) {
            let ended = matches!(message, Message::Disconnected { .. });
            if let Message::Response { id, result, .. } = &message
                && let Some((_, reply)) = replies.remove(id)
            {
                let _ = reply.send(result.clone().map_err(|error| error.message));
            }
            // A turn reply normally arrives at the end. Confirm an accepted send sooner once
            // this exact request produces observable work, so the composer can take notes.
            if let Message::Event(event) = &message
                && matches!(
                    event.kind.as_str(),
                    "text" | "reasoning" | "tool_call" | "approval" | "question"
                )
                && let Some(id) = event.request_id()
                && replies
                    .get(&id)
                    .is_some_and(|(method, _)| *method == "send")
                && let Some((_, reply)) = replies.remove(&id)
            {
                let _ = reply.send(Ok(Value::Null));
            }
            emit(Update::Message(message));
            if ended {
                return;
            }
        }
    }
}
