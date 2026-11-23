//! Starting without setup: the user's shell environment, the `dial` binary, the workspace and
//! the recent-folders list.
//!
//! An app opened from the Finder or a launcher does not inherit the login shell's environment, so
//! `dial` (often in `~/.local/bin`) and provider keys (`ANTHROPIC_API_KEY`) would be missing.
//! [`import_shell_env`] copies them in, the way editors such as VS Code do.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests;

/// How long the login shell may take to print its environment.
const SHELL_TIMEOUT: Duration = Duration::from_secs(5);
/// Recent folders kept for the welcome screen and the folder menu.
const MAX_RECENTS: usize = 12;
/// Where `dial` is commonly installed when the PATH does not say.
const FALLBACK_DIRS: [&str; 5] = [
    "~/.local/bin",
    "~/.cargo/bin",
    "~/bin",
    "/opt/homebrew/bin",
    "/usr/local/bin",
];
/// Variables that describe the probe shell itself, not the user's environment.
const SKIP_VARS: [&str; 5] = ["PWD", "OLDPWD", "SHLVL", "_", "TERM_SESSION_ID"];

/// Parse `env -0` output into pairs.
pub fn parse_env(output: &[u8]) -> Vec<(String, String)> {
    output
        .split(|&b| b == 0)
        .filter_map(|entry| {
            let entry = std::str::from_utf8(entry).ok()?;
            let (key, value) = entry.split_once('=')?;
            let key = key.trim_start_matches('\n');
            (!key.is_empty() && !key.contains(char::is_whitespace))
                .then(|| (key.to_owned(), value.to_owned()))
        })
        .collect()
}

/// `current` followed by the entries of `extra` it lacks.
pub fn merge_path(current: &str, extra: &str) -> String {
    let mut parts: Vec<&str> = current.split(':').filter(|p| !p.is_empty()).collect();
    for part in extra.split(':').filter(|p| !p.is_empty()) {
        if !parts.contains(&part) {
            parts.push(part);
        }
    }
    parts.join(":")
}

/// The changes that import `shell` into `current`: variables `current` lacks, and PATH merged.
pub fn env_changes(
    current: &[(String, String)],
    shell: &[(String, String)],
) -> Vec<(String, String)> {
    let get = |key: &str| current.iter().find(|(k, _)| k == key).map(|(_, v)| v);
    shell
        .iter()
        .filter(|(key, _)| !SKIP_VARS.contains(&key.as_str()))
        .filter_map(|(key, value)| match get(key) {
            None => Some((key.clone(), value.clone())),
            Some(existing) if key == "PATH" => {
                let merged = merge_path(existing, value);
                (merged != *existing).then(|| (key.clone(), merged))
            }
            Some(_) => None,
        })
        .collect()
}

/// Copy the login shell's environment into this process when it was not started from a terminal.
///
/// Must run at the very start of `main`, before any other thread exists.
pub fn import_shell_env() {
    use std::io::IsTerminal;
    if cfg!(windows) || std::io::stdin().is_terminal() {
        return;
    }
    let Some(shell) = std::env::var_os("SHELL").filter(|s| !s.is_empty()) else {
        return;
    };
    let Some(output) = run_with_timeout(
        Command::new(shell).args(["-l", "-i", "-c", "env -0"]),
        SHELL_TIMEOUT,
    ) else {
        return;
    };
    let current: Vec<(String, String)> = std::env::vars().collect();
    for (key, value) in env_changes(&current, &parse_env(&output)) {
        // SAFETY: called at the start of `main`, while the process is single-threaded.
        unsafe { std::env::set_var(key, value) };
    }
}

