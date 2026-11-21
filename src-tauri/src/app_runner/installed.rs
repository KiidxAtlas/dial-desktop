//! Registered provider VMs are persistent, user-owned machines, not disposable app guests.
//! Only a UI selection or a one-use grant for that selection may open a provider console.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, State, Webview};

use super::input::{Frame, Gesture, Low};
use super::{
    AppRunError, AppRunner, Channel, Controller, GuestTarget, Ledger, NEXT_RUN, Origin, Run,
    RunInfo, Screen, Status, active_project, blocking, lock, private_run_dir, start_failed,
};

pub(crate) mod inventory;
mod shutdown;
pub(crate) mod ssh;
mod ssh_setup;
mod vbox;
mod vmware;

pub(super) enum Console {
    VirtualBox(vbox::Console),
    Vmware(vmware::Console),
}

impl Console {
    fn open(vm: &inventory::Machine) -> Result<Self, String> {
        match vm.provider {
            inventory::Provider::VirtualBox => vbox::Console::open(vm).map(Self::VirtualBox),
            inventory::Provider::Vmware => vmware::Console::open(vm).map(Self::Vmware),
        }
    }

    pub(super) fn frame(&mut self) -> Result<Frame, String> {
        match self {
            Self::VirtualBox(c) => c.frame(),
            Self::Vmware(c) => c.frame(),
        }
    }

    pub(super) fn input(
        &mut self,
        gesture: &Gesture,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        match self {
            Self::VirtualBox(c) => c.input(gesture, width, height),
            Self::Vmware(c) => c.input(gesture, width, height),
        }
    }

    pub(super) fn stream(&mut self, steps: &[Low], width: u32, height: u32) -> Result<(), String> {
        match self {
            Self::VirtualBox(c) => c.stream(steps, width, height),
            Self::Vmware(c) => c.stream(steps, width, height),
        }
    }
}

/// Only one pending launch permission in the app. Never persisted or inherited by another project.
#[derive(Default)]
pub(super) struct Grants(Option<(String, String)>);

impl Grants {
    pub(super) fn get(&self, workspace: &str) -> Option<&str> {
        self.0
            .as_ref()
            .filter(|(root, _)| root == workspace)
            .map(|(_, id)| id.as_str())
    }

    fn set(&mut self, workspace: &str, id: Option<String>) {
        if id.is_some() || self.get(workspace).is_some() {
            self.0 = id.map(|id| (workspace.to_owned(), id));
        }
    }

    fn take_for_start(
        &mut self,
        workspace: &str,
        id: &str,
        origin: Origin,
    ) -> Result<(), AppRunError> {
        if origin == Origin::Agent && self.get(workspace) != Some(id) {
            return Err(AppRunError::new(
                "vm_not_approved",
                "In App → VM, select the machine and allow one agent launch first.",
            ));
        }
        self.set(workspace, None);
        Ok(())
    }
}

/// The agent sees a run's screen only while Agent access is on, for every kind of run.
pub(super) fn check_view(_guest: GuestTarget, controller: Controller) -> Result<(), AppRunError> {
    if controller != Controller::Agent {
        return Err(AppRunError::new(
            "not_controller",
            "Agent access is off for this app. Ask the user to turn on Agent access in the App sidebar.",
        ));
    }
    Ok(())
}

pub(super) fn main_view(view: &Webview) -> Result<(), AppRunError> {
    if view.label() == "main" {
        Ok(())
    } else {
        Err(AppRunError::new(
            "not_allowed",
            "Only the Dial app sidebar may access a VM console.",
        ))
    }
}

/// The VM list for App → VM: the provider inventory, each listed VM's SSH host (saved or matched by
/// name), and the host aliases in the user's SSH config to choose from.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VmList {
    #[serde(flatten)]
    inventory: inventory::Inventory,
    ssh_links: std::collections::BTreeMap<String, ssh::SshLink>,
    ssh_hosts: Vec<String>,
}

#[tauri::command]
pub(crate) async fn app_runner_vms(
    webview: Webview,
    app: AppHandle,
    runner: State<'_, AppRunner>,
    workspace_root: String,
) -> Result<VmList, AppRunError> {
    main_view(&webview)?;
    let data_dir = runner.inner.data_dir.clone();
    blocking(move || {
        active_project(&app, &workspace_root)?;
        let inventory = inventory::discover();
        let links = ssh::load(&data_dir);
        let aliases = ssh::config_aliases();
        let ssh_links = inventory
            .machines
            .iter()
            .filter_map(|vm| {
                ssh::resolve(&links, &aliases, &vm.id, Some(&vm.name)).map(|l| (vm.id.clone(), l))
            })
            .collect();
        active_project(&app, &workspace_root)?;
        Ok(VmList {
            inventory,
            ssh_links,
            ssh_hosts: aliases,
        })
    })
    .await
}

