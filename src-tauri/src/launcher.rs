//! Starting without setup: the user's shell environment, the `dial` binary, the workspace and
//! the recent-folders list.
//!
//! An app opened from the Finder or a launcher does not inherit the login shell's environment, so
//! `dial` (often in `~/.local/bin`) and provider keys (`ANTHROPIC_API_KEY`) would be missing.
//! [`import_shell_env`] copies them in, the way editors such as VS Code do.

use serde::{Deserialize, Serialize};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

mod probe;
use probe::run_detailed;
pub(crate) use probe::{available as pipe_available, run_captured};

#[cfg(test)]
mod tests;

/// How long the login shell may take to print its environment.
#[cfg(unix)]
const SHELL_TIMEOUT: Duration = Duration::from_secs(5);
/// Recent folders kept for the welcome screen and the folder menu.
const MAX_RECENTS: usize = 12;
/// Where `dial` is commonly installed when the PATH does not say.
fn fallback_dirs(os: &str) -> &'static [&'static str] {
    match os {
        "macos" => &["~/.local/bin", "~/.cargo/bin", "~/bin", "/opt/homebrew/bin", "/usr/local/bin"],
        "linux" => &["~/.local/bin", "~/.cargo/bin", "~/bin", "/usr/local/bin"],
        _ => &["~/.local/bin", "~/.cargo/bin", "~/bin"],
    }
}
/// Variables that describe the probe shell itself, not the user's environment.
#[cfg(unix)]
const SKIP_VARS: [&str; 5] = ["PWD", "OLDPWD", "SHLVL", "_", "TERM_SESSION_ID"];

/// Parse `env -0` output without requiring Unix paths or values to be Unicode.
#[cfg(unix)]
fn parse_env(output: &[u8]) -> Vec<(String, OsString)> {
    use std::os::unix::ffi::OsStrExt;
    output
        .split(|&b| b == 0)
        .filter_map(|entry| {
            let eq = entry.iter().position(|&b| b == b'=')?;
            let key = std::str::from_utf8(&entry[..eq]).ok()?.trim_start_matches('\n');
            let value = OsStr::from_bytes(&entry[eq + 1..]);
            (!key.is_empty() && !key.contains(char::is_whitespace))
                .then(|| (key.to_owned(), value.to_owned()))
        })
        .collect()
}

/// `current` followed by the entries of `extra` it lacks.
#[cfg(unix)]
fn merge_path(current: &OsStr, extra: &OsStr) -> OsString {
    let mut parts: Vec<_> = std::env::split_paths(current).collect();
    for part in std::env::split_paths(extra).filter(|p| !p.as_os_str().is_empty()) {
        if !parts.contains(&part) {
            parts.push(part);
        }
    }
    // Unix split_paths components cannot contain the ':' separator.
    std::env::join_paths(parts).expect("split PATH components can be joined")
}

/// The changes that import `shell` into `current`: variables `current` lacks, and PATH merged.
#[cfg(unix)]
fn env_changes(
    current: impl Fn(&str) -> Option<OsString>,
    shell: &[(String, OsString)],
) -> Vec<(String, OsString)> {
    shell
        .iter()
        .filter(|(key, _)| !SKIP_VARS.contains(&key.as_str()))
        .filter_map(|(key, value)| match current(key) {
            None => Some((key.clone(), value.clone())),
            Some(existing) if key == "PATH" => {
                let merged = merge_path(&existing, value);
                (merged != existing).then(|| (key.clone(), merged))
            }
            Some(_) => None,
        })
        .collect()
}

