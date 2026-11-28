//! Application state built from engine updates. Pure: no toolkit, no threads.
//!
//! [`Model::apply`] folds one [`Update`] in and returns follow-up [`Command`]s (refreshing the
//! session list, recovering after lost events). The conversation itself is
//! [`dial_client::Conversation`], the shared view model every Dial frontend uses.

use crate::engine::{Command, Purpose, QuestionRef, Update};
use crate::voice::{self, Voice};
use dial_client::{Conversation, Entry, Message, Ready, RequestId};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Legacy unmarked compaction frame prepended by older engines' model-visible `messages()`.
const COMPACTION_FRAME: &str =
    "<summary>\nSummary of the earlier conversation (older messages were compacted away):\n";

/// Whether `messages` holds a compacted model view rather than the full active-branch archive.
pub(crate) fn has_compaction_truncation(messages: &[Value]) -> bool {
    messages.iter().any(|message| {
        message["dial_compaction_summary"] == true
            || message["dial_compaction_notes"] == true
            || message["dial_compaction_files"] == true
            || message["dial_compaction_critical"] == true
            || !message["dial_snapcompact"].is_null()
            || message["content"]
                .as_str()
                .is_some_and(|text| text.starts_with(COMPACTION_FRAME))
    })
}

/// Replay `messages` into a scratch Conversation so harness user-role records stay aligned
/// with the exact row indices produced by `load_snapshot` / `load_history`.
fn replay_received_from(messages: &[Value]) -> HashMap<usize, (String, bool)> {
    let mut replay = Conversation::default();
    let mut senders = HashMap::new();
    for message in messages {
        let index = replay.entries().len();
        let sender = journal_sender(message).map(str::to_owned);
        replay.apply(&Message::Event(dial_client::Event {
            kind: "replay_message".into(),
            body: json!({ "message": message }),
        }));
        if let Some(from) = sender && replay.entries().len() == index + 1 {
            senders.insert(index, (from, true));
        }
    }
    senders
}

/// Mail is display data, not a prompt. Never classify a person's text by a bracketed prefix.
fn journal_sender(message: &Value) -> Option<&str> {
    if message["role"] != "user" || message["dial_structured_text"] == true {
        return None;
    }
    // The engine writes these records as plain text; preserve other shapes (e.g. attachments).
    let text = message["content"].as_str()?;
    if let Some(sender) = message["dial_worker_result"].as_str() {
        return Some(if sender.trim().is_empty() { "worker" } else { sender });
    }
    if message["dial_child_coordination"] != true { return None; }
    // Coordination also includes restart briefings. Only a mailbox envelope names a sender.
    let sender = text.strip_prefix("[Message from ")?.split_once("]\n")?.0;
    (!sender.trim().is_empty() && !sender.contains(['\r', '\n'])).then_some(sender)
}
#[cfg(test)]
mod tests;

/// State of the engine connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connection {
    /// Launching the engine and negotiating.
    Starting,
    /// Connected and serving.
    Live {
        model: String,
        engine_version: Option<String>,
    },
    /// No engine: the design preview.
    Preview,
    /// The engine could not be started.
    Failed(String),
    /// The engine exited or its output broke.
    Ended,
}

/// One row of the session sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    /// Last activity as the engine reports it (display text).
    pub updated: String,
    pub messages: u64,
    pub pinned: bool,
    pub current: bool,
    /// Another Dial process (a terminal, another window) holds the session; resuming it here
    /// would make the engine open a copy instead.
    pub open_elsewhere: bool,
}

impl SessionSummary {
    /// Parse one `list_sessions` entry; `None` without an id.
    pub fn from_wire(entry: &Value) -> Option<Self> {
        let id = entry["session_id"].as_str().filter(|id| !id.is_empty())?;
        let text = |key: &str| {
            entry[key]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        };
        let title = text("title")
            .or_else(|| text("first_message"))
            .unwrap_or_else(|| "Untitled session".to_owned());
        let updated = match &entry["updated"] {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        };
        Some(Self {
            id: id.to_owned(),
            title: first_line(&title, 80),
            updated,
            messages: entry["messages"].as_u64().unwrap_or_default(),
            pinned: entry["pinned"].as_bool().unwrap_or_default(),
            current: entry["current"].as_bool().unwrap_or_default(),
            open_elsewhere: entry["open_elsewhere"].as_bool().unwrap_or_default(),
        })
    }
}

/// The first line of `text`, clipped to `max` characters with an ellipsis.
pub fn first_line(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    if line.chars().count() <= max {
        line.to_owned()
    } else {
        let mut clipped: String = line.chars().take(max.saturating_sub(1)).collect();
        clipped.push('…');
        clipped
    }
}

/// `fresh` (the engine's list, newest activity first) with the rows that did not change since
/// `previous` kept in their `previous` order. Opening a session can bump its timestamp in the
/// engine without adding anything to it; without this the clicked row would jump to the top.
/// Rows that are new or gained messages keep the engine's position, so real activity still
/// floats up.
pub fn keep_session_order(
    previous: &[SessionSummary],
    fresh: Vec<SessionSummary>,
) -> Vec<SessionSummary> {
    let before: HashMap<&str, (usize, &SessionSummary)> = previous
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id.as_str(), (i, s)))
        .collect();
    let unchanged = |s: &SessionSummary| {
        before
            .get(s.id.as_str())
            .is_some_and(|(_, old)| old.messages == s.messages)
    };
    let mut kept: Vec<SessionSummary> = fresh.iter().filter(|s| unchanged(s)).cloned().collect();
    kept.sort_by_key(|s| before[s.id.as_str()].0);
    // The sidebar also groups by age. A resume touches the journal but is not a new message.
    for row in &mut kept {
        let old = before[row.id.as_str()].1;
        if !old.updated.is_empty() {
            row.updated.clone_from(&old.updated);
        }
    }
    let mut kept = kept.into_iter();
    fresh
        .into_iter()
        .map(|s| if unchanged(&s) { kept.next().unwrap_or(s) } else { s })
        .collect()
}

/// Counts of tool calls by state, for the activity summary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToolStats {
    pub running: usize,
    pub succeeded: usize,
    pub failed: usize,
}

/// The model in force and its reasoning controls (`state.current_model`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CurrentModel {
    pub provider: String,
    pub model_id: String,
    /// The effective window from `state`, absent on older engines.
    pub context_window: Option<u64>,
    /// Actual loaded window reported by a local server, when probed.
    pub loaded_context_window: Option<u64>,
    /// User/config requested window before hardware/server capping.
    pub requested_context_window: Option<u64>,
    /// True when the active model runs locally (Dial MLX/GGUF or localhost server).
    pub local: bool,
    /// True when Dial supervises this local server (so it can be unloaded/restarted).
    pub managed_local: bool,
    /// True while Dial's supervised local server currently holds the model in RAM.
    pub resident: bool,
    /// Machine physical RAM reported by the engine (bytes), when known.
    pub system_memory_bytes: Option<u64>,
    /// The session's reasoning level; `None` runs the model's default.
    pub thinking: Option<String>,
    /// The model's default level, when the catalog knows it.
    pub default_thinking: Option<String>,
    /// Levels `set_thinking` accepts for this model.
    pub levels: Vec<String>,
    pub thinking_settable: bool,
}

