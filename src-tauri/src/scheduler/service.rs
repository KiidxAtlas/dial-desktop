//! Schedules that outlive the app: `dial schedule run -C ROOT` registered with the per-user
//! service manager (launchd on macOS, `systemd --user` on Linux). The OS starts it at login and
//! restarts it after a failure, so saved schedules keep running after Dial quits.
//!
//! A sleeping computer runs nothing (a closed laptop lid sleeps it unless it is in clamshell mode
//! on power). On wake the scheduler runs whatever fell due: the engine compares each entry's last
//! run with its interval. The engine's per-project lease still allows only one scheduler, so the
//! app's own runner stays off for a project with a registered service.

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[cfg(test)]
mod tests;

/// How long a service-manager command may take.
const MANAGER_TIMEOUT: Duration = Duration::from_secs(20);
/// After a cooperative stop, how long the scheduler may take to cancel its runs.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// What Settings › Schedules shows for one project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    /// This OS has a supported per-user service manager.
    pub supported: bool,
    pub installed: bool,
    /// Where the scheduler's output goes (a file, or the journal command on Linux).
    pub log: Option<String>,
    /// Why it is unsupported, or a caveat to show.
    pub detail: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Manager {
    Launchd,
    Systemd,
}

impl Manager {
    pub fn current() -> Option<Self> {
        Self::for_os(std::env::consts::OS)
    }

    pub fn for_os(os: &str) -> Option<Self> {
        match os {
            "macos" => Some(Self::Launchd),
            "linux" => Some(Self::Systemd),
            _ => None,
        }
    }
}

/// A stable service name per project folder.
pub fn label(root: &str) -> String {
    let digest = Sha256::digest(root.as_bytes());
    let hex: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    format!("dev.dial.desktop.schedule.{hex}")
}

/// The per-user folders a service needs.
#[derive(Clone, Debug)]
pub struct Paths {
    pub home: PathBuf,
    /// `$XDG_CONFIG_HOME` on Linux; `None` uses `~/.config`.
    pub config: Option<PathBuf>,
    /// The app's folder, for macOS log files.
    pub app_dir: PathBuf,
}

impl Paths {
    pub fn current() -> Option<Self> {
        let home = crate::launcher::home_dir()?;
        let app_dir = crate::launcher::app_dir(Some(&home))?;
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute());
        Some(Self {
            home,
            config,
            app_dir,
        })
    }

    pub fn definition(&self, manager: Manager, label: &str) -> PathBuf {
        match manager {
            Manager::Launchd => self
                .home
                .join("Library/LaunchAgents")
                .join(format!("{label}.plist")),
            Manager::Systemd => self
                .config
                .clone()
                .unwrap_or_else(|| self.home.join(".config"))
                .join("systemd/user")
                .join(format!("{label}.service")),
        }
    }

    pub fn log(&self, manager: Manager, label: &str) -> String {
        match manager {
            Manager::Launchd => self
                .app_dir
                .join("schedule-logs")
                .join(format!("{label}.log"))
                .to_string_lossy()
                .into_owned(),
            Manager::Systemd => format!("journalctl --user -u {label}.service"),
        }
    }
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The LaunchAgent: start at login, restart only after a failure (at most once a minute), and
/// keep the login PATH so the engine finds the tools its runs use.
pub fn launchd_plist(
    label: &str,
    program: &Path,
    root: &str,
    path_env: Option<&str>,
    log: &str,
) -> String {
    let program = xml(&program.to_string_lossy());
    let (label, root, log) = (xml(label), xml(root), xml(log));
    let env = path_env.map_or_else(String::new, |path| {
        format!(
            "  <key>EnvironmentVariables</key>\n  <dict>\n    <key>PATH</key>\n    <string>{}</string>\n  </dict>\n",
            xml(path)
        )
    });
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{program}</string>
    <string>schedule</string>
    <string>run</string>
    <string>-C</string>
    <string>{root}</string>
  </array>
  <key>WorkingDirectory</key>
  <string>{root}</string>
{env}  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <key>ThrottleInterval</key>
  <integer>60</integer>
  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>
"#
    )
}

