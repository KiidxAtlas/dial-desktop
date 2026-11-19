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
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Weak};

#[cfg(test)]
mod tests;

/// One engine and what it showed.
pub struct Tab {
    pub model: Model,
    /// `None` in the design preview.
    pub engine: Option<Engine>,
    /// The user was told this background tab is waiting for them.
    alerted: bool,
    opening: Option<(String, Option<RequestId>)>,
    restore_target: Option<String>,
    turn_queued: bool,
    // Stand-in lifetime for tabs without a worker (preview and routing tests).
    lifetime: Arc<AtomicBool>,
}

impl Tab {
    pub fn new(model: Model, engine: Option<Engine>) -> Self {
        Self {
            model,
            engine,
            alerted: false,
            opening: None,
            restore_target: None,
            turn_queued: false,
            lifetime: Arc::new(AtomicBool::new(false)),
        }
    }

    fn session(&self) -> Option<&str> {
        self.model.conversation.session_id()
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
        self.engine
            .as_ref()
            .ok_or("Dial is not connected")?
            .enqueue(command)?;
        if let Some(id) = opening {
            self.opening = Some((id, None));
        }
        self.restore_target = None;
        self.turn_queued |= starts_turn;
        Ok(())
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
        match &update {
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
                self.opening = None;
                if result.is_ok() {
                    self.restore_target = None;
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
                self.turn_queued = false;
            }
            _ => {}
        }
        self.model.apply(update)
    }

    fn owns(&self, id: &str) -> bool {
        self.session() == Some(id)
            || self
                .opening
                .as_ref()
                .is_some_and(|(wanted, _)| wanted == id)
    }

    pub fn transitioning(&self) -> bool {
        self.opening.is_some() || self.turn_queued
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
}

/// What a session is doing in some tab, for the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    pub title: String,
    pub running: bool,
    /// An approval or a question waits for the user.
    pub waiting: bool,
}

/// The open tabs; one is on screen.
pub struct Tabs {
    tabs: Vec<Tab>,
    active: usize,
    closing: HashMap<String, Weak<AtomicBool>>,
}

impl Tabs {
    pub fn new(tab: Tab) -> Self {
        Self {
            tabs: vec![tab],
            active: 0,
            closing: HashMap::new(),
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
            None => self.route_new(),
        }
    }

    /// Where starting a fresh session goes.
    pub fn route_new(&self) -> Route {
        if self.active().busy() || self.active().opening.is_some() {
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

    /// Show tab `index`.
    pub fn switch(&mut self, index: usize) {
        if index < self.tabs.len() {
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