impl CurrentModel {
    /// Parse `state.current_model`; `None` without a provider and model id.
    pub fn from_wire(value: &Value) -> Option<Self> {
        let provider = value["provider"].as_str().filter(|s| !s.is_empty())?;
        let model_id = value["model_id"].as_str().filter(|s| !s.is_empty())?;
        let thinking = &value["thinking"];
        Some(Self {
            provider: provider.to_owned(),
            model_id: model_id.to_owned(),
            context_window: value["context_window"].as_u64().filter(|&n| n > 0),
            loaded_context_window: value["loaded_context_window"].as_u64().filter(|&n| n > 0),
            requested_context_window: value["requested_context_window"].as_u64().filter(|&n| n > 0),
            local: value["local"].as_bool().unwrap_or(provider == "mlx"),
            managed_local: value["managed_local"].as_bool().unwrap_or(false),
            resident: value["resident"].as_bool().unwrap_or(false),
            system_memory_bytes: value["system_memory_bytes"].as_u64().filter(|&n| n > 0),
            thinking: thinking["level"].as_str().map(str::to_owned),
            default_thinking: thinking["default_level"].as_str().map(str::to_owned),
            levels: strings(&thinking["levels"]),
            thinking_settable: thinking["settable"].as_bool().unwrap_or_default(),
        })
    }
}

/// One model `set_model` accepts (a `list_models` row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOption {
    pub provider: String,
    pub model_id: String,
    pub label: String,
    pub context_window: Option<u64>,
    pub reasoning: bool,
    pub vision: Option<bool>,
    /// Configured by the user: the main model, a favorite, a profile or a role.
    pub configured: bool,
    /// Discovered in a downloaded local model store.
    pub local: bool,
    /// Present in the provider's live `/models` or account listing.
    pub listed: bool,
    /// Only from the static bundled catalog (not configured, local, or live-listed).
    pub catalog_only: bool,
    pub local_engine: Option<EngineChoice>,
    pub memory_fit: Option<MemoryFit>,
}

/// Memory-fit estimate reported by the engine for a downloaded local model.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct MemoryFit {
    pub fit: String,
    pub weights_bytes: u64,
    pub kv_bytes: u64,
    pub resident_bytes: u64,
    pub memory_bytes: Option<u64>,
    pub context_tokens: u64,
    pub max_context_tokens: u64,
}

/// Only present when the connected engine supports per-model runtime selection.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, serde::Serialize)]
pub struct EngineChoice {
    pub selected: String,
    pub choices: Vec<String>,
    pub setting: String,
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub installed: std::collections::BTreeMap<String, bool>,
}
impl ModelOption {
    /// Parse one `list_models` row; `None` without a provider and model id.
    pub fn from_wire(value: &Value) -> Option<Self> {
        let provider = value["provider"].as_str().filter(|s| !s.is_empty())?;
        let model_id = value["model_id"].as_str().filter(|s| !s.is_empty())?;
        let sources = strings(&value["sources"]);
        let configured = sources.iter().any(|source| {
            matches!(source.as_str(), "main" | "favorite")
                || source.starts_with("profile:")
                || source.starts_with("role:")
        });
        let listed = sources.iter().any(|source| source == "listing");
        let catalog_only = !sources.is_empty() && sources.iter().all(|source| source == "catalog");
        Some(Self {
            provider: provider.to_owned(),
            model_id: model_id.to_owned(),
            label: value["label"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or(model_id)
                .to_owned(),
            context_window: value["context_window"].as_u64(),
            reasoning: value["reasoning"].as_bool().unwrap_or_default(),
            vision: value["vision"].as_bool(),
            configured,
            local: sources.iter().any(|source| source == "local"),
            listed,
            catalog_only,
            local_engine: serde_json::from_value(value["local_engine"].clone()).ok(),
            memory_fit: serde_json::from_value(value["memory_fit"].clone()).ok(),
        })
    }
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// The user's answer to one question of the open `ask` request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    /// The question's `id`.
    pub id: String,
    /// Chosen option labels.
    #[serde(default)]
    pub selected: Vec<String>,
    /// Free text instead of, or besides, an option.
    #[serde(default)]
    pub custom: Option<String>,
    /// Image files attached to the free text, by path (the engine reads and verifies them).
    #[serde(default)]
    pub images: Vec<String>,
    /// Pasted images as base64; the app saves them to files and sends their paths.
    #[serde(default)]
    pub inline_images: Vec<String>,
}

/// How the user closes the open question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Reply {
    /// Submit the answers.
    Answers,
    /// Decline to answer; the agent carries on without them.
    Cancel,
}

