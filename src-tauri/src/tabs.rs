//! Open engines: one `dial rpc` process per session that is running.
//!
//! An engine serves one session at a time and refuses to switch while a turn runs, and a session
//! is written by a single process. So when the user opens another session while the one on screen
//! is working, that engine keeps going in the background and a second one opens the other
//! session. Clicking a running session goes back to the engine that owns it; resuming it
//! elsewhere would fork it. A background engine closes once its turn is over.

use crate::engine::{Command, Engine, Update};
use crate::model::Model;
use dial_client::{Message, RequestId};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalLoadFailure {
    pub session_id: String,
    pub provider: String,
    pub model_id: String,
    pub error: String,
}

#[cfg(test)]
mod tests;

/// One engine and what it showed.
pub struct Tab {
    pub model: Model,
    /// `None` in the design preview.
    pub engine: Option<Engine>,
    /// The user was told this background tab is waiting for them.
    alerted: bool,
    notified: HashSet<(NoticeKind, String)>,
    turn_ended: bool,
    pub(crate) opening: Option<(String, Option<RequestId>)>,
    /// A fresh session has no destination id until the engine opens it.
    creating: bool,
    // A fresh engine has no journal id until Ready. Never bind its input to a later switch.
    startup_key: String,
    startup_session: Option<String>,
    restore_target: Option<String>,
    turn_queued: bool,
    /// Edits belong to this tab's journal, including while its turn runs in the background.
    edits: Vec<Command>,
    editing: Option<Command>,
    local_load_failure: Option<LocalLoadFailure>,
    /// The engine refused to open this session because it was still busy (with work Desktop
    /// did not see as a turn); the app opens it in a second engine instead of failing.
    busy_refusal: Option<String>,
    // Stand-in lifetime for tabs without a worker (preview and routing tests).
    lifetime: Arc<AtomicBool>,
}