/// Copy the login shell's environment into this process when it was not started from a terminal.
///
/// Must run at the very start of `main`, before any other thread exists.
#[cfg(unix)]
pub fn import_shell_env() {
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() {
        return;
    }
    let Some(shell) = std::env::var_os("SHELL").filter(|s| !s.is_empty()) else {
        warn("SHELL is not set, so the login shell's PATH and API keys were not imported.".into());
        return;
    };
    let name = Path::new(&shell)
        .file_name()
        .map_or_else(|| shell.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
    let output = match run_detailed(
        Command::new(&shell).args(["-l", "-i", "-c", "env -0"]),
        SHELL_TIMEOUT,
    ) {
        Ok(output) => output,
        Err(why) => {
            warn(format!(
                "Could not import the environment of your login shell ({name} {why}); Dial may miss `dial` or API keys set in your shell profile."
            ));
            return;
        }
    };
    for (key, value) in env_changes(|key| std::env::var_os(key), &parse_env(&output)) {
        // SAFETY: called at the start of `main`, while the process is single-threaded.
        unsafe { std::env::set_var(key, value) };
    }
}

#[cfg(not(unix))]
pub fn import_shell_env() {}

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

/// Variables the AppImage runtime and its bundled-GTK hooks set for the app itself: its own
/// identity (`APPIMAGE`, `APPDIR`, `ARGV0`, `OWD`) and paths into its private mount (`GTK_PATH`,
/// `GDK_PIXBUF_MODULE_FILE`, `$APPDIR/usr/bin` on `PATH`, …). A shell or app the user starts from
/// Dial must not load the bundled GTK modules or mistake itself for this AppImage (an updater
/// reading `APPIMAGE` would replace Dial). Returns what to change: a cleaned value, or `None` to
/// remove. Empty unless both `APPIMAGE` and an absolute, non-root `APPDIR` are set.
pub fn appimage_env_fixes(
    vars: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, Option<OsString>)> {
    const IDENTITY: [&str; 4] = ["APPIMAGE", "APPDIR", "ARGV0", "OWD"];
    let vars: Vec<(OsString, OsString)> = vars.into_iter().collect();
    let get = |name: &str| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v);
    let appdir = get("APPDIR")
        .and_then(|v| v.to_str())
        .map(|v| v.trim_end_matches('/'))
        .filter(|v| v.starts_with('/') && v.len() > 1);
    let (Some(_), Some(appdir)) = (get("APPIMAGE"), appdir) else {
        return Vec::new();
    };
    let inside = |entry: &str| {
        entry
            .strip_prefix(appdir)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    };
    let mut fixes = Vec::new();
    for (key, value) in &vars {
        if key.to_str().is_some_and(|k| IDENTITY.contains(&k)) {
            fixes.push((key.clone(), None));
            continue;
        }
        let Some(text) = value.to_str() else { continue };
        if !text.split(':').any(inside) {
            continue;
        }
        let kept: Vec<&str> = text
            .split(':')
            .filter(|e| !e.is_empty() && !inside(e))
            .collect();
        let fixed = (!kept.is_empty()).then(|| kept.join(":").into());
        fixes.push((key.clone(), fixed));
    }
    fixes
}

/// [`appimage_env_fixes`] for this process, applied to a child about to start.
pub(crate) fn outside_appimage(command: &mut Command) -> &mut Command {
    for (key, value) in appimage_env_fixes(std::env::vars_os()) {
        match value {
            Some(value) => command.env(key, value),
            None => command.env_remove(key),
        };
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
    run_detailed(command, timeout).ok()
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

/// For a known executable name (including `.exe` on Windows), reject directories
/// and Unix files with no execute bit before they can shadow a later candidate.
pub(crate) fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else { return false };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Only completed installations are selected; an interrupted install never shadows PATH.
pub fn desktop_engine(app_dir: &Path) -> Option<PathBuf> {
    let name = std::fs::read_to_string(app_dir.join(ENGINE_SELECTION)).ok()?;
    if !name.starts_with("engine-") || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return None;
    }
    let binary = app_dir.join(name).join(DIAL_NAME);
    is_executable(&binary).then_some(binary)
}

/// How the engine binary was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineOrigin {
    /// `--dial PATH`.
    Flag,
    /// `$DIAL_BIN`.
    Env,
    /// A binary picked in Settings › Updates ("Use a different engine…").
    Choice,
    /// The copy Desktop installed and selected.
    Desktop,
    /// Found on PATH.
    Path,
    /// Found in one of the usual installation folders.
    Fallback,
    /// Nowhere: the bare name, which fails to start.
    Missing,
}

