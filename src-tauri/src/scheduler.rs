//! The app-owned schedule runner: `dial schedule run` for the open project as a plain child
//! process (no terminal), so saved schedules keep running while the project is open.
//!
//! One runner at a time (the app has one open project). It is restarted with backoff when it exits
//! on its own, given up on after repeated quick failures, and stopped (SIGINT first, Ctrl-Break on
//! Windows, so the engine can cancel its own `dial -p` children, then killed) when the project
//! closes or the app quits.
//! The engine holds a per-project lease (`schedule.lock`): a second scheduler fails fast with
//! [`ELSEWHERE`], which is reported as running elsewhere and never retried. The last
//! [`LOG_LINES`] output lines are kept for the UI. Every phase change goes to the sink (the app
//! emits it as the `scheduler` event).

use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Output lines kept per runner.
pub const LOG_LINES: usize = 200;
const LINE_CHARS: usize = 2000;
/// What the engine prints when another scheduler holds the project's lease.
pub const ELSEWHERE: &str = "already running for this project";

/// How the runner is restarted and stopped; tests shorten it.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// First restart delay; doubled per consecutive quick exit up to `max_delay`.
    pub base_delay: Duration,
    pub max_delay: Duration,
    /// A run at least this long counts as healthy: the failure count starts over.
    pub stable: Duration,
    /// Quick exits in a row before giving up.
    pub max_failures: u32,
    /// How long a stop waits after asking the scheduler to finish before killing it.
    pub grace: Duration,
    /// How often the supervisor checks the child and the stop flag.
    pub poll: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            base_delay: Duration::from_secs(2),
            max_delay: Duration::from_secs(120),
            stable: Duration::from_secs(60),
            max_failures: 6,
            grace: Duration::from_secs(3),
            poll: Duration::from_millis(100),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Stopped,
    Running,
    /// Exited on its own; waiting `retry_at_ms` to start again.
    Restarting,
    /// Gave up after repeated quick exits (or could not start).
    Failed,
    /// Another scheduler (a terminal, another window) holds this project's lease.
    Elsewhere,
}

impl Phase {
    fn active(self) -> bool {
        matches!(self, Self::Running | Self::Restarting)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Exit {
    /// `None` when killed by a signal or it never started.
    pub code: Option<i32>,
    pub at_ms: u64,
    pub ran_ms: u64,
}

/// What the UI shows for one project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub root: String,
    pub phase: Phase,
    pub pid: Option<u32>,
    pub started_at_ms: Option<u64>,
    /// Automatic restarts since it was started.
    pub restarts: u32,
    pub retry_at_ms: Option<u64>,
    pub last_exit: Option<Exit>,
    pub message: Option<String>,
    /// Start with the project; `None` until the user chose.
    pub background: Option<bool>,
    pub log: Vec<String>,
}

/// What to do after the runner exited without being asked to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    Restart { failures: u32, delay: Duration },
    GiveUp { phase: Phase, message: String },
}

/// Restart delay after `failures` (≥ 1) quick exits in a row.
pub fn backoff(timing: &Timing, failures: u32) -> Duration {
    let factor = 1u32 << failures.saturating_sub(1).min(16);
    timing.base_delay.saturating_mul(factor).min(timing.max_delay)
}

/// Decide what follows an unrequested exit after `ran`, given the quick exits before it.
pub fn after_exit(timing: &Timing, failures: u32, ran: Duration, elsewhere: bool) -> Next {
    if elsewhere {
        return Next::GiveUp {
            phase: Phase::Elsewhere,
            message: "Another scheduler is already running this project's schedules (a Terminal tab, another window or a shell).".into(),
        };
    }
    let failures = if ran >= timing.stable { 1 } else { failures + 1 };
    if failures > timing.max_failures {
        return Next::GiveUp {
            phase: Phase::Failed,
            message: format!(
                "The scheduler exited {} times in a row; see the log.",
                timing.max_failures
            ),
        };
    }
    Next::Restart {
        failures,
        delay: backoff(timing, failures),
    }
}

/// The last `cap` lines, each at most [`LINE_CHARS`] characters.
#[derive(Debug, Default)]
pub struct Log {
    lines: VecDeque<String>,
}

impl Log {
    pub fn push(&mut self, line: &str) {
        let line = line.trim_end_matches(['\r', '\n']);
        let line = match line.char_indices().nth(LINE_CHARS) {
            Some((end, _)) => format!("{}…", &line[..end]),
            None => line.to_owned(),
        };
        if self.lines.len() == LOG_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    pub fn lines(&self) -> Vec<String> {
        self.lines.iter().cloned().collect()
    }
}

/// Per-project "run schedules in the background" choices, kept in a small JSON file.
#[derive(Debug, Default)]
struct Prefs {
    file: Option<PathBuf>,
    loaded: Option<BTreeMap<String, bool>>,
}

impl Prefs {
    fn map(&mut self) -> &mut BTreeMap<String, bool> {
        let file = self.file.as_deref();
        self.loaded.get_or_insert_with(|| {
            file.and_then(|f| std::fs::read(f).ok())
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default()
        })
    }

