//! Debounced native filesystem watcher for the active Desktop workspace.
//!
//! Watches the open project folder via `notify` and coalesces bursts of events (such as `git
//! checkout` or a build) into a single callback so the Files, Changes, and Diff panels refresh
//! automatically when files change outside Dial.

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Quiet window before notifying the UI (trailing-edge debounce).
const DEBOUNCE: Duration = Duration::from_millis(250);
/// Maximum wait under sustained event bursts so continuous writes never starve UI refreshes.
const MAX_DEBOUNCE: Duration = Duration::from_millis(1000);
/// Directory names whose internal churn is never worth refreshing the workspace UI for.
const IGNORED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".cache",
    "__pycache__",
    ".venv",
    ".dial",
];

type Callback = Arc<dyn Fn(String) + Send + Sync>;

struct ActiveWatch {
    canonical: PathBuf,
    emit_root: String,
    _watcher: RecommendedWatcher,
    signal: Sender<Signal>,
    thread: Option<JoinHandle<()>>,
}

enum Signal {
    Changed,
    Stop,
}

impl ActiveWatch {
    fn shutdown(mut self) {
        let _ = self.signal.send(Signal::Stop);
        drop(self._watcher);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

/// Manages one debounced workspace filesystem watcher, retargetable when switching projects.
pub struct WorkspaceWatcher {
    on_change: Callback,
    active: Mutex<Option<ActiveWatch>>,
}

impl WorkspaceWatcher {
    pub fn new(on_change: impl Fn(String) + Send + Sync + 'static) -> Self {
        Self {
            on_change: Arc::new(on_change),
            active: Mutex::new(None),
        }
    }

    /// Watch `root` recursively, replacing any previously watched workspace.
    /// Emits `root` as given (preserving symlink spelling matching `snap.workspace.root`) while
    /// watching the canonical filesystem path so `notify` path filtering stays accurate.
    /// Fails softly (logs to stderr) if the OS refuses to install watches.
    pub fn watch(&self, root: &Path) {
        let emit_root = root.to_string_lossy().into_owned();
        let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let mut slot = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        if slot
            .as_ref()
            .is_some_and(|w| w.canonical == canonical && w.emit_root == emit_root)
        {
            return;
        }
        if let Some(prev) = slot.take() {
            prev.shutdown();
        }
        match start_watch(&canonical, emit_root, Arc::clone(&self.on_change)) {
            Ok(active) => *slot = Some(active),
            Err(error) => {
                eprintln!(
                    "dial-desktop: workspace file watcher unavailable for {}: {error}",
                    canonical.display()
                );
            }
        }
    }

    /// Stop watching the current workspace.
    pub fn stop(&self) {
        let mut slot = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(prev) = slot.take() {
            prev.shutdown();
        }
    }
}

impl Drop for WorkspaceWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

fn start_watch(canonical: &Path, emit_root: String, on_change: Callback) -> Result<ActiveWatch, String> {
    let (tx, rx) = mpsc::channel::<Signal>();
    let root_filter = canonical.to_path_buf();
    let tx_events = tx.clone();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
        let Ok(event) = result else { return };
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        let relevant = event.paths.is_empty()
            || event
                .paths
                .iter()
                .any(|path| !should_ignore_path(&root_filter, path));
        if relevant {
            let _ = tx_events.send(Signal::Changed);
        }
    })
    .map_err(|e| e.to_string())?;
    watcher
        .watch(canonical, RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;

    let emit_for_thread = emit_root.clone();
    let thread = thread::Builder::new()
        .name("dial-ws-watch".into())
        .spawn(move || debounce_loop(rx, emit_for_thread, on_change))
        .map_err(|e| e.to_string())?;

    Ok(ActiveWatch {
        canonical: canonical.to_path_buf(),
        emit_root,
        _watcher: watcher,
        signal: tx,
        thread: Some(thread),
    })
}

fn debounce_loop(rx: mpsc::Receiver<Signal>, root: String, on_change: Callback) {
    debounce_loop_with(rx, root, on_change, DEBOUNCE, MAX_DEBOUNCE);
}

fn debounce_loop_with(
    rx: mpsc::Receiver<Signal>,
    root: String,
    on_change: Callback,
    debounce: Duration,
    max_debounce: Duration,
) {
    let mut first_pending: Option<Instant> = None;
    let mut last_event: Option<Instant> = None;
    loop {
        let deadline = match (first_pending, last_event) {
            (Some(first), Some(last)) => Some((last + debounce).min(first + max_debounce)),
            _ => None,
        };
        if let Some(deadline) = deadline {
            let now = Instant::now();
            if now >= deadline {
                first_pending = None;
                last_event = None;
                on_change(root.clone());
                continue;
            }
            match rx.recv_timeout(deadline.saturating_duration_since(now)) {
                Ok(Signal::Changed) => {
                    let at = Instant::now();
                    first_pending.get_or_insert(at);
                    last_event = Some(at);
                }
                Ok(Signal::Stop) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {
                    first_pending = None;
                    last_event = None;
                    on_change(root.clone());
                }
            }
        } else {
            match rx.recv() {
                Ok(Signal::Changed) => {
                    let at = Instant::now();
                    first_pending = Some(at);
                    last_event = Some(at);
                }
                Ok(Signal::Stop) | Err(_) => return,
            }
        }
    }
}

/// Whether `path` inside `root` should be ignored by the workspace UI watcher.
/// Keeps `.git/HEAD`, `.git/index`, and `.git/refs/*` so branch switches, staging, and commits
/// trigger UI updates, while skipping noisy `.git/objects`, build artifacts, and temp save files.
pub(crate) fn should_ignore_path(root: &Path, path: &Path) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let parts: Vec<&str> = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        return false;
    }
    if parts
        .last()
        .is_some_and(|name| name.starts_with(".dial-tmp-") || *name == ".DS_Store")
    {
        return true;
    }
    if parts[0] == ".git" {
        return !matches!(
            parts.get(1).copied(),
            Some("HEAD" | "index" | "refs" | "packed-refs" | "MERGE_HEAD" | "REBASE_HEAD")
        );
    }
    parts.iter().any(|part| IGNORED_DIRS.contains(part))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn ignore_filter_skips_build_churn_but_keeps_source_and_git_state() {
        let root = Path::new("/repo");
        assert!(!should_ignore_path(root, Path::new("/repo/src/main.rs")));
        assert!(!should_ignore_path(root, Path::new("/repo/.git/HEAD")));
        assert!(!should_ignore_path(root, Path::new("/repo/.git/index")));
        assert!(!should_ignore_path(
            root,
            Path::new("/repo/.git/refs/heads/main")
        ));

        assert!(should_ignore_path(
            root,
            Path::new("/repo/.git/objects/ab/cdef")
        ));
        assert!(should_ignore_path(root, Path::new("/repo/.git/logs/HEAD")));
        assert!(should_ignore_path(
            root,
            Path::new("/repo/node_modules/pkg/index.js")
        ));
        assert!(should_ignore_path(
            root,
            Path::new("/repo/target/debug/dial")
        ));
        assert!(should_ignore_path(
            root,
            Path::new("/repo/src/.dial-tmp-123-0")
        ));
    }

    #[test]
    fn watcher_reports_external_file_changes_after_debounce() {
        let dir = tempfile::tempdir().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_cb = Arc::clone(&hits);
        let (tx, rx) = mpsc::channel();
        let watcher = WorkspaceWatcher::new(move |root| {
            hits_cb.fetch_add(1, Ordering::SeqCst);
            let _ = tx.send(root);
        });
        watcher.watch(dir.path());
        std::fs::write(dir.path().join("hello.txt"), "world\n").unwrap();
        let notified = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("watcher should notify on file creation");
        let expected = dir.path().to_string_lossy().into_owned();
        assert_eq!(notified, expected);
        assert!(hits.load(Ordering::SeqCst) >= 1);
        watcher.stop();
    }

    #[cfg(unix)]
    #[test]
    fn watcher_emits_original_symlink_root_spelling() {
        let parent = tempfile::tempdir().unwrap();
        let real = parent.path().join("real");
        let link = parent.path().join("link");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let (tx, rx) = mpsc::channel();
        let watcher = WorkspaceWatcher::new(move |root| {
            let _ = tx.send(root);
        });
        watcher.watch(&link);
        std::fs::write(real.join("changed.txt"), "ok\n").unwrap();
        let notified = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("watcher should notify through symlinked workspace root");
        assert_eq!(notified, link.to_string_lossy());
        watcher.stop();
    }

    #[test]
    fn continuous_event_burst_does_not_starve_notifications() {
        let (tx, rx) = mpsc::channel();
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_cb = Arc::clone(&hits);
        let handle = thread::spawn(move || {
            debounce_loop_with(
                rx,
                "/repo".into(),
                Arc::new(move |_| {
                    hits_cb.fetch_add(1, Ordering::SeqCst);
                }),
                Duration::from_millis(40),
                Duration::from_millis(90),
            );
        });
        // Send events faster than `debounce` for longer than `max_debounce`.
        // Keep the burst going until periodic notifications arrive, with a generous ceiling so a
        // loaded CI runner cannot turn scheduling jitter into a false failure.
        let end = Instant::now() + Duration::from_secs(5);
        while hits.load(Ordering::SeqCst) < 2 && Instant::now() < end {
            tx.send(Signal::Changed).unwrap();
            thread::sleep(Duration::from_millis(10));
        }
        let _ = tx.send(Signal::Stop);
        handle.join().unwrap();
        assert!(
            hits.load(Ordering::SeqCst) >= 2,
            "expected sustained burst to trigger periodic notifications instead of starving"
        );
    }
}