/// systemd quoting: a double-quoted word with `\` and `"` escaped and `%` specifiers doubled.
fn unit_word(text: &str) -> String {
    format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    )
}

/// The user unit: SIGINT lets the scheduler cancel its own runs before systemd stops it.
pub fn systemd_unit(program: &Path, root: &str, path_env: Option<&str>) -> String {
    let env = path_env.map_or_else(String::new, |path| {
        format!("Environment={}\n", unit_word(&format!("PATH={path}")))
    });
    format!(
        "[Unit]\nDescription=Dial schedules for {}\n\n[Service]\nType=simple\nWorkingDirectory={}\n{env}ExecStart={} schedule run -C {}\nRestart=on-failure\nRestartSec=60\nKillSignal=SIGINT\nTimeoutStopSec=30\n\n[Install]\nWantedBy=default.target\n",
        root.replace('%', "%%").replace('\n', " "),
        unit_word(root),
        unit_word(&program.to_string_lossy()),
        unit_word(root),
    )
}

pub fn status(root: &str) -> ServiceStatus {
    let (Some(manager), Some(paths)) = (Manager::current(), Paths::current()) else {
        return ServiceStatus {
            supported: false,
            installed: false,
            log: None,
            detail: Some(unsupported()),
        };
    };
    status_in(manager, &paths, root)
}

fn unsupported() -> String {
    "Running schedules after Dial quits is available on macOS and Linux; on this system they run while Dial is open.".into()
}

pub fn status_in(manager: Manager, paths: &Paths, root: &str) -> ServiceStatus {
    let label = label(root);
    ServiceStatus {
        supported: true,
        installed: paths.definition(manager, &label).is_file(),
        log: Some(paths.log(manager, &label)),
        detail: Some(match manager {
            Manager::Launchd => "Starts at login. A sleeping Mac runs nothing; due schedules run when it wakes.".into(),
            Manager::Systemd => "Starts at login (user services end at logout unless lingering is enabled: loginctl enable-linger). A sleeping computer runs nothing; due schedules run when it wakes.".into(),
        }),
    }
}

/// Whether `root` has a registered service, so the app does not start a second scheduler.
pub fn installed(root: &str) -> bool {
    match (Manager::current(), Paths::current()) {
        (Some(manager), Some(paths)) => paths.definition(manager, &label(root)).is_file(),
        _ => false,
    }
}

/// Check what the service would run before anything is written.
pub fn validate(program: &Path, root: &str) -> Result<(), String> {
    if !program.is_absolute() || !crate::launcher::is_executable(program) {
        return Err(format!(
            "The engine ({}) is not an installed executable; install it in Settings › Updates first",
            program.display()
        ));
    }
    // An AppImage's mount point disappears when the app quits.
    if std::env::var_os("APPDIR").is_some_and(|dir| !dir.is_empty() && program.starts_with(&dir)) {
        return Err("The engine runs from inside the AppImage, which is gone once Dial quits. Install the engine in Settings › Updates first.".into());
    }
    if !Path::new(root).is_absolute() || !Path::new(root).is_dir() {
        return Err(format!("{root} is not a folder"));
    }
    Ok(())
}

fn run(command: &mut Command, what: &str) -> Result<(), String> {
    crate::launcher::without_console(command);
    let out = crate::updates::run_bounded(command, MANAGER_TIMEOUT)?;
    if out.success {
        return Ok(());
    }
    let text = format!("{}\n{}", out.stdout.trim(), out.stderr.trim());
    Err(format!("{what} failed: {}", text.trim()))
}

