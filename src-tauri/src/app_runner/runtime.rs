//! Where the app runner's bundled parts live, and the checks that they are the ones shipped.
//!
//! The packaged app carries a staged runtime under the Tauri resource directory: `qemu/` holds
//! `runtime.json` (schema 1), `bin/qemu-system-<arch>[.exe]` and the UEFI images under `share/`.
//! The manifest lists every file with its size, sha256 and executable flag, and the target triple
//! it was built for. Nothing is taken from the host's PATH or the environment, nothing is
//! downloaded, and a file is used only when all of these hold:
//!
//! - the manifest's target equals this build's target;
//! - no component of the path is a symlink;
//! - the file's size and sha256 match the manifest.
//!
//! The guest disk is the user-imported raw image in the app's data directory, outside the bundle.
//! Each [`GuestTarget`] names its own disk file there; a target's disk is never a fallback for
//! another one. The `macos` target never uses this runtime: it boots the packaged macOS helper
//! (see [`super::macos`]), and only on Apple silicon; any other Mac is refused before any file is
//! read.
//! When the runtime is not packaged at all, [`discover`] reports the first missing component id;
//! a runtime that is packaged but fails a check is refused with a reason.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Component ids reported as `runtime` in the `missing_runtime` state.
pub const QEMU: &str = "qemu";
pub const FIRMWARE: &str = "firmware";
pub const GUEST_IMAGE: &str = "guest-image";

/// The disk file of the legacy, unlabelled guest, under the app's `app-runner` data directory.
const LEGACY_DISK: &str = "guest.raw";

/// The app data directory that holds the guest disks and the macOS base folder.
pub const APP_RUNNER_DIR: &str = "app-runner";
/// The macOS base folder, under [`APP_RUNNER_DIR`].
pub const MAC_BASE_DIR: &str = "macos";
/// The fixed base files of the macOS guest, under [`macos_base_dir`]: the disk, the firmware
/// variable store, the machine identity and the hardware model. Discovery and import share them.
pub(crate) const MAC_BASE_FILES: [&str; 4] = [
    "Disk.img",
    "AuxiliaryStorage",
    "MachineIdentifier",
    "HardwareModel",
];
/// Upper bound for each serialized Apple hardware model and machine identifier.
pub(crate) const MAC_IDENTITY_LIMIT: u64 = 1 << 20;
/// The packaged macOS helper: `HELPER_DIR/HELPER_NAME` under the resource directory.
pub const HELPER_DIR: &str = "macos-guest";
pub const HELPER_NAME: &str = "dial-mac-guest";
/// Component ids for the macOS backend, reported like [`QEMU`] and [`GUEST_IMAGE`].
pub const MACOS_HELPER: &str = "macos-guest-helper";
pub const MACOS_IMAGE: &str = "macos-guest-image";

/// Which guest a run boots. `default` is the legacy unlabelled disk; `linux` and `windows` are
/// disks the user labelled on import. `macos` boots the packaged macOS helper, not QEMU, and has no
/// disk here: its base files are in [`macos_base_dir`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuestTarget {
    #[default]
    Default,
    Linux,
    Windows,
    Macos,
    Vm,
}

impl GuestTarget {
    /// The disk file under `app-runner/` for this target; `None` when there is no backend.
    pub fn disk_file(self) -> Option<&'static str> {
        match self {
            GuestTarget::Default => Some(LEGACY_DISK),
            GuestTarget::Linux => Some("linux.raw"),
            GuestTarget::Windows => Some("windows.raw"),
            GuestTarget::Macos | GuestTarget::Vm => None,
        }
    }

    /// The name shown to the user for this target.
    pub fn label(self) -> &'static str {
        match self {
            GuestTarget::Default => "default",
            GuestTarget::Linux => "Linux",
            GuestTarget::Windows => "Windows",
            GuestTarget::Macos => "macOS",
            GuestTarget::Vm => "installed VM",
        }
    }
}