/// Everything the UI shows that comes from the engine.
#[derive(Debug, Default)]
pub struct Model {
    pub connection: Option<Connection>,
    pub conversation: Conversation,
    /// Senders of delivery notices and controller-authored journal records, with whether the
    /// record came from the session journal (`true`) or a live arrival notice (`false`).
    /// The wire client retains the text but not its provenance. Index only rows it actually appended.
    pub(crate) received_from: HashMap<usize, (String, bool)>,
    pub sessions: Vec<SessionSummary>,
    /// Ignore late pages from a history refresh superseded by a new one.
    session_page: Option<(RequestId, u64)>,
    session_pages: Vec<SessionSummary>,
    /// Confirmed deletions: outstanding list pages (including other tabs') may predate them.
    pub(crate) deleted: HashSet<String>,
    /// The model in force, once the engine reported it.
    pub current_model: Option<CurrentModel>,
    /// The approval mode in force (`ask`, `write`, `auto`, `readonly`).
    pub approval: Option<String>,
    /// Models the user can switch to, once loaded.
    pub models: Vec<ModelOption>,
    /// Set when `models` changed; the app publishes the list and clears it.
    pub models_changed: bool,
    last_discovery: Option<Instant>,
    discovering: bool,
    /// Short-lived message for the toast area (a refused request, a failed approval).
    pub toast: Option<String>,
    /// Whether the model the engine started with takes images (`ready.image_input`).
    pub ready_image_input: Option<bool>,
    /// Whether the engine takes the structured `type_context` on `send` (`ready.features`).
    pub type_context_supported: bool,
    /// Whether a model switch can ask the engine to start its own local inference server.
    pub prepare_local_model_supported: bool,
    /// The settings reference (`settings_schema`'s `settings`), once loaded.
    pub settings_schema: Option<Vec<Value>>,
    /// Saved settings' effective values by key, once loaded.
    pub settings: Option<HashMap<String, SavedSetting>>,
    /// Set when the settings or their reference changed; the app publishes them and clears it.
    pub settings_changed: bool,
    /// Models that generate images (`list_models` rows with `purpose: "image"`), once asked; empty
    /// when the engine predates that listing.
    pub image_models: Option<Vec<Value>>,
    pending: HashMap<RequestId, Purpose>,
    /// User-visible guidance awaiting engine acknowledgement, without legacy Type framing.
    pending_notes: HashMap<RequestId, String>,
    pub(crate) recovering: bool,
    /// In-flight `messages_page` fetch for full uncompacted archived history (or oversized recovery).
    history_page: Option<(RequestId, u64)>,
    history_pages: Vec<Value>,
    history_page_session: Option<String>,
    history_page_from_truncation: bool,
    history_page_live: Option<Value>,
    /// The open question already acknowledged as presented; the engine refuses a second ack.
    presented: Option<String>,
    /// Sessions to delete once the engine has moved off them (it refuses the active one).
    delete_after_switch: Vec<String>,
    /// A rename waiting for the engine to open its session (it only renames the active one).
    rename_after_switch: Option<(String, String)>,
    /// Saved settings wait for the running turn to end before they are applied.
    apply_pending: bool,
    /// `apply_settings` has been queued or sent to the engine and has not settled yet.
    applying_settings: bool,
    /// The conversation's size in tokens as the engine last reported it (`context_usage`).
    pub context: Option<ContextUsage>,
    /// Latest telemetry for the active inference request, never conversation history.
    pub model_progress: Option<ModelProgress>,
    /// Merged telemetry from the latest turn/step (decode tok/s, prompt cache hits), retained
    /// briefly after `model_progress` clears so local users can see speed and cache efficiency.
    pub last_inference: Option<ModelProgress>,
    /// Hands-free voice mode.
    pub voice: Voice,
    /// Whether the engine reads saved sessions without opening them (`session_messages`).
    pub session_messages_supported: bool,
    /// Saved conversations read for previews: `(session id, messages)`; the app takes them.
    pub read_sessions: Vec<(String, Vec<Value>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, serde::Serialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct ModelProgress {
    pub phase: InferencePhase,
    pub processed_tokens: Option<u64>,
    pub prompt_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    #[serde(default)]
    pub tokens_per_second: Option<f64>,
    #[serde(default)]
    pub cached_tokens: Option<u64>,
    #[serde(default)]
    pub eta_secs: Option<f64>,
    #[serde(default)]
    pub estimated: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InferencePhase { Loading, Processing, Generating }

/// How much of the model's context the conversation fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextUsage {
    pub tokens: u64,
    /// Counted roughly rather than by the provider.
    pub estimated: bool,
}

/// One saved setting's effective value (`get_settings`).
#[derive(Debug, Clone, PartialEq)]
pub struct SavedSetting {
    pub value: Value,
    /// A credential: `value` is a placeholder, never the secret.
    pub redacted: bool,
    /// The saved layer that sets it (`"project"` or `"global"`); `None` when neither does or
    /// the engine predates the `scope` field.
    pub scope: Option<String>,
}

impl Model {
    /// A model waiting for its engine.
    pub fn starting() -> Self {
        Self {
            connection: Some(Connection::Starting),
            ..Self::default()
        }
    }

    /// A model for the design preview, with no engine behind it.
    pub fn preview() -> Self {
        Self {
            connection: Some(Connection::Preview),
            ..Self::default()
        }
    }

    /// Current connection state.
    pub fn connection(&self) -> &Connection {
        self.connection.as_ref().unwrap_or(&Connection::Starting)
    }

    /// Whether a new prompt can be sent now.
    pub fn can_send(&self) -> bool {
        matches!(self.connection(), Connection::Live { .. })
            && !self.conversation.turn_running()
            && !self.recovering
            && !self.applying_settings
    }

    /// Reserve one provider scan before it is dispatched (the worker acknowledges it later).
    pub fn discover_if_due(&mut self, now: Instant, interval: Duration) -> Option<Command> {
        if !matches!(self.connection(), Connection::Live { .. }) || self.discovering
            || self.last_discovery.is_some_and(|last| now.saturating_duration_since(last) < interval) {
            return None;
        }
        self.last_discovery = Some(now);
        self.discovering = true;
        Some(Command::DiscoverModels)
    }

    /// Start voice mode, or talk again while it waits. Replies are asked to suit speech unless
    /// the user set their own `voice.live.instructions`.
    pub fn start_voice(&mut self) -> Result<Option<Command>, String> {
        if !matches!(self.connection(), Connection::Live { .. }) {
            return Err("Dial is not connected".into());
        }
        if self.conversation.turn_running() && !self.voice.active() {
            return Err("Wait for Dial to finish, then talk".into());
        }
        let custom = self
            .settings
            .as_ref()
            .and_then(|s| s.get("voice.live.instructions"))
            .and_then(|s| s.value.as_str())
            .is_some_and(|text| !text.trim().is_empty());
        let instructions = (!custom).then(|| voice::SPOKEN_STYLE.to_owned());
        Ok(self.voice.start(instructions))
    }

    /// The voice panel's main control: talk over a reply, or stop recording.
    pub fn voice_tap(&mut self) -> Option<Command> {
        self.voice.tap(self.conversation.turn_running())
    }

    /// Stop the turn voice mode started from a misheard request.
    pub fn voice_cancel(&mut self) -> Option<Command> {
        self.voice.cancel(self.conversation.turn_running())
    }

    /// "Read aloud" on a reply, or stop reading with `None`.
    pub fn read_aloud(&self, text: Option<String>) -> Result<Command, String> {
        if !matches!(self.connection(), Connection::Live { .. }) {
            return Err("Dial is not connected".into());
        }
        Ok(Command::LiveSpeak(text.filter(|t| !t.trim().is_empty())))
    }

    /// The requests that delete sessions `ids`. The engine refuses to delete its active session,
    /// so when the session on screen is among them a fresh one opens first and it goes last.
    pub fn delete_sessions(&mut self, ids: &[String]) -> Result<Vec<Command>, String> {
        let current = self.conversation.session_id().map(str::to_owned);
        let includes_current = current.as_ref().is_some_and(|c| ids.contains(c));
        if includes_current && self.conversation.turn_running() {
            return Err("Stop the running turn (Esc) before deleting the open session".into());
        }
        let mut commands: Vec<Command> = ids
            .iter()
            .filter(|id| Some(*id) != current.as_ref())
            .map(|id| Command::DeleteSession(id.clone()))
            .collect();
        if let Some(current) = current.filter(|_| includes_current) {
            self.delete_after_switch.push(current);
            commands.push(Command::NewSession);
        }
        Ok(commands)
    }

    /// Save `value` for setting `key` in `scope`; it is applied once saved.
    pub fn save_setting(key: &str, value: Value, scope: &str) -> Result<Command, String> {
        if value.is_null() {
            return Err("A setting needs a value; use Reset to restore the default".into());
        }
        Ok(Command::SetSetting {
            key: key.to_owned(),
            value,
            scope: valid_scope(scope)?,
        })
    }

    /// Remove setting `key` from `scope`; applied once removed.
    pub fn reset_setting(key: &str, scope: &str) -> Result<Command, String> {
        Ok(Command::ResetSetting {
            key: key.to_owned(),
            scope: valid_scope(scope)?,
        })
    }

    /// The requests that rename session `id` to `title`. The engine renames only its active
    /// session, so another one is opened first and renamed once it is on screen.
    pub fn rename_session(&mut self, id: &str, title: &str) -> Result<Vec<Command>, String> {
        let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        if title.is_empty() {
            return Err("A session needs a name".into());
        }
        if self.conversation.session_id() == Some(id) {
            return Ok(vec![Command::Rename(title)]);
        }
        if self.conversation.turn_running() {
            return Err("Dial is working here; rename that session after this turn".into());
        }
        self.rename_after_switch = Some((id.to_owned(), title));
        Ok(vec![Command::Resume(id.to_owned())])
    }

    /// The request that undoes the last turn; refused while one runs or before any prompt.
    pub fn undo(&self) -> Result<Command, String> {
        if self.conversation.turn_running() {
            return Err("Stop the running turn (Esc) before undoing".into());
        }
        if !self
            .conversation
            .entries()
            .iter()
            .any(|e| matches!(e, Entry::User(_)))
        {
            return Err("There is no turn to undo".into());
        }
        Ok(Command::Undo)
    }

    /// Reload the open conversation from the engine, after a change made outside a turn (a move
    /// through the session tree). Refused while a turn runs.
    pub fn reload(&self) -> Result<Command, String> {
        if self.conversation.turn_running() {
            return Err("Stop the running turn (Esc) first".into());
        }
        Ok(Command::Recover)
    }

    /// The request that adds `text` to the running turn; refused when no turn runs.
    pub fn steer(&self, text: &str, scope: Option<crate::engine::TypeScope>) -> Result<Command, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("Type the guidance to add".into());
        }
        if !self.conversation.turn_running() {
            return Err("Dial is not working on a turn; send this as a new prompt".into());
        }
        Ok(Command::Steer { text: text.to_owned(), scope })
    }

