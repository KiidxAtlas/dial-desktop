//! One app console per workspace, exposing guest frames and scoped input to the sidebar.
//!
//! Dial's private guests have disposable writes, no network and no shares except an optional
//! read-only project snapshot. Registered provider VMs ([`installed`]) instead retain their own
//! storage and device settings. Both use guest input only; neither acts on the host desktop.
//!
//! Scope: every command first checks that the workspace is the project open in the window. Every
//! command that names a run requires its exact run id, and input requires the frame it was based on.
//! The agent's input also needs the single-use token from its latest screenshot (see [`control`]).
//!
//! Locking, in one order only: a run's `channel` (its QMP connection) is taken before the runs
//! table, and the table is never held across QMP I/O. Capture and frame recording happen under the
//! channel, so a frame is recorded exactly once and in order. A run's child process has its own
//! lock, which Stop takes without the channel, so Stop ends a guest whose handshake is stuck.
//!
//! Commands are `async` and run their work on a blocking thread, so nothing waits on the UI thread.
//! Start returns the run id as soon as the guest process exists; the handshake finishes in the
//! background and reports `running` or `failed` as a status event.

use std::collections::hash_map::RandomState;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::hash::{BuildHasher, Hasher};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::agent_browser::Reply;

pub mod actions;
pub mod agent;
pub mod control;
pub mod input;
pub(crate) mod host;
pub(crate) mod installed;
pub mod macos;
pub mod project;
pub mod qemu;
pub mod qmp;
pub mod runtime;
pub mod rfb;
pub mod screen;
pub mod table;

#[cfg(test)]
mod tests;

use control::Ledger;
use input::{Command as QmpCommand, Frame, Gesture, Held, Low};
use qemu::{Accel, Launch};
use qmp::Qmp;
use runtime::{Arch, Backend, GuestTarget, RuntimeError};
use screen::{Budget, MAX_CAPTURE_BYTES, Screen, read_capped};
use table::{HasRunId, Runs};

/// The event channel for frames and status.
pub const EVENT: &str = "app_runner";
/// The event that asks the window to show the App panel after an agent's explicit start. It names
/// only the project, is sent once per start attempt, and never carries frames.
pub const REVEAL_EVENT: &str = "app_runner_reveal";
/// Poll as fast as the capture allows during interaction and motion (one display refresh between
/// captures); stay responsive to guest-side changes when idle without continuous full-screen I/O.
const FRAME_INTERVAL: Duration = Duration::from_millis(16);
const IDLE_INTERVAL: Duration = Duration::from_millis(150);
const ACTIVE_PERIOD: Duration = Duration::from_secs(5);
/// Guest memory and vCPUs for an app run; small on purpose.
const MEMORY_MB: u32 = 2048;
const CPUS: u32 = 2;
/// Windows: no console window for the guest process.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

type LiveQmp = Qmp<BufReader<ChildStdout>, ChildStdin>;

/// A refusal, with the stable code the UI and the agent match on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppRunError {
    pub code: &'static str,
    pub message: String,
}

impl AppRunError {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        AppRunError {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Starting,
    Running,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Controller {
    User,
    Agent,
}

/// Who started a run. The user's start leaves control with the user. The agent's start gives the
/// agent control at once. Only the user's start may share the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    User,
    Agent,
}

/// What a run looks like to the UI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInfo {
    pub run_id: String,
    pub workspace_root: String,
    pub title: String,
    pub status: Status,
    pub controller: Controller,
    pub origin: Origin,
    /// The guest disk this run booted. It names the disk, not a verified guest OS.
    pub guest_target: GuestTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vm_id: Option<String>,
}

/// The answer to `app_runner_state`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateView {
    pub status: &'static str,
    /// The guest target the state was asked for. `status` and `message` describe this target.
    pub guest_target: GuestTarget,
    /// The target of the run that is open for the workspace, when it differs from `guest_target`.
    /// The run itself is not in `run`; a UI follows the open guest from this field, not the text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_guest_target: Option<GuestTarget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approved_vm_id: Option<String>,
    /// The SSH host the agent may use for commands in this VM: the open VM run's while Agent
    /// access is on, or the VM the user allowed the agent to launch. Absent otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh: Option<installed::ssh::SshLink>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<RunInfo>,
}

/// The newest frame of a run, for a pane that reattaches.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameView {
    pub run_id: String,
    pub workspace_root: String,
    pub seq: u64,
    pub width: u32,
    pub height: u32,
    pub mime: &'static str,
    pub data: String,
}

/// What the agent gets from a screenshot: the frame, and the token that authorizes one input on it.
#[derive(Debug, Clone)]
pub struct AgentShot {
    pub frame: Frame,
    pub run_id: String,
    pub seq: u64,
    pub token: Option<u128>,
    pub controller: Controller,
}

/// The QMP connection of one run. Held for every operation that talks to the guest.
#[derive(Default)]
struct Channel {
    qmp: Option<LiveQmp>,
    installed: Option<installed::Console>,
    pump: Option<thread::Thread>,
    /// The size of the last captured screen, which a registered VM's gestures are scaled against.
    screen_size: Option<(u32, u32)>,
    /// The QEMU guest's display over VNC, when it serves one; screendumps are the fallback.
    #[cfg(unix)]
    vnc: Option<rfb::Viewer<std::os::unix::net::UnixStream>>,
    last_input: Option<Instant>,
    /// What the UI's live input holds down in the guest.
    held: Held,
    /// The guest takes only whole clicks and presses (the virtual Mac's companion).
    whole_gestures: bool,
}

/// One guest, owned by the runs table.
struct Run {
    run_id: String,
    workspace: String,
    title: String,
    status: Status,
    origin: Origin,
    guest_target: GuestTarget,
    vm_id: Option<String>,
    ledger: Ledger,
    screen: Screen,
    child: Option<Arc<Mutex<Child>>>,
    channel: Arc<Mutex<Channel>>,
    /// The run's private directory; removed with the run.
    dir: PathBuf,
    _tempdir: tempfile::TempDir,
}

impl HasRunId for Run {
    fn run_id(&self) -> &str {
        &self.run_id
    }
}

impl Run {
    fn info(&self) -> RunInfo {
        RunInfo {
            run_id: self.run_id.clone(),
            workspace_root: self.workspace.clone(),
            title: self.title.clone(),
            status: self.status,
            controller: self.ledger.controller(),
            origin: self.origin,
            guest_target: self.guest_target,
            vm_id: self.vm_id.clone(),
        }
    }
}

type Table = Runs<Run>;