/// The staged runtime's directory under the resource directory.
pub const RUNTIME_DIR: &str = "qemu";
/// The manifest inside [`RUNTIME_DIR`].
pub const MANIFEST: &str = "runtime.json";
/// The only manifest schema this build reads.
pub const SCHEMA: u32 = 1;

/// The guest machine's architecture, the same as the host's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
}

impl Arch {
    /// The host's architecture, or `None` when no bundled guest exists for it.
    pub fn host() -> Option<Arch> {
        Arch::from_name(std::env::consts::ARCH)
    }

    pub fn from_name(name: &str) -> Option<Arch> {
        match name {
            "x86_64" => Some(Arch::X86_64),
            "aarch64" => Some(Arch::Aarch64),
            _ => None,
        }
    }

    /// The QEMU system emulator for this architecture, without the platform's executable suffix.
    pub fn qemu_name(self) -> &'static str {
        match self {
            Arch::X86_64 => "qemu-system-x86_64",
            Arch::Aarch64 => "qemu-system-aarch64",
        }
    }

    /// The machine type QEMU runs this architecture as.
    pub fn machine(self) -> &'static str {
        match self {
            Arch::X86_64 => "q35",
            Arch::Aarch64 => "virt",
        }
    }

    /// The read-only UEFI code image shipped with the app.
    pub fn firmware_code(self) -> &'static str {
        match self {
            Arch::X86_64 => "edk2-x86_64-code.fd",
            Arch::Aarch64 => "edk2-aarch64-code.fd",
        }
    }

    /// The UEFI variable store template; each run gets its own copy.
    pub fn firmware_vars(self) -> &'static str {
        match self {
            Arch::X86_64 => "edk2-x86_64-vars.fd",
            Arch::Aarch64 => "edk2-aarch64-vars.fd",
        }
    }
}

/// The target triple this build runs on, in the form the manifest records.
pub fn host_target() -> Option<String> {
    let arch = std::env::consts::ARCH;
    let vendor_os = if cfg!(target_os = "macos") {
        "apple-darwin"
    } else if cfg!(target_os = "windows") {
        "pc-windows-msvc"
    } else if cfg!(target_os = "linux") {
        "unknown-linux-gnu"
    } else {
        return None;
    };
    Some(format!("{arch}-{vendor_os}"))
}

/// One entry of `runtime.json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ManifestFile {
    /// Relative to [`RUNTIME_DIR`], with `/` separators.
    pub path: String,
    pub sha256: String,
    pub size: u64,
    pub executable: bool,
}

/// The parsed `runtime.json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub target: String,
    pub binary: String,
    pub files: Vec<ManifestFile>,
}

impl Manifest {
    pub fn parse(json: &str) -> Result<Manifest, String> {
        serde_json::from_str(json)
            .map_err(|error| format!("the runtime manifest is unreadable: {error}"))
    }

    /// The entry for `path` (relative to [`RUNTIME_DIR`], `/` separated).
    pub fn entry(&self, path: &str) -> Option<&ManifestFile> {
        self.files.iter().find(|file| file.path == path)
    }
}

/// Everything a run needs from disk, each file checked against the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runtime {
    pub arch: Arch,
    pub qemu: PathBuf,
    pub firmware_code: PathBuf,
    pub firmware_vars: PathBuf,
    pub guest_disk: PathBuf,
}

/// Why no runtime was returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    /// A component is not packaged; the id is one of [`QEMU`], [`FIRMWARE`], [`GUEST_IMAGE`].
    Missing(&'static str),
    /// The runtime is packaged but a check failed; the message says which one.
    Refused(String),
    /// The guest target has no backend in this app.
    Unsupported(GuestTarget),
}

/// The message for a target that has no backend on this host.
pub fn unsupported_message(guest: GuestTarget) -> String {
    match guest {
        GuestTarget::Macos => {
            "The macOS guest runs only on Apple silicon Macs; this app has no backend for this host."
                .into()
        }
        _ => format!(
            "This app runner has no {} guest backend yet.",
            guest.label()
        ),
    }
}

