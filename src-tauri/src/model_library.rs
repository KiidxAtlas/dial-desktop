//! Explicit weight downloads and Hub search through the official Hugging Face CLI (`hf`). Hugging
//! Face owns caching, authentication, integrity checks and retries.
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::State;
use std::path::Path;
mod setup;
mod atomic;
mod progress;
mod snapshots;
#[tauri::command]
pub async fn atomic_engine_setup(app: tauri::AppHandle) -> Result<bool, String> {
    atomic::setup(app).await
}
#[tauri::command]
pub async fn model_hub_setup(app: tauri::AppHandle) -> Result<bool, String> {
    setup::setup(app).await
}

const HF_QUERY_TIMEOUT: Duration = Duration::from_secs(30);
const HF_MAX_OUTPUT: u64 = 4 << 20;
#[derive(Deserialize)]
struct Model {
    repo: String,
    revision: String,
    file: Option<String>,
    #[serde(default)]
    companions: Vec<String>,
}

/// A Hub repo id, `owner/name`: letters, digits, `-`, `_` and `.`, never an option or a path.
fn valid_repo(repo: &str) -> bool {
    let part = |p: &str| {
        !p.is_empty()
            && !p.starts_with(['-', '.'])
            && !p.contains("--")
            && !p.contains("..")
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    repo.split_once('/')
        .is_some_and(|(owner, name)| part(owner) && part(name))
}

/// A file inside a repo, as the Hub lists it: relative, no `..`, never an option.
fn valid_file(file: &str) -> bool {
    !file.is_empty()
        && !file.starts_with(['-', '/'])
        && !file.contains('\\')
        && file
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}

/// GGUF split weights need every shard; this UI only downloads a single GGUF file.
fn split_gguf(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    let Some((prefix, total)) = path.strip_suffix(".gguf").and_then(|s| s.rsplit_once("-of-")) else {
        return false;
    };
    let Some((_, part)) = prefix.rsplit_once('-') else { return false };
    let number = |s: &str| (s.len() == 5 && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse::<u32>().ok()).flatten();
    matches!((number(part), number(total)), (Some(i), Some(n)) if n > 1 && i > 0 && i <= n)
}

fn hf(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    crate::launcher::without_console(&mut cmd);
    cmd.current_dir(std::env::temp_dir()).stdin(Stdio::null());
    cmd
}

fn hf_missing(e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        "The Hugging Face helper was not found. Choose Set up model downloads in the Model Library."
            .into()
    } else {
        format!("Could not run the Hugging Face CLI: {e}")
    }
}

/// `hf download` for `repo`: a curated model at its pinned revision and file, or any other Hub
/// repo (its whole main branch, or just `file`).
fn download_command(program: &Path, repo: &str, file: Option<&str>) -> Result<(Command, bool), String> {
    if file.is_some_and(split_gguf) {
        return Err("Split GGUF models are not supported by the Model Library".into());
    }
    let models: Vec<Model> = serde_json::from_str(include_str!("../../src/modelLibrary.json"))
        .map_err(|_| "Model catalog unavailable")?;
    let mut cmd = hf(program);
    let mut files = Vec::new();
    if let Some(m) = models
        .into_iter()
        .find(|m| m.repo == repo && (file.is_none() || file == m.file.as_deref()))
    {
        cmd.args(["download", &m.repo, "--revision", &m.revision]);
        if let Some(selected) = m.file {
            files.push(selected);
            if file.is_none() { files.extend(m.companions); }
        }
    } else {
        if !valid_repo(repo) {
            return Err("Not a Hugging Face model id".into());
        }
        cmd.args(["download", repo]);
        if let Some(file) = file {
            if !valid_file(file) {
                return Err("Not a file in that model".into());
            }
            files.push(file.to_owned());
        }
    }
    cmd.args(&files);
    progress::configure(&mut cmd);
    cmd.stdout(Stdio::null()).stderr(Stdio::piped());
    // hf downloads exactly one filename directly; zero or multiple filenames use snapshot_download.
    Ok((cmd, files.len() == 1))
}