/// The engine picked in Settings › Updates, kept in [`ENGINE_CHOICE`] in the app's folder. It
/// ranks below `--dial` and `DIAL_BIN` and above the usual order; `None` (no file) is that order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EngineChoice {
    /// This binary. One that is gone or not executable is skipped, with a startup warning.
    Custom { path: PathBuf },
    /// The first `dial` on PATH, ahead of Desktop's own copy.
    Path,
    /// Desktop's installed copy (also the usual first pick when it exists).
    Desktop,
}

pub const ENGINE_CHOICE: &str = "engine-choice.json";

/// The saved choice; `None` when there is none or the file is unreadable.
pub fn load_choice(app_dir: Option<&Path>) -> Option<EngineChoice> {
    let bytes = std::fs::read(app_dir?.join(ENGINE_CHOICE)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Save `choice`, or remove the file for the usual order.
pub fn save_choice(app_dir: &Path, choice: Option<&EngineChoice>) -> Result<(), String> {
    let file = app_dir.join(ENGINE_CHOICE);
    let Some(choice) = choice else {
        return match std::fs::remove_file(&file) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.to_string()),
            _ => Ok(()),
        };
    };
    std::fs::create_dir_all(app_dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_vec_pretty(choice).map_err(|e| e.to_string())?;
    let partial = file.with_extension("json.partial");
    std::fs::write(&partial, json).map_err(|e| e.to_string())?;
    std::fs::rename(&partial, &file).map_err(|e| e.to_string())
}

/// Why a saved choice did not pick the engine, for the startup warnings; `None` when it did or
/// when `--dial`/`DIAL_BIN` outranked it.
pub fn choice_ignored(choice: Option<&EngineChoice>, origin: EngineOrigin) -> Option<String> {
    match (choice?, origin) {
        (_, EngineOrigin::Flag | EngineOrigin::Env) => None,
        (EngineChoice::Custom { path }, o) if o != EngineOrigin::Choice => Some(format!(
            "The engine chosen in Settings › Updates ({}) is missing or not executable; using the usual one instead.",
            path.display()
        )),
        (EngineChoice::Path, o) if o != EngineOrigin::Path => {
            Some("Settings › Updates asks for the engine on PATH, but there is none; using the usual one instead.".into())
        }
        (EngineChoice::Desktop, o) if o != EngineOrigin::Desktop => {
            Some("Settings › Updates asks for Desktop's own engine, but it is not installed; using the usual one instead.".into())
        }
        _ => None,
    }
}

/// `program --version`: the first line it prints, or why it does not run.
pub fn engine_version(program: &Path) -> Result<String, String> {
    if !is_executable(program) {
        return Err(format!("{} is not an executable file", program.display()));
    }
    let out = run_detailed(Command::new(program).arg("--version"), Duration::from_secs(10))
        .map_err(|error| format!("{} {error}", program.display()))?;
    let text = String::from_utf8_lossy(&out);
    let line = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if line.is_empty() {
        return Err(format!("{} printed no version", program.display()));
    }
    Ok(line.to_owned())
}

/// An explicit override wins, then the user's Desktop install, then PATH and the usual folders.
pub fn find_dial(explicit: Option<OsString>, path: Option<&OsStr>, home: Option<&Path>) -> PathBuf {
    locate_dial(explicit, None, None, path, home).0
}

/// [`find_dial`], telling a `--dial` `flag` from `$DIAL_BIN` (`env`), honouring the Settings
/// `choice` below both, and saying which rule won.
pub fn locate_dial(
    flag: Option<OsString>,
    env: Option<OsString>,
    choice: Option<&EngineChoice>,
    path: Option<&OsStr>,
    home: Option<&Path>,
) -> (PathBuf, EngineOrigin) {
    locate_dial_in(flag, env, choice, path, home, app_dir(home).as_deref())
}

fn locate_dial_in(
    flag: Option<OsString>,
    env: Option<OsString>,
    choice: Option<&EngineChoice>,
    path: Option<&OsStr>,
    home: Option<&Path>,
    desktop_dir: Option<&Path>,
) -> (PathBuf, EngineOrigin) {
    if let Some(flag) = flag.filter(|p| !p.is_empty()) {
        return (PathBuf::from(flag), EngineOrigin::Flag);
    }
    if let Some(env) = env.filter(|p| !p.is_empty()) {
        return (PathBuf::from(env), EngineOrigin::Env);
    }
    let name = DIAL_NAME;
    let found = |dirs: &mut dyn Iterator<Item = PathBuf>| {
        dirs.map(|dir| dir.join(name)).find(|candidate| is_executable(candidate))
    };
    let on_path = || found(&mut path.into_iter().flat_map(std::env::split_paths));
    match choice {
        Some(EngineChoice::Custom { path }) if is_executable(path) => {
            return (path.clone(), EngineOrigin::Choice);
        }
        Some(EngineChoice::Path) => {
            if let Some(on_path) = on_path() {
                return (on_path, EngineOrigin::Path);
            }
        }
        _ => {}
    }
    if let Some(installed) = desktop_dir.and_then(desktop_engine) {
        return (installed, EngineOrigin::Desktop);
    }
    if let Some(on_path) = on_path() {
        return (on_path, EngineOrigin::Path);
    }
    let mut fallback = fallback_dirs(std::env::consts::OS).iter().filter_map(|dir| {
        let path = Path::new(dir);
        if path.starts_with("~") && home.is_none() { None } else { Some(expand_home(path, home)) }
    });
    match found(&mut fallback) {
        Some(usual) => (usual, EngineOrigin::Fallback),
        None => (PathBuf::from(name), EngineOrigin::Missing),
    }
}

/// What happened while starting, for Settings › Updates: the engine chosen and any warnings
/// (the shell environment, the agent's browser tools) that would otherwise only reach stderr.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Startup {
    pub engine: Option<PathBuf>,
    pub origin: Option<EngineOrigin>,
    pub warnings: Vec<String>,
}