/// The message for a missing component, named for the user. The macOS components say where the
/// packager stages the helper and which base files the image needs.
pub fn missing_message(component: &str) -> String {
    match component {
        MACOS_HELPER => format!(
            "The app runner needs the bundled {MACOS_HELPER}: the signed {HELPER_NAME} in the app's {HELPER_DIR} resource folder."
        ),
        MACOS_IMAGE => format!(
            "Import a shut-down virtual Mac ({MACOS_IMAGE}) with its guest companion configured to start at login. Required files under {APP_RUNNER_DIR}/{MAC_BASE_DIR}: {}.",
            MAC_BASE_FILES.join(", ")
        ),
        other => format!("The app runner needs the bundled {other}."),
    }
}

/// Which backend starts a guest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Qemu,
    MacOs,
}

/// Route `guest` to its backend. `macos` goes to the macOS helper on Apple silicon and nowhere
/// else; no other target uses the helper, and `macos` never falls back to QEMU.
pub fn backend_for(guest: GuestTarget, apple_silicon: bool) -> Result<Backend, RuntimeError> {
    match guest {
        GuestTarget::Default | GuestTarget::Linux | GuestTarget::Windows => Ok(Backend::Qemu),
        GuestTarget::Macos if apple_silicon => Ok(Backend::MacOs),
        GuestTarget::Macos => Err(RuntimeError::Unsupported(GuestTarget::Macos)),
        GuestTarget::Vm => Err(RuntimeError::Refused(
            "Select a registered VM instead of importing a private guest disk.".into(),
        )),
    }
}

/// Whether an OS and architecture pair is Apple silicon macOS, the only host with the macOS backend.
pub(crate) fn is_apple_silicon(os: &str, arch: &str) -> bool {
    os == "macos" && arch == "aarch64"
}

/// Whether this build runs on Apple silicon macOS.
pub(crate) fn host_is_apple_silicon() -> bool {
    is_apple_silicon(std::env::consts::OS, std::env::consts::ARCH)
}

/// Select the matching private guest; Intel Macs and other hosts keep the unlabelled disk.
pub(crate) fn preferred_for(os: &str, arch: &str) -> GuestTarget {
    match os {
        "windows" => GuestTarget::Windows,
        "linux" => GuestTarget::Linux,
        "macos" if is_apple_silicon(os, arch) => GuestTarget::Macos,
        _ => GuestTarget::Default,
    }
}

/// The guest target this host prefers when a request does not name one.
pub(crate) fn preferred_guest_target() -> GuestTarget {
    preferred_for(std::env::consts::OS, std::env::consts::ARCH)
}

/// The macOS base folder for the app data directory `data_dir`.
pub(crate) fn macos_base_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(APP_RUNNER_DIR).join(MAC_BASE_DIR)
}

/// The legacy guest disk the user imported, inside the app's data directory.
pub fn guest_disk_path(data_dir: &Path) -> PathBuf {
    data_dir.join("app-runner").join(LEGACY_DISK)
}

/// The disk file for `guest` inside the app's data directory; `None` when it has no backend.
pub fn guest_disk_path_for(data_dir: &Path, guest: GuestTarget) -> Option<PathBuf> {
    guest
        .disk_file()
        .map(|file| data_dir.join("app-runner").join(file))
}

/// Check the packaged runtime for `arch`, built for `host_target`, with the legacy guest disk.
pub fn discover(
    resource_dir: &Path,
    data_dir: &Path,
    arch: Arch,
    host_target: &str,
) -> Result<Runtime, RuntimeError> {
    discover_for(
        resource_dir,
        data_dir,
        arch,
        host_target,
        GuestTarget::Default,
    )
}

