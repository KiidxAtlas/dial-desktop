//! SSH hosts linked to registered VMs, so the agent can run commands and read files in a VM through
//! Dial's `ssh` tool instead of reading its screen.
//!
//! A link is only a name: a host alias from the user's SSH config. Dial never edits SSH config,
//! keys, the VM or its network; it stores the user's choice (App → VM) in its own app data and
//! reports it in the VM's state. When no link is saved, a concrete `Host` alias in `~/.ssh/config`
//! that equals the VM's name (ignoring ASCII case) is used automatically. Running a command over
//! SSH stays subject to the `ssh` tool's own approvals.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[cfg(test)]
mod tests;

/// The links file, under the app data's `app-runner` folder.
const LINKS_FILE: &str = "vm-ssh.json";
const MAX_LINKS: usize = 256;
const MAX_ALIAS_BYTES: usize = 255;
/// Largest links file or SSH config read.
const MAX_READ_BYTES: u64 = 256 << 10;
const MAX_ALIASES: usize = 512;

/// Where a VM's SSH host came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// The user linked it in App → VM.
    Linked,
    /// An SSH config alias equal to the VM's name.
    Auto,
}

/// The SSH host alias the agent uses for one VM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshLink {
    pub host: String,
    pub source: Source,
}

/// A host alias as SSH config names it: no spaces, patterns (`*`, `?`, `!`) or leading `-`, so it
/// can never be read as an option or match more than one host.
pub(crate) fn valid_alias(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= MAX_ALIAS_BYTES
        && !host.starts_with('-')
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-@:".contains(&b))
}

pub(crate) fn valid_vm_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256
}

fn links_file(data_dir: &Path) -> PathBuf {
    data_dir.join("app-runner").join(LINKS_FILE)
}

fn read_bounded(path: &Path) -> Option<Vec<u8>> {
    let file = fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_READ_BYTES + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= MAX_READ_BYTES).then_some(bytes)
}

/// The saved links, VM id → host alias. A missing or unreadable file is no links; invalid entries
/// are dropped.
pub(crate) fn load(data_dir: &Path) -> BTreeMap<String, String> {
    let Some(bytes) = read_bounded(&links_file(data_dir)) else {
        return BTreeMap::new();
    };
    serde_json::from_slice::<BTreeMap<String, String>>(&bytes)
        .unwrap_or_default()
        .into_iter()
        .filter(|(id, host)| valid_vm_id(id) && valid_alias(host))
        .take(MAX_LINKS)
        .collect()
}

/// Link `vm_id` to `host`, or remove its link with `None`. The file is replaced atomically.
pub(crate) fn save(data_dir: &Path, vm_id: &str, host: Option<&str>) -> Result<(), String> {
    if !valid_vm_id(vm_id) {
        return Err("Choose a VM from the provider inventory.".into());
    }
    let mut links = load(data_dir);
    match host {
        Some(host) => {
            if !valid_alias(host) {
                return Err("Use a host alias from your SSH config: letters, digits and . _ - @ : only, with no spaces or patterns.".into());
            }
            if !links.contains_key(vm_id) && links.len() >= MAX_LINKS {
                return Err(format!("Dial keeps at most {MAX_LINKS} VM SSH links."));
            }
            links.insert(vm_id.to_owned(), host.to_owned());
        }
        None => {
            links.remove(vm_id);
        }
    }
    let file = links_file(data_dir);
    let write = || -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec_pretty(&links).map_err(std::io::Error::other)?;
        let tmp = file.with_extension("tmp");
        fs::write(&tmp, json)?;
        fs::rename(&tmp, &file)
    };
    write().map_err(|_| "Dial could not save the VM's SSH link.".to_owned())
}

/// The user's SSH config file, when the home folder is known and absolute.
fn user_config() -> Option<PathBuf> {
    crate::launcher::home_dir()
        .map(|home| home.join(".ssh").join("config"))
}

/// Concrete `Host` aliases in the user's SSH config, in file order. `Include`d files are not read.
pub(crate) fn config_aliases() -> Vec<String> {
    user_config()
        .and_then(|path| read_bounded(&path))
        .map(|bytes| parse_aliases(&String::from_utf8_lossy(&bytes)))
        .unwrap_or_default()
}

/// `Host` names from SSH config text, skipping patterns and negations.
pub(crate) fn parse_aliases(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((key, rest)) = line.split_once(|c: char| c.is_whitespace() || c == '=') else {
            continue;
        };
        if !key.eq_ignore_ascii_case("host") {
            continue;
        }
        let rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '=');
        for name in rest.split_whitespace().map(|n| n.trim_matches('"')) {
            if valid_alias(name) && !out.iter().any(|n| n == name) {
                out.push(name.to_owned());
                if out.len() >= MAX_ALIASES {
                    return out;
                }
            }
        }
    }
    out
}

/// The SSH host for a VM: its saved link, else the one config alias equal to its name.
pub(crate) fn resolve(
    links: &BTreeMap<String, String>,
    aliases: &[String],
    vm_id: &str,
    vm_name: Option<&str>,
) -> Option<SshLink> {
    if let Some(host) = links.get(vm_id) {
        return Some(SshLink {
            host: host.clone(),
            source: Source::Linked,
        });
    }
    let name = vm_name?.trim();
    let mut matches = aliases.iter().filter(|a| a.eq_ignore_ascii_case(name));
    let first = matches.next()?;
    // Two aliases differing only in case are ambiguous; link one explicitly instead.
    matches.next().is_none().then(|| SshLink {
        host: first.clone(),
        source: Source::Auto,
    })
}
