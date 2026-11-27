//! Explicit USB-only Apple diagnostic. Keep device identifiers inside the process;
//! never pair, ask for a full property dump or forward tool output to the UI.
use std::{env, path::PathBuf, process::Command, time::Duration};

use serde::Serialize;

#[derive(Debug, PartialEq, Serialize)]
pub struct AppleInfo {
    model: String,
    os_version: String,
}

fn run(tool: &str, args: &[&str]) -> Result<String, String> {
    let name = format!("{tool}{}", env::consts::EXE_SUFFIX);
    let path: PathBuf = env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .filter(|p| p.is_absolute()).map(|p| p.join(&name)).find(|p| p.is_file())
        .ok_or_else(|| format!("{tool} not found. Make the libimobiledevice tools available on Dial’s PATH."))?;
    let output = crate::launcher::run_captured(
        Command::new(path).args(args).env_remove("USBMUXD_SOCKET_ADDRESS"),
        Duration::from_secs(5), 8192, |_| format!("Could not start {tool}."),
    )?;
    if !output.status.success() {
        return Err(format!("{tool} could not read the device. Check the cable, unlock the device and verify that this computer is trusted."));
    }
    String::from_utf8(output.stdout).map_err(|_| "Unexpected device response.".into())
}

fn query(mut run: impl FnMut(&str, &[&str]) -> Result<String, String>) -> Result<AppleInfo, String> {
    let devices = run("idevice_id", &["-l"])?;
    let mut ids = devices.lines().map(str::trim).filter(|s| !s.is_empty());
    let id = ids.next().ok_or("No USB iPhone or iPad found.")?;
    if ids.next().is_some() { return Err("Connect only the Apple device you want to inspect.".into()); }
    if !(1..=64).contains(&id.len()) || !id.starts_with(|c: char| c.is_ascii_hexdigit()) || !id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err("Unrecognized USB device identifier.".into());
    }
    let mut property = |key: &str| {
        let raw = run("ideviceinfo", &["-s", "-u", id, "-k", key])?;
        let value = raw.trim();
        if value.is_empty() || value.len() > 64 || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b".,-_ ".contains(&b)) {
            return Err(format!("No usable {key} reported by the device."));
        }
        Ok(value.to_owned())
    };
    Ok(AppleInfo { model: property("ProductType")?, os_version: property("ProductVersion")? })
}

#[tauri::command]
pub async fn board_apple_info(webview: tauri::Webview) -> Result<AppleInfo, String> {
    crate::app::main_only(&webview, "read Apple USB model and OS")?;
    tauri::async_runtime::spawn_blocking(|| query(run)).await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_two_named_properties_from_one_usb_target() {
        let info = query(|tool, args| Ok(match (tool, args) {
            ("idevice_id", ["-l"]) => "abcd-12\r\n",
            ("ideviceinfo", ["-s", "-u", "abcd-12", "-k", "ProductType"]) => "iPad13,4",
            ("ideviceinfo", ["-s", "-u", "abcd-12", "-k", "ProductVersion"]) => "18.1",
            _ => panic!("unexpected call"),
        }.into())).unwrap();
        assert_eq!((info.model.as_str(), info.os_version.as_str()), ("iPad13,4", "18.1"));
        for list in ["", "abc\ndef", "-abc", "bad/value"] {
            assert!(query(|tool, _| { assert_eq!(tool, "idevice_id"); Ok(list.into()) }).is_err());
        }
    }
}