/// Check the packaged runtime for `arch`, built for `host_target`, with the disk of `guest`.
/// Only that target's disk is looked at; a missing one is never replaced by another.
pub fn discover_for(
    resource_dir: &Path,
    data_dir: &Path,
    arch: Arch,
    host_target: &str,
    guest: GuestTarget,
) -> Result<Runtime, RuntimeError> {
    let Some(guest_disk) = guest_disk_path_for(data_dir, guest) else {
        return Err(RuntimeError::Unsupported(guest));
    };
    let root = resource_dir.join(RUNTIME_DIR);
    let manifest_path = root.join(MANIFEST);
    if !manifest_path.is_file() {
        // No bundled runtime: use a machine-wide QEMU the user installed from the App panel.
        let system = system_qemu(arch).ok_or(RuntimeError::Missing(QEMU))?;
        checked_guest_disk(data_dir, &guest_disk)?;
        return Ok(Runtime {
            arch,
            qemu: system.qemu,
            firmware_code: system.firmware_code,
            firmware_vars: system.firmware_vars,
            guest_disk,
        });
    }
    let manifest_path =
        checked_path(resource_dir, &root, MANIFEST).map_err(RuntimeError::Refused)?;
    let text = fs::read_to_string(&manifest_path).map_err(|error| {
        RuntimeError::Refused(format!("the runtime manifest is unreadable: {error}"))
    })?;
    let manifest = Manifest::parse(&text).map_err(RuntimeError::Refused)?;
    if manifest.schema != SCHEMA {
        return Err(RuntimeError::Refused(format!(
            "the runtime manifest is schema {}, this app reads schema {SCHEMA}",
            manifest.schema
        )));
    }
    if manifest.target != host_target {
        return Err(RuntimeError::Refused(format!(
            "the bundled runtime is for {}, this app is {host_target}",
            manifest.target
        )));
    }

    let qemu_rel = format!("bin/{}", exe_name(arch.qemu_name()));
    let code_rel = format!("share/{}", arch.firmware_code());
    let vars_rel = format!("share/{}", arch.firmware_vars());
    let qemu_entry = manifest
        .entry(&qemu_rel)
        .ok_or(RuntimeError::Missing(QEMU))?;
    if manifest.binary != qemu_rel {
        return Err(RuntimeError::Refused(
            "unexpected runtime executable path".into(),
        ));
    }
    manifest
        .entry(&code_rel)
        .ok_or(RuntimeError::Missing(FIRMWARE))?;
    manifest
        .entry(&vars_rel)
        .ok_or(RuntimeError::Missing(FIRMWARE))?;
    if !qemu_entry.executable {
        return Err(RuntimeError::Refused(format!(
            "{qemu_rel} is not marked executable"
        )));
    }

    checked_guest_disk(data_dir, &guest_disk)?;
    verify_tree(&root, &manifest).map_err(RuntimeError::Refused)?;
    let qemu = root.join(qemu_rel);
    let firmware_code = root.join(code_rel);
    let firmware_vars = root.join(vars_rel);
    Ok(Runtime {
        arch,
        qemu,
        firmware_code,
        firmware_vars,
        guest_disk,
    })
}

/// The guest disk must exist as a local regular file in the app's own `app-runner` folder.
fn checked_guest_disk(data_dir: &Path, guest_disk: &Path) -> Result<(), RuntimeError> {
    if !guest_disk.is_file() {
        return Err(RuntimeError::Missing(GUEST_IMAGE));
    }
    if !fs::symlink_metadata(data_dir.join("app-runner")).is_ok_and(|m| m.is_dir())
        || !fs::symlink_metadata(guest_disk).is_ok_and(|m| m.is_file())
    {
        return Err(RuntimeError::Refused(
            "the guest disk must be a local regular file, not a link".into(),
        ));
    }
    Ok(())
}

/// A QEMU installed machine-wide: by the official installer under `%ProgramFiles%\qemu` on
/// Windows, or by the system package manager on Linux. Used only when the build bundles no
/// runtime. Never found through `PATH`; trusted by location and ownership instead (see
/// [`system_file`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemQemu {
    pub qemu: PathBuf,
    pub firmware_code: PathBuf,
    pub firmware_vars: PathBuf,
}

