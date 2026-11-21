//! Reads the one VMX setting group the console needs. Read-only: the user's `.vmx` is never written,
//! and no credential setting is read.

use std::fs;
use std::path::{Path, PathBuf};

/// Largest `.vmx` read; real files are a few kilobytes.
const MAX_VMX_BYTES: u64 = 1 << 20;

/// The VNC endpoint a VM declares. Only loopback endpoints are accepted; see [`settings`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Vnc {
    pub(super) ip: std::net::IpAddr,
    pub(super) port: u16,
}

/// Load the inventoried `.vmx`. The locator must already be absolute and canonical, so a stale or
/// relative path from the inventory is refused instead of being resolved. Returns the path in its
/// normal form (see [`normalize`]), which is what vmrun and the process checks compare against.
pub(super) fn load(locator: &Path) -> Result<(PathBuf, Vnc), String> {
    let locator = normalize(locator);
    if !locator.is_absolute() {
        return Err("the VM path must be absolute".into());
    }
    let is_vmx = locator
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("vmx"));
    if !is_vmx {
        return Err("the VM is not a .vmx file".into());
    }
    let canonical = fs::canonicalize(&locator)
        .map_err(|_| "the VM's .vmx file is missing; refresh the VM list".to_owned())?;
    if !same_location(&canonical, &locator) {
        return Err("the VM path is not canonical; refresh the VM list".into());
    }
    let normal = normalize(&canonical);
    let metadata =
        fs::metadata(&canonical).map_err(|_| "the VM's .vmx file is unreadable".to_owned())?;
    if !metadata.is_file() || metadata.len() > MAX_VMX_BYTES {
        return Err("the VM's .vmx file is not a regular file of supported size".into());
    }
    let text = fs::read_to_string(&canonical)
        .map_err(|_| "the VM's .vmx file is unreadable".to_owned())?;
    Ok((normal, settings(&text)?))
}

/// Strip the Windows verbatim prefix (`\\?\C:\...` or `\\?\UNC\...`) that `fs::canonicalize`
/// returns, so a canonical path compares equal to the plain path the inventory recorded. Elsewhere
/// this is the identity.
pub(super) fn normalize(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(text) = path.to_str() {
            if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
                return PathBuf::from(format!(r"\\{rest}"));
            }
            if let Some(rest) = text.strip_prefix(r"\\?\") {
                return PathBuf::from(rest);
            }
        }
    }
    path.to_path_buf()
}

/// Two paths name the same location. Windows paths are compared without case, after normalizing
/// verbatim prefixes; elsewhere they must match exactly.
pub(super) fn same_location(a: &Path, b: &Path) -> bool {
    let (a, b) = (normalize(a), normalize(b));
    if cfg!(windows) {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}

/// The VNC settings in `.vmx` text. Keys are case-insensitive; a repeated key is refused.
///
/// The listen address must be declared (`RemoteDisplay.vnc.ip`) and must be a loopback IP literal.
/// A missing, wildcard or LAN address would expose an unauthenticated console to the network, so
/// the console refuses before it starts a VM whose `.vmx` does not already say loopback.
pub(super) fn settings(text: &str) -> Result<Vnc, String> {
    let mut enabled: Option<String> = None;
    let mut port: Option<String> = None;
    let mut ip: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let slot = match key.trim().to_ascii_lowercase().as_str() {
            "remotedisplay.vnc.enabled" => &mut enabled,
            "remotedisplay.vnc.port" => &mut port,
            "remotedisplay.vnc.ip" => &mut ip,
            _ => continue,
        };
        if slot.is_some() {
            return Err("the VM's .vmx sets its VNC settings more than once".into());
        }
        *slot = Some(unquote(value.trim()).to_owned());
    }
    if !enabled.is_some_and(|v| v.eq_ignore_ascii_case("true")) {
        return Err(
            "VNC is not enabled for this VM; turn on remote display (VNC) in the VM's settings and start it again"
                .into(),
        );
    }
    let ip = ip.ok_or_else(|| {
        "the VM does not declare a VNC listen address; set remote display (VNC) to listen on 127.0.0.1 only"
            .to_owned()
    })?;
    let ip = ip
        .parse::<std::net::IpAddr>()
        .ok()
        .filter(std::net::IpAddr::is_loopback)
        .ok_or_else(|| {
            "the VM's VNC listen address is not loopback; Dial cannot connect safely".to_owned()
        })?;
    let port = port
        .and_then(|p| p.parse::<u16>().ok())
        .filter(|p| *p != 0)
        .ok_or_else(|| {
            "the VM has no usable VNC port; set a port for remote display (VNC) in the VM's settings"
                .to_owned()
        })?;
    Ok(Vnc { ip, port })
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
}

/// Whether a path reported by `vmrun list` names the same file as `vmx`.
pub(super) fn same_file(listed: &str, vmx: &Path) -> bool {
    let listed = Path::new(listed.trim());
    same_location(listed, vmx) || fs::canonicalize(listed).is_ok_and(|p| same_location(&p, vmx))
}