    /// The request that sends the last prompt again; the engine refuses it unless that prompt
    /// went unanswered (a failed or stopped turn).
    pub fn retry(&self) -> Result<Command, String> {
        if self.conversation.turn_running() {
            return Err("Dial is still working on this turn".into());
        }
        if !self
            .conversation
            .entries()
            .iter()
            .any(|e| matches!(e, Entry::User(_)))
        {
            return Err("There is no prompt to retry".into());
        }
        Ok(Command::Retry)
    }

    /// The request that summarizes the conversation to free context; not while a turn runs.
    pub fn compact(&self) -> Result<Command, String> {
        if self.conversation.turn_running() {
            return Err("Stop the running turn (Esc) before compacting".into());
        }
        if self.conversation.entries().is_empty() {
            return Err("There is nothing to compact yet".into());
        }
        Ok(Command::Compact)
    }

    /// Prefer the engine's effective limit over the model's catalog capacity.
    pub fn context_window(&self) -> Option<u64> {
        let current = self.current_model.as_ref()?;
        if current.context_window.is_some() {
            return current.context_window;
        }
        self.models
            .iter()
            .find(|m| m.provider == current.provider && m.model_id == current.model_id)
            .and_then(|m| m.context_window)
    }

    /// Actual loaded window reported by a local server, when probed.
    pub fn loaded_context_window(&self) -> Option<u64> {
        self.current_model.as_ref()?.loaded_context_window
    }

    /// Configured/requested context window before hardware or server capping.
    pub fn requested_context_window(&self) -> Option<u64> {
        self.current_model.as_ref()?.requested_context_window
    }
    /// Title of the session on screen.
    pub fn title(&self) -> String {
        let id = self.conversation.session_id();
        self.sessions
            .iter()
            .find(|s| Some(s.id.as_str()) == id)
            .map(|s| s.title.clone())
            .or_else(|| {
                self.conversation.entries().iter().find_map(|e| match e {
                    Entry::User(user) => Some(first_line(&user.text, 80)),
                    _ => None,
                })
            })
            .unwrap_or_else(|| "New session".to_owned())
    }

    /// Paths changed by settled tool calls, first change first, with a per-file revision.
    pub fn changed_paths(&self) -> Vec<(String, usize)> {
        let mut paths: Vec<(String, usize)> = Vec::new();
        for entry in self.conversation.entries() {
            if let Entry::Tool(tool) = entry
                && let Some(result) = &tool.result
            {
                for path in &result.changed_paths {
                    if let Some((_, revision)) = paths.iter_mut().find(|(name, _)| name == path) {
                        *revision += 1;
                    } else {
                        paths.push((path.clone(), 1));
                    }
                }
            }
        }
        paths
    }

    /// Tool calls by state. A settled call with an unknown outcome counts as neither.
    pub fn tool_stats(&self) -> ToolStats {
        let mut stats = ToolStats::default();
        for entry in self.conversation.entries() {
            if let Entry::Tool(tool) = entry {
                match tool.result.as_ref().map(|r| r.succeeded()) {
                    None => stats.running += 1,
                    Some(Some(true)) => stats.succeeded += 1,
                    Some(Some(false)) => stats.failed += 1,
                    Some(None) => {}
                }
            }
        }
        stats
    }

    /// The `question_presented` acknowledgement for the open question `question_id`, once.
    pub fn present_question(&mut self, question_id: &str) -> Option<Command> {
        let question = QuestionRef::of(self.conversation.question()?)?;
        if question.question_id != question_id || self.presented.as_deref() == Some(question_id) {
            return None;
        }
        self.presented = Some(question.question_id.clone());
        Some(Command::QuestionPresented(question))
    }