/// The machine-wide QEMU and UEFI firmware for `arch`, when one is installed.
pub fn system_qemu(arch: Arch) -> Option<SystemQemu> {
    let (binaries, firmware) = system_candidates(arch)?;
    let qemu = binaries.into_iter().find_map(|path| system_file(&path))?;
    firmware.into_iter().find_map(|(code, vars)| {
        Some(SystemQemu {
            qemu: qemu.clone(),
            firmware_code: system_file(&code)?,
            firmware_vars: system_file(&vars)?,
        })
    })
}

type FirmwarePair = (PathBuf, PathBuf);

/// Where the Linux distributions and the Windows installer put QEMU and its UEFI firmware.
#[cfg(target_os = "linux")]
fn system_candidates(arch: Arch) -> Option<(Vec<PathBuf>, Vec<FirmwarePair>)> {
    let pairs: &[(&str, &str)] = match arch {
        Arch::X86_64 => &[
            ("/usr/share/OVMF/OVMF_CODE_4M.fd", "/usr/share/OVMF/OVMF_VARS_4M.fd"),
            ("/usr/share/OVMF/OVMF_CODE.fd", "/usr/share/OVMF/OVMF_VARS.fd"),
            ("/usr/share/edk2/ovmf/OVMF_CODE.fd", "/usr/share/edk2/ovmf/OVMF_VARS.fd"),
            ("/usr/share/edk2/x64/OVMF_CODE.4m.fd", "/usr/share/edk2/x64/OVMF_VARS.4m.fd"),
            ("/usr/share/qemu/ovmf-x86_64-code.bin", "/usr/share/qemu/ovmf-x86_64-vars.bin"),
            ("/usr/share/qemu/edk2-x86_64-code.fd", "/usr/share/qemu/edk2-i386-vars.fd"),
        ],
        Arch::Aarch64 => &[
            ("/usr/share/AAVMF/AAVMF_CODE.fd", "/usr/share/AAVMF/AAVMF_VARS.fd"),
            ("/usr/share/edk2/aarch64/QEMU_EFI-pflash.raw", "/usr/share/edk2/aarch64/vars-template-pflash.raw"),
            ("/usr/share/edk2/aarch64/QEMU_CODE.fd", "/usr/share/edk2/aarch64/QEMU_VARS.fd"),
            ("/usr/share/qemu/aavmf-aarch64-code.bin", "/usr/share/qemu/aavmf-aarch64-vars.bin"),
            ("/usr/share/qemu/edk2-aarch64-code.fd", "/usr/share/qemu/edk2-arm-vars.fd"),
        ],
    };
    Some((
        vec![Path::new("/usr/bin").join(arch.qemu_name())],
        pairs
            .iter()
            .map(|(code, vars)| (PathBuf::from(code), PathBuf::from(vars)))
            .collect(),
    ))
}

#[cfg(windows)]
fn system_candidates(arch: Arch) -> Option<(Vec<PathBuf>, Vec<FirmwarePair>)> {
    let base = std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .filter(|root| root.is_absolute())?
        .join("qemu");
    let (code, vars) = match arch {
        Arch::X86_64 => ("edk2-x86_64-code.fd", "edk2-i386-vars.fd"),
        Arch::Aarch64 => ("edk2-aarch64-code.fd", "edk2-arm-vars.fd"),
    };
    let share = base.join("share");
    Some((
        vec![base.join(exe_name(arch.qemu_name()))],
        vec![(share.join(code), share.join(vars))],
    ))
}

/// macOS uses Apple's Virtualization framework for its own guests; it has no system QEMU path.
#[cfg(not(any(target_os = "linux", windows)))]
fn system_candidates(_: Arch) -> Option<(Vec<PathBuf>, Vec<FirmwarePair>)> {
    None
}

/// A machine-wide file, resolved to its real path. On Linux it must sit under `/usr`, be owned by
/// root and not be group- or world-writable (package-manager files often are links into
/// `/usr/share/qemu`, so the link is followed and its target checked). On Windows it must stay
/// under `%ProgramFiles%`, which only administrators can write.
#[cfg(target_os = "linux")]
fn system_file(path: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let real = fs::canonicalize(path).ok()?;
    let meta = fs::metadata(&real).ok()?;
    (real.starts_with("/usr/") && meta.is_file() && meta.uid() == 0 && meta.mode() & 0o022 == 0)
        .then_some(real)
}

