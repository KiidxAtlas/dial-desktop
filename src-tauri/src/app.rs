//! The Tauri application: wires the engines, the models and the web UI together.
//!
//! One pump thread folds engine updates into each tab's [`Model`] and publishes a fresh
//! [`Snapshot`] of the tab on screen as the `snapshot` event (coalesced to about one per frame
//! while text streams). The switchable model list goes out as the `models` event when it changes,
//! and short-lived messages as `toast` events. The UI sends requests back through the commands
//! below; none of them talks to an engine directly.
//!
//! Engines run in the open workspace, one per running session ([`Tabs`]). Opening another folder,
//! or reconnecting after the engine stopped, replaces them without restarting the app.

use crate::attachments::{self, ImageFile};
use crate::browser::{self, Browser};
use crate::cli::Preview;
use crate::engine::{self, APPROVAL_MODES, Command, Engine};
use crate::files::{self, DiffView, DirEntryView, FileView};
use crate::launcher::{self, Recents};
use crate::model::{Answer, Model, Reply};
use crate::notifications::{self, Notifications};
use crate::preview;
use crate::settings::{self, SettingsView};
use crate::tabs::{Route, Tab, Tabs};
use crate::terminal::{self, Terminals};
use crate::time;
use crate::updates;
use crate::view::{EntryViews, ModelOptionView, Published, Snapshot, SnapshotAt, Workspace};
use dial_client::{Diagnostics, Launch};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder};

/// Updates arriving within one frame are published together.
const FRAME: Duration = Duration::from_millis(16);
/// Republish this often without engine activity, so relative times stay current.
const TICK: Duration = Duration::from_secs(30);
const PREVIEW_NOTICE: &str = "Design preview: nothing is sent to an engine";
const OPEN_ELSEWHERE: &str = "That session is open in another Dial process (a terminal or another window). Finish it there, or duplicate it to work on a copy.";

/// What the app connects to.
pub enum Source {
    /// A live engine: this binary, run as `dial rpc` plus `args` in the workspace.
    Engine {
        program: PathBuf,
        args: Vec<OsString>,
    },
    /// The built-in design preview.
    Preview(Preview),
}

struct Inner {
    tabs: Tabs,
    /// `None` until a folder is chosen.
    workspace: Option<Workspace>,
    recents: Recents,
    /// The on-screen tab's transcript rows, reused by the next snapshot where unchanged.
    entry_views: EntryViews,
}

/// State shared by the pump thread and the commands.
struct Shared {
    inner: Mutex<Inner>,
    source: Source,
    home: Option<String>,
    /// Wakes the pump; every engine gets a clone.
    wake: Mutex<Sender<()>>,
    /// The Terminal panel's shells, all running in the open workspace.
    terminals: Mutex<Terminals>,
    /// The engine installed from the app after none was found; used instead of the launch
    /// choice from then on.
    installed: Mutex<Option<PathBuf>>,
    /// The snapshot the UI has, so each `snapshot` event carries only what changed.
    published: Mutex<Published>,
    /// The Appearance switches for what answers may draw; each new engine is told (`answer_style`).
    answer_style: Mutex<crate::answer_style::AnswerStyle>,
    answer_support: crate::answer_style::Support,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn workspace(&self, path: &Path) -> Workspace {
        Workspace::new(&path.to_string_lossy(), self.home.as_deref())
    }

    /// The open workspace's folder.
    fn root(&self) -> Result<PathBuf, String> {
        self.lock()
            .workspace
            .as_ref()
            .map(|w| PathBuf::from(&w.root))
            .ok_or_else(|| "Open a folder first".to_owned())
    }

    fn terminals(&self) -> MutexGuard<'_, Terminals> {
        self.terminals
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Run in `path` from now on: a fresh engine there (or the preview), remembered as recent.
    fn open(
        &self,
        path: &Path,
        confirmed_from: Option<&str>,
        runner: &crate::app_runner::AppRunner,
    ) -> Result<Option<String>, String> {
        let workspace = self.workspace(path);
        let mut inner = self.lock();
        let current = inner.workspace.as_ref().map(|w| w.root.as_str());
        if current == Some(workspace.root.as_str()) {
            return Ok(None);
        }
        let mut terminals = self.terminals();
        if let Some(root) = current
            && confirmed_from != Some(root)
            && (inner.tabs.protects_work() || terminals.has_shells())
        {
            return Ok(Some(root.to_owned()));
        }
        if let Some(root) = current {
            runner.revoke_vm_launch(root);
        }
        let tab = match &self.source {
            Source::Preview(kind) => Tab::new(preview::model(*kind, &workspace.root), None),
            Source::Engine { .. } => Tab::new(Model::starting(), self.spawn(path)),
        };
        inner.recents.touch(path);
        let old = std::mem::replace(&mut inner.tabs, Tabs::new(tab));
        inner.workspace = Some(workspace);
        terminals.close_all();
        drop(terminals);
        drop(inner);
        retire(old.into_tabs());
        self.poke();
        Ok(None)
    }

    fn reconnect(&self) -> Result<(), String> {
        let mut inner = self.lock();
        let root = inner
            .workspace
            .as_ref()
            .ok_or("Open a folder first")?
            .root
            .clone();
        let active = inner.tabs.active_mut();
        if active.protects_work() {
            return Err("Wait for the current session to finish before reconnecting".into());
        }
        let target = active.reconnect_target().map(str::to_owned);
        let previous = active.engine.as_ref().map(Engine::lifetime);
        let engine = self
            .spawn_after(Path::new(&root), previous)
            .ok_or(PREVIEW_NOTICE)?;
        let mut tab = Tab::new(Model::starting(), Some(engine));
        tab.model.sessions = active.model.sessions.clone();
        if let Some(id) = target {
            tab.restore_on_connect(id)?;
        }
        let old = std::mem::replace(active, tab);
        drop(inner);
        retire(vec![old]);
        self.poke();
        Ok(())
    }

    /// The engine binary in use; `None` in the design preview.
    fn program(&self) -> Option<PathBuf> {
        let Source::Engine { program, .. } = &self.source else {
            return None;
        };
        let installed = self
            .installed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        Some(installed.unwrap_or_else(|| program.clone()))
    }
    fn launch(&self, path: &Path) -> Option<Launch> {
        let Source::Engine { args, .. } = &self.source else {
            return None;
        };
        let program = self.program()?;
        let style = *self
            .answer_style
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // Engines that predate the flag would refuse to start with it, so they go without.
        let guidance = style
            .guidance()
            .filter(|_| self.answer_support.accepts(&program));
        let mut launch = Launch::new(program).args(args.iter().cloned());
        if let Some(text) = guidance {
            launch = launch.arg(crate::answer_style::FLAG).arg(text);
        }
        Some(
            launch
                .current_dir(path)
                .diagnostics(Diagnostics::Inherit)
                .interactive(true),
        )
    }

    /// A new engine in `path`; `None` in the design preview.
    fn spawn(&self, path: &Path) -> Option<Engine> {
        self.spawn_after(path, None)
    }

    fn spawn_after(
        &self,
        path: &Path,
        previous: Option<std::sync::Weak<std::sync::atomic::AtomicBool>>,
    ) -> Option<Engine> {
        let launch = self.launch(path)?;
        let wake = self
            .wake
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let waker = Arc::new(move || {
            let _ = wake.send(());
        });
        Some(match previous {
            Some(previous) => Engine::spawn_after(launch, waker, Some(previous)),
            None => Engine::spawn(launch, waker),
        })
    }