/// Link a listed VM to an SSH host alias, or unlink it with `None`. UI-only; the agent cannot call
/// it. Dial stores only the alias; SSH config, keys and the VM are not touched.
#[tauri::command]
pub(crate) async fn app_runner_vm_ssh_link(
    webview: Webview,
    app: AppHandle,
    runner: State<'_, AppRunner>,
    workspace_root: String,
    vm_id: String,
    host: Option<String>,
) -> Result<(), AppRunError> {
    main_view(&webview)?;
    let data_dir = runner.inner.data_dir.clone();
    blocking(move || {
        active_project(&app, &workspace_root)?;
        let host = host.map(|h| h.trim().to_owned()).filter(|h| !h.is_empty());
        if !inventory::discover().machines.iter().any(|vm| vm.id == vm_id) {
            return Err(start_failed(
                "That VM is no longer in the local provider inventory. Refresh the list.",
            ));
        }
        ssh::save(&data_dir, &vm_id, host.as_deref())
            .map_err(|e| AppRunError::new("ssh_link_failed", e))
    })
    .await
}

/// App → VM **Set up SSH** for the connected VM run `run_id`. UI-only; the agent cannot call it.
/// Types one setup command into the guest as the user's input and waits until SSH answers.
#[tauri::command]
pub(crate) async fn app_runner_vm_ssh_setup(
    webview: Webview,
    app: AppHandle,
    runner: State<'_, AppRunner>,
    workspace_root: String,
    run_id: String,
    user: String,
    open_terminal: bool,
) -> Result<ssh_setup::SetupDone, AppRunError> {
    main_view(&webview)?;
    let runner = runner.inner().clone();
    blocking(move || {
        active_project(&app, &workspace_root)?;
        runner.ssh_setup(&workspace_root, &run_id, user.trim(), open_terminal)
    })
    .await
}

/// The agent has no corresponding operation. Revocation needs no working provider.
#[tauri::command]
pub(crate) fn app_runner_vm_grant(
    webview: Webview,
    app: AppHandle,
    runner: State<'_, AppRunner>,
    workspace_root: String,
    vm_id: Option<String>,
) -> Result<(), AppRunError> {
    main_view(&webview)?;
    active_project(&app, &workspace_root)?;
    if vm_id
        .as_ref()
        .is_some_and(|id| id.is_empty() || id.len() > 256)
    {
        return Err(start_failed("Choose a VM from the provider inventory."));
    }
    let table = runner.table();
    if vm_id.is_some() && table.contains(&workspace_root) {
        return Err(AppRunError::new(
            "already_running",
            "Hand off the open run instead.",
        ));
    }
    lock(&runner.inner.vm_grants).set(&workspace_root, vm_id);
    drop(table);
    // A workspace switch clears the old grant. If it won the race just before our
    // write, clear that stale write here as well.
    if let Err(e) = active_project(&app, &workspace_root) {
        runner.revoke_vm_launch(&workspace_root);
        return Err(e);
    }
    Ok(())
}

impl AppRunner {
    pub(crate) fn revoke_vm_launch(&self, workspace: &str) {
        lock(&self.inner.vm_grants).set(workspace, None);
    }

