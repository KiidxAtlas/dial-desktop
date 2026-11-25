//! Read-only inventory of the local VMs that VirtualBox and VMware already know about.
//!
//! VirtualBox: the trusted `VBoxManage` executable is asked for `list vms`, which reads the
//! registry the tool itself owns (so `VBOX_USER_HOME` is honoured by the tool, not by Dial). The
//! run is time-limited and its output is size-limited. No VM is started.
//!
//! VMware: registered machines come from the inventory files the products keep: Fusion's
//! `vmInventory` (Preferences and Application Support), and Workstation's `inventory.vmls`. These
//! are read as bounded text with one aggregate byte budget. `vmrun` has no "list registered VMs"
//! command (`vmrun list` shows running VMs only), so it is only recorded as the launch tool.
//!
//! Trust: tools are fixed locations (never `PATH`). Environment-derived directories are used only
//! when absolute. Notices never carry paths, names or file contents.
//!
//! Ids are `virtualbox:<uuid>` and `vmware:<hash>`. The VMware hash covers the canonical `.vmx`
//! path, so an id never reveals a path. The locator (the VirtualBox UUID or the canonical `.vmx`
//! path) and the tool path are kept out of serialized output.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};

#[cfg(test)]
mod tests;

/// The VirtualBox machine list is bounded in time and output.
const VBOX_LIST_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_VBOX_LIST_BYTES: u64 = 64 << 10;
/// Aggregate bytes read from all VMware inventory files together.
const MAX_INVENTORY_BYTES: u64 = 1 << 20;
/// Longest config path accepted; longer entries are malformed.
const MAX_PATH_BYTES: usize = 4096;
/// Most `.vmx` candidates checked per run, so malformed inventories cannot stall the scan.
const MAX_VMWARE_ENTRIES: usize = 1024;
/// Most machines listed per provider; the rest are reported in a notice.
const MAX_MACHINES_PER_PROVIDER: usize = 256;
/// Windows: no console window for the child process.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The hypervisor that owns a machine.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Provider {
    VirtualBox,
    Vmware,
}

/// One registered VM. `tool` and `locator` stay on the host; the UI sees only the id and name.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Machine {
    pub id: String,
    pub name: String,
    pub provider: Provider,
    /// Whether the provider reports the machine running now: `None` when it cannot tell (VMware,
    /// or VirtualBox's running list failed). Connecting to a running machine attaches to it.
    pub running: Option<bool>,
    #[serde(skip)]
    pub tool: PathBuf,
    #[serde(skip)]
    pub locator: String,
}

/// The machines found on this host, and what could not be listed, in plain words.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Inventory {
    pub machines: Vec<Machine>,
    pub notices: Vec<String>,
}

/// Inventory of this host's VirtualBox and VMware machines.
pub(crate) fn discover() -> Inventory {
    discover_in(&Locations::current())
}

fn discover_in(locations: &Locations) -> Inventory {
    let mut inventory = Inventory::default();
    discover_virtualbox(locations, &mut inventory);
    discover_vmware(locations, &mut inventory);
    inventory
}

/// Where the tools and inventories live. `current()` fills it from the host; tests fill it from
/// fixtures so that no real home directory or installed product is used.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Locations {
    virtualbox_tools: Vec<PathBuf>,
    vbox_list_timeout: Duration,
    /// Every VMware inventory file that may exist; each present one is read.
    vmware_inventories: Vec<PathBuf>,
    vmware_tools: Vec<PathBuf>,
}

impl Locations {
    fn current() -> Self {
        let home = home_dir();
        Self {
            virtualbox_tools: virtualbox_tools(),
            vbox_list_timeout: VBOX_LIST_TIMEOUT,
            vmware_inventories: vmware_inventories(home.as_deref()),
            vmware_tools: vmware_tools(),
        }
    }
}

/// A path from the environment, only when it is set, non-empty and absolute.
fn absolute_path(value: Option<OsString>) -> Option<PathBuf> {
    value
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

fn home_dir() -> Option<PathBuf> {
    absolute_path(env::var_os("HOME")).or_else(|| absolute_path(env::var_os("USERPROFILE")))
}

#[cfg(windows)]
fn program_files_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)"]
        .iter()
        .filter_map(|key| absolute_path(env::var_os(key)))
        .collect();
    if dirs.is_empty() {
        dirs.push(PathBuf::from(r"C:\Program Files"));
    }
    dirs
}

