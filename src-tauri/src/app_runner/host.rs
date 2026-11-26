//! The App panel's host setup: which hypervisor built into this computer runs the private guest,
//! what it still needs, and the install action for each missing part.
//!
//! - macOS (Apple silicon): Apple's Virtualization framework runs a virtual Mac through the packaged
//!   helper. A missing virtual Mac is installed by the packaged prep tool, which downloads Apple's
//!   restore image, installs it, and shows Setup Assistant in its own window; Dial imports the base
//!   when the user shuts it down.
//! - Windows: QEMU on the Windows Hypervisor Platform (Hyper-V). The platform feature is turned on
//!   elevated (UAC), and QEMU is installed machine-wide with winget.
//! - Linux: QEMU on KVM. Access to `/dev/kvm`, the KVM module and the QEMU/UEFI packages are set up
//!   through `pkexec` and the system package manager.
//!
//! Every action is asked for by a click in the App panel and confirmed in a native dialog first.
//! Commands are fixed: no part of a command line comes from the webview. Programs are named by
//! absolute path, never looked up on `PATH`. Only one action runs at a time.

use std::io::{BufRead, BufReader, Read};
#[cfg(any(windows, target_os = "linux"))]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, State, Webview};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

#[cfg(any(windows, target_os = "linux"))]
use super::runtime::Arch;
use super::runtime::{self, GuestTarget, RuntimeError};
use super::{AppRunError, AppRunner, blocking, installed};

/// Progress lines from a running action: `{ step, line }`.
pub const EVENT: &str = "app_runner_host";

/// Set while an action runs; a second request is refused.
static BUSY: AtomicBool = AtomicBool::new(false);
/// Set by [`app_runner_host_cancel`] while a cancellable action runs; [`stream`] then stops it.
static CANCEL: AtomicBool = AtomicBool::new(false);
/// The running action can be stopped from the panel (it runs as the user, not elevated).
static CANCELLABLE: AtomicBool = AtomicBool::new(false);
/// Windows: the hypervisor platform was turned on and needs a restart to load.
#[cfg(windows)]
static RESTART_PENDING: AtomicBool = AtomicBool::new(false);
/// Linux: the user was added to the `kvm` group and must log in again.
#[cfg(target_os = "linux")]
static RELOGIN_PENDING: AtomicBool = AtomicBool::new(false);

pub(crate) fn setup_busy() -> bool {
    BUSY.load(Ordering::SeqCst)
}

/// What this computer runs the App panel's private guest with, and what is still missing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSetup {
    /// `macos`, `windows`, `linux` or `other`.
    pub platform: &'static str,
    /// The built-in hypervisor, for display.
    pub backend: &'static str,
    /// The private guest this host runs (the selector's matching entry).
    pub guest: GuestTarget,
    /// True when every step is ready.
    pub ready: bool,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub id: &'static str,
    pub label: &'static str,
    pub status: StepStatus,
    pub detail: String,
    /// What the panel's button does for a missing step, when it can do something.
    pub action: Option<Action>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    Ready,
    Missing,
    /// Missing, and Dial cannot fix it from here; the detail says what to do.
    Blocked,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub label: &'static str,
    /// `install` runs [`app_runner_host_install`]; `import` opens the existing disk import.
    pub kind: &'static str,
    /// The system asks for an administrator password or UAC approval.
    pub admin: bool,
    /// The panel may offer Cancel while it runs. Elevated actions run as another user (pkexec,
    /// UAC), which Dial cannot stop, so they are never cancellable.
    pub cancellable: bool,
}

impl Step {
    fn ready(id: &'static str, label: &'static str, detail: impl Into<String>) -> Self {
        Step {
            id,
            label,
            status: StepStatus::Ready,
            detail: detail.into(),
            action: None,
        }
    }
    fn missing(
        id: &'static str,
        label: &'static str,
        detail: impl Into<String>,
        action: Option<Action>,
    ) -> Self {
        Step {
            id,
            label,
            status: StepStatus::Missing,
            detail: detail.into(),
            action,
        }
    }
    fn blocked(id: &'static str, label: &'static str, detail: impl Into<String>) -> Self {
        Step {
            id,
            label,
            status: StepStatus::Blocked,
            detail: detail.into(),
            action: None,
        }
    }
}