    /// Show a new engine for the open workspace that runs `first`; the busy tab keeps going.
    fn open_tab(&self, inner: &mut Inner, first: Option<Command>) -> Result<(), String> {
        let root = inner
            .workspace
            .as_ref()
            .map(|w| PathBuf::from(&w.root))
            .ok_or("Open a folder first")?;
        let engine = self.spawn(&root).ok_or(PREVIEW_NOTICE)?;
        let mut tab = Tab::new(Model::starting(), Some(engine));
        if let Some(command) = first {
            tab.enqueue(command)?;
        }
        inner.tabs.open(tab);
        Ok(())
    }

    /// Republish now.
    fn poke(&self) {
        let _ = self
            .wake
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send(());
    }
}

/// Send `commands` to the engine; in the preview, say that nothing was sent.
fn dispatch(app: &AppHandle, engine: Option<&Engine>, commands: Vec<Command>) {
    for command in commands {
        match engine {
            Some(engine) => engine.send(command),
            None => toast(app, PREVIEW_NOTICE),
        }
    }
}

fn toast(app: &AppHandle, message: &str) {
    let _ = app.emit("toast", message);
}

fn model_options(model: &Model) -> Vec<ModelOptionView> {
    model.models.iter().map(ModelOptionView::from).collect()
}

/// Shut closed tabs' engines down gracefully (bounded) off the UI path.
fn retire(tabs: Vec<Tab>) {
    if !tabs.is_empty() {
        thread::spawn(move || drop(tabs));
    }
}

/// The tab on screen, with what the other tabs are doing marked in its sidebar.
fn snapshot_of(inner: &mut Inner) -> Snapshot {
    let mut snapshot = Snapshot::reusing(
        &inner.tabs.active().model,
        inner.workspace.as_ref(),
        time::now(),
        &mut inner.entry_views,
    );
    snapshot.history_loading |= inner.tabs.active().opening_session();
    snapshot.can_send &= !inner.tabs.active().transitioning();
    snapshot.show_activity(&inner.tabs.activity());
    snapshot
}

/// Fold queued engine updates into the models and publish the tab on screen.
fn publish(app: &AppHandle) {
    let shared = app.state::<Shared>();
    let (snapshot, models, settings, messages, closed, notices) = {
        let mut inner = shared.lock();
        let tabs = &mut inner.tabs;
        let before = tabs.busy_states();
        let mut messages = Vec::new();
        for tab in tabs.iter_mut() {
            let updates = tab.engine.as_ref().map(Engine::drain).unwrap_or_default();
            for update in updates {
                let follow = tab.apply(update);
                dispatch(app, tab.engine.as_ref(), follow);
            }
            messages.extend(tab.model.toast.take());
        }
        tabs.sync_deletions();
        messages.extend(tabs.background_news(&before));
        let notices = tabs.session_notices();
        let closed = tabs.close_idle();
        let active = tabs.active_mut();
        if !closed.is_empty() {
            // Those sessions' titles and activity changed.
            dispatch(app, active.engine.as_ref(), vec![Command::ListSessions]);
        }
        let models =
            std::mem::take(&mut active.model.models_changed).then(|| model_options(&active.model));
        let settings = std::mem::take(&mut active.model.settings_changed)
            .then(|| settings_of(&active.model))
            .flatten();
        (
            snapshot_of(&mut inner),
            models,
            settings,
            messages,
            closed,
            notices,
        )
    };
    retire(closed);
    if let Some(models) = models {
        let _ = app.emit("models", models);
    }
    if let Some(settings) = settings {
        let _ = app.emit("settings", settings);
    }
    if let Some(workspace) = snapshot.workspace.as_ref() {
        notifications::publish(app, &workspace.root, notices);
    }
    let event = shared
        .published
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .advance(snapshot);
    if let Some(event) = event {
        let _ = app.emit("snapshot", event);
    }
    for message in messages {
        toast(app, &message);
    }
}

