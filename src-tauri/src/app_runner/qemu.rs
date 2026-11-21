//! The QEMU command line for one run. Nothing here reaches the host beyond the guest's display
//! and the guest disk, which QEMU only reads and discards writes to (`-snapshot`). The display is
//! served only on a Unix socket inside the private run folder (where the host supports one).
//!
//! Deliberately absent: any network device (`-nic none`), host forwarding, shared folders
//! (`-virtfs`), clipboard or USB passthrough, audio, serial and parallel ports. The only host
//! paths the guest sees are the imported disk (snapshot mode), the per-run UEFI variable copy, and,
//! when the user asked for project sharing, one read-only ISO.

use std::path::Path;

use super::runtime::{Arch, Runtime};

/// QEMU's ordered accelerator list and a CPU model valid for every entry in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accel {
    pub name: &'static str,
    pub cpu: &'static str,
}

impl Accel {
    /// Let the bundled QEMU try the kernel hypervisor, then TCG if initialization fails.
    /// `max` adapts to the active accelerator; `host` would prevent a TCG fallback.
    pub fn for_host(os: &str, arch: Arch) -> Self {
        match (os, arch) {
            ("windows", _) => Self {
                name: "whpx:tcg",
                cpu: "max",
            },
            ("linux", _) => Self {
                name: "kvm:tcg",
                cpu: "max",
            },
            ("macos", Arch::Aarch64) => Self {
                name: "hvf",
                cpu: "host",
            },
            (_, Arch::Aarch64) => Self {
                name: "tcg",
                cpu: "cortex-a72",
            },
            (_, Arch::X86_64) => Self {
                name: "tcg",
                cpu: "qemu64",
            },
        }
    }
}

/// What one run needs to start.
pub struct Launch<'a> {
    pub runtime: &'a Runtime,
    /// The per-run copy of the UEFI variable template.
    pub vars_copy: &'a Path,
    /// The directory QEMU searches for its firmware and data files (`-L`): the bundled share dir.
    pub share_dir: &'a Path,
    pub accel: Accel,
    /// The read-only project snapshot ISO, when the user chose to share the project.
    pub share_iso: Option<&'a Path>,
    /// Where QEMU serves the guest display as VNC: a Unix socket in the run folder. `None` keeps
    /// the display unserved (screendumps over QMP only).
    pub vnc_socket: Option<&'a Path>,
    pub memory_mb: u32,
    pub cpus: u32,
}

/// The arguments for `qemu-system-<arch>` (not including the program name).
pub fn args(launch: &Launch<'_>) -> Vec<String> {
    let runtime = launch.runtime;
    let mut out: Vec<String> = vec![
        // No defaults at all: every device the guest has is named below.
        "-nodefaults".into(),
        // Ignore any per-user QEMU configuration on this machine.
        "-no-user-config".into(),
        "-L".into(),
        plain_path(launch.share_dir),
        "-name".into(),
        "dial-app-runner".into(),
        "-machine".into(),
        format!("{},accel={}", runtime.arch.machine(), launch.accel.name),
        "-cpu".into(),
        launch.accel.cpu.into(),
        "-m".into(),
        launch.memory_mb.to_string(),
        "-smp".into(),
        launch.cpus.to_string(),
        // Disposable writes: every change to the guest disk is discarded when the run ends.
        "-snapshot".into(),
        "-no-reboot".into(),
        // UEFI firmware: read-only code, a per-run variable store.
        "-drive".into(),
        format!(
            "if=pflash,format=raw,unit=0,readonly=on,file={}",
            path_arg(&runtime.firmware_code)
        ),
        "-drive".into(),
        format!(
            "if=pflash,format=raw,unit=1,file={}",
            path_arg(launch.vars_copy)
        ),
        "-drive".into(),
        format!(
            "if=virtio,format=raw,file={}",
            path_arg(&runtime.guest_disk)
        ),
        // Display and pointer/keyboard, all paravirtual; UEFI gets its display from virtio-gpu.
        "-device".into(),
        "virtio-gpu-pci".into(),
        "-device".into(),
        "virtio-tablet-pci".into(),
        "-device".into(),
        "virtio-keyboard-pci".into(),
        // No network, no serial, no parallel, no display window: QMP runs over stdio.
        "-nic".into(),
        "none".into(),
        "-display".into(),
        "none".into(),
        "-chardev".into(),
        "stdio,id=qmp,signal=off".into(),
        "-mon".into(),
        "chardev=qmp,mode=control".into(),
    ];
    if let Some(socket) = launch.vnc_socket {
        // The live screen: QEMU sends only what changed. A Unix socket, never a TCP port, so no
        // other user or program outside the run folder can reach the guest.
        out.push("-vnc".into());
        out.push(format!("unix:{}", path_arg(socket)));
    }
    if let Some(iso) = launch.share_iso {
        // An explicit SCSI controller and a read-only CD: no virtio CD ambiguity.
        out.push("-device".into());
        out.push("virtio-scsi-pci,id=scsi0".into());
        out.push("-drive".into());
        out.push(format!(
            "if=none,id=projectiso,format=raw,media=cdrom,readonly=on,file={}",
            path_arg(iso)
        ));
        out.push("-device".into());
        out.push("scsi-cd,bus=scsi0.0,drive=projectiso".into());
    }
    out
}

/// A path as it appears in a `-drive file=` option. QEMU splits options on commas, so a comma in
/// a path is doubled, the way QEMU's own option parser expects.
pub fn path_arg(path: &Path) -> String {
    path.to_string_lossy().replace(',', ",,")
}

/// A path as a standalone option value (`-L`, a directory QEMU reads): no escaping.
pub fn plain_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests;
