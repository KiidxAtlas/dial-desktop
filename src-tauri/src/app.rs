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
use crate::cli::Preview;
use crate::engine::{self, APPROVAL_MODES, Command, Engine};
use crate::launcher::Recents;
use crate::model::{Answer, Model, Reply};
use crate::preview;
use crate::settings::{self, SettingsView};
use crate::tabs::{Route, Tab, Tabs};
use crate::time;
use crate::view::{ModelOptionView, Snapshot, Workspace};
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
}

/// State shared by the pump thread and the commands.
struct Shared {
    inner: Mutex<Inner>,
    source: Source,
    home: Option<String>,
    /// Wakes the pump; every engine gets a clone.
    wake: Mutex<Sender<()>>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn workspace(&self, path: &Path) -> Workspace {
        Workspace::new(&path.to_string_lossy(), self.home.as_deref())
    }

    /// Run in `path` from now on: a fresh engine there (or the preview), remembered as recent.
    fn open(&self, path: &Path) {
        let workspace = self.workspace(path);
        let tab = match &self.source {
            Source::Preview(kind) => Tab::new(preview::model(*kind, &workspace.root), None),
            Source::Engine { .. } => Tab::new(Model::starting(), self.spawn(path)),
        };
        let mut inner = self.lock();
        inner.recents.touch(path);
        let old = std::mem::replace(&mut inner.tabs, Tabs::new(tab));
        inner.workspace = Some(workspace);
        drop(inner);
        retire(old.into_tabs());
        self.poke();
    }

    /// A new engine in `path`; `None` in the design preview.
    fn spawn(&self, path: &Path) -> Option<Engine> {
        let Source::Engine { program, args } = &self.source else {
            return None;
        };
        let wake = self
            .wake
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let waker = Arc::new(move || {
            let _ = wake.send(());
        });
        // A person answers this client, so the engine may ask questions.
        let launch = Launch::new(program)
            .args(args.iter().cloned())
            .current_dir(path)
            .diagnostics(Diagnostics::Inherit)
            .interactive(true);
        Some(Engine::spawn(launch, waker))
    }