fn pump(app: &AppHandle, wake: &Receiver<()>) {
    loop {
        match wake.recv_timeout(TICK) {
            Ok(()) => {
                thread::sleep(FRAME);
                while wake.try_recv().is_ok() {}
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        publish(app);
    }
}

/// Queue one request for the engine on screen.
fn request(app: &AppHandle, shared: &Shared, command: Command) {
    let inner = shared.lock();
    dispatch(app, inner.tabs.active().engine.as_ref(), vec![command]);
}

#[tauri::command]
fn snapshot(shared: State<'_, Shared>) -> SnapshotAt {
    shared
        .published
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .current(|| snapshot_of(&mut shared.lock()))
}

#[tauri::command]
fn models(shared: State<'_, Shared>) -> Vec<ModelOptionView> {
    model_options(&shared.lock().tabs.active().model)
}

/// The settings page, once the engine sent both the reference and the saved values.
fn settings_of(model: &Model) -> Option<SettingsView> {
    let mut view = settings::view(model.settings_schema.as_deref()?, model.settings.as_ref()?);
    if let Some(rows) = &model.image_models {
        settings::offer_image_models(&mut view, rows);
    }
    Some(view)
}

/// The settings page as far as loaded; (re)loads it from the engine on screen, and the fresh
/// page follows as the `settings` event.
#[tauri::command]
fn settings(shared: State<'_, Shared>) -> Option<SettingsView> {
    let inner = shared.lock();
    let model = &inner.tabs.active().model;
    let mut load = vec![Command::GetSettings];
    if model.settings_schema.is_none() {
        load.insert(0, Command::SettingsSchema);
    }
    // Asked each time the page opens: a `/models` refresh or a download can add image models.
    load.push(Command::ListImageModels);
    if let Some(engine) = inner.tabs.active().engine.as_ref() {
        for command in load {
            engine.send(command);
        }
    }
    settings_of(model)
}

/// Save one setting in `scope` (`global` or `project`); applied to the session once saved.
#[tauri::command]
fn save_setting(
    app: AppHandle,
    shared: State<'_, Shared>,
    key: String,
    value: serde_json::Value,
    scope: String,
) -> Result<(), String> {
    request(&app, &shared, Model::save_setting(&key, value, &scope)?);
    Ok(())
}

/// Remove one setting from `scope`, restoring the default; applied once removed.
#[tauri::command]
fn reset_setting(
    app: AppHandle,
    shared: State<'_, Shared>,
    key: String,
    scope: String,
) -> Result<(), String> {
    request(&app, &shared, Model::reset_setting(&key, &scope)?);
    Ok(())
}

/// Re-read the saved settings and apply them to the session, after several `set_setting` writes
/// made through `engine_call` (roles, presets) so the session rebuilds once.
#[tauri::command]
fn apply_saved_settings(app: AppHandle, shared: State<'_, Shared>) -> Result<(), String> {
    let mut inner = shared.lock();
    let tab = inner.tabs.active_mut();
    let commands = tab.saved_settings();
    dispatch(&app, tab.engine.as_ref(), commands);
    Ok(())
}

/// Apply the preset on the selected tab's worker and wait for the engine's result.
#[tauri::command]
async fn apply_model_preset(
    shared: State<'_, Shared>,
    workspace_root: String,
    session_id: String,
    roles: serde_json::Value,
    main: Option<engine::PresetModel>,
) -> Result<(), String> {
    let reply = {
        let inner = shared.lock();
        check_session(&inner, &workspace_root, Some(&session_id))?;
        let tab = inner.tabs.active();
        if tab.transitioning() || tab.model.conversation.turn_running() {
            return Err("Wait for the session to settle before applying a preset".into());
        }
        tab.engine.as_ref().ok_or(PREVIEW_NOTICE)?.apply_preset(session_id, roles, main)?
    };
    rpc_reply(reply, None).await.map(|_| ())
}

#[tauri::command]
fn recent_workspaces(shared: State<'_, Shared>) -> Vec<Workspace> {
    let mut inner = shared.lock();
    inner.recents.refresh();
    let current = inner.workspace.as_ref().map(|w| w.root.as_str());
    inner
        .recents
        .paths
        .iter()
        .filter(|path| path.is_dir())
        .map(|path| shared.workspace(path))
        .filter(|w| Some(w.root.as_str()) != current)
        .collect()
}

/// Remove a folder from the recent projects list. The folder and its sessions stay on disk.
#[tauri::command]
fn forget_workspace(shared: State<'_, Shared>, path: String) -> bool {
    shared.lock().recents.forget(Path::new(&path))
}

#[tauri::command]
fn open_workspace(
    app: AppHandle,
    shared: State<'_, Shared>,
    runner: State<'_, crate::app_runner::AppRunner>,
    store: State<'_, crate::types::Store>,
    path: String,
    confirmed_from: Option<String>,
) -> Result<Option<String>, String> {
    let path = PathBuf::from(&path);
    let path = path.canonicalize().unwrap_or(path);
    if !path.is_dir() {
        return Err(format!("{} is not a folder", path.display()));
    }
    let confirm = shared.open(&path, confirmed_from.as_deref(), &runner)?;
    if confirm.is_none() {
        // The project's own types join the list.
        crate::types::follow_project(&app, &store, &path);
    }
    Ok(confirm)
}

#[tauri::command]
fn reconnect(shared: State<'_, Shared>) -> Result<(), String> {
    shared.reconnect()
}

/// The Appearance switches changed (or the window reports them at startup). Engines started from
/// now on get the matching guidance; an idle session restarts into itself so it applies at once.
#[tauri::command]
fn set_answer_style(shared: State<'_, Shared>, cards: bool, diagrams: bool) -> Result<(), String> {
    let style = crate::answer_style::AnswerStyle { cards, diagrams };
    {
        let mut current = shared
            .answer_style
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if *current == style {
            return Ok(());
        }
        *current = style;
    }
    if let Some(file) = crate::answer_style::file(shared.home.as_deref().map(Path::new)) {
        crate::answer_style::save(&file, style)?;
    }
    let has_engine = {
        let mut inner = shared.lock();
        inner.workspace.is_some() && inner.tabs.active_mut().engine.is_some()
    };
    if has_engine {
        // A running turn keeps its engine; the next one started picks the switches up.
        let _ = shared.reconnect();
    }
    Ok(())
}

/// The engine binary, or why there is none (the design preview).
fn engine_program(shared: &Shared) -> Result<PathBuf, String> {
    shared
        .program()
        .ok_or_else(|| "The design preview has no engine".to_owned())
}

/// Ask the engine whether a newer Dial release exists (`dial update --check --json`).
#[tauri::command]
async fn engine_update_check(
    shared: State<'_, Shared>,
) -> Result<updates::EngineUpdateView, String> {
    let program = engine_program(&shared)?;
    tauri::async_runtime::spawn_blocking(move || updates::check_engine(&program))
        .await
        .map_err(|error| error.to_string())
}

/// Update the engine the way it was installed (`dial update`). Running engines keep the old
/// version until they are reconnected; the UI decides when.
#[tauri::command]
async fn engine_update(shared: State<'_, Shared>) -> Result<String, String> {
    let program = engine_program(&shared)?;
    tauri::async_runtime::spawn_blocking(move || updates::update_engine(&program))
        .await
        .map_err(|error| error.to_string())?
}

/// Install the engine with Dial's signed release installer, then connect to it.
#[tauri::command]
async fn engine_install(shared: State<'_, Shared>) -> Result<String, String> {
    engine_program(&shared)?;
    let log = tauri::async_runtime::spawn_blocking(updates::install_engine)
        .await
        .map_err(|error| error.to_string())??;
    let home = shared.home.as_deref().map(Path::new);
    let found = launcher::find_dial(None, std::env::var_os("PATH").as_deref(), home);
    if !found.is_file() {
        return Err(format!(
            "The installer finished but no Dial engine was found in the usual folders.\n{log}"
        ));
    }
    *shared
        .installed
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(found);
    let has_workspace = shared.lock().workspace.is_some();
    if has_workspace {
        shared.reconnect()?;
    }
    Ok(log)
}

/// Whether this build checks for app updates: only release builds, whose config holds the key.
#[tauri::command]
fn app_updates_enabled(app: AppHandle) -> bool {
    updates::updater_configured(app.config().plugins.0.get("updater")) && !cfg!(debug_assertions)
}

fn check_session(
    inner: &Inner,
    workspace_root: &str,
    session_id: Option<&str>,
) -> Result<(), String> {
    if inner.workspace.as_ref().map(|w| w.root.as_str()) != Some(workspace_root)
        || inner.tabs.active().model.conversation.session_id() != session_id
    {
        return Err("The active session changed; try again from that session".into());
    }
    Ok(())
}

async fn rpc_reply(
    reply: engine::RpcReply,
    timeout: Option<Duration>,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || match timeout {
        Some(timeout) => reply
            .recv_timeout(timeout)
            .map_err(|e| format!("No reply from Dial: {e}"))?,
        None => reply
            .recv()
            .map_err(|_| "Dial disconnected before confirming the request".to_owned())?,
    })
    .await
    .map_err(|e| e.to_string())?
}

fn workspace_rpc(
    shared: &Shared,
    root: &str,
    method: &'static str,
    params: serde_json::Value,
) -> Result<engine::RpcReply, String> {
    let inner = shared.lock();
    if inner.workspace.as_ref().map(|w| w.root.as_str()) != Some(root) {
        return Err("The project changed. Try again from the current project.".into());
    }
    let tab = inner.tabs.active();
    tab.engine
        .as_ref()
        .ok_or("Dial is not connected")?
        .rpc(method, params)
}

/// Engine methods the UI may call directly through [`engine_call`], with how long each may take.
/// Read-only views and small, explicit changes the user starts from a control; anything that
/// starts a turn, contacts a relay or needs an interactive sign-in stays out.
const ENGINE_CALLS: &[(&str, u64)] = &[
    ("get_available_commands", 20),
    ("get_session_stats", 20),
    ("usage", 60),
    ("stats", 60),
    ("get_modes", 20),
    ("set_mode", 120),
    ("plan", 20),
    ("list_dirs", 20),
    ("add_dir", 20),
    ("remove_dir", 20),
    ("list_jobs", 20),
    ("stop_job", 30),
    ("list_checks", 30),
    ("add_check", 30),
    ("remove_check", 30),
    ("list_skills", 60),
    ("skill_action", 120),
    ("list_extensions", 30),
    ("memory", 30),
    ("list_schedules", 20),
    ("remove_schedule", 20),
    ("list_checkpoints", 20),
    ("checkpoint_files", 20),
    ("checkpoint_diff", 20),
    ("restore_checkpoint_files", 60),
    ("label_session", 20),
    ("restart_model", 60),
    ("get_settings", 20),
    ("set_setting", 20),
    ("reset_setting", 20),
    ("extension_action", 180),
    ("list_models", 60),
    ("image_settings", 20),
    // Slash commands typed with their arguments (see `src/commandRunner.ts`).
    ("state", 20),
    ("list_sessions", 20),
    ("search_sessions", 60),
    ("export", 60),
    ("compact", 600),
    ("undo", 120),
    ("goal", 600),
    ("side_thread", 120),
    ("task_start", 120),
    ("task_status", 20),
    ("task_message", 20),
    ("task_cancel", 30),
    ("archive", 60),
    ("pack_session", 120),
    ("branch_session", 60),
    ("move_session", 300),
    ("evidence", 60),
    ("evidence_correct", 30),
    ("evidence_compare", 300),
    ("policy", 20),
    ("reliability", 20),
    ("code_history", 60),
    ("benchmark", 60),
    ("schedule_status", 20),
    ("collaboration_start", 60),
    ("collaboration_status", 20),
    ("collaboration_stop", 30),
    ("collaboration_leave", 30),
    ("collaboration_join", 60),
    ("apply_settings", 180),
    ("mcp_logout", 30),
    ("logout", 60),
];

/// Call one allowed engine method in the open project and return its result.
#[tauri::command]
async fn engine_call(
    shared: State<'_, Shared>,
    workspace_root: String,
    method: String,
    params: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let &(method, secs) = ENGINE_CALLS
        .iter()
        .find(|(name, _)| *name == method)
        .ok_or_else(|| format!("Dial Desktop does not call `{method}`"))?;
    let params = params.unwrap_or_else(|| serde_json::json!({}));
    let reply = workspace_rpc(&shared, &workspace_root, method, params)?;
    rpc_reply(reply, Some(Duration::from_secs(secs))).await
}

#[tauri::command]
async fn search_transcripts(
    shared: State<'_, Shared>,
    workspace_root: String,
    query: String,
) -> Result<serde_json::Value, String> {
    let query = query.trim();
    if query.is_empty() || query.len() > 4096 {
        return Err("Enter a search between 1 and 4096 bytes".into());
    }
    let reply = workspace_rpc(
        &shared,
        &workspace_root,
        "search_sessions",
        serde_json::json!({"query": query, "limit": 40}),
    )?;
    rpc_reply(reply, Some(Duration::from_secs(20))).await
}

#[tauri::command]
async fn local_model_removal(
    shared: State<'_, Shared>,
    workspace_root: String,
    model: String,
    dry_run: bool,
) -> Result<serde_json::Value, String> {
    let reply = {
        let inner = shared.lock();
        if inner.workspace.as_ref().map(|w| w.root.as_str()) != Some(workspace_root.as_str()) {
            return Err("The project changed. Reopen the model picker and try again.".into());
        }
        let tab = inner.tabs.active();
        if !tab
            .model
            .models
            .iter()
            .any(|row| row.local && row.model_id == model)
        {
            return Err(
                "Dial no longer lists this downloaded model. Reload the model list.".into(),
            );
        }
        if !dry_run {
            inner.tabs.check_model_removal(&model)?;
        }
        tab.engine.as_ref().ok_or("Dial is not connected")?.rpc(
            "delete_local_model",
            serde_json::json!({"model": model, "dry_run": dry_run}),
        )?
    };
    rpc_reply(reply, Some(Duration::from_secs(60))).await
}

#[tauri::command]
async fn send_prompt(
    shared: State<'_, Shared>,
    store: State<'_, crate::types::Store>,
    workspace_root: String,
    session_id: Option<String>,
    text: String,
    images: Vec<String>,
    inline_images: Vec<String>,
) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() && images.is_empty() && inline_images.is_empty() {
        return Err("The prompt is empty".into());
    }
    attachments::check(&images, &inline_images)?;
    let text = crate::types::with_context(&store, text);
    // Pasted images go by path like chosen files: inline base64 pushed the request past the
    // engine's 1 MiB RPC limit.
    let mut images = images;
    if !inline_images.is_empty() {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);
        let dir = launcher::app_dir(home.as_deref())
            .ok_or("Could not find a folder to save the pasted image in")?
            .join("pasted-images");
        images.extend(attachments::save_pasted(&dir, &inline_images)?);
    }
    let reply = {
        let mut inner = shared.lock();
        check_session(&inner, &workspace_root, session_id.as_deref())?;
        let tab = inner.tabs.active_mut();
        if tab.transitioning() || !tab.model.can_send() {
            return Err("Dial is not ready to accept a prompt".into());
        }
        tab.submit_prompt(Command::Send {
            text,
            images,
            inline_images: Vec::new(),
        })?
    };
    rpc_reply(reply, None).await.map(|_| ())
}