fn install(label: &'static str, admin: bool) -> Option<Action> {
    Some(Action {
        label,
        kind: "install",
        admin,
        cancellable: !admin,
    })
}

fn setup(platform: &'static str, backend: &'static str, guest: GuestTarget, steps: Vec<Step>) -> HostSetup {
    HostSetup {
        platform,
        backend,
        guest,
        ready: steps.iter().all(|s| s.status == StepStatus::Ready),
        steps,
    }
}

/// The guest disk step shared by the QEMU hosts: a bootable raw disk the user imports.
#[cfg(any(windows, target_os = "linux"))]
fn disk_step(guest: GuestTarget, check: &Result<runtime::Runtime, RuntimeError>) -> Step {
    let label = match guest {
        GuestTarget::Windows => "Windows guest disk",
        _ => "Linux guest disk",
    };
    match check {
        Err(RuntimeError::Missing(runtime::GUEST_IMAGE)) => Step::missing(
            "guest-disk",
            label,
            "Choose a bootable disk (.raw or .img, or .qcow2, .vmdk or .vdi to convert with qemu-img) for this computer's architecture. Dial copies it and throws away guest changes after each run.",
            Some(Action {
                label: "Choose disk…",
                kind: "import",
                admin: false,
                cancellable: false,
            }),
        ),
        Ok(_) => Step::ready("guest-disk", label, "Imported."),
        // The runtime step reports any other problem; the disk is not checked until QEMU is.
        _ => Step::blocked("guest-disk", label, "Install QEMU first."),
    }
}

/// The QEMU step shared by the QEMU hosts, from the runtime check for `guest`.
#[cfg(any(windows, target_os = "linux"))]
fn qemu_step(check: &Result<runtime::Runtime, RuntimeError>, action: Option<Action>, help: &str) -> Step {
    const LABEL: &str = "QEMU and UEFI firmware";
    match check {
        Ok(_) | Err(RuntimeError::Missing(runtime::GUEST_IMAGE)) => {
            Step::ready("qemu", LABEL, "Installed.")
        }
        Err(RuntimeError::Missing(_)) => match action {
            Some(action) => Step::missing("qemu", LABEL, help, Some(action)),
            None => Step::blocked("qemu", LABEL, help),
        },
        Err(RuntimeError::Refused(reason)) => Step::blocked("qemu", LABEL, reason.clone()),
        Err(RuntimeError::Unsupported(target)) => {
            Step::blocked("qemu", LABEL, runtime::unsupported_message(*target))
        }
    }
}

#[cfg(any(windows, target_os = "linux"))]
fn qemu_check(runner: &AppRunner, guest: GuestTarget) -> Result<runtime::Runtime, RuntimeError> {
    let arch = Arch::host().ok_or(RuntimeError::Missing(runtime::QEMU))?;
    let target = runtime::host_target()
        .ok_or_else(|| RuntimeError::Refused("this platform has no app runner".into()))?;
    runtime::discover_for(&runner.inner.resource_dir, &runner.inner.data_dir, arch, &target, guest)
}

// ----- macOS -----

#[cfg(target_os = "macos")]
const PREPARE: &str = "dial-mac-prepare";
#[cfg(target_os = "macos")]
const TOOLS_ISO: &str = "dial-tools.iso";
/// The virtual Mac's disk. Sparse: it takes only the space the guest writes.
#[cfg(target_os = "macos")]
const MAC_DISK_GIB: &str = "64";

#[cfg(any(target_os = "macos", test))]
fn mac_image_step(have_base: bool, can_prepare: bool, have_tools: bool) -> Step {
    const ID: &str = "macos-image";
    const LABEL: &str = "macOS virtual Mac";
    if have_base {
        Step::ready(ID, LABEL, "Installed.")
    } else if can_prepare && have_tools {
        Step::missing(ID, LABEL,
            "Download macOS from Apple (about 15 GB) and install it into a private virtual Mac. A window opens for macOS setup; Dial imports the Mac when you shut it down.",
            install("Download and install macOS", false))
    } else {
        Step::missing(ID, LABEL,
            "Automatic installation needs macOS 15 or later and Dial's packaged preparation tools. Import a shut-down virtual Mac with its guest companion configured.",
            Some(Action { label: "Import virtual Mac…", kind: "import", admin: false, cancellable: false }))
    }
}