#[cfg(windows)]
fn system_file(path: &Path) -> Option<PathBuf> {
    let root = std::env::var_os("ProgramFiles").map(PathBuf::from)?;
    let real = fs::canonicalize(path).ok()?;
    let root = fs::canonicalize(root).ok()?;
    (real.starts_with(&root) && fs::metadata(&real).ok()?.is_file()).then(|| {
        // `canonicalize` yields `\\?\C:\...`; QEMU is given the plain form.
        real.to_str()
            .and_then(|text| text.strip_prefix(r"\\?\"))
            .map(PathBuf::from)
            .unwrap_or(real.clone())
    })
}

#[cfg(not(any(target_os = "linux", windows)))]
fn system_file(_: &Path) -> Option<PathBuf> {
    None
}

/// Verify the actual tree, not only argv paths: QEMU can load adjacent DLLs and data files.
fn verify_tree(root: &Path, manifest: &Manifest) -> Result<(), String> {
    let mut wanted: std::collections::HashMap<_, _> = manifest
        .files
        .iter()
        .map(|f| (f.path.as_str(), f))
        .collect();
    let unique: HashSet<_> = wanted.keys().map(|k| k.to_ascii_lowercase()).collect();
    if unique.len() != manifest.files.len() {
        return Err("duplicate runtime files".into());
    }
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            let rel = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_str()
                .ok_or("non-UTF8 runtime path")?;
            let rel = rel.replace(std::path::MAIN_SEPARATOR, "/");
            if kind.is_dir() {
                dirs.push(path);
                continue;
            }
            if !kind.is_file() {
                return Err(format!("symlink or special runtime file: {rel}"));
            }
            if rel == MANIFEST {
                continue;
            }
            let file = wanted
                .remove(rel.as_str())
                .ok_or_else(|| format!("undeclared runtime file: {rel}"))?;
            check_file(&path, file)?;
        }
    }
    if !wanted.is_empty() {
        return Err("missing file in runtime bundle".into());
    }
    Ok(())
}

/// Walk from `resource_dir` down to `rel`, refusing symlinks and any `..`, `/` or drive prefix.
fn checked_path(resource_dir: &Path, root: &Path, rel: &str) -> Result<PathBuf, String> {
    let mut current = resource_dir.to_path_buf();
    let steps = Path::new(RUNTIME_DIR).join(rel);
    for component in steps.components() {
        let Component::Normal(part) = component else {
            return Err(format!("{rel} is not a plain relative path"));
        };
        current.push(part);
        let meta = fs::symlink_metadata(&current)
            .map_err(|error| format!("{} is missing: {error}", current.display()))?;
        if meta.file_type().is_symlink() {
            return Err(format!(
                "{} is a symlink; the runtime refuses links",
                current.display()
            ));
        }
    }
    debug_assert!(current.starts_with(root));
    if !current.is_file() {
        return Err(format!("{} is not a file", current.display()));
    }
    Ok(current)
}

/// The size and sha256 of `path` against `entry`.
fn check_file(path: &Path, entry: &ManifestFile) -> Result<(), String> {
    let mut file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let size = file
        .metadata()
        .map_err(|error| format!("{}: {error}", path.display()))?
        .len();
    if size != entry.size {
        return Err(format!(
            "{} is {size} bytes; the manifest says {}",
            entry.path, entry.size
        ));
    }
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)
        .map_err(|error| format!("{}: {error}", path.display()))
        .map(|_| ())?;
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if !digest.eq_ignore_ascii_case(&entry.sha256) {
        return Err(format!(
            "{} does not match its sha256 in the manifest",
            entry.path
        ));
    }
    Ok(())
}

/// The platform's executable name for a binary (`.exe` on Windows).
pub fn exe_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}
