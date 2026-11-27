//! The firmware project in the open folder: which toolchain it uses, the commands that build it
//! and put it on a board, and the one build or upload running at a time. An upload frees the
//! serial console's port first and reopens the console afterwards.
use super::{inventory, resume, suspend};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// Shell commands for a project. `{port}` stands for the chosen serial port.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commands {
    #[serde(default)]
    pub build: String,
    #[serde(default)]
    pub upload: String,
}

#[derive(Debug, Serialize)]
pub struct Project {
    /// The toolchain's name, or `None` when the folder is not a recognised firmware project.
    pub toolchain: Option<&'static str>,
    pub commands: Commands,
    /// Whether `commands` are the user's own rather than the toolchain's usual ones.
    pub custom: bool,
    /// What must be installed, and anything the usual commands still need.
    pub needs: Option<&'static str>,
    /// MicroPython or CircuitPython: code runs through the board's prompt, with no build step.
    pub python: bool,
    /// Source files worth editing, the main one first.
    pub sources: Vec<String>,
}

struct Toolchain {
    name: &'static str,
    build: &'static str,
    upload: &'static str,
    needs: &'static str,
    /// Candidate main files, most likely first.
    main: &'static [&'static str],
}

const PLATFORMIO: Toolchain = Toolchain {
    name: "PlatformIO",
    build: "pio run",
    upload: "pio run -t upload --upload-port {port}",
    needs: "Needs PlatformIO Core (pip install platformio).",
    main: &["src/main.cpp", "src/main.c", "src/main.ino"],
};
const ARDUINO: Toolchain = Toolchain {
    name: "Arduino",
    build: "arduino-cli compile .",
    upload: "arduino-cli upload -p {port} .",
    needs: "Needs arduino-cli and a board type: set default_fqbn in sketch.yaml, or add --fqbn (for example --fqbn arduino:avr:uno) to both commands.",
    main: &[],
};
const ESP_IDF: Toolchain = Toolchain {
    name: "ESP-IDF",
    build: "idf.py build",
    upload: "idf.py -p {port} flash",
    needs: "Needs ESP-IDF with its environment loaded by your login shell (export.sh).",
    main: &["main/main.c", "main/main.cpp"],
};
const ZEPHYR: Toolchain = Toolchain {
    name: "Zephyr",
    build: "west build",
    upload: "west flash",
    needs: "Needs west and the Zephyr SDK; set the board with west build -b once.",
    main: &["src/main.c", "src/main.cpp"],
};
const RUST: Toolchain = Toolchain {
    name: "Rust (embedded)",
    build: "cargo build --release",
    upload: "cargo run --release",
    needs: "Uses the runner in .cargo/config.toml (espflash, probe-rs or similar) to flash.",
    main: &["src/main.rs"],
};
const PYTHON: Toolchain = Toolchain {
    name: "MicroPython / CircuitPython",
    build: "",
    upload: "",
    needs: "No tools needed: connect to the board, then run or save files from here.",
    main: &["main.py", "code.py", "boot.py"],
};

fn detect(root: &Path) -> Option<&'static Toolchain> {
    let has = |name: &str| root.join(name).is_file();
    let contains = |name: &str, text: &str| std::fs::read_to_string(root.join(name)).is_ok_and(|s| s.contains(text));
    let sketch = || std::fs::read_dir(root).is_ok_and(|dir| dir.flatten().any(|e| e.path().extension().is_some_and(|x| x == "ino")));
    Some(if has("platformio.ini") {
        &PLATFORMIO
    } else if has("sketch.yaml") || sketch() {
        &ARDUINO
    } else if has("sdkconfig") || has("sdkconfig.defaults") || contains("CMakeLists.txt", "IDF_PATH") {
        &ESP_IDF
    } else if has("west.yml") || has("prj.conf") {
        &ZEPHYR
    } else if has("Cargo.toml") && contains(".cargo/config.toml", "runner") {
        &RUST
    } else if PYTHON.main.iter().any(|name| has(name)) {
        &PYTHON
    } else {
        return None;
    })
}

const SOURCE_DIRS: [&str; 5] = ["", "src", "main", "include", "lib"];
const SOURCE_TYPES: [&str; 14] = ["ino", "c", "cc", "cpp", "h", "hpp", "py", "rs", "s", "ini", "yaml", "toml", "conf", "cmake"];
const MAX_SOURCES: usize = 60;

