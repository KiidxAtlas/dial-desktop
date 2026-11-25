//! What each VM provider still needs before Dial can open its console, shown in App → VM.
//!
//! Checks are read-only and run nothing new: the trusted tool locations the inventory uses,
//! VirtualBox's Python SDK files beside `VBoxManage` with the system Python 3, and each listed
//! VMware machine's `.vmx` VNC settings. Like inventory notices, details carry no paths or names.

use serde::Serialize;

use super::inventory::{self, Inventory, Provider};
use super::{vbox, vmware};

/// One readiness check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Check {
    pub provider: Provider,
    pub id: &'static str,
    pub label: &'static str,
    /// `ready`, `missing`, or `unknown` (nothing to check yet).
    pub status: &'static str,
    pub detail: String,
}

/// What was found on this host, gathered by [`probe`]; [`checks`] turns it into the list.
#[derive(Debug, Default)]
struct Probe {
    vboxmanage: bool,
    /// The SDK check, when `VBoxManage` was found.
    vbox_sdk: Option<Result<(), String>>,
    vmrun: bool,
    /// One VNC check per listed VMware machine.
    vmware_vnc: Vec<Result<(), String>>,
}

/// The readiness of both providers for the machines in `inventory`.
pub(crate) fn readiness(inventory: &Inventory) -> Vec<Check> {
    checks(&probe(inventory))
}

fn probe(inventory: &Inventory) -> Probe {
    let vbox = inventory::virtualbox_tool();
    Probe {
        vboxmanage: vbox.is_some(),
        vbox_sdk: vbox.as_deref().map(vbox::sdk_ready),
        vmrun: inventory::vmware_tool().is_some(),
        vmware_vnc: inventory
            .machines
            .iter()
            .filter(|vm| vm.provider == Provider::Vmware)
            .map(|vm| vmware::vnc_ready(&vm.locator))
            .collect(),
    }
}

fn check(provider: Provider, id: &'static str, label: &'static str, status: &'static str, detail: impl Into<String>) -> Check {
    Check { provider, id, label, status, detail: detail.into() }
}

fn checks(p: &Probe) -> Vec<Check> {
    let mut out = Vec::new();
    out.push(if p.vboxmanage {
        check(Provider::VirtualBox, "vboxmanage", "VBoxManage", "ready", "Found in VirtualBox's standard install location.")
    } else {
        check(Provider::VirtualBox, "vboxmanage", "VBoxManage", "missing", "Install VirtualBox from virtualbox.org. Dial looks only in its standard install location, not on PATH.")
    });
    out.push(match &p.vbox_sdk {
        None => check(Provider::VirtualBox, "vbox-sdk", "VirtualBox Python SDK", "unknown", "Checked once VBoxManage is found."),
        Some(Ok(())) => check(
            Provider::VirtualBox,
            "vbox-sdk",
            "VirtualBox Python SDK",
            "ready",
            "The SDK files beside VBoxManage and the system Python 3 were found. Whether the SDK imports is confirmed when you connect.",
        ),
        Some(Err(reason)) => check(Provider::VirtualBox, "vbox-sdk", "VirtualBox Python SDK", "missing", reason.clone()),
    });
    out.push(if p.vmrun {
        check(Provider::Vmware, "vmrun", "vmrun", "ready", "Found in VMware's standard install location.")
    } else {
        check(Provider::Vmware, "vmrun", "vmrun", "missing", "Install VMware Fusion or Workstation. Dial looks only in its standard install location, not on PATH.")
    });
    let total = p.vmware_vnc.len();
    let ready = p.vmware_vnc.iter().filter(|r| r.is_ok()).count();
    out.push(if total == 0 {
        check(Provider::Vmware, "vmware-vnc", "VNC console", "unknown", "Checked for each VMware machine Dial lists. Each VM needs remote display (VNC) on, listening on 127.0.0.1 with a port.")
    } else if ready == total {
        check(Provider::Vmware, "vmware-vnc", "VNC console", "ready", format!("{ready} of {total} VMware machine(s) declare a loopback VNC console."))
    } else {
        let reason = p.vmware_vnc.iter().find_map(|r| r.as_ref().err()).cloned().unwrap_or_default();
        check(
            Provider::Vmware,
            "vmware-vnc",
            "VNC console",
            "missing",
            format!("{ready} of {total} VMware machine(s) declare a loopback VNC console. First problem: {reason}."),
        )
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn statuses(p: &Probe) -> Vec<(&'static str, &'static str)> {
        checks(p).iter().map(|c| (c.id, c.status)).collect()
    }

    #[test]
    fn each_provider_reports_what_it_still_needs() {
        let nothing = Probe::default();
        assert_eq!(
            statuses(&nothing),
            [("vboxmanage", "missing"), ("vbox-sdk", "unknown"), ("vmrun", "missing"), ("vmware-vnc", "unknown")]
        );
        let ready = Probe { vboxmanage: true, vbox_sdk: Some(Ok(())), vmrun: true, vmware_vnc: vec![Ok(()), Ok(())] };
        assert!(checks(&ready).iter().all(|c| c.status == "ready"));
        let partly = Probe {
            vboxmanage: true,
            vbox_sdk: Some(Err("VirtualBox's Python SDK was not found beside VBoxManage.".into())),
            vmrun: true,
            vmware_vnc: vec![Ok(()), Err("VNC is not enabled for this VM".into())],
        };
        let list = checks(&partly);
        assert_eq!(list[1].status, "missing");
        assert!(list[1].detail.contains("Python SDK"));
        assert_eq!(list[3].status, "missing");
        assert!(list[3].detail.starts_with("1 of 2") && list[3].detail.contains("VNC is not enabled"));
        let value = serde_json::to_value(&list[0]).unwrap();
        assert_eq!(value["provider"], "virtualbox");
        assert_eq!(value["status"], "ready");
    }
}