/// Run `cmd` with a deadline and bounded stdout/stderr; kills the child on timeout.
fn run_bounded(
    mut cmd: Command,
    timeout: Duration,
) -> Result<(std::process::ExitStatus, Vec<u8>, Vec<u8>), String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(hf_missing)?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let read_pipe = |pipe: Option<std::process::ChildStdout>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(p) = pipe {
                let _ = p.take(HF_MAX_OUTPUT).read_to_end(&mut buf);
            }
            buf
        })
    };
    let read_err = |pipe: Option<std::process::ChildStderr>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(p) = pipe {
                let _ = p.take(HF_MAX_OUTPUT).read_to_end(&mut buf);
            }
            buf
        })
    };
    let out_handle = read_pipe(stdout);
    let err_handle = read_err(stderr);
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out_handle.join();
                let _ = err_handle.join();
                return Err(format!(
                    "Hugging Face request timed out after {} s",
                    timeout.as_secs()
                ));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Could not check `hf`: {e}"));
            }
        }
    };
    let out = out_handle.join().unwrap_or_default();
    let err = err_handle.join().unwrap_or_default();
    Ok((status, out, err))
}

/// Run `hf` with `args` and parse its JSON output.
fn hf_json<T: DeserializeOwned>(program: &Path, args: &[String]) -> Result<T, String> {
    let mut cmd = hf(program);
    cmd.args(args);
    let (status, stdout, stderr) = run_bounded(cmd, HF_QUERY_TIMEOUT)?;
    if !status.success() {
        let err = String::from_utf8_lossy(&stderr);
        let line = err
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("no details");
        return Err(format!("Hugging Face search failed: {}", line.trim()));
    }
    serde_json::from_slice(&stdout).map_err(|e| format!("Unexpected Hugging Face output: {e}"))
}

/// `hf models list` arguments: the most downloaded `format` (MLX or GGUF) models matching `query`.
fn search_args(query: &str, format: &str) -> Result<Vec<String>, String> {
    let filter = match format {
        "MLX" => "mlx",
        "GGUF" => "gguf",
        _ => return Err("Choose MLX or GGUF".into()),
    };
    let mut args = vec!["models".to_string(), "list".into()];
    if !query.trim().is_empty() {
        args.push(format!("--search={}", query.trim()));
    }
    for a in [
        "--filter",
        filter,
        "--sort",
        "downloads",
        "--limit",
        "40",
        "--format",
        "json",
    ] {
        args.push(a.into());
    }
    Ok(args)
}

/// A model found on the Hub.
#[derive(Serialize, Deserialize)]
pub struct HubModel {
    id: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    likes: u64,
    #[serde(default)]
    pipeline_tag: Option<String>,
}

/// Search the Hub for MLX or GGUF models, most downloaded first.
#[tauri::command]
pub async fn model_hub_search(app: tauri::AppHandle, query: String, format: String) -> Result<Vec<HubModel>, String> {
    let program = setup::hf_program(&app)?;
    let args = search_args(&query, &format)?;
    tauri::async_runtime::spawn_blocking(move || hf_json(&program, &args))
        .await
        .map_err(|e| e.to_string())?
}

/// A file in a Hub repo.
#[derive(Serialize, Deserialize)]
pub struct HubFile {
    path: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    split: bool,
}

/// The files in `repo` on the Hub, with sizes, so a download can be sized before it starts.
#[tauri::command]
pub async fn model_hub_files(app: tauri::AppHandle, repo: String) -> Result<Vec<HubFile>, String> {
    let program = setup::hf_program(&app)?;
    if !valid_repo(&repo) {
        return Err("Not a Hugging Face model id".into());
    }
    let args: Vec<String> = ["models", "list", &repo, "--recursive", "--format", "json"]
        .map(String::from)
        .to_vec();
    let mut files: Vec<HubFile> = tauri::async_runtime::spawn_blocking(move || hf_json(&program, &args))
        .await
        .map_err(|e| e.to_string())??;
    for file in &mut files {
        file.split = split_gguf(&file.path);
    }
    Ok(files)
}