#[cfg(target_os = "macos")]
fn virtualbox_tools() -> Vec<PathBuf> {
    vec![PathBuf::from(
        "/Applications/VirtualBox.app/Contents/MacOS/VBoxManage",
    )]
}

#[cfg(windows)]
fn virtualbox_tools() -> Vec<PathBuf> {
    program_files_dirs()
        .into_iter()
        .map(|dir| dir.join("Oracle").join("VirtualBox").join("VBoxManage.exe"))
        .collect()
}

#[cfg(not(any(target_os = "macos", windows)))]
fn virtualbox_tools() -> Vec<PathBuf> {
    ["/usr/bin/VBoxManage", "/usr/lib/virtualbox/VBoxManage"]
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

#[cfg(target_os = "macos")]
fn vmware_tools() -> Vec<PathBuf> {
    vec![PathBuf::from(
        "/Applications/VMware Fusion.app/Contents/Library/vmrun",
    )]
}

#[cfg(windows)]
fn vmware_tools() -> Vec<PathBuf> {
    program_files_dirs()
        .into_iter()
        .map(|dir| {
            dir.join("VMware")
                .join("VMware Workstation")
                .join("vmrun.exe")
        })
        .collect()
}

#[cfg(not(any(target_os = "macos", windows)))]
fn vmware_tools() -> Vec<PathBuf> {
    vec![PathBuf::from("/usr/bin/vmrun")]
}

/// Fusion keeps its inventory under Preferences and, in newer releases, Application Support.
#[cfg(target_os = "macos")]
fn vmware_inventories(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    vec![
        home.join("Library/Preferences/VMware Fusion/vmInventory"),
        home.join("Library/Application Support/VMware Fusion/vmInventory"),
    ]
}

#[cfg(windows)]
fn vmware_inventories(_home: Option<&Path>) -> Vec<PathBuf> {
    absolute_path(env::var_os("APPDATA"))
        .map(|appdata| vec![appdata.join("VMware").join("inventory.vmls")])
        .unwrap_or_default()
}

#[cfg(not(any(target_os = "macos", windows)))]
fn vmware_inventories(home: Option<&Path>) -> Vec<PathBuf> {
    home.map(|home| vec![home.join(".vmware/inventory.vmls")])
        .unwrap_or_default()
}

fn discover_virtualbox(locations: &Locations, inventory: &mut Inventory) {
    // Without the tool nothing is listed, and the product is simply not installed here.
    let Some(tool) = first_executable(&locations.virtualbox_tools) else {
        return;
    };
    let Some(stdout) = list_vms(&tool, "vms", locations.vbox_list_timeout) else {
        inventory.notices.push(
            "VirtualBox's machine list could not be read: VBoxManage failed, timed out or printed too much."
                .into(),
        );
        return;
    };
    // Which are running now. A failure leaves the state unknown rather than hiding machines.
    let running: Option<Vec<String>> = list_vms(&tool, "runningvms", locations.vbox_list_timeout)
        .map(|out| out.lines().filter_map(parse_list_line).map(|(_, uuid)| uuid).collect());

    let mut skipped = 0usize;
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let Some((name, uuid)) = parse_list_line(line) else {
            skipped += 1;
            continue;
        };
        let id = format!("virtualbox:{uuid}");
        if inventory.machines.iter().any(|machine| machine.id == id) {
            continue;
        }
        if count_provider(inventory, Provider::VirtualBox) == MAX_MACHINES_PER_PROVIDER {
            inventory.notices.push(format!(
                "VirtualBox lists only the first {MAX_MACHINES_PER_PROVIDER} machines."
            ));
            break;
        }
        inventory.machines.push(Machine {
            id,
            name,
            provider: Provider::VirtualBox,
            running: running.as_ref().map(|r| r.contains(&uuid)),
            tool: tool.clone(),
            locator: uuid,
        });
    }
    if skipped > 0 {
        inventory.notices.push(format!(
            "{skipped} VirtualBox line(s) were not in the expected form and were skipped."
        ));
    }
}