    fn get(&mut self, root: &str) -> Option<bool> {
        self.map().get(root).copied()
    }

    fn set(&mut self, root: &str, on: bool) -> Result<(), String> {
        self.map().insert(root.to_owned(), on);
        let Some(file) = &self.file else {
            return Ok(());
        };
        let text = serde_json::to_vec_pretty(self.loaded.as_ref().expect("loaded above"))
            .map_err(|e| e.to_string())?;
        let save = || -> std::io::Result<()> {
            if let Some(dir) = file.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = file.with_extension("json.tmp");
            std::fs::write(&tmp, text)?;
            std::fs::rename(&tmp, file)
        };
        save().map_err(|e| format!("Could not save the background schedules choice: {e}"))
    }
}

struct Run {
    id: u64,
    root: String,
    program: PathBuf,
    stop: Arc<AtomicBool>,
    phase: Phase,
    pid: Option<u32>,
    started_at_ms: Option<u64>,
    restarts: u32,
    retry_at_ms: Option<u64>,
    last_exit: Option<Exit>,
    message: Option<String>,
    log: Log,
    /// This attempt printed [`ELSEWHERE`].
    elsewhere: bool,
    supervisor: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct State {
    run: Option<Run>,
    next_id: u64,
    prefs: Prefs,
}

type Sink = dyn Fn(Status) + Send + Sync;

struct Core {
    state: Mutex<State>,
    sink: Box<Sink>,
    timing: Timing,
}

/// The app's background scheduler; cheap to clone.
#[derive(Clone)]
pub struct Scheduler {
    core: Arc<Core>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

impl Core {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn status_of(state: &mut State, root: &str) -> Status {
        let background = state.prefs.get(root);
        match state.run.as_ref().filter(|r| r.root == root) {
            Some(run) => Status {
                root: root.to_owned(),
                phase: run.phase,
                pid: run.pid,
                started_at_ms: run.started_at_ms,
                restarts: run.restarts,
                retry_at_ms: run.retry_at_ms,
                last_exit: run.last_exit.clone(),
                message: run.message.clone(),
                background,
                log: run.log.lines(),
            },
            None => Status {
                root: root.to_owned(),
                phase: Phase::Stopped,
                pid: None,
                started_at_ms: None,
                restarts: 0,
                retry_at_ms: None,
                last_exit: None,
                message: None,
                background,
                log: Vec::new(),
            },
        }
    }

    /// Change run `id` (if it is still the current one) and report the result.
    fn update(&self, id: u64, change: impl FnOnce(&mut Run)) -> bool {
        let status = {
            let mut state = self.lock();
            let Some(run) = state.run.as_mut().filter(|r| r.id == id) else {
                return false;
            };
            change(run);
            let root = run.root.clone();
            Self::status_of(&mut state, &root)
        };
        (self.sink)(status);
        true
    }

    fn log(&self, id: u64, line: &str) {
        let mut state = self.lock();
        if let Some(run) = state.run.as_mut().filter(|r| r.id == id) {
            run.elsewhere |= line.contains(ELSEWHERE);
            run.log.push(line);
        }
    }
}

impl Scheduler {
    /// `prefs` is where background choices are saved (`None` keeps them in memory).
    pub fn new(prefs: Option<PathBuf>, sink: impl Fn(Status) + Send + Sync + 'static) -> Self {
        Self::with_timing(prefs, Timing::default(), sink)
    }

    pub fn with_timing(
        prefs: Option<PathBuf>,
        timing: Timing,
        sink: impl Fn(Status) + Send + Sync + 'static,
    ) -> Self {
        let state = State {
            prefs: Prefs {
                file: prefs,
                loaded: None,
            },
            ..State::default()
        };
        Self {
            core: Arc::new(Core {
                state: Mutex::new(state),
                sink: Box::new(sink),
                timing,
            }),
        }
    }

    pub fn status(&self, root: &str) -> Status {
        Core::status_of(&mut self.core.lock(), root)
    }

    pub(crate) fn running(&self) -> bool {
        self.core.lock().run.as_ref().is_some_and(|run| run.phase.active())
    }

    /// Remember whether `root` runs its schedules whenever it is open.
    pub fn set_background(&self, root: &str, on: bool) -> Result<(), String> {
        self.core.lock().prefs.set(root, on)
    }