#[derive(Default)]
pub struct Downloads(std::sync::Arc<Mutex<Job>>);
#[derive(Default)]
struct Job {
    child: Option<Child>,
    cache_lock: Option<std::fs::File>,
    status: Status,
    started: Option<Instant>,
    report: std::sync::Arc<Mutex<progress::Report>>,
    failed_exit: Option<std::process::ExitStatus>,
}
#[derive(Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    repo: String,
    running: bool,
    message: String,
    file: Option<String>,
    expected_bytes: u64,
    #[serde(flatten)]
    progress: progress::Metrics,
    elapsed_secs: u64,
    retryable: bool,
}
impl Job {
    fn refresh(&mut self) -> Result<(), String> {
        if let Some(t) = self.started { self.status.elapsed_secs = t.elapsed().as_secs(); }
        let report = self.report.lock().map_err(|_| "Download progress unavailable")?.clone();
        self.status.progress = report.metrics;
        if let Some(child) = self.child.as_mut()
            && let Some(exit) = child.try_wait().map_err(|e| e.to_string())?
        {
            self.child = None;
            self.cache_lock = None;
            self.status.running = false;
            self.started = None;
            self.status.message = if exit.success() {
                "Download complete. Open the session model picker to use it.".into()
            } else {
                self.failed_exit = Some(exit);
                self.status.retryable = true;
                String::new()
            };
        }
        if let Some(exit) = self.failed_exit {
            let advice = report.recovery.unwrap_or("Check your connection, free disk space and Hugging Face CLI installation, then retry.");
            self.status.message = format!("Download failed ({exit}). {advice}");
        }
        Ok(())
    }
    fn cancel(&mut self) -> Result<(), String> {
        self.refresh()?;
        if let Some(child) = self.child.as_mut() {
            child
                .kill()
                .and_then(|_| child.wait())
                .map_err(|e| e.to_string())?;
            self.child = None;
            self.cache_lock = None;
            self.status.running = false;
            self.started = None;
            self.status.retryable = true;
            self.status.message = "Cancelled. Cached files can be reused on retry.".into();
        }
        Ok(())
    }
}
impl Downloads {
    pub(crate) fn running(&self) -> bool {
        let mut job = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let _ = job.refresh();
        job.child.is_some()
    }

    pub fn stop(&self) {
        let _ = self.0.lock().unwrap_or_else(|e| e.into_inner()).cancel();
    }
}
#[tauri::command]
pub fn model_download_status(downloads: State<'_, Downloads>) -> Result<Status, String> {
    let mut job = downloads
        .0
        .lock()
        .map_err(|_| "Download manager unavailable")?;
    job.refresh()?;
    Ok(job.status.clone())
}
#[tauri::command]
pub fn model_download_start(
    app: tauri::AppHandle,
    downloads: State<'_, Downloads>,
    repo: String,
    file: Option<String>,
    expected_bytes: u64,
) -> Result<Option<Status>, String> {
    let mut job = downloads
        .0
        .lock()
        .map_err(|_| "Download manager unavailable")?;
    job.refresh()?;
    if job.child.is_some() {
        return Err("A model is already downloading in this window".into());
    }
    let cache = hub_cache().ok_or("No Hugging Face cache folder")?;
    let lock = lock_cache_repo(&cache, &repo)?;
    // A missing executable is distinct from permission, cache and model errors. Only that case
    // prompts the GUI to offer setup and retry the exact download that was already confirmed.
    let (mut command, single_file) = download_command(&setup::hf_program(&app)?, &repo, file.as_deref())?;
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(hf_missing(e)),
    };
    job.report = std::sync::Arc::new(Mutex::new(progress::Report::new(single_file)));
    if let Some(stderr) = child.stderr.take() {
        let report = job.report.clone();
        std::thread::spawn(move || progress::drain(stderr, &report));
    }
    job.started = Some(Instant::now());
    job.failed_exit = None;
    job.child = Some(child);
    job.cache_lock = Some(lock);
    job.status = Status {
        repo,
        running: true,
        message: "Downloading to your Hugging Face cache…".into(),
        file,
        expected_bytes,
        ..Status::default()
    };
    // Release the cache lock even if the user closes the Model Library while hf runs.
    let downloads = std::sync::Arc::downgrade(&downloads.0);
    let report = job.report.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let Some(downloads) = downloads.upgrade() else { break };
        let Ok(mut job) = downloads.lock() else { break };
        if !std::sync::Arc::ptr_eq(&job.report, &report) { break; }
        if job.refresh().is_err() || !job.status.running { break }
    });
    Ok(Some(job.status.clone()))
}
#[tauri::command]
pub fn model_download_cancel(downloads: State<'_, Downloads>) -> Result<Status, String> {
    let mut job = downloads
        .0
        .lock()
        .map_err(|_| "Download manager unavailable")?;
    job.cancel()?;
    Ok(job.status.clone())
}