/// Send `text` to session `session_id` without showing it: to the tab that has it open, or to a
/// new engine behind the one on screen. Saving to a type adds the item in the type's own
/// conversation this way, while the user stays where they are.
#[tauri::command]
fn send_background(
    shared: State<'_, Shared>,
    store: State<'_, crate::types::Store>,
    workspace_root: String,
    session_id: String,
    text: String,
) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("The prompt is empty".into());
    }
    let send = Command::Send {
        text: crate::types::with_context(&store, text),
        images: Vec::new(),
        inline_images: Vec::new(),
    };
    let mut inner = shared.lock();
    if inner.workspace.as_ref().map(|w| w.root.as_str()) != Some(workspace_root.as_str()) {
        return Err("The project changed".into());
    }
    match inner.tabs.route_resume(&session_id) {
        Route::Switch(index) => {
            let tab = inner.tabs.get_mut(index).ok_or("That session closed")?;
            if tab.protects_work() || !tab.model.can_send() {
                return Err("Dial is still working on the last one; try again when it finishes".into());
            }
            tab.enqueue(send)?;
        }
        Route::Closing => return Err("Dial is just finishing up; try again in a moment".into()),
        Route::Elsewhere => return Err(OPEN_ELSEWHERE.into()),
        Route::Here | Route::Spawn => {
            let engine = shared
                .spawn(Path::new(&workspace_root))
                .ok_or(PREVIEW_NOTICE)?;
            let mut tab = Tab::new(Model::starting(), Some(engine));
            tab.enqueue(Command::Resume(session_id))?;
            tab.enqueue(send)?;
            inner.tabs.add_background(tab);
        }
    }
    drop(inner);
    shared.poke();
    Ok(())
}

/// Check an image chosen or dropped by the user and return its preview. Async, so reading a
/// large file runs off the main thread.
#[tauri::command]
async fn read_image(webview: tauri::Webview, path: String) -> Result<ImageFile, String> {
    if webview.label() != "main" {
        return Err("Images are only available in the app UI".into());
    }
    tauri::async_runtime::spawn_blocking(move || attachments::read_image(Path::new(&path)))
        .await
        .map_err(|e| e.to_string())?
}