    /// Run `program schedule run` in `root`. Already running there is fine (never a second one);
    /// a runner for another project is stopped first.
    pub fn start(&self, root: &str, program: &Path) -> Status {
        let previous = {
            let mut state = self.core.lock();
            if let Some(run) = state.run.as_ref()
                && run.root == root
                && run.phase.active()
                && !run.stop.load(Ordering::Acquire)
            {
                return Core::status_of(&mut state, root);
            }
            state.run.as_mut().map(|run| {
                run.stop.store(true, Ordering::Release);
                (run.root == root, run.supervisor.take())
            })
        };
        // The old child must release this project's lease before a new one asks for it; another
        // project's runner finishes stopping on its own.
        if let Some((true, Some(handle))) = previous {
            let _ = handle.join();
        }
        let stop = Arc::new(AtomicBool::new(false));
        let id = {
            let mut state = self.core.lock();
            state.next_id += 1;
            let id = state.next_id;
            state.run = Some(Run {
                id,
                root: root.to_owned(),
                program: program.to_owned(),
                stop: Arc::clone(&stop),
                phase: Phase::Running,
                pid: None,
                started_at_ms: None,
                restarts: 0,
                retry_at_ms: None,
                last_exit: None,
                message: None,
                log: Log::default(),
                elsewhere: false,
                supervisor: None,
            });
            id
        };
        let core = Arc::clone(&self.core);
        let spawned = thread::Builder::new()
            .name("dial-scheduler".into())
            .spawn(move || supervise(&core, id, &stop));
        let mut state = self.core.lock();
        match spawned {
            Ok(handle) => {
                if let Some(run) = state.run.as_mut().filter(|r| r.id == id) {
                    run.supervisor = Some(handle);
                }
            }
            Err(e) => {
                if let Some(run) = state.run.as_mut().filter(|r| r.id == id) {
                    run.phase = Phase::Failed;
                    run.message = Some(format!("Could not start the scheduler: {e}"));
                }
            }
        }
        Core::status_of(&mut state, root)
    }

    /// Stop the runner for `root` (no-op if none); `wait` blocks until it has exited.
    pub fn stop(&self, root: &str, wait: bool) -> Status {
        let handle = {
            let mut state = self.core.lock();
            match state.run.as_mut().filter(|r| r.root == root) {
                Some(run) => {
                    run.stop.store(true, Ordering::Release);
                    // Not waiting leaves the handle, so a later start here still waits for it.
                    if wait { run.supervisor.take() } else { None }
                }
                None => None,
            }
        };
        if let Some(handle) = handle {
            let _ = handle.join();
        }
        self.status(root)
    }

    /// Stop whatever runs; `wait` bounds the app's exit by [`Timing::grace`].
    pub fn stop_all(&self, wait: bool) {
        let root = self.core.lock().run.as_ref().map(|r| r.root.clone());
        if let Some(root) = root {
            self.stop(&root, wait);
        }
    }