/// A model repo already in the local Hugging Face cache.
#[derive(Serialize, Debug, PartialEq)]
pub struct CachedModel {
    repo: String,
    bytes: u64,
    files: Vec<snapshots::CachedFile>,
}

/// The hub cache `hf` uses: `HF_HUB_CACHE`, else `HF_HOME/hub`, else `$XDG_CACHE_HOME` /
/// `$HOME/.cache` / `%USERPROFILE%\.cache` + `huggingface/hub`.
fn hub_cache_from(mut var: impl FnMut(&str) -> Option<std::ffi::OsString>) -> Option<std::path::PathBuf> {
    let mut get = |k: &str| var(k).filter(|v| !v.is_empty());
    if let Some(dir) = get("HF_HUB_CACHE") {
        return Some(dir.into());
    }
    if let Some(home) = get("HF_HOME") {
        return Some(std::path::PathBuf::from(home).join("hub"));
    }
    get("XDG_CACHE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            // Python's expanduser on Windows uses USERPROFILE rather than a Git shell's HOME.
            let home = if cfg!(windows) {
                get("USERPROFILE").or_else(|| get("HOME"))
            } else {
                get("HOME").or_else(|| get("USERPROFILE"))
            };
            home.map(|h| std::path::PathBuf::from(h).join(".cache"))
        })
        .map(|c| c.join("huggingface").join("hub"))
}

fn hub_cache() -> Option<std::path::PathBuf> {
    hub_cache_from(|k| std::env::var_os(k))
}

/// Model repos in `cache` (`models--owner--name` folders), largest first. Sizes count the
/// downloaded blobs and unlinked snapshot files; partial downloads (`*.incomplete`) are left out.
fn cached_models_in(cache: &std::path::Path) -> Vec<CachedModel> {
    let Ok(entries) = std::fs::read_dir(cache) else {
        return Vec::new();
    };
    let mut out: Vec<CachedModel> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let repo = name.strip_prefix("models--")?.replacen("--", "/", 1);
            if !valid_repo(&repo) || !snapshots::real_dir(&entry.path()) { return None; }
            let mut files = Vec::new();
            let snapshot_bytes = snapshots::walk(&entry.path(), |_, f| files.push(f));
            files.sort_by(|a, b| a.path.cmp(&b.path).then(a.revision.cmp(&b.revision)));
            let bytes = std::fs::read_dir(entry.path().join("blobs"))
                .map(|blobs| {
                    blobs
                        .flatten()
                        .filter(|b| !b.file_name().to_string_lossy().ends_with(".incomplete"))
                        // Follow links: a blob may be a symlink to weights kept elsewhere.
                        .filter_map(|b| std::fs::metadata(b.path()).ok())
                        .filter(|m| m.is_file())
                        .map(|m| m.len())
                        .sum::<u64>()
                })
                .unwrap_or(0) + snapshot_bytes;
            (bytes > 0 || !files.is_empty()).then_some(CachedModel { repo, bytes, files })
        })
        .collect();
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.repo.cmp(&b.repo)));
    out
}

/// Models already downloaded to the Hugging Face cache, read from disk (no `hf` needed).
#[tauri::command]
pub async fn model_cache_list() -> Vec<CachedModel> {
    tauri::async_runtime::spawn_blocking(|| {
        hub_cache()
            .map(|c| cached_models_in(&c))
            .unwrap_or_default()
    })
    .await
    .unwrap_or_default()
}