/// Piping standard I/O does not prevent Windows from opening a separate console for a GUI's child.
pub(crate) fn without_console(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Run `command` with no stdin and no controlling terminal, returning its stdout if it succeeds
/// within `timeout`.
///
/// The probe shell is interactive (`-i`, so `.zshrc` runs), and an interactive shell that shares
/// our terminal makes itself that terminal's foreground job. That suspends whatever was in the
/// foreground (`zsh: suspended (tty output)`) and, if the probe is then killed, leaves the terminal
/// owned by a dead process group. A new session detaches it from the terminal entirely.
pub fn run_with_timeout(command: &mut Command, timeout: Duration) -> Option<Vec<u8>> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe extern "C" {
            fn setsid() -> i32;
        }
        // SAFETY: `setsid` is async-signal-safe, as required between fork and exec.
        unsafe {
            command.pre_exec(|| {
                if setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut child = without_console(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut stdout, &mut out).map(|_| out)
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return reader.join().ok()?.ok(),
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

/// `~/x` relative to `home`.
pub fn expand_home(path: &Path, home: Option<&Path>) -> PathBuf {
    match (path.strip_prefix("~"), home) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => path.to_owned(),
    }
}

pub const DIAL_NAME: &str = if cfg!(windows) { "dial.exe" } else { "dial" };
pub const ENGINE_SELECTION: &str = "engine-selection";

/// Only completed installations are selected; an interrupted install never shadows PATH.
pub fn desktop_engine(app_dir: &Path) -> Option<PathBuf> {
    let name = std::fs::read_to_string(app_dir.join(ENGINE_SELECTION)).ok()?;
    if !name.starts_with("engine-") || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return None;
    }
    let binary = app_dir.join(name).join(DIAL_NAME);
    binary.is_file().then_some(binary)
}

/// An explicit override wins, then the user's Desktop install, then PATH and the usual folders.
pub fn find_dial(explicit: Option<OsString>, path: Option<&OsStr>, home: Option<&Path>) -> PathBuf {
    if let Some(explicit) = explicit.filter(|p| !p.is_empty()) {
        return PathBuf::from(explicit);
    }
    if let Some(installed) = app_dir(home).and_then(|dir| desktop_engine(&dir)) {
        return installed;
    }
    let name = DIAL_NAME;
    let on_path = path.into_iter().flat_map(std::env::split_paths);
    let fallback = FALLBACK_DIRS
        .iter()
        .map(|dir| expand_home(Path::new(dir), home));
    on_path
        .chain(fallback)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

/// Whether `cwd` is a folder the user meant to work in. A Finder launch starts in `/`, and
/// `tauri dev` starts in this app's own `src-tauri`; neither is a project choice.
pub fn meaningful_cwd(cwd: &Path, home: Option<&Path>) -> bool {
    let own_crate = Path::new(env!("CARGO_MANIFEST_DIR"));
    cwd.parent().is_some() && Some(cwd) != home && cwd != own_crate
}

/// The workspace to open: an explicit argument, else a meaningful current directory, else the
/// most recent folder that still exists. `None` shows the folder picker.
pub fn choose_workspace(
    arg: Option<&Path>,
    cwd: &Path,
    home: Option<&Path>,
    recents: &[PathBuf],
) -> Option<PathBuf> {
    if let Some(arg) = arg {
        return Some(cwd.join(expand_home(arg, home)));
    }
    if meaningful_cwd(cwd, home) {
        return Some(cwd.to_owned());
    }
    recents.iter().find(|path| path.is_dir()).cloned()
}

/// Recently opened folders, most recent first.
pub struct Recents {
    file: Option<PathBuf>,
    pub paths: Vec<PathBuf>,
}

impl Recents {
    /// Load from `file`; a missing or unreadable file is an empty list.
    pub fn load(file: Option<PathBuf>) -> Self {
        let paths = file
            .as_deref()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
            .unwrap_or_default()
            .into_iter()
            .map(PathBuf::from)
            .collect();
        Self { file, paths }
    }

    /// The per-user default location.
    pub fn default_file(home: Option<&Path>) -> Option<PathBuf> {
        app_dir(home).map(|dir| dir.join("recent-workspaces.json"))
    }

    /// Move `path` to the front and save. Saving is best effort: a read-only disk only loses
    /// the list.
    pub fn touch(&mut self, path: &Path) {
        self.update(|paths| {
            paths.retain(|p| p != path);
            paths.insert(0, path.to_owned());
            paths.truncate(MAX_RECENTS);
            true
        });
    }

    /// Drop `path` from the list (the folder itself is untouched) and save. Whether it was listed.
    pub fn forget(&mut self, path: &Path) -> bool {
        self.update(|paths| {
            let before = paths.len();
            paths.retain(|p| p != path);
            paths.len() != before
        })
    }

    pub fn refresh(&mut self) {
        if self.file.is_some() {
            self.paths = Self::load(self.file.clone()).paths;
        }
    }

    fn update(&mut self, change: impl FnOnce(&mut Vec<PathBuf>) -> bool) -> bool {
        let Some(file) = self.file.clone() else {
            return change(&mut self.paths);
        };
        // Keep a separate lock file: the recents file is replaced atomically for readers.
        let lock = (|| -> std::io::Result<_> {
            if let Some(dir) = file.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir)?;
            }
            let lock = std::fs::File::options()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(file.with_extension("lock"))?;
            lock.lock()?;
            Ok(lock)
        })();
        let Ok(_lock) = lock else {
            return change(&mut self.paths);
        };
        self.refresh();
        let changed = change(&mut self.paths);
        if changed {
            let list: Vec<_> = self.paths.iter().map(|p| p.to_string_lossy()).collect();
            if let Ok(json) = serde_json::to_vec_pretty(&list) {
                let tmp = file.with_extension("tmp");
                if std::fs::write(&tmp, json).is_ok() {
                    let _ = std::fs::rename(&tmp, &file);
                }
            }
        }
        changed
    }
}

/// The app's per-user folder (recent folders, the browser's endpoint for the agent).
pub fn app_dir(home: Option<&Path>) -> Option<PathBuf> {
    let home = home?;
    Some(if cfg!(target_os = "macos") {
        home.join("Library/Application Support/dev.dial.desktop")
    } else if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map_or_else(|| home.join("AppData/Roaming"), PathBuf::from)
            .join("dev.dial.desktop")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map_or_else(|| home.join(".config"), PathBuf::from)
            .join("dial-desktop")
    })
}