/// Source files in the folders firmware projects keep them in, the main file first.
fn sources(root: &Path, main: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for dir in SOURCE_DIRS {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else { continue };
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| {
                let kind = Path::new(name).extension().and_then(|x| x.to_str()).unwrap_or_default().to_ascii_lowercase();
                !name.starts_with('.') && (SOURCE_TYPES.contains(&kind.as_str()) || name == "CMakeLists.txt")
            })
            .collect();
        names.sort();
        found.extend(names.into_iter().map(|name| if dir.is_empty() { name } else { format!("{dir}/{name}") }));
    }
    let rank = |path: &String| {
        let listed = main.iter().position(|m| m == path);
        let code = ["ino", "c", "cc", "cpp", "py", "rs"].iter().any(|x| path.ends_with(&format!(".{x}")));
        (listed.unwrap_or(usize::MAX), !path.ends_with(".ino"), !code)
    };
    found.sort_by_key(rank);
    found.truncate(MAX_SOURCES);
    found
}

/// The user's own commands, by project folder, kept in the app's folder (not the project's, so
/// nothing that edits project files can change what an upload runs).
fn saved_file() -> Option<PathBuf> {
    crate::launcher::app_dir(crate::launcher::home_dir().as_deref()).map(|dir| dir.join("board-projects.json"))
}
fn load(file: &Path) -> BTreeMap<String, Commands> {
    std::fs::read(file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}
fn save(file: &Path, root: &Path, commands: Option<Commands>) -> Result<(), String> {
    let mut all = load(file);
    let key = root.to_string_lossy().into_owned();
    match commands {
        Some(commands) => all.insert(key, commands),
        None => all.remove(&key),
    };
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(&all).map_err(|e| e.to_string())?;
    std::fs::write(file, bytes).map_err(|e| format!("Could not save the board commands: {e}"))
}

fn describe(root: &Path, saved: Option<Commands>) -> Project {
    let found = detect(root);
    let usual = found.map(|t| Commands { build: t.build.into(), upload: t.upload.into() }).unwrap_or_default();
    let custom = saved.as_ref().is_some_and(|s| *s != usual);
    Project {
        toolchain: found.map(|t| t.name),
        commands: saved.unwrap_or(usual),
        custom,
        needs: found.map(|t| t.needs),
        python: found.is_some_and(|t| t.name == PYTHON.name),
        sources: sources(root, found.map_or(&[], |t| t.main)),
    }
}

/// The project in `root`, with the user's saved commands if any.
pub fn project(root: &Path) -> Project {
    let saved = saved_file().and_then(|file| load(&file).remove(root.to_string_lossy().as_ref()));
    describe(root, saved)
}

#[tauri::command]
pub async fn board_project(app: AppHandle, webview: tauri::Webview, workspace_root: String) -> Result<Project, String> {
    crate::app::main_only(&webview, "read the board project")?;
    let root = crate::app::board_root(&app, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || project(&root)).await.map_err(|e| e.to_string())
}

/// Save the user's own commands for this project, or go back to the usual ones with `None`.
#[tauri::command]
pub async fn board_project_set(
    app: AppHandle,
    webview: tauri::Webview,
    workspace_root: String,
    commands: Option<Commands>,
) -> Result<Project, String> {
    crate::app::main_only(&webview, "change the board commands")?;
    let root = crate::app::board_root(&app, &workspace_root)?;
    if commands.as_ref().is_some_and(|c| c.build.len() + c.upload.len() > 4096) {
        return Err("Commands can be at most 4096 characters".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        save(&saved_file().ok_or("No app folder to save the commands in")?, &root, commands)?;
        Ok(project(&root))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// What a build or upload reports, as `board-job` app events.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum JobEvent {
    Started { job: JobView },
    Data { id: u32, text: String },
    Exit { id: u32, code: Option<i32> },
}
const JOB_EVENT: &str = "board-job";
const JOB_LOG_LIMIT: usize = 200_000;

#[derive(Debug, Clone, Serialize)]
pub struct JobView {
    pub id: u32,
    /// `build` or `upload`.
    pub action: String,
    pub command: String,
    pub running: bool,
    /// The exit status once it ended; `None` while running or when it was killed.
    pub code: Option<i32>,
    pub log: String,
}

struct Job {
    view: JobView,
    pid: u32,
}
static JOB: Mutex<Option<Job>> = Mutex::new(None);
static NEXT_JOB: AtomicU32 = AtomicU32::new(0);

fn job() -> std::sync::MutexGuard<'static, Option<Job>> {
    JOB.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The newest or running job.
pub fn current() -> Option<JobView> {
    job().as_ref().map(|j| j.view.clone())
}

/// `template` with `{port}` filled in. Ports are checked so a device name cannot add shell words.
fn fill(template: &str, port: Option<&str>) -> Result<String, String> {
    if !template.contains("{port}") {
        return Ok(template.to_owned());
    }
    let port = port.ok_or("Choose a serial device first: this command needs its port")?;
    if port.is_empty() || !port.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | ':')) {
        return Err(format!("Port {port:?} has characters that cannot be passed to a command"));
    }
    Ok(template.replace("{port}", port))
}

fn shell(command: &str, root: &Path, port: Option<&str>) -> Command {
    // A login shell, so tools the user installed (pio, arduino-cli, idf.py) are on PATH even
    // though the app was not started from a terminal.
    let mut child = if cfg!(windows) {
        let mut child = Command::new("cmd");
        child.args(["/C", command]);
        child
    } else {
        let mut child = Command::new(std::env::var_os("SHELL").filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".into()));
        child.args(["-lc", command]);
        child
    };
    child.current_dir(root).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(port) = port {
        child.env("DIAL_BOARD_PORT", port).env("ESPFLASH_PORT", port);
    }
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut child, 0);
    crate::launcher::without_console(&mut child);
    child
}

/// Forward one of the job's output streams to its log and the panel.
fn forward(app: AppHandle, id: u32, mut stream: impl Read + Send + 'static) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut pending = Vec::new();
        while let Ok(n) = stream.read(&mut buf) {
            if n == 0 {
                break;
            }
            pending.extend_from_slice(&buf[..n]);
            let text = crate::terminal::take_utf8(&mut pending);
            if text.is_empty() {
                continue;
            }
            if let Some(j) = job().as_mut().filter(|j| j.view.id == id) {
                j.view.log.push_str(&text);
                let mut cut = j.view.log.len().saturating_sub(JOB_LOG_LIMIT);
                while !j.view.log.is_char_boundary(cut) {
                    cut += 1;
                }
                j.view.log.drain(..cut);
            }
            let _ = app.emit(JOB_EVENT, JobEvent::Data { id, text });
        }
    })
}