struct Inner {
    table: Mutex<Table>,
    vm_grants: Mutex<installed::Grants>,
    app: AppHandle,
    resource_dir: PathBuf,
    data_dir: PathBuf,
}

/// The managed state. Cheap to clone; clones share the same runs.
#[derive(Clone)]
pub struct AppRunner {
    inner: Arc<Inner>,
}

/// What a recorded frame changed.
struct Observed {
    seq: u64,
    changed: bool,
}

static NEXT_RUN: AtomicU64 = AtomicU64::new(1);

impl AppRunner {
    pub fn new(app: AppHandle, resource_dir: PathBuf, data_dir: PathBuf) -> Self {
        AppRunner {
            inner: Arc::new(Inner {
                table: Mutex::new(Table::default()),
                vm_grants: Mutex::default(),
                app,
                resource_dir,
                data_dir,
            }),
        }
    }

    fn table(&self) -> MutexGuard<'_, Table> {
        lock(&self.inner.table)
    }

    /// The state for `workspace` and `guest`: whether that target's runtime and disk are ready, and
    /// the run if one exists for exactly that target. A run for another target is named in the
    /// message, because one run is open per workspace.
    pub fn state(&self, workspace: &str, guest: GuestTarget) -> StateView {
        let open = self
            .table()
            .get(workspace)
            .map(|run| (run.guest_target, run.info()));
        if let Some((_, info)) = open.as_ref().filter(|(target, _)| *target == guest) {
            // The VM's SSH host is shown only while the agent may use the VM.
            let ssh = match (&info.vm_id, info.controller) {
                (Some(id), Controller::Agent) => self.vm_ssh(id, Some(&info.title)),
                _ => None,
            };
            return StateView {
                status: status_name(info.status),
                guest_target: guest,
                open_guest_target: None,
                approved_vm_id: None,
                ssh,
                runtime: None,
                message: None,
                run: Some(info.clone()),
            };
        }
        let open_other = open.map(|(target, _)| target);
        if guest == GuestTarget::Vm {
            let mut view = state_view(guest, open_other, Ok(()));
            view.approved_vm_id = lock(&self.inner.vm_grants)
                .get(workspace)
                .map(str::to_owned);
            view.ssh = view
                .approved_vm_id
                .as_deref()
                .and_then(|id| self.vm_ssh(id, None));
            return view;
        }
        state_view(guest, open_other, self.check_runtime(guest).map(|_| ()))
    }

    /// The backend for `guest` and its checked files. `macos` uses the macOS helper only; every other
    /// target uses QEMU. A refused backend never falls back to the other.
    fn check_runtime(&self, guest: GuestTarget) -> Result<Plan, RuntimeError> {
        let resource = &self.inner.resource_dir;
        let data = &self.inner.data_dir;
        match runtime::backend_for(guest, runtime::host_is_apple_silicon())? {
            Backend::MacOs => macos::discover(resource, data).map(Plan::MacOs),
            Backend::Qemu => {
                let arch = Arch::host().ok_or(RuntimeError::Missing(runtime::QEMU))?;
                let target = runtime::host_target().ok_or_else(|| {
                    RuntimeError::Refused("this platform has no app runner".into())
                })?;
                runtime::discover_for(resource, data, arch, &target, guest).map(Plan::Qemu)
            }
        }
    }

    /// Start the guest for `workspace` and `guest` and return at once, as `starting`. One run per
    /// workspace.
    pub fn start(
        &self,
        workspace: &str,
        share_project: bool,
        origin: Origin,
        guest: GuestTarget,
    ) -> Result<RunInfo, AppRunError> {
        let root = Path::new(workspace);
        if !root.is_dir() {
            return Err(AppRunError::new(
                "stale_workspace",
                "The workspace folder is no longer available.",
            ));
        }
        if let Some(run) = self.table().get(workspace) {
            if origin == Origin::Agent && run.ledger.taken_over() {
                return Err(AppRunError::new(
                    "taken_over",
                    "The user took control of the app; the agent cannot start another run to regain it.",
                ));
            }
            return Err(AppRunError::new(
                "already_running",
                format!(
                    "A run is already open for this workspace (run {}).",
                    run.run_id
                ),
            ));
        }
        let plan = self.check_runtime(guest).map_err(runtime_error)?;

        let runs_root = self.inner.data_dir.join("app-runner").join("runs");
        fs::create_dir_all(&runs_root).map_err(|e| start_failed(&e.to_string()))?;
        let tempdir = private_run_dir(&runs_root).map_err(|e| start_failed(&e.to_string()))?;
        let dir = tempdir.path().to_path_buf();
        let vnc = match &plan {
            Plan::Qemu(_) => vnc_socket_for(&dir),
            Plan::MacOs(_) => None,
        };
        let mut command = match &plan {
            Plan::Qemu(runtime) => qemu_command(runtime, &dir, root, share_project, vnc.as_deref())?,
            Plan::MacOs(mac) => macos_command(mac, &dir, root, share_project)?,
        };
        let mut child = command
            .spawn()
            .map_err(|e| start_failed(&format!("could not start the guest: {e}")))?;
        let (stdin, stdout) = match (child.stdin.take(), child.stdout.take()) {
            (Some(stdin), Some(stdout)) => (stdin, stdout),
            _ => {
                let _ = child.kill();
                return Err(start_failed("the guest has no control channel"));
            }
        };

        let run_id = format!("run-{}", NEXT_RUN.fetch_add(1, Ordering::Relaxed));
        let title = root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("App")
            .to_owned();
        let child = Arc::new(Mutex::new(child));
        let channel = Arc::new(Mutex::new(Channel {
            whole_gestures: guest == GuestTarget::Macos,
            ..Channel::default()
        }));
        let ledger = match origin {
            Origin::User => Ledger::default(),
            Origin::Agent => Ledger::agent_launch(),
        };
        let run = Run {
            run_id: run_id.clone(),
            workspace: workspace.to_owned(),
            title,
            status: Status::Starting,
            origin,
            guest_target: guest,
            vm_id: None,
            ledger,
            screen: Screen::default(),
            child: Some(Arc::clone(&child)),
            channel: Arc::clone(&channel),
            dir,
            _tempdir: tempdir,
        };
        let info = run.info();
        if self.table().insert(workspace, run).is_err() {
            kill(&child);
            return Err(AppRunError::new(
                "already_running",
                "A run is already open for this workspace.",
            ));
        }
        self.emit_status(&info, None);

        let runner = self.clone();
        let workspace = workspace.to_owned();
        thread::Builder::new()
            .name("dial-app-runner-handshake".into())
            .spawn(move || runner.handshake(&workspace, &run_id, &channel, stdin, stdout, vnc))
            .map_err(|e| start_failed(&e.to_string()))?;
        Ok(info)
    }

    /// The agent's launch: an unshared guest the agent controls from its first frame. Refused while
    /// any run is open for the workspace; a run the user took over is never handed back this way.
    pub fn start_for_agent(
        &self,
        workspace: &str,
        guest: GuestTarget,
    ) -> Result<RunInfo, AppRunError> {
        self.start(workspace, false, Origin::Agent, guest)
    }

    /// Finish starting: the QMP handshake, the VNC display when QEMU serves one, then `running` (or
    /// `failed`). Off the command's thread.
    fn handshake(
        &self,
        workspace: &str,
        run_id: &str,
        channel: &Arc<Mutex<Channel>>,
        stdin: ChildStdin,
        stdout: ChildStdout,
        vnc: Option<PathBuf>,
    ) {
        let connected = {
            let mut slot = lock(channel);
            Qmp::connect(BufReader::new(stdout), stdin).map(|qmp| {
                slot.qmp = Some(qmp);
                attach_vnc(&mut slot, vnc.as_deref());
            })
        };
        match connected {
            Ok(()) => {
                let info = {
                    let mut table = self.table();
                    table.exact(workspace, run_id).ok().map(|run| {
                        run.status = Status::Running;
                        run.info()
                    })
                };
                if let Some(info) = info {
                    self.emit_status(&info, None);
                    self.spawn_pump(workspace.to_owned(), run_id.to_owned());
                }
            }
            Err(error) => {
                self.fail(
                    workspace,
                    run_id,
                    &format!("The guest did not start: {error}"),
                );
            }
        }
    }

    /// Stop a private guest or disconnect a provider console. Never power off a registered VM.
    pub fn stop(&self, workspace: &str, run_id: &str) -> Result<(), AppRunError> {
        // A provider VM is persistent. Wait for any in-flight launch/gesture, then close its
        // console before allowing another workspace to acquire it. QMP children can be killed
        // without waiting on their channel (which may be blocked reading that child).
        let vm_channel = {
            let mut table = self.table();
            let run = table.exact(workspace, run_id)?;
            run.vm_id.as_ref().map(|_| Arc::clone(&run.channel))
        };
        let mut vm_slot = vm_channel.as_ref().map(|channel| lock(channel));
        if let Some(slot) = &mut vm_slot {
            slot.installed.take();
        }
        let run = {
            let mut table = self.table();
            table.exact(workspace, run_id)?;
            table.remove_exact(workspace, run_id)
        };
        if let Some(mut run) = run {
            run.status = Status::Stopped;
            if let Some(child) = &run.child {
                kill(child);
            }
            let message = if run.vm_id.is_some() {
                "VM disconnected; Dial did not power it off."
            } else {
                "The app run was stopped."
            };
            self.emit_status(&run.info(), Some(message.into()));
        }
        Ok(())
    }

    /// Remove the run for `run_id` only if it is still the current one, mark it failed and kill it.
    /// Reports only when this call did the removal; a run already stopped reports nothing.
    fn fail(&self, workspace: &str, run_id: &str, message: &str) {
        let removed = self.table().remove_exact(workspace, run_id);
        if let Some(mut run) = removed {
            run.status = Status::Failed;
            if let Some(child) = &run.child {
                kill(child);
            }
            self.emit_status(&run.info(), Some(message.to_owned()));
        }
    }

    pub(crate) fn open_count(&self) -> usize {
        self.table().entries().count()
    }

    /// Stop every run (called when the app exits).
    pub fn stop_all(&self) {
        let runs = self.table().drain();
        for run in runs {
            if let Some(child) = &run.child {
                kill(child);
            }
        }
    }

    /// The newest frame of the run as a PNG, for a pane that reattaches.
    pub fn frame(&self, workspace: &str, run_id: &str) -> Result<Option<FrameView>, AppRunError> {
        let (frame, seq, run_id, workspace) = {
            let mut table = self.table();
            let run = table.exact(workspace, run_id)?;
            let Some(frame) = run.screen.frame.clone() else {
                return Ok(None);
            };
            (frame, run.ledger.seq(), run.run_id.clone(), run.workspace.clone())
        };
        let png = frame.png().map_err(|e| AppRunError::new("capture_failed", e))?;
        Ok(Some(FrameView {
            run_id,
            workspace_root: workspace,
            seq,
            width: frame.width,
            height: frame.height,
            mime: "image/png",
            data: base64::engine::general_purpose::STANDARD.encode(png),
        }))
    }

    /// What changed on the live screen since the frame the window shows (`since`; `None` for a
    /// window that shows nothing), as raw RGBA rectangles (see [`screen::Plan::packet`]). Empty
    /// until the guest has a screen. The pixels are copied outside the runs table.
    pub fn frame_bytes(
        &self,
        workspace: &str,
        run_id: &str,
        since: Option<u64>,
    ) -> Result<Vec<u8>, AppRunError> {
        let plan = {
            let mut table = self.table();
            let run = table.exact(workspace, run_id)?;
            run.screen.plan(since, run.ledger.seq())
        };
        Ok(plan.map(|plan| plan.packet()).unwrap_or_default())
    }

    /// The UI's input. Names a frame already shown; takes control for the user.
    pub fn ui_input(
        &self,
        workspace: &str,
        run_id: &str,
        frame_seq: u64,
        gesture: &Gesture,
    ) -> Result<(), AppRunError> {
        let channel = self.channel_of(workspace, run_id)?;
        let mut channel = lock(&channel);
        if !gesture.is_atomic() {
            return self.ui_stream(&mut channel, workspace, run_id, frame_seq, gesture);
        }
        let command = {
            let mut table = self.table();
            let run = table.exact(workspace, run_id)?;
            run.ledger.check_ui_frame(frame_seq)?;
            let command = gesture
                .command(run.screen.width, run.screen.height)
                .map_err(unsupported)?;
            run.ledger.user_acted();
            command
        };
        send_gesture(&mut channel, gesture, &command)
    }

    /// The UI's live input (moves, held buttons and keys, wheel, release), under the run's channel.
    /// The pointer only passing over the screen while the agent has access neither moves the
    /// guest's pointer nor voids the agent's frame; anything else does void it. Who may act is the
    /// Agent access switch's decision ([`Self::set_agent_access`]), not a side effect of input.
    fn ui_stream(
        &self,
        channel: &mut Channel,
        workspace: &str,
        run_id: &str,
        frame_seq: u64,
        gesture: &Gesture,
    ) -> Result<(), AppRunError> {
        let passive = match gesture {
            Gesture::Pointer { buttons, .. } => buttons & 7 == 0 && !channel.held.any(),
            Gesture::Release => !channel.held.any(),
            _ => false,
        };
        let (width, height) = {
            let mut table = self.table();
            let run = table.exact(workspace, run_id)?;
            run.ledger.check_ui_frame(frame_seq)?;
            if passive && (run.ledger.controller() == Controller::Agent || matches!(gesture, Gesture::Release)) {
                return Ok(());
            }
            run.ledger.user_acted();
            (run.screen.width, run.screen.height)
        };
        let steps = channel.held.apply(gesture, width, height).map_err(unsupported)?;
        send_stream(channel, &steps, width, height)
    }

    /// The App panel's Agent access switch. On: the agent may view the screen and send input. Off:
    /// it may do neither until the user turns it on again. The only way access changes.
    pub fn set_agent_access(
        &self,
        workspace: &str,
        run_id: &str,
        allowed: bool,
    ) -> Result<RunInfo, AppRunError> {
        if allowed {
            return self.resume(workspace, run_id);
        }
        let info = {
            let mut table = self.table();
            let run = table.exact(workspace, run_id)?;
            run.ledger.take_over();
            run.info()
        };
        self.emit_status(&info, None);
        Ok(info)
    }

    /// Hand control to the agent (Agent access on).
    pub fn resume(&self, workspace: &str, run_id: &str) -> Result<RunInfo, AppRunError> {
        let channel = self.channel_of(workspace, run_id)?;
        let mut channel = lock(&channel);
        let (info, width, height) = {
            let mut table = self.table();
            let run = table.exact(workspace, run_id)?;
            run.ledger.resume();
            (run.info(), run.screen.width, run.screen.height)
        };
        // Nothing the user held stays down for the agent.
        let steps = channel.held.release();
        let _ = send_stream(&mut channel, &steps, width, height);
        drop(channel);
        self.emit_status(&info, None);
        Ok(info)
    }

    /// The agent's frame for the exact run `run_id`: a fresh capture, recorded if it changed, with
    /// the token that authorizes one input on it. The token is issued only when the agent holds
    /// control. A wrong or missing run id is refused; nothing is substituted.
    pub fn agent_screenshot(
        &self,
        workspace: &str,
        run_id: &str,
    ) -> Result<AgentShot, AppRunError> {
        let channel = self.channel_of(workspace, run_id)?;
        let mut channel = lock(&channel);
        let dir = {
            let mut table = self.table();
            let run = table.exact(workspace, run_id)?;
            installed::check_view(run.guest_target, run.ledger.controller())?;
            run.dir.clone()
        };
        let frame =
            capture(&mut channel, &dir).map_err(|e| AppRunError::new("capture_failed", e))?;
        let (observed, token, controller) = {
            let mut table = self.table();
            let run = table.exact(workspace, run_id)?;
            let observed = record(run, &frame);
            let token = run.ledger.issue_token(new_token());
            (observed, token, run.ledger.controller())
        };
        drop(channel);
        if observed.changed {
            self.emit_frame(workspace, run_id, observed.seq, &frame);
        }
        Ok(AgentShot {
            frame,
            run_id: run_id.to_owned(),
            seq: observed.seq,
            token,
            controller,
        })
    }

    /// The agent's batch: needs Agent access, the current frame `seq` and the token from its latest
    /// screenshot, which this spends once for the whole batch. The batch is checked before
    /// anything is sent (an invalid action spends nothing), then run step by step under the run's
    /// channel. It stops early, before its next step, when the user acts in the guest, turns
    /// access off, or the screen changes size; a drag in progress is let go.
    pub fn agent_actions(
        &self,
        workspace: &str,
        run_id: &str,
        seq: u64,
        token: u128,
        actions: &[actions::Action],
    ) -> Result<BatchOutcome, AppRunError> {
        let channel = self.channel_of(workspace, run_id)?;
        let (plan, epoch, width, height) = {
            let channel = lock(&channel);
            let mut table = self.table();
            let run = table.exact(workspace, run_id)?;
            let (width, height) = (run.screen.width, run.screen.height);
            let whole = channel.whole_gestures && channel.installed.is_none();
            let plan = actions::plan(actions, width, height, whole).map_err(unsupported)?;
            run.ledger.spend_agent_token(seq, token)?;
            (plan, run.ledger.user_epoch(), width, height)
        };
        let mut held_buttons = 0u8;
        let mut held_at = (0u32, 0u32);
        let mut stopped = None;
        let mut stop_pos = plan.steps.len();
        for (pos, (_, step)) in plan.steps.iter().enumerate() {
            if let actions::Step::Wait(wait) = step {
                thread::sleep(*wait);
                continue;
            }
            let mut channel = lock(&channel);
            let still = {
                let mut table = self.table();
                let run = table.exact(workspace, run_id)?;
                if !run.ledger.agent_may_continue(epoch) {
                    Some("the user acted in the app or turned Agent access off")
                } else if (run.screen.width, run.screen.height) != (width, height) {
                    Some("the screen changed size")
                } else {
                    None
                }
            };
            if let Some(reason) = still {
                stopped = Some(reason.to_owned());
                stop_pos = pos;
                release_agent_buttons(&mut channel, held_buttons, held_at, width, height);
                break;
            }
            let sent = match step {
                actions::Step::Low { steps, buttons_after } => {
                    if let Some(Low::Mouse { x, y, .. }) =
                        steps.iter().rev().find(|s| matches!(s, Low::Mouse { .. }))
                    {
                        held_at = (*x, *y);
                    }
                    let sent = send_stream(&mut channel, steps, width, height);
                    if sent.is_ok() {
                        held_buttons = *buttons_after;
                    }
                    sent
                }
                actions::Step::Whole(gestures) => gestures.iter().try_for_each(|gesture| {
                    let command = gesture.command(width, height).map_err(unsupported)?;
                    send_gesture(&mut channel, gesture, &command)
                }),
                actions::Step::Wait(_) => unreachable!(),
            };
            if let Err(e) = sent {
                release_agent_buttons(&mut channel, held_buttons, held_at, width, height);
                stopped = Some(e.message);
                stop_pos = pos;
                break;
            }
        }
        // An action is complete when all its steps ran before the stop.
        let completed = (0..plan.actions)
            .filter(|&a| {
                plan.steps
                    .iter()
                    .rposition(|(i, _)| *i == a)
                    .is_some_and(|last| last < stop_pos)
            })
            .count();
        Ok(BatchOutcome {
            completed,
            total: plan.actions,
            stopped,
        })
    }

    /// Input the user asked Dial to send for them (App → VM **Set up SSH**): the batch runs as the
    /// user's own typing. Like typing in the screen, it voids the agent's token, and Agent access
    /// is unchanged.
    pub(crate) fn user_actions(
        &self,
        workspace: &str,
        run_id: &str,
        actions: &[actions::Action],
    ) -> Result<(), AppRunError> {
        let channel = self.channel_of(workspace, run_id)?;
        let (plan, width, height) = {
            let channel = lock(&channel);
            let mut table = self.table();
            let run = table.exact(workspace, run_id)?;
            if run.status != Status::Running {
                return Err(AppRunError::new("not_running", "The VM is not connected."));
            }
            let (width, height) = (run.screen.width, run.screen.height);
            let whole = channel.whole_gestures && channel.installed.is_none();
            let plan = actions::plan(actions, width, height, whole).map_err(unsupported)?;
            run.ledger.user_acted();
            (plan, width, height)
        };
        for (_, step) in &plan.steps {
            match step {
                actions::Step::Wait(wait) => thread::sleep(*wait),
                actions::Step::Low { steps, .. } => {
                    send_stream(&mut lock(&channel), steps, width, height)?;
                }
                actions::Step::Whole(gestures) => {
                    let mut channel = lock(&channel);
                    for gesture in gestures {
                        let command = gesture.command(width, height).map_err(unsupported)?;
                        send_gesture(&mut channel, gesture, &command)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn channel_of(
        &self,
        workspace: &str,
        run_id: &str,
    ) -> Result<Arc<Mutex<Channel>>, AppRunError> {
        let mut table = self.table();
        Ok(Arc::clone(&table.exact(workspace, run_id)?.channel))
    }

    fn spawn_pump(&self, workspace: String, run_id: String) {
        let runner = self.clone();
        thread::Builder::new()
            .name("dial-app-runner-frames".into())
            .spawn(move || runner.pump(&workspace, &run_id))
            .ok();
    }

    /// Poll the guest's screen until the run ends. Capture and recording happen under the channel,
    /// so they are serialized. Capture failures are retried a bounded number of times, then the
    /// run is failed visibly.
    fn pump(&self, workspace: &str, run_id: &str) {
        let mut budget = Budget::default();
        let mut last_change = Instant::now();
        loop {
            let (channel, child, dir) = {
                let mut table = self.table();
                match table.exact(workspace, run_id) {
                    Ok(run) => (Arc::clone(&run.channel), run.child.clone(), run.dir.clone()),
                    Err(_) => return,
                }
            };
            if child.as_ref().is_some_and(|child| {
                lock(child)
                    .try_wait()
                    .map_or(true, |status| status.is_some())
            }) {
                self.fail(workspace, run_id, "The guest stopped unexpectedly.");
                return;
            }
            let outcome = {
                let mut channel = lock(&channel);
                channel.pump.get_or_insert_with(thread::current);
                capture(&mut channel, &dir).map(|frame| {
                    let observed = {
                        let mut table = self.table();
                        table
                            .exact(workspace, run_id)
                            .map(|run| record(run, &frame))
                    };
                    (observed, frame, channel.last_input)
                })
            };
            match outcome {
                Err(_) => {
                    if budget.failed() {
                        self.fail(
                            workspace,
                            run_id,
                            "The app's screen could not be read; the run was stopped.",
                        );
                        return;
                    }
                    thread::sleep(IDLE_INTERVAL);
                }
                Ok((Err(_), _, _)) => return,
                Ok((Ok(observed), frame, last_input)) => {
                    budget.succeeded();
                    if observed.changed {
                        last_change = Instant::now();
                        self.emit_frame(workspace, run_id, observed.seq, &frame);
                    }
                    let active = last_change.elapsed() < ACTIVE_PERIOD
                        || last_input.is_some_and(|at| at.elapsed() < ACTIVE_PERIOD);
                    // A successful gesture wakes an idle pump without starting a second capture.
                    thread::park_timeout(if active {
                        FRAME_INTERVAL
                    } else {
                        IDLE_INTERVAL
                    });
                }
            }
        }
    }

    /// Tell the window a new frame is ready. The bytes follow through `app_runner_frame_bytes`, so
    /// the event stays small and a busy window can skip to the newest frame.
    fn emit_frame(&self, workspace: &str, run_id: &str, seq: u64, frame: &Frame) {
        let _ = self.inner.app.emit_to(
            "main",
            EVENT,
            json!({
                "kind": "frame",
                "runId": run_id,
                "workspaceRoot": workspace,
                "seq": seq,
                "width": frame.width,
                "height": frame.height,
            }),
        );
    }

    fn emit_status(&self, info: &RunInfo, message: Option<String>) {
        let _ = self
            .inner
            .app
            .emit_to("main", EVENT, status_payload(info, message));
    }
}

/// Run-local frames and project media must not inherit world-readable directory permissions.
fn private_run_dir(root: &Path) -> std::io::Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("run-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    builder.tempdir_in(root)
}

/// The backend a start uses, with the files it checked.
enum Plan {
    Qemu(runtime::Runtime),
    MacOs(macos::MacRuntime),
}

/// The VNC socket a QEMU run serves its display on: a fixed name in the private run folder. None
/// where Unix sockets are unavailable, or when the path would not fit a socket address (the run
/// then uses screendumps).
fn vnc_socket_for(dir: &Path) -> Option<PathBuf> {
    let path = dir.join("vnc.sock");
    // `sun_path` holds 104 bytes on macOS and 108 on Linux, with a terminating NUL.
    (cfg!(unix) && path.as_os_str().len() < 100).then_some(path)
}

/// Connect to QEMU's VNC display. QEMU creates the socket before its QMP greeting, so one attempt
/// is enough; any failure leaves the run on screendumps.
fn attach_vnc(channel: &mut Channel, socket: Option<&Path>) {
    #[cfg(unix)]
    if let Some(socket) = socket {
        channel.vnc = std::os::unix::net::UnixStream::connect(socket)
            .map_err(|e| e.to_string())
            .and_then(rfb::Session::connect)
            .map(rfb::Viewer::new)
            .ok();
    }
    #[cfg(not(unix))]
    let _ = (channel, socket);
}

/// The QEMU command for a run: the per-run firmware store, the optional project ISO and the log.
/// The guest disk is only read (`-snapshot`).
fn qemu_command(
    runtime: &runtime::Runtime,
    dir: &Path,
    root: &Path,
    share_project: bool,
    vnc_socket: Option<&Path>,
) -> Result<Command, AppRunError> {
    let vars_copy = dir.join("vars.fd");
    fs::copy(&runtime.firmware_vars, &vars_copy).map_err(|e| start_failed(&e.to_string()))?;
    let share_iso = share_iso_for(root, dir, share_project)?;
    let share_dir = runtime
        .firmware_code
        .parent()
        .ok_or_else(|| start_failed("the bundled firmware has no folder"))?
        .to_path_buf();
    let accel = Accel::for_host(std::env::consts::OS, runtime.arch);
    let argv = qemu::args(&Launch {
        runtime,
        vars_copy: &vars_copy,
        share_dir: &share_dir,
        accel,
        share_iso: share_iso.as_deref(),
        vnc_socket,
        memory_mb: MEMORY_MB,
        cpus: CPUS,
    });
    let log = create_log(dir, "qemu.log")?;
    Ok(guest_command(&runtime.qemu, argv, dir, log))
}

/// The macOS helper's command for a run: private copies of the base files, the optional project
/// ISO and the helper's log. The helper is given only the run folder.
fn macos_command(
    mac: &macos::MacRuntime,
    dir: &Path,
    root: &Path,
    share_project: bool,
) -> Result<Command, AppRunError> {
    macos::stage(mac, dir).map_err(|e| start_failed(&e))?;
    let share_iso = share_iso_for(root, dir, share_project)?;
    let log = create_log(dir, "helper.log")?;
    Ok(guest_command(
        &mac.helper,
        macos::args(dir, share_iso.as_deref()),
        dir,
        log,
    ))
}

/// The read-only project snapshot, when the user chose to share the project.
fn share_iso_for(
    root: &Path,
    dir: &Path,
    share_project: bool,
) -> Result<Option<PathBuf>, AppRunError> {
    if !share_project {
        return Ok(None);
    }
    project::build_snapshot(root, dir)
        .map(Some)
        .map_err(|e| AppRunError::new("snapshot_failed", e))
}

fn create_log(dir: &Path, name: &str) -> Result<File, AppRunError> {
    File::create(dir.join(name)).map_err(|e| start_failed(&e.to_string()))
}

/// The guest process: no inherited environment, its own run folder, QMP on piped stdio and its
/// diagnostics in the run's log.
fn guest_command<I, S>(program: &Path, args: I, dir: &Path, log: File) -> Command
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new(program);
    command
        .args(args)
        .env_clear()
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// The state a request for `guest` reports when no run of that target is open. `open_other` is the
/// target of the run open for the workspace, when that run is for a different target. `check` is
/// the requested target's runtime check: its failure is the status, whatever else is open.
fn state_view(
    guest: GuestTarget,
    open_other: Option<GuestTarget>,
    check: Result<(), RuntimeError>,
) -> StateView {
    let view = |status, runtime, message| StateView {
        status,
        guest_target: guest,
        open_guest_target: open_other,
        approved_vm_id: None,
        ssh: None,
        runtime,
        message,
        run: None,
    };
    match check {
        Ok(()) => {
            let message = open_other.map(|open| {
                format!(
                    "A {} guest is open for this project; stop it before starting this one.",
                    open.label()
                )
            });
            view("idle", None, message)
        }
        Err(RuntimeError::Missing(component)) => view(
            "missing_runtime",
            Some(component),
            Some(runtime::missing_message(component)),
        ),
        Err(RuntimeError::Refused(reason)) => view("failed", None, Some(reason)),
        Err(RuntimeError::Unsupported(target)) => view(
            "unsupported",
            None,
            Some(runtime::unsupported_message(target)),
        ),
    }
}

/// The status event: the run's id, workspace, status, controller, origin and guest target, and any
/// message.
fn status_payload(info: &RunInfo, message: Option<String>) -> Value {
    let mut payload = json!({
        "kind": "status",
        "runId": info.run_id,
        "workspaceRoot": info.workspace_root,
        "status": status_name(info.status),
        "controller": info.controller,
        "origin": info.origin,
        "guestTarget": info.guest_target,
    });
    if let Some(message) = message {
        payload["message"] = json!(message);
    }
    payload
}

/// The reveal event's payload: the project and the guest target the agent asked for, so the panel
/// can follow the start without parsing a message.
fn reveal_payload(workspace: &str, guest: GuestTarget) -> Value {
    json!({ "kind": "reveal", "workspaceRoot": workspace, "guestTarget": guest })
}

/// Store `frame` as the run's newest if its pixels changed. Called under the run's channel.
fn record(run: &mut Run, frame: &Frame) -> Observed {
    let resized = run.screen.frame.is_some()
        && (run.screen.width, run.screen.height) != (frame.width, frame.height);
    if resized {
        run.ledger.screen_resized();
    }
    let ledger = &mut run.ledger;
    match run.screen.record(frame, || ledger.advance_frame()) {
        Some(seq) => Observed { seq, changed: true },
        None => Observed {
            seq: run.ledger.seq(),
            changed: false,
        },
    }
}

/// The run's current screen: a registered VM's console, else QEMU's VNC display (only the changed
/// rectangles arrive; nothing touches the disk), else one QMP screendump read with the size cap.
/// A VNC display that fails is dropped for the rest of the run, and that capture falls back.
fn capture(channel: &mut Channel, dir: &Path) -> Result<Frame, String> {
    let frame = match &mut channel.installed {
        Some(vm) => vm.frame()?,
        None => match vnc_frame(channel) {
            Some(frame) => frame,
            None => {
                let path = dir.join("screen.ppm");
                let qmp = channel.qmp.as_mut().ok_or("the guest is not connected")?;
                qmp.execute("screendump", json!({ "filename": path.to_string_lossy() }))?;
                input::frame_from_ppm(&read_capped(&path, MAX_CAPTURE_BYTES)?)?
            }
        },
    };
    channel.screen_size = Some((frame.width, frame.height));
    Ok(frame)
}

/// How long the first VNC screen may take; QEMU answers the first full request at once.
#[cfg(unix)]
const VNC_FIRST_WAIT: Duration = Duration::from_secs(2);

/// The newest screen from QEMU's VNC display, without waiting for a change. `None` when the run
/// has no display connection, or it failed (and is then dropped).
fn vnc_frame(channel: &mut Channel) -> Option<Frame> {
    #[cfg(unix)]
    {
        let viewer = channel.vnc.as_mut()?;
        match viewer.frame(Duration::ZERO, VNC_FIRST_WAIT) {
            Ok(frame) => Some(frame),
            Err(_) => {
                channel.vnc = None;
                None
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = channel;
        None
    }
}

fn send_gesture(
    channel: &mut Channel,
    gesture: &Gesture,
    command: &QmpCommand,
) -> Result<(), AppRunError> {
    if let Some(vm) = &mut channel.installed {
        let (width, height) = channel
            .screen_size
            .ok_or_else(|| AppRunError::new("stale_frame", "No VM screen is available."))?;
        vm.input(gesture, width, height)
            .map_err(|e| AppRunError::new("unsupported_input", e))?;
        wake_after_input(channel);
        Ok(())
    } else {
        execute(channel, command)
    }
}

fn execute(channel: &mut Channel, command: &QmpCommand) -> Result<(), AppRunError> {
    let qmp = channel
        .qmp
        .as_mut()
        .ok_or_else(|| AppRunError::new("not_running", "The guest is not connected."))?;
    qmp.execute(command.name, command.arguments.clone())
        .map_err(|e| AppRunError::new("unsupported_input", e))?;
    wake_after_input(channel);
    Ok(())
}

fn wake_after_input(channel: &mut Channel) {
    channel.last_input = Some(Instant::now());
    if let Some(pump) = &channel.pump {
        pump.unpark();
    }
}

/// How far an agent batch got: `completed` of `total` actions, and why it stopped early, if it did.
#[derive(Debug)]
pub struct BatchOutcome {
    pub completed: usize,
    pub total: usize,
    pub stopped: Option<String>,
}

impl BatchOutcome {
    pub fn summary(&self) -> String {
        match &self.stopped {
            None => format!("Ran all {} action(s).", self.total),
            Some(reason) => format!(
                "Stopped after {} of {} action(s): {reason}.",
                self.completed, self.total
            ),
        }
    }
}

/// Let go of a button an agent drag still holds, where the pointer last was. Best effort: the
/// batch is already stopping.
fn release_agent_buttons(channel: &mut Channel, buttons: u8, at: (u32, u32), width: u32, height: u32) {
    if buttons != 0 {
        let up = [Low::Mouse { x: at.0, y: at.1, buttons: 0, prev: buttons }];
        let _ = send_stream(channel, &up, width, height);
    }
}

/// Send live-input steps: as they are to a registered VM or a QEMU guest; as whole clicks and
/// presses to a guest that takes only those.
fn send_stream(channel: &mut Channel, steps: &[Low], width: u32, height: u32) -> Result<(), AppRunError> {
    if steps.is_empty() {
        return Ok(());
    }
    if let Some(vm) = &mut channel.installed {
        vm.stream(steps, width, height)
            .map_err(|e| AppRunError::new("unsupported_input", e))?;
        wake_after_input(channel);
        return Ok(());
    }
    if channel.whole_gestures {
        let gestures = input::atomic(steps, &channel.held);
        for gesture in gestures {
            let command = gesture.command(width, height).map_err(unsupported)?;
            execute(channel, &command)?;
        }
        return Ok(());
    }
    match input::stream_command(steps, width, height) {
        Some(command) => execute(channel, &command),
        None => Ok(()),
    }
}

fn unsupported(error: input::Unsupported) -> AppRunError {
    AppRunError::new("unsupported_input", error.0)
}

fn new_token() -> u128 {
    let high = RandomState::new().build_hasher().finish();
    let low = RandomState::new().build_hasher().finish();
    (u128::from(high) << 64) | u128::from(low)
}

fn kill(child: &Arc<Mutex<Child>>) {
    let mut child = lock(child);
    let _ = child.kill();
    let _ = child.wait();
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn start_failed(message: &str) -> AppRunError {
    AppRunError::new("start_failed", message)
}

fn runtime_error(error: RuntimeError) -> AppRunError {
    match error {
        RuntimeError::Missing(component) => {
            AppRunError::new("missing_runtime", runtime::missing_message(component))
        }
        RuntimeError::Refused(reason) => AppRunError::new("start_failed", reason),
        RuntimeError::Unsupported(target) => {
            AppRunError::new("unsupported", runtime::unsupported_message(target))
        }
    }
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Starting => "starting",
        Status::Running => "running",
        Status::Stopped => "stopped",
        Status::Failed => "failed",
    }
}

/// Every command, the UI's and the agent's, is refused unless `workspace` is the project open in
/// the window.
fn active_project(app: &AppHandle, workspace: &str) -> Result<(), AppRunError> {
    if crate::app::browser_project(app, workspace) {
        Ok(())
    } else {
        Err(AppRunError::new(
            "stale_workspace",
            "The project open in the window changed; this app run belongs to another project.",
        ))
    }
}

/// Run blocking work off the UI thread.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, AppRunError> + Send + 'static,
) -> Result<T, AppRunError> {
    match tauri::async_runtime::spawn_blocking(work).await {
        Ok(result) => result,
        Err(error) => Err(AppRunError::new("internal", error.to_string())),
    }
}

// ----- Tauri commands. The UI calls these; the agent calls `agent_call`. -----

/// The guest target this host prefers when a request omits one (`macos` on Apple silicon, else
/// `default`). The UI shows it as the default; it never starts a run.
#[tauri::command]
pub fn app_runner_preferred_target() -> GuestTarget {
    runtime::preferred_guest_target()
}

#[tauri::command]
pub async fn app_runner_state(
    app: AppHandle,
    webview: tauri::Webview,
    runner: State<'_, AppRunner>,
    workspace_root: String,
    guest_target: Option<GuestTarget>,
) -> Result<StateView, AppRunError> {
    installed::main_view(&webview)?;
    let runner = runner.inner().clone();
    blocking(move || {
        active_project(&app, &workspace_root)?;
        Ok(runner.state(
            &workspace_root,
            guest_target.unwrap_or_else(runtime::preferred_guest_target),
        ))
    })
    .await
}

#[tauri::command]
pub async fn app_runner_start(
    app: AppHandle,
    webview: tauri::Webview,
    runner: State<'_, AppRunner>,
    workspace_root: String,
    share_project: Option<bool>,
    guest_target: Option<GuestTarget>,
    vm_id: Option<String>,
) -> Result<RunInfo, AppRunError> {
    installed::main_view(&webview)?;
    let runner = runner.inner().clone();
    blocking(move || {
        active_project(&app, &workspace_root)?;
        if guest_target == Some(GuestTarget::Vm) {
            if share_project == Some(true) {
                return Err(start_failed(
                    "Installed VMs do not accept a Dial project share.",
                ));
            }
            return runner.start_vm(&workspace_root, vm_id.as_deref(), Origin::User);
        }
        if vm_id.is_some() {
            return Err(start_failed("Choose VM mode to start an installed VM."));
        }
        runner.start(
            &workspace_root,
            share_project.unwrap_or(false),
            Origin::User,
            guest_target.unwrap_or_else(runtime::preferred_guest_target),
        )
    })
    .await
}

#[tauri::command]
pub async fn app_runner_stop(
    app: AppHandle,
    webview: tauri::Webview,
    runner: State<'_, AppRunner>,
    run_id: String,
    workspace_root: String,
) -> Result<(), AppRunError> {
    installed::main_view(&webview)?;
    let runner = runner.inner().clone();
    blocking(move || {
        active_project(&app, &workspace_root)?;
        runner.stop(&workspace_root, &run_id)
    })
    .await
}

#[tauri::command]
pub async fn app_runner_frame(
    app: AppHandle,
    webview: tauri::Webview,
    runner: State<'_, AppRunner>,
    run_id: String,
    workspace_root: String,
) -> Result<Option<FrameView>, AppRunError> {
    installed::main_view(&webview)?;
    let runner = runner.inner().clone();
    blocking(move || {
        active_project(&app, &workspace_root)?;
        runner.frame(&workspace_root, &run_id)
    })
    .await
}

/// What changed on the live screen since frame `since` (see [`AppRunner::frame_bytes`]); arrives
/// in JavaScript as an `ArrayBuffer`.
#[tauri::command]
pub async fn app_runner_frame_bytes(
    app: AppHandle,
    webview: tauri::Webview,
    runner: State<'_, AppRunner>,
    run_id: String,
    workspace_root: String,
    since: Option<u64>,
) -> Result<tauri::ipc::Response, AppRunError> {
    installed::main_view(&webview)?;
    let runner = runner.inner().clone();
    blocking(move || {
        active_project(&app, &workspace_root)?;
        runner
            .frame_bytes(&workspace_root, &run_id, since)
            .map(tauri::ipc::Response::new)
    })
    .await
}

#[tauri::command]
pub async fn app_runner_input(
    app: AppHandle,
    webview: tauri::Webview,
    runner: State<'_, AppRunner>,
    run_id: String,
    workspace_root: String,
    frame_seq: u64,
    event: Gesture,
) -> Result<(), AppRunError> {
    installed::main_view(&webview)?;
    let runner = runner.inner().clone();
    blocking(move || {
        active_project(&app, &workspace_root)?;
        runner.ui_input(&workspace_root, &run_id, frame_seq, &event)
    })
    .await
}

/// The App panel's Agent access switch (see [`AppRunner::set_agent_access`]).
#[tauri::command]
pub async fn app_runner_set_agent_access(
    app: AppHandle,
    webview: tauri::Webview,
    runner: State<'_, AppRunner>,
    run_id: String,
    workspace_root: String,
    allowed: bool,
) -> Result<RunInfo, AppRunError> {
    installed::main_view(&webview)?;
    let runner = runner.inner().clone();
    blocking(move || {
        active_project(&app, &workspace_root)?;
        runner.set_agent_access(&workspace_root, &run_id, allowed)
    })
    .await
}

/// How long the guest gets to draw its response to an agent gesture before the reply's frame.
const AFTER_INPUT_SETTLE: std::time::Duration = std::time::Duration::from_millis(400);

/// The agent's screenshot of the exact run as a tool reply: PNG plus `runId`, `frameSeq`, `token`.
fn frame_reply(
    runner: &AppRunner,
    workspace: &str,
    run_id: &str,
    lead: Option<&str>,
) -> Result<Reply, String> {
    let shot = runner
        .agent_screenshot(workspace, run_id)
        .map_err(|e| e.message)?;
    let reply = agent::FrameReply {
        run_id: shot.run_id,
        frame_seq: shot.seq,
        token: shot.token.map(agent::format_token),
        controller: shot.controller,
        width: shot.frame.width,
        height: shot.frame.height,
    };
    // The agent's model reads PNG; only its screenshots are encoded.
    let png = shot.frame.png()?;
    let text = match lead {
        Some(lead) => format!("{lead} {}", reply.to_text()),
        None => reply.to_text(),
    };
    Ok(Reply {
        ok: true,
        text,
        image: Some(base64::engine::general_purpose::STANDARD.encode(png)),
    })
}

/// The agent's `app_desktop` tool, on the agent's thread. Wire names are public (see [`agent`]):
///
/// - `state`: the run state for the workspace.
/// - `frame` `{runId}`: the exact run's screen, with `runId`, `frameSeq`, `token` (string or null).
/// - `input` `{runId, frameSeq, token, event}`: one gesture, spent against the token. The reply
///   carries the screen after the gesture with its own `frameSeq` and `token`, like `frame`.
pub fn agent_call(app: &AppHandle, workspace: &str, args: &Value) -> Result<Reply, String> {
    active_project(app, workspace).map_err(|e| e.message)?;
    let runner = app.state::<AppRunner>();
    match args.get("action").and_then(Value::as_str) {
        Some("state") => {
            let guest = agent::guest_target_of(args)?;
            let view = runner.state(workspace, guest);
            Ok(Reply::text(
                serde_json::to_string(&view).map_err(|e| e.to_string())?,
            ))
        }
        Some("frame") => {
            let run_id = agent::run_id_of(args)?;
            frame_reply(&runner, workspace, &run_id, None)
        }
        Some("input") => {
            let request = agent::parse_input(args)?;
            let outcome = runner
                .agent_actions(
                    workspace,
                    &request.run_id,
                    request.frame_seq,
                    request.token,
                    &request.actions,
                )
                .map_err(|e| e.message)?;
            // Let the guest draw its response, then hand back the new screen and token, so the
            // agent can chain the next batch without a separate `frame` call.
            std::thread::sleep(AFTER_INPUT_SETTLE);
            let summary = outcome.summary();
            match frame_reply(&runner, workspace, &request.run_id, Some(&summary)) {
                Ok(reply) => Ok(reply),
                Err(e) => Ok(Reply::text(format!(
                    "{summary} No new frame: {e}. Call frame before the next input."
                ))),
            }
        }
        Some("start") => {
            // Sent after the project check above and whatever the start returns, so a refused start
            // (for example a missing runtime) still opens the panel's remediation.
            let guest = agent::guest_target_of(args)?;
            let started = if guest == GuestTarget::Vm {
                let id = args.get("vmId").and_then(Value::as_str);
                runner.start_vm(workspace, id, Origin::Agent)
            } else if args.get("vmId").is_some() {
                Err(start_failed("A vmId requires guestTarget vm."))
            } else {
                runner.start_for_agent(workspace, guest)
            };
            let _ = app.emit_to("main", REVEAL_EVENT, reveal_payload(workspace, guest));
            let info = started.map_err(|e| e.message)?;
            Ok(Reply::text(
                serde_json::to_string(&info).map_err(|e| e.to_string())?,
            ))
        }
        _ => Err("app_desktop needs action \"state\", \"start\", \"frame\" or \"input\"".into()),
    }
}