    /// `root` is now the open project: stop another project's runner and start this one's if
    /// the user chose to run it in the background. `program` is `None` in the design preview.
    pub fn project_opened(&self, root: &str, program: Option<&Path>) {
        let other = self
            .core
            .lock()
            .run
            .as_ref()
            .filter(|r| r.root != root)
            .map(|r| r.root.clone());
        if let Some(other) = other {
            self.stop(&other, false);
        }
        let wanted = self.core.lock().prefs.get(root) == Some(true);
        if let (true, Some(program)) = (wanted, program) {
            self.start(root, program);
        }
    }
}

fn spawn(program: &Path, root: &str) -> std::io::Result<Child> {
    let mut command = Command::new(program);
    crate::launcher::outside_appimage(&mut command);
    command
        .args(["schedule", "run"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Its own process group: signals meant for the app (a Ctrl-C in a dev terminal) stay ours,
    // and on Windows the group's id is the child's, so [`ask_to_finish`] can address it alone.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    command.spawn()
}

/// Ask the scheduler to finish (it cancels its runs on SIGINT, or Ctrl-Break on Windows); kill
/// it after `grace`. Returns its exit code (`None` when it had to be killed).
fn end(child: &mut Child, grace: Duration, poll: Duration) -> Option<i32> {
    if ask_to_finish(child) {
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            if let Ok(Some(status)) = child.try_wait() {
                return status.code();
            }
            thread::sleep(poll);
        }
    }
    let _ = child.kill();
    child.wait().ok().and_then(|status| status.code())
}

/// Send the scheduler its cooperative-stop request; `false` when it could not be sent.
#[cfg(unix)]
fn ask_to_finish(child: &Child) -> bool {
    let Ok(pid) = libc::pid_t::try_from(child.id()) else {
        return false;
    };
    // SAFETY: signalling our own, not yet reaped, child process.
    unsafe { libc::kill(pid, libc::SIGINT) == 0 }
}

/// Ctrl-Break reaches only processes on the sender's console, and the app (a GUI program) has
/// none: attach to the child's hidden one just to send it to the child's own process group.
#[cfg(windows)]
fn ask_to_finish(child: &Child) -> bool {
    use windows::Win32::System::Console::{
        AttachConsole, CTRL_BREAK_EVENT, FreeConsole, GenerateConsoleCtrlEvent,
    };
    let pid = child.id();
    // SAFETY: scalar arguments only. `pid` is our own unreaped child, which leads its process
    // group; the app is not in that group, so the event never reaches the app itself.
    unsafe {
        if AttachConsole(pid).is_err() {
            return false;
        }
        let sent = GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid).is_ok();
        let _ = FreeConsole();
        sent
    }
}

#[cfg(not(any(unix, windows)))]
fn ask_to_finish(_: &Child) -> bool {
    false
}

fn read_lines(core: Arc<Core>, id: u64, source: impl Read + Send + 'static) {
    let _ = thread::Builder::new()
        .name("dial-scheduler-log".into())
        .spawn(move || {
            let mut reader = BufReader::new(source);
            let mut buf = Vec::new();
            while matches!(reader.read_until(b'\n', &mut buf), Ok(n) if n > 0) {
                core.log(id, &String::from_utf8_lossy(&buf));
                buf.clear();
            }
        });
}

/// Sleep `total` in `poll` slices; `true` if a stop came in meanwhile.
fn sleep_unless_stopped(stop: &AtomicBool, total: Duration, poll: Duration) -> bool {
    let deadline = Instant::now() + total;
    while Instant::now() < deadline {
        if stop.load(Ordering::Acquire) {
            return true;
        }
        thread::sleep(poll.min(deadline.saturating_duration_since(Instant::now())));
    }
    stop.load(Ordering::Acquire)
}

fn supervise(core: &Arc<Core>, id: u64, stop: &AtomicBool) {
    let timing = core.timing;
    let mut failures = 0;
    loop {
        let Some((root, program)) = core
            .lock()
            .run
            .as_ref()
            .filter(|r| r.id == id)
            .map(|r| (r.root.clone(), r.program.clone()))
        else {
            return;
        };
        let began = Instant::now();
        let code = match spawn(&program, &root) {
            Err(e) => {
                core.log(id, &format!("Could not start {}: {e}", program.display()));
                None
            }
            Ok(mut child) => {
                if let Some(out) = child.stdout.take() {
                    read_lines(Arc::clone(core), id, out);
                }
                if let Some(err) = child.stderr.take() {
                    read_lines(Arc::clone(core), id, err);
                }
                let pid = child.id();
                core.update(id, |run| {
                    run.phase = Phase::Running;
                    run.pid = Some(pid);
                    run.started_at_ms = Some(now_ms());
                    run.retry_at_ms = None;
                    run.elsewhere = false;
                });
                loop {
                    if stop.load(Ordering::Acquire) {
                        break end(&mut child, timing.grace, timing.poll);
                    }
                    match child.try_wait() {
                        Ok(Some(status)) => break status.code(),
                        Ok(None) => thread::sleep(timing.poll),
                        Err(_) => {
                            break end(&mut child, Duration::ZERO, timing.poll);
                        }
                    }
                }
            }
        };
        let ran = began.elapsed();
        let exit = Exit {
            code,
            at_ms: now_ms(),
            ran_ms: u64::try_from(ran.as_millis()).unwrap_or(u64::MAX),
        };
        if stop.load(Ordering::Acquire) {
            core.update(id, |run| {
                run.phase = Phase::Stopped;
                run.pid = None;
                run.retry_at_ms = None;
                run.last_exit = Some(exit);
            });
            return;
        }
        // Let the log readers catch the last lines (the lease message) before deciding.
        thread::sleep(timing.poll);
        let elsewhere = core
            .lock()
            .run
            .as_ref()
            .is_some_and(|r| r.id == id && r.elsewhere);
        match after_exit(&timing, failures, ran, elsewhere) {
            Next::GiveUp { phase, message } => {
                core.update(id, |run| {
                    run.phase = phase;
                    run.pid = None;
                    run.retry_at_ms = None;
                    run.last_exit = Some(exit);
                    run.message = Some(message);
                });
                return;
            }
            Next::Restart { failures: f, delay } => {
                failures = f;
                let retry_at = now_ms().saturating_add(u64::try_from(delay.as_millis()).unwrap_or(0));
                let current = core.update(id, |run| {
                    run.phase = Phase::Restarting;
                    run.pid = None;
                    run.retry_at_ms = Some(retry_at);
                    run.last_exit = Some(exit);
                });
                if !current {
                    return;
                }
                if sleep_unless_stopped(stop, delay, timing.poll) {
                    core.update(id, |run| {
                        run.phase = Phase::Stopped;
                        run.retry_at_ms = None;
                    });
                    return;
                }
                core.update(id, |run| run.restarts += 1);
            }
        }
    }
}

pub mod service;

#[cfg(test)]
mod tests;