/// Start building (`upload` false) or uploading the project in `root`. `port` is the device to
/// upload to; without one, the open console's port or the only USB device is used.
pub fn start(app: &AppHandle, root: &Path, upload: bool, port: Option<String>) -> Result<JobView, String> {
    let found = project(root);
    let (action, template) = if upload { ("upload", found.commands.upload) } else { ("build", found.commands.build) };
    if template.trim().is_empty() {
        return Err(if found.python {
            "MicroPython and CircuitPython projects have no build or upload step: run the code or save it to the board instead".into()
        } else {
            format!("No {action} command is set for this project. Set one in the Boards panel.")
        });
    }
    let port = port.or_else(super::live_path).or_else(|| {
        let mut usb = inventory().ok()?.into_iter().filter(|b| b.transport == "usb");
        usb.next().filter(|_| usb.next().is_none()).map(|b| b.path)
    });
    if let Some(port) = &port
        && !inventory()?.iter().any(|b| b.path == *port)
    {
        return Err(format!("Serial port {port} is not connected"));
    }
    let command = fill(&template, port.as_deref())?;
    let mut slot = job();
    if slot.as_ref().is_some_and(|j| j.view.running) {
        return Err("A build or upload is already running".into());
    }
    // The uploader needs the port the console holds.
    let console = if upload { suspend() } else { None };
    let mut child = match shell(&command, root, port.as_deref()).spawn() {
        Ok(child) => child,
        Err(e) => {
            if let Some(console) = console {
                resume(app, console);
            }
            return Err(format!("Could not start `{command}`: {e}"));
        }
    };
    let id = NEXT_JOB.fetch_add(1, Ordering::Relaxed) + 1;
    let view = JobView { id, action: action.into(), command, running: true, code: None, log: String::new() };
    *slot = Some(Job { view: view.clone(), pid: child.id() });
    drop(slot);
    let _ = app.emit(JOB_EVENT, JobEvent::Started { job: view.clone() });
    let readers = [
        child.stdout.take().map(|s| forward(app.clone(), id, s)),
        child.stderr.take().map(|s| forward(app.clone(), id, s)),
    ];
    let app = app.clone();
    std::thread::spawn(move || {
        let code = child.wait().ok().and_then(|status| status.code());
        for reader in readers.into_iter().flatten() {
            let _ = reader.join();
        }
        if let Some(j) = job().as_mut().filter(|j| j.view.id == id) {
            j.view.running = false;
            j.view.code = code;
        }
        let _ = app.emit(JOB_EVENT, JobEvent::Exit { id, code });
        if let Some(console) = console {
            resume(&app, console);
        }
    });
    Ok(view)
}