impl Tab {
    pub fn new(model: Model, engine: Option<Engine>) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            model,
            engine,
            alerted: false,
            notified: HashSet::new(),
            turn_ended: false,
            opening: None,
            creating: false,
            startup_key: format!("startup:{}", NEXT.fetch_add(1, Ordering::Relaxed)),
            startup_session: None,
            restore_target: None,
            turn_queued: false,
            edits: Vec::new(),
            editing: None,
            local_load_failure: None,
            busy_refusal: None,
            lifetime: Arc::new(AtomicBool::new(false)),
        }
    }

    fn session(&self) -> Option<&str> {
        self.model.conversation.session_id()
    }

    pub fn startup_draft_key(&self) -> Option<&str> {
        (!self.startup_key.is_empty() && (self.session().is_none() || self.opening.is_some()
            || self.session() == self.startup_session.as_deref())).then_some(&self.startup_key)
    }

    pub fn inherit_unbound_draft(&mut self, previous: &Tab) {
        if previous.session().is_none() && previous.startup_key.starts_with("startup:") {
            self.startup_key.clone_from(&previous.startup_key);
        }
    }

    pub fn reconnect_target(&self) -> Option<&str> {
        self.restore_target.as_deref().or_else(|| self.session())
    }

    pub fn restore_on_connect(&mut self, id: String) -> Result<(), String> {
        self.enqueue(Command::Resume(id.clone()))?;
        self.restore_target = Some(id);
        Ok(())
    }

    pub fn protects_work(&self) -> bool {
        self.busy() || self.waiting() || self.transitioning()
    }

    pub fn enqueue(&mut self, command: Command) -> Result<(), String> {
        let opening = match &command {
            Command::Resume(id) => Some(id.clone()),
            _ => None,
        };
        let starts_turn = matches!(command, Command::Send { .. } | Command::Retry);
        let creates = command == Command::NewSession;
        self.engine
            .as_ref()
            .ok_or("Dial is not connected")?
            .enqueue(command)?;
        if creates || opening.is_some() {
            self.startup_key.clear();
            if let Some(id) = &opening && self.session().is_none() {
                self.startup_key.clone_from(id);
                self.startup_session = Some(id.clone());
            }
        }
        if let Some(id) = opening {
            self.opening = Some((id, None));
        }
        self.restore_target = None;
        self.turn_queued |= starts_turn;
        self.creating |= creates;
        Ok(())
    }

    pub fn saved_settings(&mut self) -> Vec<Command> {
        self.model.saved_settings(self.turn_queued)
    }

    pub fn select_model_setting(&mut self, command: Command) -> Result<Option<Command>, String> {
        if self.opening_session() || self.model.recovering {
            return Err("Wait for the conversation to open".into());
        }
        // A different model resets effort. Repeated effort picks replace the previous one.
        match &command {
            Command::SetModel { .. } => {
                self.edits.clear();
                self.local_load_failure = None;
            }
            Command::SetThinking { .. } => self
                .edits
                .retain(|c| !matches!(c, Command::SetThinking { .. })),
            _ => return Err("Not a model setting".into()),
        }
        self.edits.push(command);
        Ok(self.next_edit())
    }

    fn next_edit(&mut self) -> Option<Command> {
        if self.busy() || self.editing.is_some() || self.edits.is_empty() {
            return None;
        }
        let next = self.edits.remove(0);
        if matches!(next, Command::SetModel { .. }) {
            self.local_load_failure = None;
        }
        self.editing = Some(next.clone());
        Some(next)
    }

    fn settle_edit(&mut self, method: &str, error: Option<&str>) {
        if self
            .editing
            .as_ref()
            .is_some_and(|c| c.request().1 == method)
        {
            if let Some(Command::SetModel { session_id, provider, model_id }) = &self.editing
                && self.session() == Some(session_id.as_str())
                && crate::engine::managed_local_provider(provider)
            {
                if let Some(error) = error {
                    self.local_load_failure = Some(LocalLoadFailure {
                        session_id: session_id.clone(), provider: provider.clone(),
                        model_id: model_id.clone(), error: error.to_owned(),
                    });
                } else if !self.model.prepare_local_model_supported {
                    self.model.toast = Some("Model selected. This engine loads it on the first prompt. Update Dial engine to load it on selection.".into());
                }
            }
            self.editing = None;
            if error.is_some()
                && method == "set_model"
                && matches!(self.edits.first(), Some(Command::SetThinking { .. }))
            {
                self.edits.clear(); // Don't apply the new model's effort to the old model.
            }
        }
    }

    pub fn submit_prompt(&mut self, command: Command) -> Result<crate::engine::RpcReply, String> {
        let reply = self
            .engine
            .as_ref()
            .ok_or("Dial is not connected")?
            .submit(command)?;
        self.turn_queued = true;
        Ok(reply)
    }

    pub fn apply(&mut self, update: Update) -> Vec<Command> {
        if let Update::Connected(ready) = &update && self.startup_session.is_none() {
            self.startup_session = Some(ready.session_id.clone());
        }
        match &update {
            Update::Message(Message::Response { method, result, .. }) => {
                self.settle_edit(method, result.as_ref().err().map(|err| err.message.as_str()))
            }
            Update::RequestFailed { command, error } => self.settle_edit(command.request().1, Some(error)),
            Update::ConnectFailed(_) | Update::Message(Message::Disconnected { .. }) => {
                self.edits.clear();
                self.editing = None;
                self.local_load_failure = None;
            }
            _ => {}
        }
        match &update {
            Update::Requested { command: Command::NewSession, .. } => self.creating = true,
            Update::RequestFailed { command: Command::NewSession, .. } => self.creating = false,
            Update::Message(Message::Response { method, .. }) if method == "new_session" => {
                self.creating = false;
            }
            Update::Requested {
                command: Command::Resume(target),
                id,
            } => {
                if let Some((_, request)) =
                    self.opening.as_mut().filter(|(wanted, _)| wanted == target)
                {
                    *request = Some(*id);
                }
            }
            Update::Message(Message::Response { id, result, .. })
                if self
                    .opening
                    .as_ref()
                    .is_some_and(|(_, request)| *request == Some(*id)) =>
            {
                let (target, _) = self.opening.take().expect("checked above");
                match result {
                    Ok(_) => self.restore_target = None,
                    Err(error) if error.code.as_deref() == Some("busy") => {
                        self.busy_refusal = Some(target);
                    }
                    Err(_) => {}
                }
            }
            Update::RequestFailed {
                command: Command::Resume(target),
                ..
            } if self
                .opening
                .as_ref()
                .is_some_and(|(wanted, _)| wanted == target) =>
            {
                self.opening = None;
            }
            Update::Requested {
                command: Command::Send { .. } | Command::Retry,
                ..
            }
            | Update::RequestFailed {
                command: Command::Send { .. } | Command::Retry,
                ..
            } => {
                self.turn_queued = false;
            }
            Update::ConnectFailed(_) | Update::Message(Message::Disconnected { .. }) => {
                self.opening = None;
                self.creating = false;
                self.turn_queued = false;
            }
            _ => {}
        }
        let was_running = self.model.conversation.turn_running();
        let mut follow = self.model.apply(update);
        if self.busy_refusal.is_some() {
            // The app retries in another engine; the refusal is not the user's error.
            self.model.toast = None;
        }
        follow.extend(self.next_edit());
        if self.local_load_failure.as_ref().is_some_and(|failure| self.session() != Some(failure.session_id.as_str())) {
            self.local_load_failure = None;
        }
        self.turn_ended |= was_running && !self.model.conversation.turn_running();
        follow
    }

    fn owns(&self, id: &str) -> bool {
        self.session() == Some(id)
            || self
                .opening
                .as_ref()
                .is_some_and(|(wanted, _)| wanted == id)
    }

    pub fn opening_session(&self) -> bool {
        self.creating || self.opening.is_some()
    }

    /// The session this engine refused to open while busy, once.
    pub fn take_busy_refusal(&mut self) -> Option<String> {
        self.busy_refusal.take()
    }

    /// The session this tab is opening, if any.
    pub fn opening_id(&self) -> Option<&str> {
        self.opening.as_ref().map(|(id, _)| id.as_str())
    }

    pub fn local_load_failure(&self) -> Option<&LocalLoadFailure> {
        self.local_load_failure.as_ref()
    }

    /// The model being switched now (not one deferred until the current turn ends).
    pub fn loading_local_model(&self) -> Option<&str> {
        let Command::SetModel { provider, model_id, .. } = self.editing.as_ref()? else { return None };
        if !self.model.prepare_local_model_supported || !crate::engine::managed_local_provider(provider) { return None; }
        Some(self.model.models.iter().find(|row| &row.provider == provider && &row.model_id == model_id)
            .map_or(model_id.as_str(), |row| row.label.as_str()))
    }

    pub fn transitioning(&self) -> bool {
        self.opening_session()
            || self.turn_queued
            || self.editing.is_some()
            || !self.edits.is_empty()
    }

    fn busy(&self) -> bool {
        self.turn_queued || self.model.conversation.turn_running()
    }

    fn waiting(&self) -> bool {
        !self.model.conversation.pending_approvals().is_empty()
            || self.model.conversation.question().is_some()
    }
}