    /// The request that closes the open question `question_id` with `reply`.
    pub fn answer_question(
        &self,
        question_id: &str,
        reply: Reply,
        answers: &[Answer],
    ) -> Result<Command, String> {
        let request = self
            .conversation
            .question()
            .ok_or("There is no open question")?;
        let question = QuestionRef::of(request)
            .filter(|q| q.question_id == question_id)
            .ok_or("That question is no longer open")?;
        if self.presented.as_deref() != Some(question_id) {
            return Err("The question is still being shown; try again".into());
        }
        let results = match reply {
            Reply::Cancel => Vec::new(),
            Reply::Answers => {
                let asked: Vec<&str> = request["questions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|q| q["id"].as_str())
                    .collect();
                let mut results = Vec::with_capacity(asked.len());
                for id in asked {
                    let answer = answers
                        .iter()
                        .find(|a| a.id == id)
                        .ok_or_else(|| format!("Answer every question (missing `{id}`)"))?;
                    let custom = answer
                        .custom
                        .as_deref()
                        .map(str::trim)
                        .filter(|text| !text.is_empty());
                    let mut result = json!({"id": id, "selectedOptions": answer.selected});
                    if let Some(custom) = custom {
                        result["customInput"] = json!(custom);
                    }
                    if !answer.images.is_empty() {
                        result["customInputImages"] = json!(answer.images);
                    }
                    results.push(result);
                }
                results
            }
        };
        Ok(Command::AnswerQuestion {
            question,
            kind: match reply {
                Reply::Answers => "answers",
                Reply::Cancel => "cancel",
            }
            .to_owned(),
            results,
        })
    }

    /// Fold one update in; returns commands to send next.
    pub fn apply(&mut self, update: Update) -> Vec<Command> {
        let mut follow = Vec::new();
        match update {
            Update::Connected(ready) => {
                self.connect(&ready);
                follow.extend([Command::ListSessions, Command::State, Command::GetSettings]);
                self.discovering = false;
                follow.extend(self.discover_if_due(Instant::now(), Duration::ZERO));
            }
            Update::ConnectFailed(error) => self.connection = Some(Connection::Failed(error)),
            Update::Requested { command, id } => {
                let (purpose, _, _) = command.request();
                if matches!(purpose, Purpose::Turn | Purpose::Recover) {
                    self.model_progress = None;
                    self.last_inference = None;
                }
                match &command {
                    Command::Steer { text, .. } => { self.pending_notes.insert(id, text.clone()); }
                    Command::ListSessions => {
                        self.session_pages.clear();
                        self.session_page = Some((id, 0));
                    }
                    Command::ListSessionsPage { offset, after }
                        if self
                            .session_page
                            .is_some_and(|(current, _)| current == *after) =>
                    {
                        self.session_page = Some((id, *offset));
                    }
                    Command::MessagesPage { offset, after: None } => {
                        self.history_pages.clear();
                        self.history_page = Some((id, *offset));
                    }
                    Command::MessagesPage { offset, after: Some(after) }
                        if self
                            .history_page
                            .is_some_and(|(current, _)| current == *after) =>
                    {
                        self.history_page = Some((id, *offset));
                    }
                    _ => {}
                }
                if let Command::Send {
                    text,
                    images,
                    inline_images,
                    ..
                } = &command
                {
                    self.conversation
                        .push_turn(text, images.len() + inline_images.len(), id);
                }
                if command == Command::Retry {
                    self.conversation.expect_turn(id);
                }
                if purpose == Purpose::Recover {
                    self.recovering = true;
                }
                if purpose == Purpose::ApplySettings {
                    self.applying_settings = true;
                }
                self.voice.requested(&command, id);
                self.pending.insert(id, purpose);
            }
            Update::RequestFailed { command, error } => {
                if command == Command::LiveStop {
                    self.voice.stopped();
                }
                if command == Command::DiscoverModels {
                    self.discovering = false;
                }
                if matches!(command, Command::Recover) {
                    self.recovering = false;
                }
                if matches!(command, Command::MessagesPage { .. }) {
                    let from_truncation = self.history_page_from_truncation;
                    self.reset_history_pages();
                    self.recovering = false;
                    if from_truncation {
                        return follow;
                    }
                }
                if matches!(command, Command::ApplySettings) {
                    self.applying_settings = self
                        .pending
                        .values()
                        .any(|purpose| *purpose == Purpose::ApplySettings);
                }
                if matches!(command, Command::Send { .. } | Command::Retry) && !self.conversation.turn_running() {
                    self.apply_after_turn(&mut follow);
                }
                self.toast = Some(format!("Could not {}: {error}", describe(&command)));
            }
            Update::Message(mut message) => {
                // Preserve delivery metadata and journal attachments before the shared
                // conversation model reduces the wire message to one of its row types.
                let index = self.conversation.entries().len();
                let mut sender = None;
                match &mut message {
                    Message::Event(event) if event.kind == "notice" && event.body["kind"].is_null() && event.body["lane"].is_null() => {
                        let mail = &event.body["received_message"];
                        if let (Some(from), Some(text)) = (mail["from"].as_str().filter(|s| !s.trim().is_empty()), mail["text"].as_str()) {
                            sender = Some((from.to_owned(), false));
                            event.body["message"] = Value::String(text.to_owned());
                        }
                    }
                    Message::Event(event) if event.kind == "replay_message" => {
                        replay_images(&mut event.body["message"]);
                        sender = journal_sender(&event.body["message"]).map(|from| (from.to_owned(), true));
                        if sender.is_none() { crate::legacy_text::project_user_message(&mut event.body["message"]); }
                    }
                    Message::Response { result: Ok(body), .. } if body.get("messages").is_some() => {
                        if let Some(messages) = body["messages"].as_array_mut() {
                            for message in messages {
                                replay_images(message);
                                if journal_sender(message).is_none() { crate::legacy_text::project_user_message(message); }
                            }
                        }
                    }
                    _ => {}
                }
                self.message(&message, &mut follow);
                if let Some(sender) = sender && self.conversation.entries().len() == index + 1 {
                    self.received_from.insert(index, sender);
                }
            }
        }
        if !self.conversation.turn_running() || !matches!(self.connection(), Connection::Live { .. }) {
            self.model_progress = None;
        }
        // A frame may be published before the worker acknowledges the follow-up request.
        self.recovering |= follow.contains(&Command::Recover);
        self.applying_settings |= follow.contains(&Command::ApplySettings);
        if self.conversation.needs_reload()
            && !self.recovering
            && self.history_page.is_none()
            && !self.conversation.turn_running()
            && matches!(self.connection(), Connection::Live { .. })
        {
            self.recovering = true;
            follow.push(Command::Recover);
        }
        follow
    }

    fn connect(&mut self, ready: &Ready) {
        self.voice.reset();
        self.conversation.new_connection(ready);
        if self.conversation.entries().is_empty() { self.received_from.clear(); }
        self.pending.clear();
        self.pending_notes.clear();
        self.recovering = false;
        self.reset_history_pages();
        self.applying_settings = false;
        self.ready_image_input = ready.raw["image_input"].as_bool();
        self.type_context_supported = ready.raw["features"]["type_context"].as_bool() == Some(true);
        self.prepare_local_model_supported = crate::engine::supports_local_preparation(ready);
        self.session_messages_supported = ready.raw["methods"]
            .as_array()
            .is_some_and(|methods| methods.iter().any(|m| m == "session_messages"));
        self.connection = Some(Connection::Live {
            model: ready.model.clone(),
            engine_version: ready.engine_version.clone(),
        });
    }

    fn message(&mut self, message: &Message, follow: &mut Vec<Command>) {
        // A recording that failed before any speech was heard is the voice panel's to show,
        // not a failed turn.
        let failed_listen = matches!(message, Message::Response { id, result, .. }
            if self.voice.owns_failed_listen(*id, result.is_err()));
        if !failed_listen && self.conversation.apply(message)
            && matches!(message, Message::Event(e) if e.kind == "active_session")
        {
            self.received_from.clear();
            self.pending_notes.clear();
        }
        match message {
            Message::Response { id, result, .. } => {
                let Some(purpose) = self.pending.remove(id) else {
                    return;
                };
                if result.is_err() { self.pending_notes.remove(id); }
                if purpose == Purpose::Models {
                    self.discovering = false;
                }
                match (purpose, result) {
                    (Purpose::Sessions, result) => {
                        let Some((current, offset)) = self.session_page else {
                            return;
                        };
                        if current != *id {
                            return;
                        }
                        match result {
                            Ok(page) => {
                                let items = page["sessions"].as_array().into_iter().flatten();
                                self.session_pages
                                    .extend(items.filter_map(SessionSummary::from_wire));
                                if let Some(next) =
                                    page["next_offset"].as_u64().filter(|n| *n > offset)
                                {
                                    follow.push(Command::ListSessionsPage {
                                        offset: next,
                                        after: *id,
                                    });
                                } else {
                                    self.session_page = None;
                                    let mut seen = HashSet::new();
                                    let fresh = std::mem::take(&mut self.session_pages)
                                        .into_iter()
                                        .filter(|s| !self.deleted.contains(&s.id) && seen.insert(s.id.clone()))
                                        .collect();
                                    self.sessions = keep_session_order(&self.sessions, fresh);
                                }
                            }
                            Err(error) => {
                                self.session_page = None;
                                self.session_pages.clear();
                                self.toast = Some(format!("Could not load sessions: {error}"));
                            }
                        }
                    }
                    (Purpose::Recover, Ok(snapshot)) => {
                        self.recovering = false;
                        if !self.conversation.load_snapshot(snapshot) {
                            self.toast = Some("The engine's recovery snapshot was incomplete; the view may be out of date".into());
                        } else {
                            let messages = snapshot["messages"].as_array();
                            self.received_from = messages
                                .map(|rows| replay_received_from(rows))
                                .unwrap_or_default();
                            if let Some(messages) = messages {
                                if has_compaction_truncation(messages) {
                                    // Older engines returned the compacted model view from
                                    // `recovery_snapshot` while `messages_page` already serves
                                    // the full active-branch archive. Page full history now.
                                    self.reset_history_pages();
                                    self.history_page_session =
                                        self.conversation.session_id().map(str::to_owned);
                                    self.history_page_from_truncation = true;
                                    follow.push(Command::MessagesPage {
                                        offset: 0,
                                        after: None,
                                    });
                                } else if let Some(id) = self.conversation.session_id().map(str::to_owned) {
                                    self.reset_history_pages();
                                    self.read_sessions.push((id, messages.clone()));
                                }
                            }
                        }
                    }
                    (Purpose::Recover, Err(error))
                        if error.code.as_deref() == Some("refused")
                            && (error.message.contains("messages_page")
                                || error.message.contains("budget")) =>
                    {
                        // Session exceeded the single-response recovery budget; load full
                        // history via `messages_page` and live interaction state via `state`.
                        self.reset_history_pages();
                        self.history_page_session =
                            self.conversation.session_id().map(str::to_owned);
                        self.history_page_from_truncation = false;
                        self.recovering = true;
                        follow.extend([
                            Command::MessagesPage {
                                offset: 0,
                                after: None,
                            },
                            Command::State,
                        ]);
                    }
                    (Purpose::MessagesPage, result) => {
                        let Some((current, offset)) = self.history_page else {
                            return;
                        };
                        if current != *id {
                            return;
                        }
                        match result {
                            Ok(page) => {
                                let items = page["messages"].as_array().into_iter().flatten();
                                self.history_pages.extend(items.cloned());
                                if let Some(next) =
                                    page["next_offset"].as_u64().filter(|n| *n > offset)
                                {
                                    follow.push(Command::MessagesPage {
                                        offset: next,
                                        after: Some(*id),
                                    });
                                } else {
                                    let messages = std::mem::take(&mut self.history_pages);
                                    let from_truncation = self.history_page_from_truncation;
                                    let target_session = self.history_page_session.take();
                                    let live = self.history_page_live.take();
                                    self.history_page = None;
                                    self.history_page_from_truncation = false;
                                    self.recovering = false;
                                    let same_session = target_session.as_deref().is_none_or(|wanted| {
                                        self.conversation.session_id().is_none_or(|cur| cur == wanted)
                                    });
                                    if same_session
                                        && !self.conversation.turn_running()
                                        && (!from_truncation || !has_compaction_truncation(&messages))
                                    {
                                        let mut loaded_snapshot = false;
                                        if !from_truncation
                                            && let Some(Value::Object(mut live_map)) = live
                                        {
                                            live_map.insert(
                                                "messages".into(),
                                                Value::Array(messages.clone()),
                                            );
                                            loaded_snapshot = self
                                                .conversation
                                                .load_snapshot(&Value::Object(live_map));
                                        }
                                        if !loaded_snapshot {
                                            let sid = target_session
                                                .or_else(|| self.conversation.session_id().map(str::to_owned));
                                            self.conversation.load_history(sid.as_deref(), &messages);
                                        }
                                        self.received_from = replay_received_from(&messages);
                                        if !has_compaction_truncation(&messages)
                                            && let Some(id) = self.conversation.session_id()
                                        {
                                            self.read_sessions.push((id.to_owned(), messages));
                                        }
                                    }
                                }
                            }
                            Err(error) => {
                                let from_truncation = self.history_page_from_truncation;
                                self.reset_history_pages();
                                self.recovering = false;
                                if !from_truncation {
                                    self.toast =
                                        Some(format!("Could not reload the session: {error}"));
                                }
                            }
                        }
                    }
                    (Purpose::LiveStart, result) => {
                        let result = result.as_ref().map_err(ToString::to_string);
                        if let Err(error) = &result {
                            self.toast = Some(format!("Could not start voice mode: {error}"));
                        }
                        follow.extend(self.voice.started(result));
                    }
                    (Purpose::Listen, result) => {
                        let result = result
                            .as_ref()
                            .map_err(|error| (error.code.as_deref(), error.to_string()));
                        follow.extend(self.voice.listened(*id, result));
                        if !failed_listen {
                            follow.push(Command::ListSessions);
                            self.apply_after_turn(follow);
                        }
                    }
                    (Purpose::LiveMute | Purpose::Speak, Ok(_)) => {}
                    (Purpose::SessionMessages, Ok(read)) => {
                        if let (Some(id), Some(messages)) = (read["session_id"].as_str(), read["messages"].as_array())
                            && !has_compaction_truncation(messages)
                        {
                            self.read_sessions.push((id.to_owned(), messages.clone()));
                        }
                    }
                    (Purpose::SessionMessages, Err(_)) => {}
                    // Ending voice mode after the engine's session is gone is not a problem.
                    (Purpose::LiveStop, _) => self.voice.stopped(),
                    (Purpose::State, Ok(state)) => {
                        if state["session_id"].as_str().is_some_and(|id| Some(id) != self.conversation.session_id()) {
                            return;
                        }
                        if let Some(current) = CurrentModel::from_wire(&state["current_model"]) {
                            self.current_model = Some(current);
                        }
                        if let Some(mode) = state["approval"].as_str() {
                            self.approval = Some(mode.to_owned());
                        }
                        if self.history_page.is_some()
                            && !self.history_page_from_truncation
                            && state["live"].is_object()
                        {
                            self.history_page_live = Some(state["live"].clone());
                        }
                    }
                    (Purpose::Models, Ok(page)) => {
                        self.models = page["models"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(ModelOption::from_wire)
                            .collect();
                        self.models_changed = true;
                    }
                    // An engine without image listings answers with its chat list, whose rows have
                    // no `image_api`; that counts as no listing, not as image models.
                    (Purpose::ImageModels, Ok(page)) => {
                        let rows = page["models"].as_array().cloned().unwrap_or_default();
                        self.image_models = Some(
                            rows.into_iter()
                                .filter(|row| row.get("image_api").is_some())
                                .collect(),
                        );
                        self.settings_changed = true;
                    }
                    // Optional: the page keeps its built-in list, so a refusal is not an error.
                    (Purpose::ImageModels, Err(_)) => {
                        self.image_models = Some(Vec::new());
                        self.settings_changed = true;
                    }
                    // A switch can change the model, its reasoning levels and the approval mode.
                    (Purpose::SetModel | Purpose::SetThinking | Purpose::SetApproval, Ok(_)) => {
                        follow.push(Command::State);
                    }
                    // Titles and activity change when a turn settles; a session switch can also
                    // restore that session's model. `dial rpc` replays no history on a switch, so
                    // an opened or forked session's transcript is loaded from `recovery_snapshot`.
                    (Purpose::NewSession | Purpose::Resume, Ok(result)) => {
                        if purpose == Purpose::Resume {
                            follow.push(Command::Recover);
                            // The source was claimed elsewhere after the sidebar last looked.
                            if result["forked_from"]
                                .as_str()
                                .is_some_and(|s| !s.is_empty())
                            {
                                self.toast = Some(
                                    "That session is open in another Dial process, so Dial opened a copy of it".into(),
                                );
                            }
                        }
                        follow.extend(
                            self.delete_after_switch
                                .drain(..)
                                .map(Command::DeleteSession),
                        );
                        if let Some((id, title)) = self.rename_after_switch.take() {
                            if self.conversation.session_id() == Some(id.as_str()) {
                                follow.push(Command::Rename(title));
                            } else {
                                self.toast =
                                    Some("Could not rename: the session did not open".into());
                            }
                        }
                        follow.extend([Command::ListSessions, Command::State]);
                    }
                    // A fork opens the copy, like `resume`.
                    (Purpose::Fork, Ok(_)) => {
                        follow.extend([Command::Recover, Command::ListSessions, Command::State])
                    }
                    (Purpose::Rename | Purpose::Pin, Ok(_)) => follow.push(Command::ListSessions),
                    (Purpose::Export, Ok(result)) => {
                        let path = result["path"].as_str().unwrap_or("the project folder");
                        self.toast = Some(format!("Exported to {path}"));
                    }
                    // The rewound conversation is reloaded whole; the engine reports what it did.
                    (Purpose::Undo, Ok(result)) => {
                        follow.extend([Command::Recover, Command::ListSessions]);
                        self.toast = Some(undo_summary(result));
                    }
                    // Compaction rewrites the history the conversation shows.
                    (Purpose::Compact, Ok(_)) => {
                        follow.extend([Command::Recover, Command::ListSessions]);
                    }
                    // A steer is a note inside the current turn, not a new prompt to undo or retry.
                    (Purpose::Steer, Ok(_)) => {
                        if let Some(text) = self.pending_notes.remove(id) {
                            self.conversation.apply_event(&dial_client::Event {
                                kind: "notice".into(),
                                body: json!({"kind":"steer", "lane":"user", "message":text}),
                            });
                        }
                    }
                    (Purpose::Steer, Err(error))
                        if error.code.as_deref() == Some("not_pending") =>
                    {
                        self.toast = Some(
                            "Dial finished before your note arrived; send it as a new prompt"
                                .into(),
                        );
                    }
                    (Purpose::DeleteSession, Ok(result)) => {
                        if let Some(id) = result["session_id"].as_str() {
                            self.sessions.retain(|s| s.id != id);
                            self.deleted.insert(id.to_owned());
                        }
                        // One refresh after the last of several deletes.
                        if !self.pending.values().any(|p| *p == Purpose::DeleteSession) {
                            follow.push(Command::ListSessions);
                        }
                    }
                    (Purpose::SettingsSchema, Ok(reference)) => {
                        self.settings_schema = reference["settings"].as_array().cloned();
                        self.settings_changed = true;
                    }
                    (Purpose::Settings, Ok(saved)) => {
                        self.settings = Some(
                            saved["settings"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|entry| {
                                    let key = entry["key"].as_str()?;
                                    let setting = SavedSetting {
                                        value: entry["value"].clone(),
                                        redacted: entry["redacted"].as_bool().unwrap_or_default(),
                                        scope: entry["scope"]
                                            .as_str()
                                            .filter(|s| matches!(*s, "project" | "global"))
                                            .map(str::to_owned),
                                    };
                                    Some((key.to_owned(), setting))
                                })
                                .collect(),
                        );
                        self.settings_changed = true;
                    }
                    // Saved settings take effect once applied; the engine refuses that mid-turn.
                    (Purpose::SetSetting | Purpose::ResetSetting, Ok(_)) => {
                        follow.extend(self.saved_settings(false));
                    }
                    // Applying can change the model and the approval mode.
                    (Purpose::ApplySettings, Ok(_)) => {
                        self.applying_settings = self
                            .pending
                            .values()
                            .any(|purpose| *purpose == Purpose::ApplySettings);
                        follow.push(Command::State);
                    }
                    (Purpose::Turn, Ok(_)) => {
                        follow.push(Command::ListSessions);
                        // Keep the session's preview copy current.
                        if self.session_messages_supported && let Some(id) = self.conversation.session_id() {
                            follow.push(Command::SessionMessages(id.to_owned()));
                        }
                        self.apply_after_turn(follow);
                    }

                    (
                        Purpose::Approve
                        | Purpose::Abort
                        | Purpose::QuestionPresented
                        | Purpose::Answer,
                        Ok(_),
                    ) => {}
                    // The conversation already shows a failed turn as an error row.
                    (Purpose::Turn, Err(_)) => {
                        follow.push(Command::ListSessions);
                        self.apply_after_turn(follow);
                    }
                    (Purpose::ApplySettings, Err(error)) if error.code.as_deref() == Some("busy") && self.conversation.turn_running() => {
                        self.applying_settings = self
                            .pending
                            .values()
                            .any(|purpose| *purpose == Purpose::ApplySettings);
                        self.apply_pending = true;
                    }
                    (purpose, Err(error)) => {
                        // A valid selection can stick even if starting its local runtime failed.
                        if purpose == Purpose::SetModel {
                            follow.push(Command::State);
                        }
                        if purpose == Purpose::Recover {
                            self.recovering = false;
                        }
                        if purpose == Purpose::ApplySettings {
                            self.applying_settings = self
                                .pending
                                .values()
                                .any(|purpose| *purpose == Purpose::ApplySettings);
                        }
                        if purpose == Purpose::NewSession {
                            self.delete_after_switch.clear();
                        }
                        if purpose == Purpose::Resume {
                            self.rename_after_switch = None;
                        }
                        let reason = if error.code.as_deref() == Some("busy") {
                            "Dial is still working; stop the turn first".to_owned()
                        } else {
                            error.to_string()
                        };
                        self.toast = Some(format!("{}: {reason}", purpose_label(purpose)));
                    }
                }
            }
            Message::Disconnected { .. } => {
                self.pending.clear();
                self.pending_notes.clear();
                self.recovering = false;
                self.reset_history_pages();
                self.applying_settings = false;
                self.voice.reset();
                self.connection = Some(Connection::Ended);
            }
            Message::Event(event) => match event.kind.as_str() {
                "model_progress" if self.conversation.turn_running()
                    && event.body["session_id"].as_str().is_none_or(|s| Some(s) == self.conversation.session_id())
                    && event.request_id().is_some_and(|id| matches!(self.pending.get(&id), Some(Purpose::Turn | Purpose::Listen))) => {
                    let next: Option<ModelProgress> = serde_json::from_value(event.body["progress"].clone()).ok();
                    if let Some(p) = next {
                        let merged = match self.last_inference {
                            Some(prev) => ModelProgress {
                                phase: p.phase,
                                processed_tokens: p.processed_tokens.or(prev.processed_tokens),
                                prompt_tokens: p.prompt_tokens.or(prev.prompt_tokens),
                                output_tokens: p.output_tokens.or(prev.output_tokens),
                                tokens_per_second: p.tokens_per_second.or(prev.tokens_per_second),
                                cached_tokens: p.cached_tokens.or(prev.cached_tokens),
                                eta_secs: p.eta_secs,
                                estimated: p.estimated,
                            },
                            None => p,
                        };
                        if merged.tokens_per_second.is_some()
                            || merged.cached_tokens.is_some()
                            || merged.output_tokens.is_some()
                        {
                            self.last_inference = Some(merged);
                        }
                    }
                    self.model_progress = next;
                }
                "usage" if self.conversation.turn_running()
                    && event.body["session_id"].as_str().is_none_or(|s| Some(s) == self.conversation.session_id())
                    && event.request_id().is_some_and(|id| matches!(self.pending.get(&id), Some(Purpose::Turn | Purpose::Listen))) => {
                    let prompt_tokens = event.body["prompt_tokens"].as_u64();
                    let output_tokens = event.body["output_tokens"].as_u64();
                    let seconds = event.body["seconds"].as_f64();
                    let fallback_tps = match (output_tokens, seconds) {
                        (Some(out), Some(secs)) if out >= 8 && secs > 0.05 => Some(out as f64 / secs),
                        _ => None,
                    };
                    let prev = self.last_inference;
                    let from_live_progress = self.model_progress.is_some();
                    let next = ModelProgress {
                        phase: prev.map_or(InferencePhase::Generating, |p| p.phase),
                        processed_tokens: prev.and_then(|p| p.processed_tokens),
                        prompt_tokens: prompt_tokens.or_else(|| prev.and_then(|p| p.prompt_tokens)),
                        output_tokens: output_tokens.or_else(|| prev.and_then(|p| p.output_tokens)),
                        tokens_per_second: if from_live_progress {
                            prev.and_then(|p| p.tokens_per_second).or(fallback_tps)
                        } else {
                            fallback_tps
                        },
                        cached_tokens: if from_live_progress {
                            prev.and_then(|p| p.cached_tokens)
                        } else {
                            None
                        },
                        eta_secs: None,
                        estimated: false,
                    };
                    if next.prompt_tokens.is_some() || next.tokens_per_second.is_some() || next.output_tokens.is_some() {
                        self.last_inference = Some(next);
                    }
                }
                "cached_usage" if self.conversation.turn_running()
                    && event.body["session_id"].as_str().is_none_or(|s| Some(s) == self.conversation.session_id())
                    && event.request_id().is_some_and(|id| matches!(self.pending.get(&id), Some(Purpose::Turn | Purpose::Listen))) => {
                    if let Some(read_tokens) = event.body["read_tokens"].as_u64() {
                        let mut next = self.last_inference.unwrap_or(ModelProgress {
                            phase: InferencePhase::Generating,
                            processed_tokens: None,
                            prompt_tokens: None,
                            output_tokens: None,
                            tokens_per_second: None,
                            cached_tokens: None,
                            eta_secs: None,
                            estimated: false,
                        });
                        next.cached_tokens = Some(read_tokens);
                        self.last_inference = Some(next);
                    }
                }
                "state_node" if event.body["node"]["kind"] == "inference"
                    && event.body["node"]["parent"].is_null()
                    && event.body["node"]["result"]["phase"] == "completed" => self.model_progress = None,
                "tool_call" | "retry" => self.model_progress = None,
                "context_usage" => {
                    self.context = event.body["tokens"].as_u64().map(|tokens| ContextUsage {
                        tokens,
                        estimated: event.body["estimated"].as_bool().unwrap_or_default(),
                    });
                }
                // Another session's size is unknown until the engine reports it.
                "active_session" => { self.context = None; self.current_model = None; self.model_progress = None; self.last_inference = None; self.reset_history_pages(); }
                "live_transcript" => {
                    let text = event.body["text"].as_str().unwrap_or_default();
                    if let Some((id, repeated)) = self.voice.transcript(text) {
                        self.conversation.push_turn(text, 0, id);
                        if repeated {
                            follow.push(Command::Abort);
                        }
                    }
                }
                "live_level" => {
                    let level = event.body["level"].as_f64().unwrap_or_default();
                    let speech = event.body["speech"].as_bool().unwrap_or_default();
                    self.voice.heard_level(level, speech);
                }
                "live_speech" => {
                    let speaking = event.body["speaking"].as_bool().unwrap_or_default();
                    let running = self.conversation.turn_running();
                    follow.extend(self.voice.speech(speaking, running));
                }
                // A read-out failed, in voice mode or one asked for with "Read aloud".
                "live_error" => {
                    let message = event.body["message"].as_str().unwrap_or("speech failed");
                    self.toast = Some(format!("Voice: {message}"));
                }
                _ => {}
            },
            Message::ProtocolError(_) => {}
        }
    }

    /// Refresh saved values immediately; rebuild this engine only between turns.
    pub fn saved_settings(&mut self, turn_queued: bool) -> Vec<Command> {
        if turn_queued || self.conversation.turn_running() {
            self.apply_pending = true;
            vec![Command::GetSettings]
        } else {
            if matches!(self.connection(), Connection::Live { .. }) {
                self.applying_settings = true;
            }
            vec![Command::GetSettings, Command::ApplySettings]
        }
    }

    fn reset_history_pages(&mut self) {
        self.history_page = None;
        self.history_pages.clear();
        self.history_page_session = None;
        self.history_page_from_truncation = false;
        self.history_page_live = None;
    }

    fn apply_after_turn(&mut self, follow: &mut Vec<Command>) {
        if std::mem::take(&mut self.apply_pending) {
            self.applying_settings = true;
            follow.push(Command::ApplySettings);
        }
    }
}

fn replay_images(message: &mut Value) {
    if message["role"] == "tool" && let Some(images) = message["dial_images"].as_array() {
        let refs: Vec<_> = images.iter().filter_map(|r| r["uri"].as_str()).map(|uri| json!({"uri":uri})).collect();
        if !refs.is_empty() && (message["data"].is_null() || message["data"].is_object()) {
            message["data"]["dial_images"] = Value::Array(refs);
        }
    }
}

/// What `undo` did, in a sentence: how many files it put back, and any trouble.
fn undo_summary(result: &Value) -> String {
    let count = |key: &str| match &result["files"][key] {
        Value::Array(items) => items.len() as u64,
        Value::Number(n) => n.as_u64().unwrap_or_default(),
        _ => 0,
    };
    let files = count("restored") + count("removed");
    let mut summary = match files {
        0 => "Undid the last turn".to_owned(),
        1 => "Undid the last turn and restored 1 file".to_owned(),
        n => format!("Undid the last turn and restored {n} files"),
    };
    let trouble = ["rollback_error", "rewind_error"]
        .iter()
        .find_map(|key| result[*key].as_str().filter(|s| !s.is_empty()));
    if let Some(error) = trouble {
        summary.push_str(&format!(" ({error})"));
    } else if count("unrestorable") > 0 {
        summary.push_str(&format!(
            "; {} could not be restored",
            count("unrestorable")
        ));
    }
    summary
}

fn valid_scope(scope: &str) -> Result<String, String> {
    match scope {
        "global" | "project" => Ok(scope.to_owned()),
        _ => Err(format!("Unknown settings scope `{scope}`")),
    }
}

fn describe(command: &Command) -> &'static str {
    match command {
        Command::Send { .. } => "send the prompt",
        Command::Approve { .. } => "answer the approval",
        Command::Abort => "stop the turn",
        Command::ListSessions | Command::ListSessionsPage { .. } => "load sessions",
        Command::NewSession => "start a session",
        Command::Resume(_) => "open the session",
        Command::SessionMessages(_) => "read the session",
        Command::DeleteSession(_) => "delete the session",
        Command::SettingsSchema => "load the settings reference",
        Command::GetSettings => "load settings",
        Command::SetSetting { .. } => "save the setting",
        Command::ResetSetting { .. } => "reset the setting",
        Command::ApplySettings => "apply settings",
        Command::Recover => "reload the session",
        Command::MessagesPage { .. } => "load conversation history",
        Command::State => "read the session settings",
        Command::DiscoverModels => "refresh models",
        Command::ListImageModels => "load image models",
        Command::SetModel { .. } => "switch the model",
        Command::SetThinking { .. } => "set the reasoning level",
        Command::SetApproval(_) => "set the approval mode",
        Command::QuestionPresented(_) => "show the question",
        Command::AnswerQuestion { .. } => "answer the question",
        Command::Rename(_) => "rename the session",
        Command::PinSession { .. } => "pin the session",
        Command::ForkSession(_) => "duplicate the session",
        Command::Export(_) => "export the session",
        Command::Undo => "undo the last turn",
        Command::Steer { .. } => "add to the running turn",
        Command::Retry => "retry the prompt",
        Command::Compact => "compact the conversation",
        Command::LiveStart { .. } => "start voice mode",
        Command::LiveListen => "listen",
        Command::LiveMute(_) => "mute voice replies",
        Command::LiveStop => "end voice mode",
        Command::LiveSpeak(_) => "read aloud",
    }
}

fn purpose_label(purpose: Purpose) -> &'static str {
    match purpose {
        Purpose::Turn => "Turn failed",
        Purpose::Approve => "Approval not applied",
        Purpose::Abort => "Could not stop the turn",
        Purpose::Sessions => "Could not load sessions",
        Purpose::NewSession => "Could not start a session",
        Purpose::Resume => "Could not open the session",
        Purpose::SessionMessages => "Could not read the session",
        Purpose::DeleteSession => "Could not delete the session",
        Purpose::SettingsSchema => "Could not load the settings reference",
        Purpose::Settings => "Could not load settings",
        Purpose::SetSetting => "Could not save the setting",
        Purpose::ResetSetting => "Could not reset the setting",
        Purpose::ApplySettings => "Could not apply settings",
        Purpose::Recover => "Could not reload the session",
        Purpose::MessagesPage => "Could not load conversation history",
        Purpose::State => "Could not read the session settings",
        Purpose::Models => "Could not load models",
        Purpose::ImageModels => "Could not load image models",
        Purpose::SetModel => "Could not switch the model",
        Purpose::SetThinking => "Could not set the reasoning level",
        Purpose::SetApproval => "Could not set the approval mode",
        Purpose::QuestionPresented => "Could not show the question",
        Purpose::Answer => "Answer not accepted",
        Purpose::Rename => "Could not rename the session",
        Purpose::Pin => "Could not pin the session",
        Purpose::Fork => "Could not duplicate the session",
        Purpose::Export => "Could not export the session",
        Purpose::Undo => "Could not undo the last turn",
        Purpose::Steer => "Could not add to the running turn",
        Purpose::Compact => "Could not compact the conversation",
        Purpose::LiveStart => "Could not start voice mode",
        Purpose::Listen => "Could not listen",
        Purpose::LiveMute => "Could not mute voice replies",
        Purpose::LiveStop => "Could not end voice mode",
        Purpose::Speak => "Could not read aloud",
    }
}