fn write_atomic(file: &Path, text: &str) -> Result<(), String> {
    let dir = file.parent().ok_or("The service file has no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let partial = file.with_extension("partial");
    std::fs::write(&partial, text)
        .map_err(|e| format!("Could not write {}: {e}", partial.display()))?;
    std::fs::rename(&partial, file).map_err(|e| format!("Could not write {}: {e}", file.display()))
}

#[cfg(unix)]
fn gui_domain() -> String {
    // SAFETY: getuid has no preconditions and cannot fail.
    format!("gui/{}", unsafe { libc::getuid() })
}

#[cfg(not(unix))]
fn gui_domain() -> String {
    String::new()
}

/// Register (or re-register with a new engine) the project's scheduler service and start it.
pub fn install(program: &Path, root: &str) -> Result<ServiceStatus, String> {
    let manager = Manager::current().ok_or_else(unsupported)?;
    let paths = Paths::current().ok_or("Could not find your home folder")?;
    validate(program, root)?;
    let label = label(root);
    let file = paths.definition(manager, &label);
    let path_env = std::env::var("PATH").ok().filter(|p| !p.is_empty());
    match manager {
        Manager::Launchd => {
            let log = paths.log(manager, &label);
            if let Some(dir) = Path::new(&log).parent() {
                std::fs::create_dir_all(dir)
                    .map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
            }
            let domain = gui_domain();
            if file.is_file() {
                stop_launchd(&domain, &label);
            }
            write_atomic(
                &file,
                &launchd_plist(&label, program, root, path_env.as_deref(), &log),
            )?;
            if let Err(error) = run(
                Command::new("launchctl")
                    .arg("bootstrap")
                    .arg(&domain)
                    .arg(&file),
                "launchctl bootstrap",
            ) {
                let _ = std::fs::remove_file(&file);
                return Err(error);
            }
        }
        Manager::Systemd => {
            write_atomic(&file, &systemd_unit(program, root, path_env.as_deref()))?;
            let unit = format!("{label}.service");
            let started = run(
                Command::new("systemctl").args(["--user", "daemon-reload"]),
                "systemctl --user daemon-reload",
            )
            .and_then(|()| {
                run(
                    Command::new("systemctl")
                        .args(["--user", "enable"])
                        .arg(&unit),
                    "systemctl --user enable",
                )
            })
            .and_then(|()| {
                run(
                    Command::new("systemctl")
                        .args(["--user", "restart"])
                        .arg(&unit),
                    "systemctl --user restart",
                )
            });
            if let Err(error) = started {
                let _ = run(
                    Command::new("systemctl")
                        .args(["--user", "disable", "--now"])
                        .arg(&unit),
                    "systemctl --user disable",
                );
                let _ = std::fs::remove_file(&file);
                return Err(error);
            }
        }
    }
    Ok(status_in(manager, &paths, root))
}

/// Ask the scheduler to cancel its runs (SIGINT), then unload it.
fn stop_launchd(domain: &str, label: &str) {
    let target = format!("{domain}/{label}");
    if run(
        Command::new("launchctl").args(["kill", "SIGINT", &target]),
        "launchctl kill",
    )
    .is_ok()
    {
        let deadline = std::time::Instant::now() + STOP_GRACE;
        while std::time::Instant::now() < deadline {
            let printed = crate::updates::run_bounded(
                Command::new("launchctl").args(["print", &target]),
                MANAGER_TIMEOUT,
            );
            // `pid = N` disappears from `launchctl print` once the job has exited.
            if !printed.is_ok_and(|out| out.success && out.stdout.contains("pid = ")) {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    let _ = run(
        Command::new("launchctl").args(["bootout", &target]),
        "launchctl bootout",
    );
}

/// Stop and unregister the project's scheduler service; a no-op when there is none.
pub fn remove(root: &str) -> Result<ServiceStatus, String> {
    let manager = Manager::current().ok_or_else(unsupported)?;
    let paths = Paths::current().ok_or("Could not find your home folder")?;
    let label = label(root);
    let file = paths.definition(manager, &label);
    if file.is_file() {
        match manager {
            Manager::Launchd => stop_launchd(&gui_domain(), &label),
            Manager::Systemd => {
                let unit = format!("{label}.service");
                run(
                    Command::new("systemctl")
                        .args(["--user", "disable", "--now"])
                        .arg(&unit),
                    "systemctl --user disable",
                )?;
            }
        }
        std::fs::remove_file(&file)
            .map_err(|e| format!("Could not remove {}: {e}", file.display()))?;
        if manager == Manager::Systemd {
            let _ = run(
                Command::new("systemctl").args(["--user", "daemon-reload"]),
                "systemctl --user daemon-reload",
            );
        }
    }
    Ok(status_in(manager, &paths, root))
}