/// Read the native clipboard only for the app UI, never for the integrated browser.
#[tauri::command]
fn read_clipboard_image(webview: tauri::Webview) -> Result<Option<String>, String> {
    if webview.label() != "main" {
        return Err("Clipboard images are only available in the composer".into());
    }
    #[cfg(target_os = "macos")]
    return attachments::clipboard_image();
    #[cfg(not(target_os = "macos"))]
    Ok(None)
}

/// Save an image shown in the app to a path the user picked in the save dialog. Only the app UI
/// may call it; the integrated browser's pages never can.
#[tauri::command]
async fn save_image(webview: tauri::Webview, path: String, data: String) -> Result<(), String> {
    if webview.label() != "main" {
        return Err("Saving images is only available in the app UI".into());
    }
    tauri::async_runtime::spawn_blocking(move || attachments::save_image(Path::new(&path), &data))
        .await
        .map_err(|e| e.to_string())?
}

/// Load a relative transcript image from the conversation's project, not the web app bundle.
#[tauri::command]
async fn read_transcript_image(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    workspace_root: String,
    path: String,
) -> Result<ImageFile, String> {
    if webview.label() != "main" {
        return Err("Transcript images are only available in the app UI".into());
    }
    if path.starts_with("artifact://") {
        let reply = workspace_rpc(&shared, &workspace_root, "read_image_artifact", serde_json::json!({"uri":path}))?;
        let result = rpc_reply(reply, Some(Duration::from_secs(20))).await?;
        let mime = result["media_type"].as_str().ok_or("Missing image type")?;
        let extension = match mime { "image/png" => "png", "image/jpeg" => "jpg", "image/webp" => "webp", "image/gif" => "gif", _ => return Err("Unsupported image type".into()) };
        let data = result["data"].as_str().ok_or("Missing image bytes")?;
        return Ok(ImageFile { name: format!("Tool image.{extension}"), url: format!("data:{mime};base64,{data}"), path });
    }
    {
        let inner = shared.lock();
        if inner.workspace.as_ref().map(|w| w.root.as_str()) != Some(workspace_root.as_str()) {
            return Err("The project changed".into());
        }
    }
    tauri::async_runtime::spawn_blocking(move || {
        let resolved = files::resolve(Path::new(&workspace_root), &path)?;
        attachments::read_image(&resolved)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Open an image from the conversation's project in the system's default viewer (Preview on a
/// Mac). Only raster images inside the open project; never anything the transcript names elsewhere.
#[tauri::command]
async fn open_transcript_image(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    workspace_root: String,
    path: String,
) -> Result<(), String> {
    if webview.label() != "main" {
        return Err("Transcript images are only available in the app UI".into());
    }
    {
        let inner = shared.lock();
        if inner.workspace.as_ref().map(|w| w.root.as_str()) != Some(workspace_root.as_str()) {
            return Err("The project changed".into());
        }
    }
    tauri::async_runtime::spawn_blocking(move || {
        let resolved = files::resolve(Path::new(&workspace_root), &path)?;
        if attachments::image_mime(&resolved).is_none() || !resolved.is_file() {
            return Err(format!("{path} is not an image file"));
        }
        tauri_plugin_opener::open_path(&resolved, None::<&str>)
            .map_err(|e| format!("Could not open {path}: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn approve(
    app: AppHandle,
    shared: State<'_, Shared>,
    approval_id: String,
    decision: String,
) -> Result<(), String> {
    let decision = engine::parse_decision(&decision)
        .ok_or_else(|| format!("Unknown approval decision `{decision}`"))?;
    request(
        &app,
        &shared,
        Command::Approve {
            approval_id,
            decision,
        },
    );
    Ok(())
}

#[tauri::command]
fn abort(app: AppHandle, shared: State<'_, Shared>) {
    request(&app, &shared, Command::Abort);
}

/// A fresh session; while the one on screen works, in a new engine beside it.
#[tauri::command]
fn new_session(app: AppHandle, shared: State<'_, Shared>) -> Result<(), String> {
    let mut inner = shared.lock();
    match inner.tabs.route_new() {
        Route::Spawn => shared.open_tab(&mut inner, None)?,
        Route::Here | Route::Switch(_) | Route::Closing | Route::Elsewhere => {
            dispatch(
                &app,
                inner.tabs.active().engine.as_ref(),
                vec![Command::NewSession],
            );
        }
    }
    drop(inner);
    shared.poke();
    Ok(())
}

/// Show session `session_id`: the tab already running it, this tab when idle, or a new engine.
#[tauri::command]
async fn resume(
    app: AppHandle,
    session_id: String,
    workspace_root: Option<String>,
    wait_for_close: Option<bool>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let shared = app.state::<Shared>();
        let deadline = std::time::Instant::now() + Duration::from_secs(6);
        loop {
            if resume_once(&app, &shared, &session_id, workspace_root.as_deref())? {
                return Ok(());
            }
            if wait_for_close != Some(true) || std::time::Instant::now() >= deadline {
                return Err("That session just finished. Try opening it again in a moment.".into());
            }
            thread::sleep(Duration::from_millis(100));
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

fn resume_once(
    app: &AppHandle,
    shared: &Shared,
    session_id: &str,
    workspace_root: Option<&str>,
) -> Result<bool, String> {
    let mut inner = shared.lock();
    if let Some(root) = workspace_root
        && inner.workspace.as_ref().map(|w| w.root.as_str()) != Some(root)
    {
        return Err("That search result belongs to a different project".into());
    }
    match inner.tabs.route_resume(session_id) {
        Route::Switch(index) => inner.tabs.switch(index),
        Route::Spawn => {
            shared.open_tab(&mut inner, Some(Command::Resume(session_id.to_owned())))?
        }
        Route::Here => inner
            .tabs
            .active_mut()
            .enqueue(Command::Resume(session_id.to_owned()))?,
        Route::Closing => return Ok(false),
        Route::Elsewhere => {
            // Refresh the sidebar so a session released meanwhile opens on the next click.
            let tab = inner.tabs.active();
            dispatch(app, tab.engine.as_ref(), vec![Command::ListSessions]);
            return Err(OPEN_ELSEWHERE.into());
        }
    }
    drop(inner);
    shared.poke();
    Ok(true)
}

/// Delete sessions `session_ids` (one or several selected in the sidebar).
#[tauri::command]
fn delete_sessions(
    app: AppHandle,
    shared: State<'_, Shared>,
    session_ids: Vec<String>,
) -> Result<(), String> {
    if session_ids.is_empty() {
        return Ok(());
    }
    let mut inner = shared.lock();
    if let Some(reason) = inner.tabs.delete_refusal(&session_ids) {
        return Err(reason);
    }
    let tab = inner.tabs.active_mut();
    let commands = tab.model.delete_sessions(&session_ids)?;
    dispatch(&app, tab.engine.as_ref(), commands);
    Ok(())
}

/// Rename session `session_id`: in the tab that runs it, else here (opening it first).
#[tauri::command]
fn rename_session(
    app: AppHandle,
    shared: State<'_, Shared>,
    session_id: String,
    title: String,
) -> Result<(), String> {
    let mut inner = shared.lock();
    let tabs = &mut inner.tabs;
    let index = match tabs.route_resume(&session_id) {
        Route::Switch(index) => index,
        Route::Here | Route::Spawn => tabs.active_index(),
        Route::Closing => {
            return Err("That session is still closing. Try again in a moment.".into());
        }
        Route::Elsewhere => return Err(OPEN_ELSEWHERE.into()),
    };
    let tab = tabs
        .iter_mut()
        .nth(index)
        .ok_or("That session is not open")?;
    let commands = tab.model.rename_session(&session_id, &title)?;
    dispatch(&app, tab.engine.as_ref(), commands);
    Ok(())
}

#[tauri::command]
fn pin_session(app: AppHandle, shared: State<'_, Shared>, session_id: String, pinned: bool) {
    request(&app, &shared, Command::PinSession { session_id, pinned });
}

/// The copy is made and rewound on a private connection. No request can rewind the source.
#[tauri::command]
async fn fork_before(
    shared: State<'_, Shared>,
    workspace_root: String,
    session_id: String,
    prompt: String,
    occurrence: usize,
    matches: usize,
) -> Result<String, String> {
    let launch = {
        let inner = shared.lock();
        check_session(&inner, &workspace_root, Some(&session_id))?;
        if inner.tabs.active().model.conversation.turn_running() {
            return Err("Wait for the turn to finish".into());
        }
        shared
            .launch(Path::new(&workspace_root))
            .ok_or(PREVIEW_NOTICE)?
    };
    tauri::async_runtime::spawn_blocking(move || {
        engine::fork_before(&launch, &session_id, &prompt, occurrence, matches)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn project_window(shared: State<'_, Shared>, path: String) -> Result<(), String> {
    let path = PathBuf::from(path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !path.is_dir() {
        return Err("Choose a project directory".into());
    }
    let mut child = std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    match &shared.source {
        Source::Engine { args, .. } => {
            child
                .arg("--dial")
                .arg(shared.program().ok_or("No engine configured")?)
                .arg(&path)
                .arg("--")
                .args(args);
        }
        Source::Preview(kind) => {
            child
                .arg(match kind {
                    Preview::Empty => "--preview=empty",
                    Preview::Question => "--preview=question",
                    Preview::Session => "--preview",
                })
                .arg(&path);
        }
    }
    let mut child = child
        .spawn()
        .map_err(|e| format!("Could not open window: {e}"))?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub(crate) fn browser_project(app: &AppHandle, root: &str) -> bool {
    app.state::<Shared>()
        .root()
        .is_ok_and(|p| p == Path::new(root))
}

/// Copy session `session_id` into a new one and open the copy.
#[tauri::command]
fn fork_session(
    app: AppHandle,
    shared: State<'_, Shared>,
    session_id: String,
) -> Result<(), String> {
    let inner = shared.lock();
    let tab = inner.tabs.active();
    if tab.model.conversation.turn_running() {
        return Err("Stop the running turn (Esc) before duplicating".into());
    }
    dispatch(
        &app,
        tab.engine.as_ref(),
        vec![Command::ForkSession(session_id)],
    );
    Ok(())
}

/// Export the session on screen as Markdown to `path`, which must lie in the project folder.
#[tauri::command]
fn export_session(app: AppHandle, shared: State<'_, Shared>, path: String) -> Result<(), String> {
    let inner = shared.lock();
    let root = inner
        .workspace
        .as_ref()
        .map(|w| PathBuf::from(&w.root))
        .ok_or("Open a folder first")?;
    // The dialog may report the folder through a symlink (`/var` and `/private/var` on macOS).
    let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let chosen = Path::new(&path);
    let parent = chosen.parent().map(real).unwrap_or_default();
    let chosen = parent.join(chosen.file_name().unwrap_or_default());
    let relative = chosen
        .strip_prefix(real(&root))
        .map_err(|_| "Choose a place inside the project folder; Dial exports there".to_owned())?
        .to_string_lossy()
        .into_owned();
    if relative.is_empty() {
        return Err("Choose a file name".into());
    }
    dispatch(
        &app,
        inner.tabs.active().engine.as_ref(),
        vec![Command::Export(relative)],
    );
    Ok(())
}

/// Undo the last turn of the session on screen, files included.
#[tauri::command]
fn undo_turn(
    shared: State<'_, Shared>,
    workspace_root: String,
    session_id: Option<String>,
) -> Result<(), String> {
    let inner = shared.lock();
    check_session(&inner, &workspace_root, session_id.as_deref())?;
    let tab = inner.tabs.active();
    let command = tab.model.undo()?;
    tab.engine
        .as_ref()
        .ok_or("Dial is not connected")?
        .enqueue(command)
}

/// Add guidance to the turn running in the session on screen.
#[tauri::command]
async fn steer(
    shared: State<'_, Shared>,
    store: State<'_, crate::types::Store>,
    workspace_root: String,
    session_id: Option<String>,
    text: String,
) -> Result<(), String> {
    let text = crate::types::with_context(&store, text.trim());
    let reply = {
        let inner = shared.lock();
        check_session(&inner, &workspace_root, session_id.as_deref())?;
        let tab = inner.tabs.active();
        let command = tab.model.steer(&text)?;
        tab.engine
            .as_ref()
            .ok_or("Dial is not connected")?
            .submit(command)?
    };
    rpc_reply(reply, Some(Duration::from_secs(15)))
        .await
        .map(|_| ())
}

/// Send the last unanswered prompt of the session on screen again.
#[tauri::command]
fn retry_turn(shared: State<'_, Shared>) -> Result<(), String> {
    let mut inner = shared.lock();
    let tab = inner.tabs.active_mut();
    if tab.transitioning() {
        return Err("Wait for the session to finish opening or sending".into());
    }
    let command = tab.model.retry()?;
    tab.enqueue(command)
}

/// Summarize the conversation on screen to free context.
#[tauri::command]
fn compact_session(app: AppHandle, shared: State<'_, Shared>) -> Result<(), String> {
    let inner = shared.lock();
    let tab = inner.tabs.active();
    let command = tab.model.compact()?;
    dispatch(&app, tab.engine.as_ref(), vec![command]);
    Ok(())
}

/// Start voice mode in the session on screen, or talk again while it waits.
#[tauri::command]
fn voice_start(app: AppHandle, shared: State<'_, Shared>) -> Result<(), String> {
    let mut inner = shared.lock();
    let tab = inner.tabs.active_mut();
    if tab.transitioning() {
        return Err("Wait for the session to finish opening".into());
    }
    let command = tab.model.start_voice()?;
    dispatch(&app, tab.engine.as_ref(), command.into_iter().collect());
    Ok(())
}

/// The voice orb: talk over the reply being read, or stop recording.
#[tauri::command]
fn voice_tap(app: AppHandle, shared: State<'_, Shared>) {
    let mut inner = shared.lock();
    let tab = inner.tabs.active_mut();
    let command = tab.model.voice_tap();
    dispatch(&app, tab.engine.as_ref(), command.into_iter().collect());
}

/// Stop the turn voice mode started, because Dial misheard.
#[tauri::command]
fn voice_cancel(app: AppHandle, shared: State<'_, Shared>) {
    let mut inner = shared.lock();
    let tab = inner.tabs.active_mut();
    let command = tab.model.voice_cancel();
    dispatch(&app, tab.engine.as_ref(), command.into_iter().collect());
}

#[tauri::command]
fn voice_mute(app: AppHandle, shared: State<'_, Shared>, muted: bool) {
    let mut inner = shared.lock();
    let tab = inner.tabs.active_mut();
    let command = tab.model.voice.mute(muted);
    dispatch(&app, tab.engine.as_ref(), command.into_iter().collect());
}

/// End voice mode; a turn already running carries on.
#[tauri::command]
fn voice_stop(app: AppHandle, shared: State<'_, Shared>) {
    let mut inner = shared.lock();
    let tab = inner.tabs.active_mut();
    let commands = tab.model.voice.stop();
    dispatch(&app, tab.engine.as_ref(), commands);
}

/// Read a reply aloud; without `text`, stop reading.
#[tauri::command]
fn read_aloud(
    app: AppHandle,
    shared: State<'_, Shared>,
    text: Option<String>,
) -> Result<(), String> {
    let mut inner = shared.lock();
    let tab = inner.tabs.active_mut();
    let command = tab.model.read_aloud(text)?;
    dispatch(&app, tab.engine.as_ref(), vec![command]);
    Ok(())
}

#[tauri::command]
fn refresh_sessions(app: AppHandle, shared: State<'_, Shared>) {
    request(&app, &shared, Command::ListSessions);
}

#[tauri::command]
fn refresh_models(app: AppHandle, shared: State<'_, Shared>) {
    request(&app, &shared, Command::ListModels);
}

#[tauri::command]
fn discover_models(app: AppHandle, shared: State<'_, Shared>) {
    request(&app, &shared, Command::DiscoverModels);
}

#[tauri::command]
fn set_model(app: AppHandle, shared: State<'_, Shared>, workspace_root: String, session_id: String, provider: String, model_id: String) -> Result<(), String> {
    select_model_setting(&app, &shared, &workspace_root, &session_id, Command::SetModel { session_id: session_id.clone(), provider, model_id })
}

#[tauri::command]
fn set_thinking(app: AppHandle, shared: State<'_, Shared>, workspace_root: String, session_id: String, level: String) -> Result<(), String> {
    select_model_setting(&app, &shared, &workspace_root, &session_id, Command::SetThinking { session_id: session_id.clone(), level })
}

fn select_model_setting(app: &AppHandle, shared: &Shared, root: &str, session: &str, command: Command) -> Result<(), String> {
    let mut inner = shared.lock();
    check_session(&inner, root, Some(session))?;
    let tab = inner.tabs.active_mut();
    if tab.engine.is_none() { return Err("Dial is not connected".into()); }
    if let Some(command) = tab.select_model_setting(command)? {
        dispatch(app, tab.engine.as_ref(), vec![command]);
    }
    drop(inner);
    shared.poke();
    Ok(())
}

#[tauri::command]
fn set_approval(app: AppHandle, shared: State<'_, Shared>, mode: String) -> Result<(), String> {
    if !APPROVAL_MODES.contains(&mode.as_str()) {
        return Err(format!("Unknown approval mode `{mode}`"));
    }
    request(&app, &shared, Command::SetApproval(mode));
    Ok(())
}

/// The UI shows the open question; acknowledged once, which starts its answer timeout.
#[tauri::command]
fn question_presented(shared: State<'_, Shared>, question_id: String) {
    let mut inner = shared.lock();
    let tab = inner.tabs.active_mut();
    let command = tab.model.present_question(&question_id);
    // In the preview the acknowledgement has nowhere to go and is not worth a toast.
    if let (Some(engine), Some(command)) = (tab.engine.as_ref(), command) {
        engine.send(command);
    }
}

#[tauri::command]
fn answer_question(
    app: AppHandle,
    shared: State<'_, Shared>,
    question_id: String,
    reply: Reply,
    answers: Vec<Answer>,
) -> Result<(), String> {
    let inner = shared.lock();
    let tab = inner.tabs.active();
    let command = tab.model.answer_question(&question_id, reply, &answers)?;
    dispatch(&app, tab.engine.as_ref(), vec![command]);
    Ok(())
}

/// The entries of folder `path` in the workspace (`""` for its root).
#[tauri::command]
async fn list_dir(
    shared: State<'_, Shared>,
    path: String,
    workspace_root: String,
) -> Result<Vec<DirEntryView>, String> {
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::list(&root, &path))
        .await
        .map_err(|e| e.to_string())?
}

/// File `path` in the workspace, for the viewer.
#[tauri::command]
async fn read_file(
    shared: State<'_, Shared>,
    path: String,
    workspace_root: String,
) -> Result<FileView, String> {
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::read(&root, &path))
        .await
        .map_err(|e| e.to_string())?
}

/// The uncommitted changes of file `path` in the workspace.
#[tauri::command]
async fn file_diff(
    shared: State<'_, Shared>,
    path: String,
    workspace_root: String,
) -> Result<DiffView, String> {
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::diff(&root, &path))
        .await
        .map_err(|e| e.to_string())?
}

fn panel_root(shared: &Shared, expected: &str) -> Result<PathBuf, String> {
    let root = shared.root()?;
    if root != Path::new(expected) {
        return Err("The project changed before the file panel could load".into());
    }
    Ok(root)
}

/// Fuzzy-search files inside the workspace for `@` mentions in the composer.
#[tauri::command]
async fn search_files(
    shared: State<'_, Shared>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<String>, String> {
    let root = shared.root()?;
    let cap = limit.unwrap_or(20);
    tauri::async_runtime::spawn_blocking(move || files::search(&root, &query, cap))
        .await
        .map_err(|error| error.to_string())?
}

/// Provider setup has its own PTY: it must not depend on RPC or type into the user's shell.
#[tauri::command]
fn provider_setup_open(
    shared: State<'_, Shared>,
    workspace_root: String,
    cols: u16,
    rows: u16,
    output: tauri::ipc::Channel<terminal::TerminalEvent>,
) -> Result<u32, String> {
    let program = engine_program(&shared)?;
    let inner = shared.lock();
    let workspace = inner.workspace.as_ref().ok_or("Open a folder first")?;
    if workspace.root != workspace_root {
        return Err("The open folder changed".into());
    }
    let mut command = portable_pty::CommandBuilder::new(program);
    command.arg("/providers");
    command.cwd(&workspace.root);
    command.env("TERM", "xterm-256color");
    shared.terminals().open(command, cols, rows, move |event| {
        let _ = output.send(event);
    })
}

/// Start a shell in the workspace; its output follows as `terminal` events.
#[tauri::command]
fn terminal_open(
    app: AppHandle,
    shared: State<'_, Shared>,
    cols: u16,
    rows: u16,
) -> Result<u32, String> {
    let root = shared.root()?;
    shared
        .terminals()
        .open(terminal::login_shell(&root), cols, rows, move |event| {
            let _ = app.emit("terminal", event);
        })
}

#[tauri::command]
fn terminal_write(shared: State<'_, Shared>, id: u32, data: String) -> Result<(), String> {
    shared.terminals().write(id, &data)
}

#[tauri::command]
fn terminal_resize(shared: State<'_, Shared>, id: u32, cols: u16, rows: u16) -> Result<(), String> {
    shared.terminals().resize(id, cols, rows)
}

#[tauri::command]
fn terminal_close(shared: State<'_, Shared>, id: u32) {
    shared.terminals().close(id);
}

/// Open the window and run until it closes. With no `workspace` the UI asks for a folder.
pub fn run(source: Source, workspace: Option<PathBuf>, recents: Recents) -> tauri::Result<()> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok();
    let endpoint = crate::agent_browser::endpoint_file(home.as_deref().map(Path::new));
    let endpoint_on_exit = endpoint.clone();

    let app = tauri::Builder::default()
        // Links in answers open in the system browser, never in the app window.
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        // The app's own signed updates, verified with `plugins.updater.pubkey` from the config;
        // checked only when `app_updates_enabled`.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // A custom type layout's own page (`types::custom_page`), shown in a sandboxed frame.
        .register_uri_scheme_protocol(crate::types::CUSTOM_SCHEME, |ctx, request| {
            use tauri::Manager as _;
            let store = ctx.app_handle().state::<crate::types::Store>();
            crate::types::custom_page(&store, request.uri().path())
        })
        .invoke_handler(tauri::generate_handler![
            notifications::set_notifications,
            notifications::type_reminders_allow,
            notifications::type_reminder,
            crate::types::types_list,
            crate::types::types_get,
            crate::types::types_location,
            crate::types::types_set_location,
            crate::types::types_create,
            crate::types::types_update,
            crate::types::types_claim_session,
            crate::types::types_delete,
            crate::types::types_add_item,
            crate::types::types_update_item,
            crate::types::types_remove_item,
            crate::types::types_move_item,
            crate::types::types_import_image,
            crate::types::types_import_image_data,
            crate::types::types_open_asset,
            snapshot,
            set_answer_style,
            models,
            settings,
            save_setting,
            reset_setting,
            apply_saved_settings,
            apply_model_preset,
            recent_workspaces,
            forget_workspace,
            open_workspace,
            reconnect,
            engine_update_check,
            engine_update,
            engine_install,
            app_updates_enabled,
            send_prompt,
            send_background,
            read_image,
            read_clipboard_image,
            save_image,
            read_transcript_image,
            open_transcript_image,
            approve,
            abort,
            new_session,
            resume,
            delete_sessions,
            rename_session,
            pin_session,
            fork_session,
            fork_before,
            project_window,
            export_session,
            undo_turn,
            steer,
            retry_turn,
            compact_session,
            voice_start,
            voice_tap,
            voice_cancel,
            voice_mute,
            voice_stop,
            read_aloud,
            refresh_sessions,
            search_transcripts,
            engine_call,
            local_model_removal,
            crate::model_library::model_download_status,
            crate::model_library::model_download_start,
            crate::model_library::model_download_cancel,
            crate::model_library::model_cache_list,
            crate::model_library::model_cache_remove,
            crate::model_library::model_hub_search,
            crate::model_library::model_hub_files,
            refresh_models,
            discover_models,
            set_model,
            set_thinking,
            set_approval,
            question_presented,
            answer_question,
            list_dir,
            read_file,
            file_diff,
            search_files,
            terminal_open,
            provider_setup_open,
            terminal_write,
            terminal_resize,
            terminal_close,
            browser::browser_show,
            browser::browser_bounds,
            browser::browser_hide,
            browser::browser_navigate,
            browser::browser_go,
            browser::browser_inspect,
            browser::browser_state,
            crate::app_disk::app_runner_import,
            crate::app_runner::app_runner_preferred_target,
            crate::app_runner::host::app_runner_host,
            crate::app_runner::host::app_runner_host_install,
            crate::app_runner::installed::app_runner_vms,
            crate::app_runner::app_runner_frame_bytes,
            crate::app_runner::installed::app_runner_vm_grant,
            crate::app_runner::installed::app_runner_vm_shutdown,
            crate::app_runner::installed::app_runner_vm_ssh_link,
            crate::app_runner::installed::app_runner_vm_ssh_setup,
            crate::app_runner::app_runner_state,
            crate::app_runner::app_runner_frame,
            crate::app_runner::app_runner_start,
            crate::app_runner::app_runner_stop,
            crate::app_runner::app_runner_input,
            crate::app_runner::app_runner_set_agent_access
        ])
        .setup(move |app| {
            let (wake_tx, wake_rx) = mpsc::channel();
            app.manage(Notifications::default());
            // `DIAL_TYPES_DIR` keeps a separate set of types (design previews, tests).
            app.manage(match std::env::var_os("DIAL_TYPES_DIR") {
                Some(dir) => crate::types::Store::new(Some(PathBuf::from(dir))),
                None => crate::types::Store::for_home(home.as_deref().map(Path::new)),
            });
            app.manage(crate::model_library::Downloads::default());
            let answer_style = crate::answer_style::load(
                crate::answer_style::file(home.as_deref().map(Path::new)).as_deref(),
            );
            app.manage(Shared {
                inner: Mutex::new(Inner {
                    tabs: Tabs::new(Tab::new(Model::default(), None)),
                    workspace: None,
                    recents,
                    entry_views: EntryViews::default(),
                }),
                source,
                home,
                wake: Mutex::new(wake_tx),
                terminals: Mutex::new(Terminals::default()),
                installed: Mutex::new(None),
                published: Mutex::new(Published::default()),
                answer_style: Mutex::new(answer_style),
                answer_support: crate::answer_style::Support::default(),
            });
            app.manage(Browser::default());
            app.manage(crate::app_runner::AppRunner::new(
                app.handle().clone(),
                app.path().resource_dir()?,
                app.path().app_data_dir()?,
            ));
            // The agent's tools for the Browser panel (`agent_browser`); without them the panel
            // still works for the user.
            if let Some(file) = &endpoint
                && let Err(error) = crate::agent_browser::start(app.handle(), file)
            {
                eprintln!("dial-desktop: the agent's browser tools are off: {error}");
            }
            if let Some(path) = workspace {
                let path_root = path.clone();
                app.state::<Shared>().open(
                    &path,
                    None,
                    &app.state::<crate::app_runner::AppRunner>(),
                )?;
                app.state::<crate::types::Store>().set_project(Some(&path_root));
            }
            let handle = app.handle().clone();
            thread::Builder::new()
                .name("dial-ui-pump".into())
                .spawn(move || pump(&handle, &wake_rx))?;

            let builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("Dial")
                .inner_size(1380.0, 880.0)
                .min_inner_size(960.0, 600.0)
                .background_color(tauri::window::Color(0x0E, 0x0E, 0x11, 0xFF));
            // A unified window: content runs under the macOS title bar, like native editors.
            #[cfg(target_os = "macos")]
            let builder = builder
                .title_bar_style(tauri::TitleBarStyle::Overlay)
                .hidden_title(true);
            builder.build()?;
            Ok(())
        })
        .build(tauri::generate_context!())?;

    app.run(move |app, event| {
        if let RunEvent::Exit = event
            && let Some(shared) = app.try_state::<Shared>()
        {
            app.state::<crate::app_runner::AppRunner>().stop_all();
            shared.terminals().close_all();
            app.state::<crate::model_library::Downloads>().stop();
            if let Some(file) = &endpoint_on_exit {
                crate::agent_browser::remove_endpoint(file);
            }
            // Shut every engine down gracefully (bounded, in parallel) before the process ends.
            let empty = Tabs::new(Tab::new(Model::default(), None));
            let tabs = std::mem::replace(&mut shared.lock().tabs, empty);
            let closing: Vec<_> = tabs
                .into_tabs()
                .into_iter()
                .map(|tab| thread::spawn(move || drop(tab)))
                .collect();
            for handle in closing {
                let _ = handle.join();
            }
        }
    });
    Ok(())
}