/// Delete `repo`'s folder from `cache` and return the bytes freed. Only a real `models--owner--name`
/// folder directly in the cache is removed; links inside it are unlinked, never followed.
fn remove_cached_in(cache: &std::path::Path, repo: &str) -> Result<u64, String> {
    if !valid_repo(repo) {
        return Err("Not a Hugging Face model id".into());
    }
    let dir = cache.join(format!("models--{}", repo.replacen('/', "--", 1)));
    if !snapshots::real_dir(&dir) {
        return Err(format!("{repo} is not a cached model folder"));
    }
    let bytes = cached_models_in(cache)
        .into_iter()
        .find(|m| m.repo == repo)
        .map_or(0, |m| m.bytes);
    std::fs::remove_dir_all(&dir).map_err(|e| format!("Could not remove {repo}: {e}"))?;
    Ok(bytes)
}

/// Serialize Desktop downloads and removals across project windows. The lock file
/// stays outside the repo directory, which a removal deletes in its entirety.
fn lock_cache_repo(cache: &std::path::Path, repo: &str) -> Result<std::fs::File, String> {
    if !valid_repo(repo) {
        return Err("Not a Hugging Face model id".into());
    }
    let locks = cache.join(".dial-desktop-locks");
    let file = (|| -> std::io::Result<_> {
        std::fs::create_dir_all(&locks)?;
        std::fs::File::options().create(true).truncate(false).read(true).write(true)
            .open(locks.join(repo.replacen('/', "--", 1)))
    })().map_err(|e| format!("Could not open the model cache lock: {e}"))?;
    file.try_lock().map_err(|e| format!("Could not lock {repo}; another window may be downloading or removing it: {e}"))?;
    Ok(file)
}