static STARTUP: Mutex<Startup> = Mutex::new(Startup {
    engine: None,
    origin: None,
    warnings: Vec::new(),
});

/// Record the engine chosen at start.
pub fn note_engine(program: &Path, origin: EngineOrigin) {
    let mut startup = STARTUP.lock().unwrap_or_else(PoisonError::into_inner);
    startup.engine = Some(program.to_owned());
    startup.origin = Some(origin);
}

/// Record a startup problem the user should be able to see (and copy).
pub fn warn(message: String) {
    let mut startup = STARTUP.lock().unwrap_or_else(PoisonError::into_inner);
    if !startup.warnings.contains(&message) {
        startup.warnings.push(message);
    }
}

/// Everything recorded so far.
pub fn startup() -> Startup {
    STARTUP
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
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

/// Prefer HOME when usable, then the native profile on Windows. Never use the working directory.
pub fn home_dir() -> Option<PathBuf> {
    home_from(std::env::var_os("HOME"), cfg!(windows).then(|| std::env::var_os("USERPROFILE")).flatten())
}

fn home_from(home: Option<OsString>, profile: Option<OsString>) -> Option<PathBuf> {
    [home, profile]
        .into_iter()
        .flatten()
        .map(PathBuf::from)
        .find(|p| p.is_absolute())
}

/// The app's per-user folder (recent folders, the browser's endpoint for the agent).
pub fn app_dir(home: Option<&Path>) -> Option<PathBuf> {
    let base = match std::env::consts::OS {
        "windows" => std::env::var_os("APPDATA"),
        "linux" => std::env::var_os("XDG_CONFIG_HOME"),
        _ => None,
    };
    app_dir_from(std::env::consts::OS, home, base.as_deref().map(Path::new))
}

fn app_dir_from(os: &str, home: Option<&Path>, config: Option<&Path>) -> Option<PathBuf> {
    let home = home.filter(|p| p.is_absolute());
    let config = config.filter(|p| p.is_absolute()).map(Path::to_owned);
    match os {
        "macos" => Some(home?.join("Library/Application Support/dev.dial.desktop")),
        "windows" => Some(config.or_else(|| home.map(|p| p.join("AppData/Roaming")))?.join("dev.dial.desktop")),
        _ => Some(config.or_else(|| home.map(|p| p.join(".config")))?.join("dial-desktop")),
    }
}