    /// The SSH host the agent may use for VM `vm_id` (named `vm_name` when known).
    pub(super) fn vm_ssh(&self, vm_id: &str, vm_name: Option<&str>) -> Option<ssh::SshLink> {
        let links = ssh::load(&self.inner.data_dir);
        let aliases = if links.contains_key(vm_id) {
            Vec::new()
        } else {
            ssh::config_aliases()
        };
        ssh::resolve(&links, &aliases, vm_id, vm_name)
    }
    pub(super) fn start_vm(
        &self,
        workspace: &str,
        requested: Option<&str>,
        origin: Origin,
    ) -> Result<RunInfo, AppRunError> {
        let id = requested
            .filter(|id| !id.is_empty() && id.len() <= 256)
            .ok_or_else(|| start_failed("Choose an exact VM id from App → VM."))?;
        if origin == Origin::Agent && lock(&self.inner.vm_grants).get(workspace) != Some(id) {
            return Err(AppRunError::new(
                "vm_not_approved",
                "Allow this VM in the App sidebar first.",
            ));
        }
        // Resolve afresh. The renderer/agent can never pass a .vmx path, executable or arguments.
        let vm = inventory::discover()
            .machines
            .into_iter()
            .find(|vm| vm.id == id)
            .ok_or_else(|| {
                start_failed(
                    "That VM is no longer in the local provider inventory. Refresh the list.",
                )
            })?;
        let root = self.inner.data_dir.join("app-runner/runs");
        std::fs::create_dir_all(&root).map_err(|e| start_failed(&e.to_string()))?;
        let tempdir = private_run_dir(&root).map_err(|e| start_failed(&e.to_string()))?;
        let run_id = format!("run-{}", NEXT_RUN.fetch_add(1, Ordering::Relaxed));
        let channel = Arc::new(Mutex::new(Channel::default()));
        let mut slot = lock(&channel);
        let run = Run {
            run_id: run_id.clone(),
            workspace: workspace.to_owned(),
            title: vm.name.clone(),
            status: Status::Starting,
            origin,
            guest_target: GuestTarget::Vm,
            vm_id: Some(id.to_owned()),
            ledger: match origin {
                Origin::User => Ledger::default(),
                Origin::Agent => Ledger::agent_launch(),
            },
            screen: Screen::default(),
            child: None,
            channel: channel.clone(),
            dir: tempdir.path().to_owned(),
            _tempdir: tempdir,
        };
        let starting = run.info();
        {
            active_project(&self.inner.app, workspace)?;
            let mut table = self.table();
            if table.contains(workspace) || table.entries().any(|r| r.vm_id.as_deref() == Some(id))
            {
                return Err(AppRunError::new(
                    "already_running",
                    "This project or VM already has an open run.",
                ));
            }
            lock(&self.inner.vm_grants).take_for_start(workspace, id, origin)?;
            table
                .insert(workspace, run)
                .map_err(|_| start_failed("This project already has an open run."))?;
        }
        self.emit_status(&starting, None);
        // Reserve the run *before* the provider may power on a persistent machine. Disconnect
        // synchronizes with this same channel and never kills the provider's VM process.
        match Console::open(&vm) {
            Ok(c) => slot.installed = Some(c),
            Err(e) => {
                drop(slot);
                self.fail(workspace, &run_id, &e);
                return Err(start_failed(&e));
            }
        }
        let info = {
            let mut table = self.table();
            let run = table.exact(workspace, &run_id)?;
            run.status = Status::Running;
            run.info()
        };
        self.emit_status(&info, None);
        drop(slot);
        self.spawn_pump(workspace.to_owned(), run_id);
        Ok(info)
    }

    /// The App panel's Shut down: disconnect Dial, then ask the guest OS to shut down gracefully.
    /// UI-only; the agent has no corresponding operation. The VM is resolved afresh by its id.
    pub(super) fn shut_down_vm(&self, workspace: &str, run_id: &str) -> Result<(), AppRunError> {
        let id = {
            let mut table = self.table();
            let run = table.exact(workspace, run_id)?;
            run.vm_id.clone().ok_or_else(|| {
                AppRunError::new("not_a_vm", "Only a connected VM can be shut down here.")
            })?
        };
        let vm = inventory::discover()
            .machines
            .into_iter()
            .find(|vm| vm.id == id)
            .ok_or_else(|| {
                AppRunError::new("shutdown_failed", "That VM is no longer in the provider inventory.")
            })?;
        self.stop(workspace, run_id)?;
        shutdown::request(&vm).map_err(|e| AppRunError::new("shutdown_failed", e))
    }
}

/// The App panel's Shut down for a connected VM. UI-only (main view); the agent cannot call it.
#[tauri::command]
pub(crate) async fn app_runner_vm_shutdown(
    app: AppHandle,
    webview: Webview,
    runner: State<'_, AppRunner>,
    run_id: String,
    workspace_root: String,
) -> Result<(), AppRunError> {
    main_view(&webview)?;
    let runner = runner.inner().clone();
    blocking(move || {
        active_project(&app, &workspace_root)?;
        runner.shut_down_vm(&workspace_root, &run_id)
    })
    .await
}

#[cfg(test)]
mod tests;