/// Remove a downloaded model from the Hugging Face cache (not while it is downloading).
#[tauri::command]
pub async fn model_cache_remove(repo: String) -> Result<u64, String> {
    let cache = hub_cache().ok_or("No Hugging Face cache folder")?;
    tauri::async_runtime::spawn_blocking(move || {
        let _lock = lock_cache_repo(&cache, &repo)?;
        remove_cached_in(&cache, &repo)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Remove one GGUF entry, not the repository or any shared backing blob.
#[tauri::command]
pub async fn model_cache_remove_file(repo: String, revision: String, file: String) -> Result<(), String> {
    let cache = hub_cache().ok_or("No Hugging Face cache folder")?;
    tauri::async_runtime::spawn_blocking(move || {
        let _lock = lock_cache_repo(&cache, &repo)?;
        snapshots::remove(&cache, &repo, &revision, &file)
    }).await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn download_command(repo: &str, file: Option<&str>) -> Result<Command, String> {
        super::download_command(Path::new("hf"), repo, file).map(|(cmd, _)| cmd)
    }
    #[test]
    fn inkling_bundle_downloads_pinned_pair_but_file_picker_downloads_only_requested_file() {
        let repo = "AtomicChat/Inkling-2-Mini-GGUF";
        let target = "Inkling-2-Mini-AD-IQ2_M.gguf";
        let args = |file| download_command(repo, file).unwrap().get_args().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(args(None), ["download", repo, "--revision", "309509848b3151419aa26d069eb03db57fd29a00", target, "dspark-Inkling-2-Mini-Q8_0.gguf"]);
        assert_eq!(args(Some(target)).last().unwrap(), target);
        for (repo, file, single) in [(repo, None, false), (repo, Some(target), true), ("org/model", None, false), ("Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF", None, true)] {
            assert_eq!(super::download_command(Path::new("hf"), repo, file).unwrap().1, single);
        }
    }
    #[test]
    fn split_gguf_shards_are_not_individual_downloads() {
        for shard in ["q/model-00001-of-00002.gguf", "q/model-00002-of-00002.GGUF"] {
            assert!(split_gguf(shard));
            assert!(download_command("test/model", Some(shard)).is_err());
        }
        assert!(!split_gguf("q/model-Q4_K_M.gguf"));
        assert!(!split_gguf("q/model-00001-of-00001.gguf"));
    }

    #[test]
    fn cache_locks_exclude_independent_handles_for_the_same_repo() {
        let dir = std::env::temp_dir().join(format!("dial-hub-lock-{}", std::process::id()));
        let download = lock_cache_repo(&dir, "org/model").unwrap();
        assert!(lock_cache_repo(&dir, "org/model").is_err());
        assert!(lock_cache_repo(&dir, "org/another").is_ok());
        drop(download);
        assert!(lock_cache_repo(&dir, "org/model").is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn only_catalog_assets_or_hub_ids_can_be_requested() {
        assert!(download_command("--local-dir", None).is_err());
        let cmd = download_command("Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF", None).unwrap();
        assert_eq!(
            cmd.get_args().last().unwrap(),
            "qwen2.5-coder-1.5b-instruct-q4_k_m.gguf"
        );
        let cmd =
            download_command("bartowski/Some-Model-GGUF", Some("q/Model-Q4_K_M.gguf")).unwrap();
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(
            args,
            [
                "download",
                "bartowski/Some-Model-GGUF",
                "q/Model-Q4_K_M.gguf"
            ]
        );
        for (repo, file) in [
            ("owner", None),
            ("-x/y", None),
            ("a/../b", None),
            ("a/b/c", None),
            ("a/b", Some("--local-dir=/tmp")),
            ("a/b", Some("../x")),
            ("a/b", Some("/etc/passwd")),
        ] {
            assert!(download_command(repo, file).is_err(), "{repo} {file:?}");
        }
    }

    #[test]
    fn search_passes_the_query_as_a_value_never_an_option() {
        let args = search_args(" --token x ", "GGUF").unwrap();
        assert_eq!(args[2], "--search=--token x");
        assert!(args.windows(2).any(|w| w == ["--filter", "gguf"]));
        assert!(
            !search_args("", "MLX")
                .unwrap()
                .iter()
                .any(|a| a.starts_with("--search"))
        );
        assert!(search_args("q", "exe").is_err());
    }

    /// Searches the live Hub: `cargo test -- --ignored live_hub_search --nocapture`.
    #[test]
    #[ignore]
    fn live_hub_search() {
        let found: Vec<HubModel> = hf_json(Path::new("hf"), &search_args("qwen coder", "GGUF").unwrap()).unwrap();
        assert!(found.len() > 4, "only {} results", found.len());
        let files: Vec<HubFile> = hf_json(
            Path::new("hf"),
            &[
                "models",
                "list",
                &found[0].id,
                "--recursive",
                "--format",
                "json",
            ]
            .map(String::from),
        )
        .unwrap();
        let gguf: Vec<_> = files.iter().filter(|f| f.path.ends_with(".gguf")).collect();
        println!(
            "HUB {} results; {} has {} gguf files",
            found.len(),
            found[0].id,
            gguf.len()
        );
        assert!(gguf.iter().any(|f| f.size > 100_000_000));
    }

    #[cfg(unix)]
    #[test]
    fn finished_jobs_keep_the_exact_selection_and_only_unsuccessful_jobs_offer_retry() {
        for (program, cancel, retryable) in [("/usr/bin/true", false, false), ("/usr/bin/false", false, true), ("/bin/sleep", true, true)] {
            let mut cmd = Command::new(program);
            if cancel { cmd.arg("30"); }
            let mut child = cmd.spawn().unwrap();
            if !cancel { child.wait().unwrap(); }
            let mut job = Job { child: Some(child), status: Status { running: true, repo: "org/m".into(), file: Some("nested/q.gguf".into()), expected_bytes: 123, ..Status::default() }, ..Job::default() };
            if cancel { job.cancel().unwrap(); } else { job.refresh().unwrap(); }
            assert!(!job.status.running);
            assert_eq!(job.status.retryable, retryable);
            assert_eq!(job.status.file.as_deref(), Some("nested/q.gguf"));
            assert_eq!((&*job.status.repo, job.status.expected_bytes), ("org/m", 123));
        }
    }

    #[test]
    fn lists_cached_model_repos_with_downloaded_sizes() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let blobs = dir.join("models--mlx-community--Qwen2.5-7B").join("blobs");
        std::fs::create_dir_all(&blobs).unwrap();
        std::fs::write(blobs.join("a"), [0u8; 10]).unwrap();
        std::fs::write(blobs.join("b.incomplete"), [0u8; 99]).unwrap();
        std::fs::create_dir_all(dir.join("datasets--x--y").join("blobs")).unwrap();
        std::fs::create_dir_all(dir.join("models--empty--repo").join("blobs")).unwrap();
        let snapshots = dir.join("models--org--GGUF/snapshots/revision/quants");
        std::fs::create_dir_all(&snapshots).unwrap();
        std::fs::write(snapshots.join("model.gguf"), [0u8; 8]).unwrap();
        std::fs::write(snapshots.join("other.gguf.incomplete"), [0u8; 3]).unwrap();
        #[cfg(unix)]
        {
            let normal = blobs.parent().unwrap().join("snapshots/revision");
            std::fs::create_dir_all(&normal).unwrap();
            std::os::unix::fs::symlink(blobs.join("a"), normal.join("weights")).unwrap();
        }
        let found = cached_models_in(dir);
        assert_eq!(
            found,
            vec![
                CachedModel {
                    repo: "mlx-community/Qwen2.5-7B".into(),
                    bytes: 10,
                    files: vec![],
                },
                CachedModel {
                    repo: "org/GGUF".into(),
                    bytes: 8,
                    files: vec![snapshots::CachedFile { revision: "revision".into(), path: "quants/model.gguf".into(), bytes: 8 }],
                },
            ]
        );
        assert!(cached_models_in(std::path::Path::new("/nonexistent/hub")).is_empty());
    }

    #[test]
    fn removes_only_a_cached_model_folder() {
        let dir = std::env::temp_dir().join(format!("dial-hub-rm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let blobs = dir.join("models--org--Model-1").join("blobs");
        std::fs::create_dir_all(&blobs).unwrap();
        std::fs::write(blobs.join("a"), [0u8; 7]).unwrap();
        let keep = dir.join("models--org--Other").join("blobs");
        std::fs::create_dir_all(&keep).unwrap();
        std::fs::write(keep.join("b"), [0u8; 3]).unwrap();
        for bad in ["../x", "org", "org/../Other", "-x/y", "a/b/c"] {
            assert!(remove_cached_in(&dir, bad).is_err(), "{bad}");
        }
        assert!(remove_cached_in(&dir, "org/Missing").is_err());
        assert_eq!(remove_cached_in(&dir, "org/Model-1"), Ok(7));
        let left: Vec<String> = cached_models_in(&dir).into_iter().map(|m| m.repo).collect();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(left, ["org/Other"]);
    }

    #[test]
    fn hub_cache_falls_back_to_windows_userprofile() {
        let mut env = std::collections::HashMap::new();
        env.insert("USERPROFILE", std::ffi::OsString::from("C:\\Users\\alice"));
        let path = hub_cache_from(|k| env.get(k).cloned()).unwrap();
        assert_eq!(
            path,
            std::path::PathBuf::from("C:\\Users\\alice")
                .join(".cache")
                .join("huggingface")
                .join("hub")
        );
        env.insert("HOME", std::ffi::OsString::from("/home/alice"));
        let path = hub_cache_from(|k| env.get(k).cloned()).unwrap();
        assert_eq!(
            path,
            std::path::PathBuf::from(if cfg!(windows) { "C:\\Users\\alice" } else { "/home/alice" })
                .join(".cache")
                .join("huggingface")
                .join("hub")
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_bounded_kills_a_hanging_command_on_timeout() {
        let mut cmd = Command::new("/bin/sleep");
        cmd.arg("10");
        let err = run_bounded(cmd, Duration::from_millis(120)).unwrap_err();
        assert!(err.contains("timed out"), "{err}");
    }
    /// Lists the real cache: `cargo test -- --ignored real_hub_cache_has_models --nocapture`.
    #[test]
    #[ignore]
    fn real_hub_cache_has_models() {
        let found = cached_models_in(&hub_cache().expect("no cache dir"));
        for m in &found {
            println!("CACHED {} {}", m.repo, m.bytes);
        }
        assert!(!found.is_empty());
    }
}