    /// Show a new engine for the open workspace that runs `first`; the busy tab keeps going.
    fn open_tab(&self, inner: &mut Inner, first: Option<Command>) -> Result<(), String> {
        let root = inner
            .workspace
            .as_ref()
            .map(|w| PathBuf::from(&w.root))
            .ok_or("Open a folder first")?;
        let engine = self.spawn(&root).ok_or(PREVIEW_NOTICE)?;
        if let Some(command) = first {
            engine.send(command);
        }
        inner.tabs.open(Tab::new(Model::starting(), Some(engine)));
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
fn snapshot_of(inner: &Inner) -> Snapshot {
    let mut snapshot = Snapshot::new(
        &inner.tabs.active().model,
        inner.workspace.as_ref(),
        time::now(),
    );
    snapshot.show_activity(&inner.tabs.activity());
    snapshot
}

/// Fold queued engine updates into the models and publish the tab on screen.
fn publish(app: &AppHandle) {
    let shared = app.state::<Shared>();
    let (snapshot, models, settings, messages, closed) = {
        let mut inner = shared.lock();
        let tabs = &mut inner.tabs;
        let before = tabs.busy_states();
        let mut messages = Vec::new();
        for tab in tabs.iter_mut() {
            let updates = tab.engine.as_ref().map(Engine::drain).unwrap_or_default();
            for update in updates {
                let follow = tab.model.apply(update);
                dispatch(app, tab.engine.as_ref(), follow);
            }
            messages.extend(tab.model.toast.take());
        }
        messages.extend(tabs.background_news(&before));
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
        (snapshot_of(&inner), models, settings, messages, closed)
    };
    retire(closed);
    if let Some(models) = models {
        let _ = app.emit("models", models);
    }
    if let Some(settings) = settings {
        let _ = app.emit("settings", settings);
    }
    let _ = app.emit("snapshot", snapshot);
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
fn snapshot(shared: State<'_, Shared>) -> Snapshot {
    snapshot_of(&shared.lock())
}

#[tauri::command]
fn models(shared: State<'_, Shared>) -> Vec<ModelOptionView> {
    model_options(&shared.lock().tabs.active().model)
}

/// The settings page, once the engine sent both the reference and the saved values.
fn settings_of(model: &Model) -> Option<SettingsView> {
    Some(settings::view(
        model.settings_schema.as_deref()?,
        model.settings.as_ref()?,
    ))
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

#[tauri::command]
fn recent_workspaces(shared: State<'_, Shared>) -> Vec<Workspace> {
    let inner = shared.lock();
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

#[tauri::command]
fn open_workspace(shared: State<'_, Shared>, path: String) -> Result<(), String> {
    let path = PathBuf::from(&path);
    let path = path.canonicalize().unwrap_or(path);
    if !path.is_dir() {
        return Err(format!("{} is not a folder", path.display()));
    }
    shared.open(&path);
    Ok(())
}

#[tauri::command]
fn reconnect(shared: State<'_, Shared>) -> Result<(), String> {
    let root = shared
        .lock()
        .workspace
        .as_ref()
        .map(|w| PathBuf::from(&w.root));
    let root = root.ok_or("Open a folder first")?;
    shared.open(&root);
    Ok(())
}

#[tauri::command]
fn send_prompt(
    app: AppHandle,
    shared: State<'_, Shared>,
    text: String,
    images: Vec<String>,
    inline_images: Vec<String>,
) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() && images.is_empty() && inline_images.is_empty() {
        return Err("The prompt is empty".into());
    }
    attachments::check(&images, &inline_images)?;
    request(
        &app,
        &shared,
        Command::Send {
            text: text.to_owned(),
            images,
            inline_images,
        },
    );
    Ok(())
}

/// Check an image chosen or dropped by the user and return its preview. Async, so reading a
/// large file runs off the main thread.
#[tauri::command]
async fn read_image(path: String) -> Result<ImageFile, String> {
    attachments::read_image(Path::new(&path))
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
        Route::Here | Route::Switch(_) => {
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
fn resume(app: AppHandle, shared: State<'_, Shared>, session_id: String) -> Result<(), String> {
    let mut inner = shared.lock();
    match inner.tabs.route_resume(&session_id) {
        Route::Switch(index) => inner.tabs.switch(index),
        Route::Spawn => shared.open_tab(&mut inner, Some(Command::Resume(session_id)))?,
        Route::Here => dispatch(
            &app,
            inner.tabs.active().engine.as_ref(),
            vec![Command::Resume(session_id)],
        ),
    }
    drop(inner);
    shared.poke();
    Ok(())
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
fn undo_turn(app: AppHandle, shared: State<'_, Shared>) -> Result<(), String> {
    let inner = shared.lock();
    let tab = inner.tabs.active();
    let command = tab.model.undo()?;
    dispatch(&app, tab.engine.as_ref(), vec![command]);
    Ok(())
}

/// Add guidance to the turn running in the session on screen.
#[tauri::command]
fn steer(app: AppHandle, shared: State<'_, Shared>, text: String) -> Result<(), String> {
    let inner = shared.lock();
    let tab = inner.tabs.active();
    let command = tab.model.steer(&text)?;
    dispatch(&app, tab.engine.as_ref(), vec![command]);
    Ok(())
}

/// Send the last unanswered prompt of the session on screen again.
#[tauri::command]
fn retry_turn(app: AppHandle, shared: State<'_, Shared>) -> Result<(), String> {
    let inner = shared.lock();
    let tab = inner.tabs.active();
    let command = tab.model.retry()?;
    dispatch(&app, tab.engine.as_ref(), vec![command]);
    Ok(())
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

#[tauri::command]
fn refresh_sessions(app: AppHandle, shared: State<'_, Shared>) {
    request(&app, &shared, Command::ListSessions);
}

#[tauri::command]
fn refresh_models(app: AppHandle, shared: State<'_, Shared>) {
    request(&app, &shared, Command::ListModels);
}

#[tauri::command]
fn set_model(app: AppHandle, shared: State<'_, Shared>, provider: String, model_id: String) {
    request(&app, &shared, Command::SetModel { provider, model_id });
}

#[tauri::command]
fn set_thinking(app: AppHandle, shared: State<'_, Shared>, level: String) {
    request(&app, &shared, Command::SetThinking(level));
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

/// Open the window and run until it closes. With no `workspace` the UI asks for a folder.
pub fn run(source: Source, workspace: Option<PathBuf>, recents: Recents) -> tauri::Result<()> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok();

    let app = tauri::Builder::default()
        // Links in answers open in the system browser, never in the app window.
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            snapshot,
            models,
            settings,
            save_setting,
            reset_setting,
            recent_workspaces,
            open_workspace,
            reconnect,
            send_prompt,
            read_image,
            approve,
            abort,
            new_session,
            resume,
            delete_sessions,
            rename_session,
            pin_session,
            fork_session,
            export_session,
            undo_turn,
            steer,
            retry_turn,
            compact_session,
            refresh_sessions,
            refresh_models,
            set_model,
            set_thinking,
            set_approval,
            question_presented,
            answer_question
        ])
        .setup(move |app| {
            let (wake_tx, wake_rx) = mpsc::channel();
            app.manage(Shared {
                inner: Mutex::new(Inner {
                    tabs: Tabs::new(Tab::new(Model::default(), None)),
                    workspace: None,
                    recents,
                }),
                source,
                home,
                wake: Mutex::new(wake_tx),
            });
            if let Some(path) = workspace {
                app.state::<Shared>().open(&path);
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

    app.run(|app, event| {
        if let RunEvent::Exit = event
            && let Some(shared) = app.try_state::<Shared>()
        {
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