#[cfg(target_os = "macos")]
fn host_setup(runner: &AppRunner) -> HostSetup {
    const BACKEND: &str = "Apple Virtualization";
    if !runtime::host_is_apple_silicon() {
        return setup("macos", BACKEND, GuestTarget::Macos, vec![Step::blocked(
            "virtualization",
            "Apple silicon",
            "Virtual Macs need a Mac with Apple silicon. Use App → VM for VirtualBox or VMware machines.",
        )]);
    }
    let resource = &runner.inner.resource_dir;
    let helper_dir = resource.join(runtime::HELPER_DIR);
    let mut steps = vec![Step::ready(
        "virtualization",
        "Apple Virtualization",
        "Built into macOS on Apple silicon.",
    )];
    steps.push(match super::macos::discover(resource, &runner.inner.data_dir) {
        Err(RuntimeError::Missing(runtime::MACOS_HELPER)) => Step::blocked(
            "helper",
            "Dial's virtual Mac helper",
            "This build of Dial does not include it. Install a Dial release for Apple silicon.",
        ),
        Err(RuntimeError::Refused(reason)) if !helper_dir.join(runtime::HELPER_NAME).is_file() => {
            Step::blocked("helper", "Dial's virtual Mac helper", reason)
        }
        _ => Step::ready("helper", "Dial's virtual Mac helper", "Included with Dial."),
    });
    let base = runtime::macos_base_dir(&runner.inner.data_dir);
    let have_base = runtime::MAC_BASE_FILES.iter().all(|name| base.join(name).is_file());
    steps.push(mac_image_step(have_base, objc2::available!(macos = 15.0),
        helper_dir.join(PREPARE).is_file() && helper_dir.join(TOOLS_ISO).is_file()));
    setup("macos", BACKEND, GuestTarget::Macos, steps)
}

#[cfg(target_os = "macos")]
fn run_install(app: &AppHandle, runner: &AppRunner, step: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if step != "macos-image" || !runtime::host_is_apple_silicon() || !objc2::available!(macos = 15.0) {
        return Err("Nothing to install for that step.".into());
    }
    let confirmed = app
        .dialog()
        .message(
            "Dial will download the latest macOS that this Mac supports from Apple (about 15 GB) and install it into a new private virtual Mac. This takes a while.\n\n\
             When the install finishes, a window opens. In that window:\n\
             1. Complete Setup Assistant with a local account (no Apple ID).\n\
             2. In Terminal, run: sudo sh /Volumes/DIAL_TOOLS/install-dial-companion.sh\n\
             3. Grant Screen Recording and Accessibility to /Library/Application Support/Dial/dial-guest, and turn on automatic login.\n\
             4. Shut the virtual Mac down from the Apple menu.\n\n\
             Dial then imports it. Closing the window cancels and removes the partial install.",
        )
        .title("Install a macOS virtual Mac?")
        .buttons(MessageDialogButtons::OkCancelCustom("Download and install".into(), "Cancel".into()))
        .blocking_show();
    if !confirmed {
        return Err("Cancelled.".into());
    }
    let helper_dir = runner.inner.resource_dir.join(runtime::HELPER_DIR);
    let prepare = helper_dir.join(PREPARE);
    let work = runner.inner.data_dir.join(runtime::APP_RUNNER_DIR).join("prepare");
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    // The prep tool refuses a destination whose parent others can write.
    std::fs::set_permissions(&work, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let work = std::fs::canonicalize(&work).map_err(|e| e.to_string())?;
    let ipsw = work.join("restore.ipsw");
    let dest = work.join(format!("base-{}", std::process::id()));
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
    }
    let mut command = Command::new(&prepare);
    command
        .arg("--download-latest")
        .arg("--consent-given")
        .arg("--ipsw")
        .arg(&ipsw)
        .arg("--dest")
        .arg(&dest)
        .arg("--disk-gib")
        .arg(MAC_DISK_GIB);
    command.arg("--tools-iso").arg(helper_dir.join(TOOLS_ISO));
    let code = stream(app, step, command);
    let code = match code {
        Ok(code) => code,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&dest);
            return Err(error);
        }
    };
    match code {
        0 => {}
        130 => {
            let _ = std::fs::remove_dir_all(&dest);
            return Err("The macOS install was cancelled. Nothing was kept.".into());
        }
        _ => return Err("The macOS install did not finish. See the lines above for the reason.".into()),
    }
    emit(app, step, "Importing the virtual Mac into Dial…");
    crate::app_disk::import_prepared_mac(&runner.inner.data_dir, &dest)
        .map_err(|e| format!("Could not import the virtual Mac: {e}"))?;
    let _ = std::fs::remove_dir_all(&dest);
    // The restore image is only needed to install; free its space.
    let _ = std::fs::remove_file(&ipsw);
    emit(app, step, "The virtual Mac is ready.");
    Ok(())
}