/// Where a request to open a session goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// The tab on screen can take it.
    Here,
    /// Tab `n` already has that session open: show it.
    Switch(usize),
    /// The tab on screen is busy: open it in a new engine.
    Spawn,
    /// The old worker has not finished releasing the journal yet.
    Closing,
    /// Another Dial process holds the session; resuming it would fork a copy.
    Elsewhere,
}

/// What a session is doing in some tab, for the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    pub title: String,
    pub running: bool,
    /// An approval or a question waits for the user.
    pub waiting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoticeKind {
    Approval,
    Question,
    Finished,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SessionNotice {
    pub session_id: String,
    pub kind: NoticeKind,
}
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum RestartDecision {
    Busy,
    ConfirmTerminals { ids: Vec<u32> },
    Reserved,
}

/// The open tabs; one is on screen.
pub struct Tabs {
    tabs: Vec<Tab>,
    active: usize,
    closing: HashMap<String, Weak<AtomicBool>>,
    restart_reserved: bool,
}

impl Tabs {
    pub fn new(tab: Tab) -> Self {
        Self {
            tabs: vec![tab],
            active: 0,
            closing: HashMap::new(),
            restart_reserved: false,
        }
    }

    pub fn active(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub fn active_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    pub fn protects_work(&self) -> bool {
        self.tabs.iter().any(Tab::protects_work)
    }

    /// Number of sessions that closing this window would interrupt, including background tabs.
    pub(crate) fn active_session_count(&self) -> usize {
        self.tabs.iter().filter(|tab| tab.protects_work() || tab.model.voice.prevents_restart()).count()
    }

    /// Called with the same Inner lock as turn submissions, voice starts and terminal opens.
    pub fn reserve_restart(
        &mut self,
        terminals: Vec<u32>,
        confirmed: &[u32],
    ) -> Result<RestartDecision, String> {
        self.allow_work()?;
        if self.active_session_count() != 0 {
            return Ok(RestartDecision::Busy);
        }
        if terminals.iter().any(|id| !confirmed.contains(id)) {
            return Ok(RestartDecision::ConfirmTerminals { ids: terminals });
        }
        self.restart_reserved = true;
        Ok(RestartDecision::Reserved)
    }

    pub fn allow_work(&self) -> Result<(), String> {
        if self.restart_reserved {
            Err("Dial Desktop is installing an update and restarting. Try again after restart.".into())
        } else {
            Ok(())
        }
    }

    pub fn release_restart(&mut self) {
        self.restart_reserved = false;
    }

    pub fn check_model_removal(&self, model: &str) -> Result<(), String> {
        if self.protects_work() {
            return Err("Wait for running sessions before removing a model.".into());
        }
        if self.tabs.iter().any(|tab| {
            tab.model
                .current_model
                .as_ref()
                .is_some_and(|current| current.model_id == model)
                || tab
                    .model
                    .models
                    .iter()
                    .any(|row| row.model_id == model && row.configured)
        }) {
            return Err("Switch away from this model and remove it from Dial’s configuration before deleting it.".into());
        }
        Ok(())
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Tab> {
        self.tabs.iter_mut()
    }

    /// Every tab, for shutdown.
    pub fn into_tabs(self) -> Vec<Tab> {
        self.tabs
    }

    /// Where opening session `id` goes.
    pub fn route_resume(&self, id: &str) -> Route {
        match self.tabs.iter().position(|t| t.owns(id)) {
            Some(index) => Route::Switch(index),
            None if self
                .closing
                .get(id)
                .is_some_and(|owner| owner.strong_count() > 0) =>
            {
                Route::Closing
            }
            None if self
                .active()
                .model
                .sessions
                .iter()
                .any(|s| s.id == id && s.open_elsewhere) =>
            {
                Route::Elsewhere
            }
            None => self.route_new(),
        }
    }

    /// Where starting a fresh session goes.
    pub fn route_new(&self) -> Route {
        let tab = self.active();
        if tab.busy() || tab.transitioning()
            || matches!(tab.model.connection(), crate::model::Connection::Ended | crate::model::Connection::Failed(_))
        {
            Route::Spawn
        } else {
            Route::Here
        }
    }

    /// Why sessions `ids` cannot be deleted from here: another tab is running one of them.
    pub fn delete_refusal(&self, ids: &[String]) -> Option<String> {
        self.tabs
            .iter()
            .enumerate()
            .find(|(index, tab)| *index != self.active && ids.iter().any(|id| tab.owns(id)))
            .map(|(_, tab)| {
                format!(
                    "“{}” is still running; open it and stop it before deleting",
                    tab.model.title()
                )
            })
    }

    /// Each engine caches its own sidebar and may still be receiving an older page.
    pub fn sync_deletions(&mut self) {
        if self.tabs.len() < 2 {
            return;
        }
        let deleted: HashSet<_> = self
            .tabs
            .iter()
            .flat_map(|t| t.model.deleted.iter().cloned())
            .collect();
        if deleted.is_empty() {
            return;
        }
        for tab in &mut self.tabs {
            tab.model.sessions.retain(|s| !deleted.contains(&s.id));
            tab.model.deleted.extend(deleted.iter().cloned());
        }
    }

    /// Show tab `index`.
    pub fn switch(&mut self, index: usize) {
        if index < self.tabs.len() {
            if index != self.active {
                // Switching engines isn't a history refresh. Keep the project list on screen
                // until the newly visible engine returns a current list_sessions response.
                self.tabs[index].model.sessions = self.active().model.sessions.clone();
            }
            self.active = index;
            self.tabs[index].alerted = false;
            // The picker lists the models of the engine on screen.
            self.tabs[index].model.models_changed = true;
        }
    }

    /// Add `tab` and show it. It starts with the sidebar the user was looking at.
    pub fn open(&mut self, mut tab: Tab) {
        if tab.model.sessions.is_empty() {
            tab.model.sessions = self.active().model.sessions.clone();
        }
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
    }

    /// Add `tab` behind the one on screen: it works in the background and closes when done.
    pub fn add_background(&mut self, mut tab: Tab) {
        if tab.model.sessions.is_empty() {
            tab.model.sessions = self.active().model.sessions.clone();
        }
        self.tabs.push(tab);
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut Tab> {
        self.tabs.get_mut(index)
    }

    /// Close background tabs with nothing left to do and return them for shutdown.
    pub fn close_idle(&mut self) -> Vec<Tab> {
        self.closing.retain(|_, owner| owner.strong_count() > 0);
        let mut closed = Vec::new();
        let mut index = 0;
        while index < self.tabs.len() {
            if index != self.active
                && !self.tabs[index].busy()
                && !self.tabs[index].transitioning()
                && !self.tabs[index].waiting()
            {
                let tab = self.tabs.remove(index);
                if let Some(id) = tab.session() {
                    let lifetime = tab
                        .engine
                        .as_ref()
                        .map_or_else(|| Arc::downgrade(&tab.lifetime), Engine::lifetime);
                    self.closing.insert(id.to_owned(), lifetime);
                }
                closed.push(tab);
                if index < self.active {
                    self.active -= 1;
                }
            } else {
                index += 1;
            }
        }
        closed
    }

    /// Messages for things that happened in background tabs since `before`, which holds each
    /// tab's busy state from before the latest updates.
    pub fn background_news(&mut self, before: &[bool]) -> Vec<String> {
        let mut news = Vec::new();
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            if index == self.active {
                continue;
            }
            let title = tab.model.title();
            if before.get(index) == Some(&true) && !tab.busy() {
                news.push(format!("“{title}” finished"));
            }
            if tab.waiting() && !tab.alerted {
                tab.alerted = true;
                news.push(format!("“{title}” is waiting for you"));
            } else if !tab.waiting() {
                tab.alerted = false;
            }
        }
        news
    }

    /// Each tab's busy state, to pass to [`Tabs::background_news`] after the next updates.
    pub fn busy_states(&self) -> Vec<bool> {
        self.tabs.iter().map(Tab::busy).collect()
    }

    /// Native alerts are emitted only for new requests and live-to-idle turns, not on every poll.
    pub fn session_notices(&mut self) -> Vec<SessionNotice> {
        let mut notices = Vec::new();
        for tab in &mut self.tabs {
            let turn_ended = std::mem::take(&mut tab.turn_ended);
            let Some(session_id) = tab.session().map(str::to_owned) else {
                continue;
            };
            if !matches!(
                tab.model.connection,
                Some(crate::model::Connection::Live { .. })
            ) {
                tab.notified.clear();
                continue;
            }
            let conversation = &tab.model.conversation;
            let mut pending = HashSet::new();
            for approval in conversation.pending_approvals() {
                pending.insert((NoticeKind::Approval, approval.request.approval_id.clone()));
            }
            if let Some(id) = conversation
                .question()
                .and_then(|question| question["question_id"].as_str())
            {
                pending.insert((NoticeKind::Question, id.to_owned()));
            }
            for kind in [NoticeKind::Approval, NoticeKind::Question] {
                if pending
                    .difference(&tab.notified)
                    .any(|(new, _)| *new == kind)
                {
                    notices.push(SessionNotice {
                        session_id: session_id.clone(),
                        kind,
                    });
                }
            }
            tab.notified = pending;
            if turn_ended && !tab.busy() && !tab.waiting() {
                notices.push(SessionNotice {
                    session_id,
                    kind: NoticeKind::Finished,
                });
            }
        }
        notices
    }

    /// What each open session is doing, by session id.
    pub fn activity(&self) -> HashMap<String, Activity> {
        self.tabs
            .iter()
            .filter_map(|tab| {
                let id = tab.session()?;
                Some((
                    id.to_owned(),
                    Activity {
                        title: tab.model.title(),
                        running: tab.busy(),
                        waiting: tab.waiting(),
                    },
                ))
            })
            .collect()
    }
}
