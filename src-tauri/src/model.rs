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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
    pub(crate) recovering: bool,
    /// The open question already acknowledged as presented; the engine refuses a second ack.
    presented: Option<String>,
    /// Sessions to delete once the engine has moved off them (it refuses the active one).
    delete_after_switch: Vec<String>,
    /// A rename waiting for the engine to open its session (it only renames the active one).
    rename_after_switch: Option<(String, String)>,
    /// Saved settings wait for the running turn to end before they are applied.
    apply_pending: bool,
    /// The conversation's size in tokens as the engine last reported it (`context_usage`).
    pub context: Option<ContextUsage>,
    /// Hands-free voice mode.
    pub voice: Voice,
}

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
        matches!(self.connection(), Connection::Live { .. }) && !self.conversation.turn_running()
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

    /// The request that adds `text` to the running turn; refused when no turn runs.
    pub fn steer(&self, text: &str) -> Result<Command, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("Type the guidance to add".into());
        }
        if !self.conversation.turn_running() {
            return Err("Dial is not working on a turn; send this as a new prompt".into());
        }
        Ok(Command::Steer(text.to_owned()))
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

    /// The context window of the model in force, when the model list knows it.
    pub fn context_window(&self) -> Option<u64> {
        let current = self.current_model.as_ref()?;
        self.models
            .iter()
            .find(|m| m.provider == current.provider && m.model_id == current.model_id)
            .and_then(|m| m.context_window)
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
                follow.extend([Command::ListSessions, Command::State]);
                self.discovering = false;
                follow.extend(self.discover_if_due(Instant::now(), Duration::ZERO));
            }
            Update::ConnectFailed(error) => self.connection = Some(Connection::Failed(error)),
            Update::Requested { command, id } => {
                let (purpose, _, _) = command.request();
                match &command {
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
                if matches!(command, Command::Send { .. } | Command::Retry) && !self.conversation.turn_running() {
                    self.apply_after_turn(&mut follow);
                }
                self.toast = Some(format!("Could not {}: {error}", describe(&command)));
            }
            Update::Message(mut message) => {
                // The wire client keeps structured tool data but not top-level journal attachment
                // fields. Project retained references into the same channel as live results.
                match &mut message {
                    Message::Event(event) if event.kind == "replay_message" => {
                        replay_images(&mut event.body["message"]);
                        crate::legacy_text::project_user_message(&mut event.body["message"]);
                    }
                    Message::Response { result: Ok(body), .. } if body.get("messages").is_some() => {
                        if let Some(messages) = body["messages"].as_array_mut() {
                            for message in messages {
                                replay_images(message);
                                crate::legacy_text::project_user_message(message);
                            }
                        }
                    }
                    _ => {}
                }
                self.message(&message, &mut follow);
            }
        }
        // A frame may be published before the worker acknowledges the follow-up request.
        self.recovering |= follow.contains(&Command::Recover);
        if self.conversation.needs_reload()
            && !self.recovering
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
        self.pending.clear();
        self.recovering = false;
        self.ready_image_input = ready.raw["image_input"].as_bool();
        self.type_context_supported = ready.raw["features"]["type_context"].as_bool() == Some(true);
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
        if !failed_listen {
            self.conversation.apply(message);
        }
        match message {
            Message::Response { id, result, .. } => {
                let Some(purpose) = self.pending.remove(id) else {
                    return;
                };
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
                                    self.sessions = std::mem::take(&mut self.session_pages)
                                        .into_iter()
                                        .filter(|s| !self.deleted.contains(&s.id) && seen.insert(s.id.clone()))
                                        .collect();
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
                    // The engine takes the guidance at the running turn's next step.
                    (Purpose::Steer, Ok(_)) => {}
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
                    (Purpose::ApplySettings, Ok(_)) => follow.push(Command::State),
                    (Purpose::Turn, Ok(_)) => {
                        follow.push(Command::ListSessions);
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
                        self.apply_pending = true;
                    }
                    (purpose, Err(error)) => {
                        if purpose == Purpose::Recover {
                            self.recovering = false;
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
                self.recovering = false;
                self.voice.reset();
                self.connection = Some(Connection::Ended);
            }
            Message::Event(event) => match event.kind.as_str() {
                "context_usage" => {
                    self.context = event.body["tokens"].as_u64().map(|tokens| ContextUsage {
                        tokens,
                        estimated: event.body["estimated"].as_bool().unwrap_or_default(),
                    });
                }
                // Another session's size is unknown until the engine reports it.
                "active_session" => { self.context = None; self.current_model = None; }
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
            vec![Command::GetSettings, Command::ApplySettings]
        }
    }

    fn apply_after_turn(&mut self, follow: &mut Vec<Command>) {
        if std::mem::take(&mut self.apply_pending) {
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
        Command::DeleteSession(_) => "delete the session",
        Command::SettingsSchema => "load the settings reference",
        Command::GetSettings => "load settings",
        Command::SetSetting { .. } => "save the setting",
        Command::ResetSetting { .. } => "reset the setting",
        Command::ApplySettings => "apply settings",
        Command::Recover => "reload the session",
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
        Command::Steer(_) => "add to the running turn",
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
        Purpose::DeleteSession => "Could not delete the session",
        Purpose::SettingsSchema => "Could not load the settings reference",
        Purpose::Settings => "Could not load settings",
        Purpose::SetSetting => "Could not save the setting",
        Purpose::ResetSetting => "Could not reset the setting",
        Purpose::ApplySettings => "Could not apply settings",
        Purpose::Recover => "Could not reload the session",
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