/// Runs `<tool> list <which>` (`vms` or `runningvms`) with a time limit and an output cap. Returns
/// stdout only when the tool exits successfully within both limits. The child is killed on every
/// other path.
fn list_vms(tool: &Path, which: &str, timeout: Duration) -> Option<String> {
    let mut command = Command::new(tool);
    command
        .args(["list", which])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().ok()?;
    let Some(pipe) = child.stdout.take() else {
        stop(&mut child);
        return None;
    };
    // The reader stops at the cap, so an oversized stream cannot block the wait below.
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let read = pipe
            .take(MAX_VBOX_LIST_BYTES + 1)
            .read_to_end(&mut buf)
            .ok()
            .map(|_| buf);
        let _ = sender.send(read);
    });
    let output = match receiver.recv_timeout(timeout) {
        Ok(Some(buf)) if buf.len() as u64 <= MAX_VBOX_LIST_BYTES => buf,
        _ => {
            stop(&mut child);
            return None;
        }
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return status
                    .success()
                    .then(|| String::from_utf8_lossy(&output).into_owned());
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                stop(&mut child);
                return None;
            }
        }
    }
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// One `list vms` line: `"name" {uuid}`. Returns the name and the lowercase UUID.
fn parse_list_line(line: &str) -> Option<(String, String)> {
    let line = line.trim().strip_suffix('}')?;
    let open = line.rfind('{')?;
    let uuid = normalize_uuid(&line[open..])?;
    let name = line[..open]
        .trim_end()
        .strip_prefix('"')?
        .strip_suffix('"')?
        .trim();
    (!name.is_empty()).then(|| (name.to_owned(), uuid))
}

fn discover_vmware(locations: &Locations, inventory: &mut Inventory) {
    let mut budget = MAX_INVENTORY_BYTES;
    let mut entries: Vec<(String, Option<String>)> = Vec::new();
    let mut unreadable = 0usize;
    for path in &locations.vmware_inventories {
        if !path.is_file() {
            continue;
        }
        let Some(bytes) = read_bounded(path, budget) else {
            unreadable += 1;
            continue;
        };
        budget -= bytes.len() as u64;
        if bytes.starts_with(b"bplist00") {
            inventory.notices.push(
                "VMware Fusion's inventory is in a binary format that Dial cannot read yet.".into(),
            );
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        if text.trim_start().starts_with('<') {
            entries.extend(plist_vmx_paths(&text));
        } else {
            entries.extend(vmls_entries(&text));
        }
    }
    if unreadable > 0 {
        inventory.notices.push(format!(
            "{unreadable} VMware inventory file(s) could not be read or exceeded the read budget."
        ));
    }
    if entries.is_empty() {
        return;
    }
    let Some(tool) = first_executable(&locations.vmware_tools) else {
        inventory.notices.push(
            "VMware has registered machines, but vmrun was not found; they are hidden.".into(),
        );
        return;
    };

    let mut skipped = 0usize;
    for (attempt, (config, display_name)) in entries.into_iter().enumerate() {
        if attempt == MAX_VMWARE_ENTRIES {
            inventory.notices.push(format!(
                "VMware inventory lists more than {MAX_VMWARE_ENTRIES} entries; the rest were ignored."
            ));
            break;
        }
        let Some(vmx) = canonical_vmx(&config) else {
            skipped += 1;
            continue;
        };
        let locator = vmx.to_string_lossy().into_owned();
        let id = format!("vmware:{}", digest_hex(&locator));
        if inventory.machines.iter().any(|machine| machine.id == id) {
            continue;
        }
        if count_provider(inventory, Provider::Vmware) == MAX_MACHINES_PER_PROVIDER {
            inventory.notices.push(format!(
                "VMware lists only the first {MAX_MACHINES_PER_PROVIDER} machines."
            ));
            break;
        }
        let name = display_name
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
            .or_else(|| {
                vmx.file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "Virtual machine".into());
        inventory.machines.push(Machine {
            id,
            name,
            provider: Provider::Vmware,
            running: None,
            tool: tool.clone(),
            locator,
        });
    }
    if skipped > 0 {
        inventory.notices.push(format!(
            "{skipped} VMware machine(s) were skipped: their .vmx file is missing, unreadable or malformed."
        ));
    }
}

fn count_provider(inventory: &Inventory, provider: Provider) -> usize {
    inventory
        .machines
        .iter()
        .filter(|machine| machine.provider == provider)
        .count()
}

/// The `.vmx` string values in an XML property list, with no display name each.
fn plist_vmx_paths(text: &str) -> Vec<(String, Option<String>)> {
    const OPEN: &str = "<string>";
    const CLOSE: &str = "</string>";
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        let body = &rest[start + OPEN.len()..];
        let Some(end) = body.find(CLOSE) else {
            break;
        };
        let value = decode_entities(body[..end].trim());
        if has_vmx_extension(&value) {
            out.push((value, None));
        }
        rest = &body[end + CLOSE.len()..];
    }
    out
}

/// The `(config, DisplayName)` of each `vmlistN` entry in a `vmInventory` or `inventory.vmls`
/// text file, in index order.
fn vmls_entries(text: &str) -> Vec<(String, Option<String>)> {
    let mut by_index: BTreeMap<u32, (Option<String>, Option<String>)> = BTreeMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some(rest) = key.trim().strip_prefix("vmlist") else {
            continue;
        };
        let Some((index, field)) = rest.split_once('.') else {
            continue;
        };
        let Ok(index) = index.parse::<u32>() else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_owned();
        let slot = by_index.entry(index).or_default();
        if field == "config" {
            slot.0 = Some(value);
        } else if field.eq_ignore_ascii_case("displayname") {
            slot.1 = Some(value);
        }
    }
    by_index
        .into_values()
        .filter_map(|(config, name)| config.map(|config| (config, name)))
        .collect()
}