// ----- Windows -----

#[cfg(windows)]
fn system32() -> Option<PathBuf> {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .filter(|root| root.is_absolute())
        .map(|root| root.join("System32"))
}

#[cfg(windows)]
fn winget() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|root| root.is_absolute())
        .map(|root| root.join(r"Microsoft\WindowsApps\winget.exe"))
        .filter(|path| path.exists())
}

/// WHPX is optional; statically importing the DLL would stop Dial itself from
/// starting on hosts without it. Only load the system copy and test the same
/// HypervisorPresent capability that QEMU requires, not just DLL presence.
#[cfg(windows)]
fn hypervisor_present() -> bool {
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
    use windows::core::{s, w};
    // SAFETY: both names are static and nul-terminated. The function signature
    // matches WHvGetCapability and its output pointers live through the call.
    unsafe {
        let Ok(dll) = LoadLibraryExW(w!("WinHvPlatform.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) else { return false };
        let present = GetProcAddress(dll, s!("WHvGetCapability")).is_some_and(|proc| {
            type GetCapability = unsafe extern "system" fn(i32, *mut i32, u32, *mut u32) -> i32;
            let get: GetCapability = std::mem::transmute(proc);
            let (mut present, mut written) = (0, 0);
            let size = size_of::<i32>() as u32;
            get(0 /* WHvCapabilityCodeHypervisorPresent */, &mut present, size, &mut written) >= 0
                && written == size && present != 0
        });
        let _ = FreeLibrary(dll);
        present
    }
}

#[cfg(windows)]
fn host_setup(runner: &AppRunner) -> HostSetup {
    let guest = GuestTarget::Windows;
    let mut steps = Vec::new();
    steps.push(if hypervisor_present() {
        Step::ready("hypervisor", "Windows Hypervisor Platform", "On.")
    } else if RESTART_PENDING.load(Ordering::Relaxed) {
        Step::blocked("hypervisor", "Windows Hypervisor Platform", "Turned on. Restart Windows to finish.")
    } else {
        Step::missing(
            "hypervisor",
            "Windows Hypervisor Platform",
            "The Hyper-V platform QEMU uses for hardware speed. Turning it on needs administrator approval and a restart. Virtualization must also be enabled in your PC's firmware settings.",
            install("Turn on", true),
        )
    });
    let check = qemu_check(runner, guest);
    let action = winget().and(install("Install QEMU", true));
    let help = if action.is_some() {
        "Installs QEMU machine-wide with winget (Program Files). Windows asks for administrator approval."
    } else {
        "winget was not found. Install App Installer from the Microsoft Store, or QEMU from qemu.org into Program Files, then check again."
    };
    steps.push(qemu_step(&check, action, help));
    steps.push(disk_step(guest, &check));
    setup("windows", "Hyper-V (Windows Hypervisor Platform)", guest, steps)
}

// DISM has documented exit codes (including 3010); a PowerShell cmdlet does not
// set $LASTEXITCODE. Do not convert UAC cancellation or a missing status to zero.
#[cfg(windows)]
const ENABLE_HYPERVISOR: &str = r#"
try {
    $p = Start-Process -ErrorAction Stop -FilePath (Join-Path ([Environment]::SystemDirectory) 'dism.exe') -Verb RunAs -Wait -PassThru -WindowStyle Hidden -ArgumentList '/Online','/Enable-Feature','/FeatureName:HypervisorPlatform','/All','/NoRestart'
    if ($null -eq $p.ExitCode) { exit 1 }
    exit $p.ExitCode
} catch { exit 1 }
"#;

#[cfg(windows)]
fn run_install(app: &AppHandle, _runner: &AppRunner, step: &str) -> Result<(), String> {
    match step {
        "hypervisor" => {
            confirm(
                app,
                "Turn on Windows Hypervisor Platform?",
                "Windows asks for administrator approval, then turns on the Windows Hypervisor Platform feature. Restart Windows afterwards to finish.",
                "Turn on",
            )?;
            let powershell = system32()
                .map(|dir| dir.join(r"WindowsPowerShell\v1.0\powershell.exe"))
                .ok_or("Windows PowerShell was not found.")?;
            let mut command = Command::new(powershell);
            command.args(["-NoProfile", "-NonInteractive", "-Command", ENABLE_HYPERVISOR]);
            let code = stream(app, step, command)?;
            // 3010: success, restart required.
            if code != 0 && code != 3010 {
                return Err("Windows did not turn on the hypervisor platform (approval declined or the feature failed).".into());
            }
            RESTART_PENDING.store(true, Ordering::Relaxed);
            emit(app, step, "Done. Restart Windows to finish.");
            Ok(())
        }
        "qemu" => {
            let winget = winget().ok_or("winget was not found. Install App Installer from the Microsoft Store.")?;
            confirm(
                app,
                "Install QEMU?",
                "Dial runs winget to install QEMU (SoftwareFreedomConservancy.QEMU) for all users in Program Files. Windows asks for administrator approval.",
                "Install",
            )?;
            let mut command = Command::new(winget);
            command.args([
                "install",
                "--id",
                "SoftwareFreedomConservancy.QEMU",
                "--exact",
                "--scope",
                "machine",
                "--silent",
                "--accept-package-agreements",
                "--accept-source-agreements",
            ]);
            if stream(app, step, command)? != 0 {
                return Err("winget could not install QEMU. See the lines above.".into());
            }
            emit(app, step, "QEMU is installed.");
            Ok(())
        }
        _ => Err("Nothing to install for that step.".into()),
    }
}

// ----- Linux -----

#[cfg(target_os = "linux")]
const PKEXEC: &str = "/usr/bin/pkexec";

#[cfg(target_os = "linux")]
fn first_existing(paths: &[&'static str]) -> Option<&'static str> {
    paths.iter().copied().find(|p| Path::new(p).exists())
}

/// The CPU's KVM module, from its virtualization flag; `None` when the CPU shows none.
#[cfg(target_os = "linux")]
fn kvm_module() -> Option<&'static str> {
    let info = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    let flags = info.lines().find(|l| l.starts_with("flags"))?;
    let has = |flag: &str| flags.split_whitespace().any(|f| f == flag);
    if has("vmx") {
        Some("kvm_intel")
    } else if has("svm") {
        Some("kvm_amd")
    } else {
        None
    }
}

/// The package manager command that installs QEMU and its UEFI firmware for this architecture.
#[cfg(target_os = "linux")]
fn package_install() -> Option<(&'static str, Vec<&'static str>)> {
    let arm = Arch::host() == Some(Arch::Aarch64);
    if Path::new("/usr/bin/apt-get").exists() {
        let script = if arm {
            "apt-get update && apt-get install -y qemu-system-arm qemu-efi-aarch64"
        } else {
            "apt-get update && apt-get install -y qemu-system-x86 ovmf"
        };
        return Some(("apt-get", vec!["/bin/sh", "-c", script]));
    }
    if Path::new("/usr/bin/dnf").exists() {
        let p = if arm { ["qemu-system-aarch64-core", "edk2-aarch64"] } else { ["qemu-system-x86-core", "edk2-ovmf"] };
        return Some(("dnf", vec!["/usr/bin/dnf", "install", "-y", p[0], p[1]]));
    }
    if Path::new("/usr/bin/pacman").exists() {
        let p = if arm { ["qemu-system-aarch64", "edk2-aarch64"] } else { ["qemu-system-x86", "edk2-ovmf"] };
        return Some(("pacman", vec!["/usr/bin/pacman", "-S", "--needed", "--noconfirm", p[0], p[1]]));
    }
    if Path::new("/usr/bin/zypper").exists() {
        let p = if arm { ["qemu-arm", "qemu-uefi-aarch64"] } else { ["qemu-x86", "qemu-ovmf-x86_64"] };
        return Some(("zypper", vec!["/usr/bin/zypper", "--non-interactive", "install", p[0], p[1]]));
    }
    None
}

#[cfg(target_os = "linux")]
fn current_user() -> Option<String> {
    std::env::var("USER").ok().filter(|name| {
        !name.is_empty()
            && name.len() <= 32
            && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
            && !name.starts_with('-')
    })
}

#[cfg(target_os = "linux")]
fn host_setup(runner: &AppRunner) -> HostSetup {
    let guest = GuestTarget::Linux;
    let mut steps = Vec::new();
    const KVM: &str = "KVM";
    let kvm = Path::new("/dev/kvm");
    let pkexec = Path::new(PKEXEC).exists();
    steps.push(if std::fs::OpenOptions::new().read(true).write(true).open(kvm).is_ok() {
        Step::ready("kvm", KVM, "Available.")
    } else if kvm.exists() {
        if RELOGIN_PENDING.load(Ordering::Relaxed) {
            Step::blocked("kvm", KVM, "Access was granted. Log out and back in to finish.")
        } else if pkexec && current_user().is_some() {
            Step::missing(
                "kvm",
                KVM,
                "KVM is present but your account cannot use it. Dial adds you to the kvm group (asks for your password); then log out and back in.",
                install("Give me access", true),
            )
        } else {
            Step::blocked("kvm", KVM, "Add your account to the kvm group, then log out and back in.")
        }
    } else if Arch::host() == Some(Arch::X86_64) && kvm_module().is_none() {
        Step::blocked(
            "kvm",
            KVM,
            "This CPU reports no virtualization support. Turn on Intel VT-x or AMD-V in your computer's firmware settings.",
        )
    } else if pkexec && kvm_module().is_some() {
        Step::missing("kvm", KVM, "The KVM kernel module is not loaded. Dial loads it (asks for your password).", install("Load KVM", true))
    } else {
        Step::blocked("kvm", KVM, "The KVM kernel module is not loaded. Load it with modprobe, or enable virtualization in your firmware settings.")
    });
    let check = qemu_check(runner, guest);
    let manager = package_install().filter(|_| pkexec);
    let help = match &manager {
        Some((name, _)) => format!("Installs QEMU and UEFI firmware with {name} (asks for your password)."),
        None => "Install QEMU and OVMF (UEFI firmware) with your package manager, then check again.".into(),
    };
    steps.push(qemu_step(&check, manager.and(install("Install QEMU", true)), &help));
    steps.push(disk_step(guest, &check));
    setup("linux", "KVM", guest, steps)
}

#[cfg(target_os = "linux")]
fn run_install(app: &AppHandle, _runner: &AppRunner, step: &str) -> Result<(), String> {
    let argv: Vec<String> = match step {
        "kvm" if Path::new("/dev/kvm").exists() => {
            let user = current_user().ok_or("Dial could not tell your user name.")?;
            let usermod = first_existing(&["/usr/sbin/usermod", "/sbin/usermod"]).ok_or("usermod was not found.")?;
            confirm(app, "Give your account access to KVM?", &format!("Dial runs `usermod -aG kvm {user}` as administrator. Your system asks for your password. Log out and back in afterwards."), "Continue")?;
            vec![usermod.into(), "-aG".into(), "kvm".into(), user]
        }
        "kvm" => {
            let module = kvm_module().ok_or("This CPU reports no virtualization support.")?;
            let modprobe = first_existing(&["/usr/sbin/modprobe", "/sbin/modprobe"]).ok_or("modprobe was not found.")?;
            confirm(app, "Load KVM?", &format!("Dial runs `modprobe {module}` as administrator. Your system asks for your password."), "Continue")?;
            vec![modprobe.into(), module.into()]
        }
        "qemu" => {
            let (name, argv) = package_install().ok_or("No supported package manager was found.")?;
            confirm(app, "Install QEMU?", &format!("Dial installs QEMU and UEFI firmware with {name} as administrator. Your system asks for your password."), "Install")?;
            argv.into_iter().map(String::from).collect()
        }
        _ => return Err("Nothing to install for that step.".into()),
    };
    let mut command = Command::new(PKEXEC);
    command.args(&argv);
    match stream(app, step, command)? {
        0 => {}
        126 | 127 => return Err("Administrator approval was declined.".into()),
        _ => return Err("The install did not finish. See the lines above.".into()),
    }
    if step == "kvm" && argv.iter().any(|a| a == "-aG") {
        RELOGIN_PENDING.store(true, Ordering::Relaxed);
        emit(app, step, "Done. Log out and back in to finish.");
    } else {
        emit(app, step, "Done.");
    }
    Ok(())
}

// ----- Other hosts -----

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
fn host_setup(_: &AppRunner) -> HostSetup {
    setup("other", "none", runtime::preferred_guest_target(), vec![Step::blocked(
        "virtualization",
        "Built-in hypervisor",
        "Dial has no private guest backend for this operating system.",
    )])
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
fn run_install(_: &AppHandle, _: &AppRunner, _: &str) -> Result<(), String> {
    Err("Nothing to install on this system.".into())
}

// ----- Shared -----

#[cfg(any(windows, target_os = "linux"))]
fn confirm(app: &AppHandle, title: &str, message: &str, ok: &str) -> Result<(), String> {
    app.dialog()
        .message(message)
        .title(title)
        .buttons(MessageDialogButtons::OkCancelCustom(ok.into(), "Cancel".into()))
        .blocking_show()
        .then_some(())
        .ok_or_else(|| "Cancelled.".into())
}

fn emit(app: &AppHandle, step: &str, line: &str) {
    let _ = app.emit(EVENT, json!({ "step": step, "line": line }));
}

/// Run `command` with no stdin, sending each output line as a progress event. Returns the exit code.
fn stream(app: &AppHandle, step: &str, mut command: Command) -> Result<i32, String> {
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = command.spawn().map_err(|e| format!("Could not start the installer: {e}"))?;
    let forward = |reader: Box<dyn Read + Send>| {
        let app = app.clone();
        let step = step.to_owned();
        thread::spawn(move || {
            for line in BufReader::new(reader).lines().map_while(Result::ok) {
                let line = line.trim();
                if !line.is_empty() {
                    emit(&app, &step, line);
                }
            }
        })
    };
    let out = child.stdout.take().map(|s| forward(Box::new(s)));
    let err = child.stderr.take().map(|s| forward(Box::new(s)));
    let status = loop {
        if CANCELLABLE.load(Ordering::SeqCst) && CANCEL.load(Ordering::SeqCst) {
            emit(app, step, "Cancelling…");
            stop_child(&mut child);
            for handle in [out, err].into_iter().flatten() {
                let _ = handle.join();
            }
            return Err("Cancelled. Nothing was kept.".into());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        thread::sleep(std::time::Duration::from_millis(150));
    };
    for handle in [out, err].into_iter().flatten() {
        let _ = handle.join();
    }
    Ok(status.code().unwrap_or(-1))
}

/// Ask a cancelled action to stop (SIGINT on Unix, so it can clean up), then end it.
fn stop_child(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("/bin/kill")
            .args(["-INT", &child.id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        for _ in 0..100 {
            if child.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Stop the running install when it is cancellable (see [`Action::cancellable`]). Returns
/// whether a cancel was requested; an elevated action keeps running.
#[tauri::command]
pub fn app_runner_host_cancel(webview: Webview) -> Result<bool, AppRunError> {
    installed::main_view(&webview)?;
    if !BUSY.load(Ordering::SeqCst) || !CANCELLABLE.load(Ordering::SeqCst) {
        return Ok(false);
    }
    CANCEL.store(true, Ordering::SeqCst);
    Ok(true)
}

#[tauri::command]
pub async fn app_runner_host(
    webview: Webview,
    runner: State<'_, AppRunner>,
) -> Result<HostSetup, AppRunError> {
    installed::main_view(&webview)?;
    let runner = runner.inner().clone();
    blocking(move || Ok(host_setup(&runner))).await
}

/// Run the install action for one step of [`HostSetup`], after a native confirmation, and return
/// the setup as it is afterwards.
#[tauri::command]
pub async fn app_runner_host_install(
    webview: Webview,
    app: AppHandle,
    runner: State<'_, AppRunner>,
    step: String,
) -> Result<HostSetup, AppRunError> {
    installed::main_view(&webview)?;
    let runner = runner.inner().clone();
    blocking(move || {
        let offered = host_setup(&runner).steps.into_iter().find_map(|s| {
            (s.id == step)
                .then_some(s.action)
                .flatten()
                .filter(|a| a.kind == "install")
        });
        let Some(action) = offered else {
            return Err(AppRunError::new("not_offered", "That step has nothing to install right now. Check again."));
        };
        let start = crate::app::starting_service(&app).map_err(|e| AppRunError::new("busy", e))?;
        if BUSY.swap(true, Ordering::SeqCst) {
            return Err(AppRunError::new("busy", "Another install is still running."));
        }
        drop(start); // Now visible to the updater as App host setup.
        CANCEL.store(false, Ordering::SeqCst);
        CANCELLABLE.store(action.cancellable, Ordering::SeqCst);
        let result = run_install(&app, &runner, &step);
        CANCELLABLE.store(false, Ordering::SeqCst);
        CANCEL.store(false, Ordering::SeqCst);
        BUSY.store(false, Ordering::SeqCst);
        result.map_err(|message| AppRunError::new("install_failed", message))?;
        Ok(host_setup(&runner))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn elevated_feature_command_propagates_child_status_and_cancellation() {
        let ps = system32().unwrap().join(r"WindowsPowerShell\v1.0\powershell.exe");
        for (mock, code) in [
            ("[pscustomobject]@{ ExitCode = 0 }", 0),
            ("[pscustomobject]@{ ExitCode = 3010 }", 3010),
            ("[pscustomobject]@{ ExitCode = 87 }", 87),
            ("throw 'declined'", 1),
            ("$null", 1),
        ] {
            // Shadow the launch cmdlet: no DISM, UAC prompt or feature change.
            let script = format!("function Start-Process {{ {mock} }}\n{ENABLE_HYPERVISOR}");
            let status = Command::new(&ps).args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .status().unwrap();
            assert_eq!(status.code(), Some(code));
        }
    }

    #[test]
    fn mac_image_action_requires_supported_host_and_bundled_tools() {
        for (base, supported, tools, action) in [
            (false, false, true, Some("import")), (false, true, false, Some("import")),
            (false, true, true, Some("install")), (true, false, true, None),
        ] {
            assert_eq!(mac_image_step(base, supported, tools).action.map(|a| a.kind), action);
        }
    }

    #[test]
    fn setup_serializes_in_the_shape_the_panel_reads() {
        let view = setup(
            "macos",
            "Apple Virtualization",
            GuestTarget::Macos,
            vec![
                Step::ready("virtualization", "Apple Virtualization", "Built in."),
                Step::missing("macos-image", "macOS virtual Mac", "Download.", install("Download and install macOS", false)),
                Step::blocked("helper", "Helper", "Not in this build."),
            ],
        );
        assert!(!view.ready);
        let value = serde_json::to_value(&view).unwrap();
        assert_eq!(value["platform"], "macos");
        assert_eq!(value["guest"], "macos");
        assert_eq!(value["ready"], false);
        assert_eq!(value["steps"][0]["status"], "ready");
        assert_eq!(value["steps"][0]["action"], serde_json::Value::Null);
        assert_eq!(value["steps"][1]["status"], "missing");
        assert_eq!(value["steps"][1]["action"]["kind"], "install");
        assert_eq!(value["steps"][1]["action"]["admin"], false);
        assert_eq!(value["steps"][1]["action"]["cancellable"], true);
        assert!(!install("Install", true).unwrap().cancellable);
        assert_eq!(value["steps"][2]["status"], "blocked");
    }

    #[test]
    fn ready_only_when_every_step_is_ready() {
        let view = setup("linux", "KVM", GuestTarget::Linux, vec![Step::ready("kvm", "KVM", "ok")]);
        assert!(view.ready);
    }
}
