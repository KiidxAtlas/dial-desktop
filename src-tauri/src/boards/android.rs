//! User-triggered Android battery reading. No device labels, paths or commands come
//! from the webview. `-d` selects exactly one USB transport (never an emulator/Wi-Fi).
use std::{env, path::PathBuf, process::Command, time::Duration};
use serde::Serialize;

#[derive(Debug, PartialEq, Serialize)]
pub struct Battery {
    percent: Option<u8>,
    usb_powered: Option<bool>,
    temperature_c: Option<f64>,
    voltage_mv: Option<u16>,
}

fn parse_battery(text: &str) -> Result<Battery, String> {
    let field = |key: &str| text.lines().filter_map(|s| s.trim().split_once(':'))
        .find(|(k, _)| *k == key).map(|(_, v)| v.trim());
    let number = |key| field(key)?.parse::<u32>().ok();
    let percent = number("level").zip(number("scale")).filter(|(l, s)| *s > 0 && l <= s)
        .map(|(l, s)| (100 * u64::from(l) / u64::from(s)) as u8);
    let battery = Battery {
        percent,
        usb_powered: field("USB powered").and_then(|s| s.parse().ok()),
        temperature_c: field("temperature").and_then(|s| s.parse::<i16>().ok()).map(|t| f64::from(t) / 10.0),
        voltage_mv: field("voltage").and_then(|s| s.parse().ok()),
    };
    if battery.percent.is_none() && battery.usb_powered.is_none() && battery.temperature_c.is_none() && battery.voltage_mv.is_none() {
        return Err("Android returned no recognized battery readings.".into());
    }
    Ok(battery)
}

fn adb_path() -> Option<PathBuf> {
    let name = if cfg!(windows) { "adb.exe" } else { "adb" };
    ["ANDROID_HOME", "ANDROID_SDK_ROOT"].into_iter().filter_map(env::var_os)
        .map(|p| PathBuf::from(p).join("platform-tools"))
        .chain(env::split_paths(&env::var_os("PATH").unwrap_or_default()))
        .filter(|p| p.is_absolute()).map(|p| p.join(name)).find(|p| p.is_file())
}

fn read_battery() -> Result<Battery, String> {
    let program = adb_path().ok_or("Android SDK Platform-Tools not found. Make adb available to Dial via ANDROID_HOME or PATH.")?;
    let output = crate::launcher::run_captured(
        Command::new(program).args(["-H", "127.0.0.1", "-P", "5037", "-d", "shell", "dumpsys", "battery"])
            .env_remove("ADB_SERVER_SOCKET").env_remove("ANDROID_SERIAL"),
        Duration::from_secs(10), 32 * 1024, |e| format!("Could not start adb: {e}"),
    )?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(if err.contains("unauthorized") { "Unlock the phone and approve USB debugging for this computer, then retry." }
            else if err.contains("more than one") { "Connect only one USB Android debugging device, then retry." }
            else { "ADB could not read the phone. Check the data cable, USB debugging, and authorization." }.into());
    }
    parse_battery(&String::from_utf8_lossy(&output.stdout))
}

#[tauri::command]
pub async fn board_android_battery(webview: tauri::Webview) -> Result<Battery, String> {
    crate::app::main_only(&webview, "read Android USB battery status")?;
    tauri::async_runtime::spawn_blocking(read_battery).await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_only_expected_fields_and_preserves_missing_values() {
        assert_eq!(parse_battery("  USB powered: true\r\n  level: 3\n  scale: 4\nvoltage: 4090\ntemperature: -16\nprivate: hidden").unwrap(), Battery {
            percent: Some(75), usb_powered: Some(true), voltage_mv: Some(4090), temperature_c: Some(-1.6),
        });
        assert_eq!(parse_battery("USB powered: false\nlevel: 90\nscale: 0").unwrap().percent, None);
        assert!(parse_battery("permission denied").is_err());
        assert!(parse_battery("level: 110\nscale: 100").is_err());
    }
}
