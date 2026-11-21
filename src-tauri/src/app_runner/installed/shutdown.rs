//! The App panel's **Shut down** for a registered VM: a graceful request to the guest OS (the ACPI
//! power button for VirtualBox, `vmrun stop … soft` for VMware). It is the only place Dial asks a
//! provider VM to turn off, and only the user's button reaches it; the agent has no such operation.
//! Nothing here forces power off, saves state or changes settings. The console helper and
//! `vmrun.rs` stay free of lifecycle stops.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::inventory::{Machine, Provider};
use super::vmware::kind_for;

const TIMEOUT: Duration = Duration::from_secs(60);

/// The exact program and arguments for a graceful shutdown of `vm`. The tool and locator come
/// from the fresh inventory, never from the renderer.
pub(super) fn command_for(vm: &Machine) -> (PathBuf, Vec<String>) {
    let args = match vm.provider {
        Provider::VirtualBox => vec!["controlvm".into(), vm.locator.clone(), "acpipowerbutton".into()],
        Provider::Vmware => vec![
            "-T".into(),
            kind_for(&vm.tool).into(),
            "stop".into(),
            vm.locator.clone(),
            "soft".into(),
        ],
    };
    (vm.tool.clone(), args)
}

/// Ask the guest to shut down. Returns once the provider accepted the request; the guest may take
/// longer to finish. Errors are fixed sentences: tool output can carry paths and stays out.
pub(super) fn request(vm: &Machine) -> Result<(), String> {
    let (program, args) = command_for(vm);
    let mut command = Command::new(program);
    command
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| "Dial could not run the VM tool to shut the VM down.".to_owned())?;
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => {
                return Err(
                    "The VM did not accept the shutdown request. Is it still running?".to_owned(),
                );
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("The VM tool did not answer the shutdown request in time.".to_owned());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(provider: Provider, tool: &str, locator: &str) -> Machine {
        Machine {
            id: "x".into(),
            name: "x".into(),
            provider,
            running: Some(true),
            tool: PathBuf::from(tool),
            locator: locator.into(),
        }
    }

    #[test]
    fn virtualbox_presses_the_acpi_power_button() {
        let vm = machine(Provider::VirtualBox, "/opt/VBoxManage", "8986e113-5c34-4c15-9e64-dada2451c4f0");
        let (program, args) = command_for(&vm);
        assert_eq!(program, PathBuf::from("/opt/VBoxManage"));
        assert_eq!(args, ["controlvm", "8986e113-5c34-4c15-9e64-dada2451c4f0", "acpipowerbutton"]);
    }

    #[test]
    fn vmware_stops_softly_with_the_product_type() {
        let fusion = machine(
            Provider::Vmware,
            "/Applications/VMware Fusion.app/Contents/Library/vmrun",
            "/vms/A.vmx",
        );
        assert_eq!(command_for(&fusion).1, ["-T", "fusion", "stop", "/vms/A.vmx", "soft"]);
        let ws = machine(Provider::Vmware, "/usr/bin/vmrun", "/vms/B.vmx");
        assert_eq!(command_for(&ws).1, ["-T", "ws", "stop", "/vms/B.vmx", "soft"]);
    }

    #[test]
    fn no_command_forces_power_off() {
        for provider in [Provider::VirtualBox, Provider::Vmware] {
            let (_, args) = command_for(&machine(provider, "/t", "l"));
            for forbidden in ["poweroff", "hard", "savestate", "reset", "unregistervm"] {
                assert!(!args.iter().any(|a| a == forbidden), "{forbidden}");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_tool_is_reported() {
        let vm = machine(Provider::VirtualBox, "/usr/bin/false", "u");
        assert!(request(&vm).is_err());
        let vm = machine(Provider::VirtualBox, "/usr/bin/true", "u");
        assert!(request(&vm).is_ok());
    }
}