/// Wait up to `limit` for job `id` to end, then report it.
pub fn wait(id: u32, limit: Duration) -> Option<JobView> {
    let start = Instant::now();
    loop {
        let view = current().filter(|j| j.id == id)?;
        if !view.running || start.elapsed() >= limit {
            return Some(view);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Stop the running job and everything it started.
pub fn stop() {
    let Some(pid) = job().as_ref().filter(|j| j.view.running).map(|j| j.pid) else { return };
    #[cfg(unix)]
    // SAFETY: the job leads its own process group, and it is not reaped until its waiter thread
    // sees it exit, so the id still names it.
    unsafe {
        libc::killpg(pid as libc::pid_t, libc::SIGTERM);
    }
    #[cfg(windows)]
    let _ = crate::launcher::without_console(Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"])).status();
}

#[tauri::command]
pub async fn board_job_start(
    app: AppHandle,
    webview: tauri::Webview,
    workspace_root: String,
    upload: bool,
    port: Option<String>,
) -> Result<JobView, String> {
    crate::app::main_only(&webview, "build or upload firmware")?;
    let root = crate::app::board_root(&app, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || start(&app, &root, upload, port)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn board_job_stop(webview: tauri::Webview) -> Result<(), String> {
    crate::app::main_only(&webview, "stop a build or upload")?;
    stop();
    Ok(())
}

#[tauri::command]
pub fn board_job(webview: tauri::Webview) -> Result<Option<JobView>, String> {
    crate::app::main_only(&webview, "read the build output")?;
    Ok(current())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    #[test]
    fn recognises_toolchains_by_their_project_files() {
        let name = |files: &[(&str, &str)]| describe(folder(files).path(), None).toolchain;
        assert_eq!(name(&[("platformio.ini", ""), ("blink.ino", "")]), Some("PlatformIO"));
        assert_eq!(name(&[("blink.ino", "")]), Some("Arduino"));
        assert_eq!(name(&[("CMakeLists.txt", "include($ENV{IDF_PATH}/tools/cmake/project.cmake)")]), Some("ESP-IDF"));
        assert_eq!(name(&[("prj.conf", "")]), Some("Zephyr"));
        assert_eq!(name(&[("Cargo.toml", ""), (".cargo/config.toml", "runner = \"espflash flash\"")]), Some("Rust (embedded)"));
        assert_eq!(name(&[("Cargo.toml", "")]), None, "an ordinary crate is not firmware");
        assert_eq!(name(&[("code.py", "")]), Some("MicroPython / CircuitPython"));
        assert_eq!(name(&[("README.md", "")]), None);
    }

    #[test]
    fn lists_sources_main_first_and_uses_saved_commands() {
        let dir = folder(&[("platformio.ini", ""), ("src/util.cpp", ""), ("src/main.cpp", ""), ("include/pins.h", ""), ("notes.txt", ""), (".env", "")]);
        let usual = describe(dir.path(), None);
        assert_eq!(usual.sources, ["src/main.cpp", "src/util.cpp", "platformio.ini", "include/pins.h"]);
        assert_eq!((usual.commands.build.as_str(), usual.custom, usual.python), ("pio run", false, false));
        let own = Commands { build: "make".into(), upload: "make flash PORT={port}".into() };
        let custom = describe(dir.path(), Some(own.clone()));
        assert_eq!((custom.commands, custom.custom), (own, true));
        assert!(describe(folder(&[("main.py", "")]).path(), None).python);
    }

    #[test]
    fn saved_commands_round_trip_per_project() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("nested/board-projects.json");
        let (a, b) = (Path::new("/code/a"), Path::new("/code/b"));
        let own = Commands { build: "make".into(), upload: String::new() };
        save(&file, a, Some(own.clone())).unwrap();
        save(&file, b, Some(Commands::default())).unwrap();
        assert_eq!(load(&file).get("/code/a"), Some(&own));
        save(&file, a, None).unwrap();
        assert_eq!(load(&file).keys().collect::<Vec<_>>(), ["/code/b"]);
    }

    #[test]
    fn the_port_fills_the_command_and_cannot_add_shell_words() {
        assert_eq!(fill("pio run", None).unwrap(), "pio run");
        assert_eq!(fill("up -p {port} .", Some("/dev/cu.usbmodem2101")).unwrap(), "up -p /dev/cu.usbmodem2101 .");
        assert_eq!(fill("up {port}", Some("COM3")).unwrap(), "up COM3");
        assert!(fill("up {port}", None).is_err());
        for port in ["", "/dev/x; rm -rf ~", "$(id)", "a b"] {
            assert!(fill("up {port}", Some(port)).is_err(), "{port}");
        }
    }
}