fn has_vmx_extension(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("vmx"))
}

/// The canonical path of an existing absolute `.vmx` file, or `None` for anything else.
fn canonical_vmx(config: &str) -> Option<PathBuf> {
    let path = Path::new(config);
    if config.len() > MAX_PATH_BYTES || !path.is_absolute() {
        return None;
    }
    let canonical = strip_verbatim(fs::canonicalize(path).ok()?);
    (canonical.is_file() && has_vmx_extension(&canonical.to_string_lossy())).then_some(canonical)
}

/// `canonicalize` yields `\\?\` paths on Windows; hand back the ordinary form.
fn strip_verbatim(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path
}

/// The trusted `VBoxManage`, when VirtualBox is installed in its standard location.
pub(crate) fn virtualbox_tool() -> Option<PathBuf> {
    first_executable(&virtualbox_tools())
}

/// The trusted `vmrun`, when VMware is installed in its standard location.
pub(crate) fn vmware_tool() -> Option<PathBuf> {
    first_executable(&vmware_tools())
}

/// The first candidate that is a regular file with an execute bit (on Unix).
fn first_executable(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|candidate| crate::launcher::is_executable(candidate))
        .cloned()
}


/// A short, stable, path-free id suffix: the first 16 bytes of SHA-256, in hex.
fn digest_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Reads a whole file of at most `max` bytes; `None` when it is missing, unreadable or larger.
fn read_bounded(path: &Path, max: u64) -> Option<Vec<u8>> {
    let file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= max).then_some(bytes)
}

/// Decodes the five XML entities and numeric character references. Anything unknown is kept
/// as written.
fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let semi = rest
            .as_bytes()
            .iter()
            .take(12)
            .position(|&byte| byte == b';');
        let decoded = semi.and_then(|end| {
            let entity = &rest[1..end];
            let ch = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => match entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                {
                    Some(hex) => u32::from_str_radix(hex, 16).ok().and_then(char::from_u32),
                    None => entity
                        .strip_prefix('#')
                        .and_then(|dec| dec.parse::<u32>().ok())
                        .and_then(char::from_u32),
                },
            }?;
            Some((ch, end))
        });
        match decoded {
            Some((ch, end)) => {
                out.push(ch);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A VirtualBox UUID as lowercase hyphenated text, with or without braces.
fn normalize_uuid(raw: &str) -> Option<String> {
    let text = raw
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .to_ascii_lowercase();
    let well_formed = text.len() == 36
        && text.bytes().enumerate().all(|(i, byte)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        });
    well_formed.then_some(text)
}
