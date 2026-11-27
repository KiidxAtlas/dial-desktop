//! Git branch, remote sync, and stash operations for the workspace panel.

use super::{
    GitBranchInfo, GitStashEntry, git_command, git_in, outside, panel_entry, relative,
    require_workspace_index,
};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};
/// Local branches and upstream ahead/behind counts for the workspace repository.
pub fn git_branches(root: &Path) -> Result<GitBranchInfo, String> {
    let inside = git_in(root, &["rev-parse", "--is-inside-work-tree"])?;
    if !inside.status.success() {
        return Err("Not a Git repository".into());
    }
    let head = git_in(root, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
    let current = if head.status.success() {
        Some(String::from_utf8_lossy(&head.stdout).trim().to_owned()).filter(|s| !s.is_empty())
    } else {
        None
    };
    let refs = git_in(root, &["for-each-ref", "--sort=refname", "--format=%(refname:short)", "refs/heads"])?;
    let local = if refs.status.success() {
        String::from_utf8_lossy(&refs.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect()
    } else {
        Vec::new()
    };
    let up = git_in(root, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])?;
    let upstream = if up.status.success() {
        Some(String::from_utf8_lossy(&up.stdout).trim().to_owned()).filter(|s| !s.is_empty())
    } else {
        None
    };
    let (mut ahead, mut behind) = (0, 0);
    if upstream.is_some() {
        let counts = git_in(root, &["rev-list", "--left-right", "--count", "@{u}...HEAD"])?;
        if counts.status.success() {
            let text = String::from_utf8_lossy(&counts.stdout);
            let mut parts = text.split_whitespace();
            behind = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            ahead = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        }
    }
    Ok(GitBranchInfo {
        current,
        local,
        upstream,
        ahead,
        behind,
    })
}

/// Switch to an existing local branch, or create and switch to a new branch when `create` is true.
pub fn git_checkout(root: &Path, branch: &str, create: bool) -> Result<String, String> {
    let name = branch.trim();
    if name.is_empty() || name.starts_with('-') {
        return Err("Enter a valid branch name".into());
    }
    let valid = git_in(root, &["check-ref-format", "--branch", name])?;
    if !valid.status.success() {
        return Err(format!("`{name}` is not a valid Git branch name"));
    }
    let out = if create {
        git_in(root, &["checkout", "-b", name])?
    } else {
        let exists = git_in(root, &["show-ref", "--verify", "--quiet", &format!("refs/heads/{name}")])?;
        if !exists.status.success() {
            return Err(format!("Local branch `{name}` does not exist"));
        }
        git_in(root, &["checkout", name])?
    };
    if out.status.success() {
        Ok(name.to_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        Err(if err.is_empty() { format!("Could not switch to `{name}`") } else { err })
    }
}

/// Tracks active `git_sync` operations by workspace root so the UI can cancel a hung remote call.
#[derive(Default)]
pub struct GitSyncCanceller {
    active: Mutex<HashMap<PathBuf, Arc<AtomicBool>>>,
}

pub struct GitSyncGuard {
    registry: Arc<GitSyncCanceller>,
    root: PathBuf,
    flag: Arc<AtomicBool>,
}

impl GitSyncGuard {
    pub fn flag(&self) -> &AtomicBool {
        &self.flag
    }
}

impl Drop for GitSyncGuard {
    fn drop(&mut self) {
        let mut map = self.registry.active.lock().unwrap_or_else(PoisonError::into_inner);
        if map.get(&self.root).is_some_and(|existing| Arc::ptr_eq(existing, &self.flag)) {
            map.remove(&self.root);
        }
    }
}

impl GitSyncCanceller {
    /// Register an active sync for `root`. Cancels any superseded sync on the same workspace.
    pub fn begin(self: &Arc<Self>, root: &Path) -> GitSyncGuard {
        let key = root.to_path_buf();
        let flag = Arc::new(AtomicBool::new(false));
        let mut map = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(prev) = map.insert(key.clone(), Arc::clone(&flag)) {
            prev.store(true, Ordering::SeqCst);
        }
        GitSyncGuard {
            registry: Arc::clone(self),
            root: key,
            flag,
        }
    }

    /// Request cancellation of the active sync for `root`. Returns `true` if one was running.
    pub fn cancel(&self, root: &Path) -> bool {
        let map = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(flag) = map.get(root) {
            flag.store(true, Ordering::SeqCst);
            true
        } else {
            false
        }
    }
}
fn terminate_child(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as libc::pid_t;
        if pid > 0 {
            // SAFETY: pid belongs to a process group created by this child via setpgid(0, 0).
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn run_cancellable(
    cmd: &mut Command,
    timeout: Duration,
    cancel: &AtomicBool,
    action: &str,
) -> Result<crate::updates::RunOutput, String> {
    if cancel.load(Ordering::SeqCst) {
        return Err(format!("Git {action} cancelled"));
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setpgid is async-signal-safe and isolates git + ssh/credential helpers so
        // cancellation terminates the entire process tree instead of leaving orphaned pipes.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setpgid(0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("could not start git {action}: {e}"))?;
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_reader = thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(pipe) = stdout.as_mut() {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let err_reader = thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(pipe) = stderr.as_mut() {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let started = Instant::now();
    let status = loop {
        if cancel.load(Ordering::SeqCst) {
            terminate_child(&mut child);
            let _ = out_reader.join();
            let _ = err_reader.join();
            return Err(format!("Git {action} cancelled"));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() >= timeout {
                    terminate_child(&mut child);
                    let _ = out_reader.join();
                    let _ = err_reader.join();
                    return Err(format!(
                        "Git {action} timed out after {} seconds",
                        timeout.as_secs()
                    ));
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                terminate_child(&mut child);
                let _ = out_reader.join();
                let _ = err_reader.join();
                return Err(format!("Git {action} failed while waiting: {e}"));
            }
        }
    };
    let stdout = String::from_utf8_lossy(&out_reader.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).into_owned();
    Ok(crate::updates::RunOutput {
        success: status.success(),
        stdout,
        stderr,
    })
}

/// Run a non-interactive remote sync (`fetch`, `pull` with `--ff-only`, or `push`).
pub fn git_sync(root: &Path, action: &str) -> Result<String, String> {
    git_sync_cancellable(root, action, &AtomicBool::new(false))
}

/// Run a cancellable non-interactive remote sync (`fetch`, `pull`, or `push`).
pub fn git_sync_cancellable(
    root: &Path,
    action: &str,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let mut cmd = match action {
        "fetch" => git_command(root, &["fetch", "--prune"]),
        "pull" => git_command(root, &["pull", "--ff-only"]),
        "push" => {
            let has_upstream = git_in(root, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
                .is_ok_and(|o| o.status.success());
            if has_upstream {
                git_command(root, &["push"])
            } else if let Ok(head) = git_in(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
                let branch = String::from_utf8_lossy(&head.stdout).trim().to_owned();
                let has_origin = git_in(root, &["remote", "get-url", "origin"])
                    .is_ok_and(|o| o.status.success());
                if head.status.success() && !branch.is_empty() && has_origin {
                    git_command(root, &["push", "-u", "origin", &branch])
                } else {
                    git_command(root, &["push"])
                }
            } else {
                git_command(root, &["push"])
            }
        }
        other => return Err(format!("Unknown Git sync action `{other}`")),
    };
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    let output = run_cancellable(&mut cmd, Duration::from_secs(120), cancel, action)?;
    if output.success {
        let combined = format!("{}\n{}", output.stdout.trim(), output.stderr.trim())
            .trim()
            .to_owned();
        Ok(if combined.is_empty() {
            match action {
                "fetch" => "Fetched remote changes".into(),
                "pull" => "Already up to date".into(),
                _ => "Pushed to remote".into(),
            }
        } else {
            combined
        })
    } else {
        let detail = if output.stderr.trim().is_empty() {
            output.stdout.trim()
        } else {
            output.stderr.trim()
        };
        Err(format_git_sync_error(action, detail))
    }
}
pub(super) fn format_git_sync_error(action: &str, detail: &str) -> String {
    let lower = detail.to_ascii_lowercase();
    if lower.contains("could not read username")
        || lower.contains("authentication failed")
        || lower.contains("permission denied (publickey)")
        || lower.contains("terminal prompts disabled")
    {
        return format!(
            "Git authentication is required for `{action}` (configure an SSH key or Git credential helper in your terminal).\n{detail}"
        );
    }
    if action == "pull"
        && (lower.contains("not possible to fast-forward") || lower.contains("divergent branches"))
    {
        return format!(
            "Branch has diverged from upstream and cannot fast-forward. Merge or rebase in the Terminal panel.\n{detail}"
        );
    }
    if lower.contains("would be overwritten by merge")
        || lower.contains("you have unstaged changes")
        || lower.contains("conflict")
    {
        return format!(
            "Working tree conflicts prevent `{action}`. Commit or stash your local changes first.\n{detail}"
        );
    }
    if detail.is_empty() {
        format!("git {action} failed")
    } else {
        detail.to_owned()
    }
}

/// List, push (including untracked files) or pop entries in Git's stash.
pub fn git_stash(root: &Path, action: &str, message: Option<&str>) -> Result<Vec<GitStashEntry>, String> {
    match action {
        "list" => {}
        "push" => {
            let msg = message.map(str::trim).filter(|m| !m.is_empty());
            let out = if let Some(msg) = msg {
                git_in(root, &["stash", "push", "-u", "-m", msg])?
            } else {
                git_in(root, &["stash", "push", "-u"])?
            };
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
                return Err(if err.is_empty() { "Could not stash changes".into() } else { err });
            }
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.contains("No local changes to save") {
                return Err("No local changes to stash".into());
            }
        }
        "pop" => {
            let out = git_in(root, &["stash", "pop"])?;
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
                return Err(if err.is_empty() { "Could not apply the latest stash".into() } else { err });
            }
        }
        other => return Err(format!("Unknown stash action `{other}`")),
    }
    let out = git_in(root, &["stash", "list", "--format=%gd\t%gs"])?;
    if !out.status.success() {
        return Err(format!(
            "git stash list failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let (reference, summary) = line.split_once('\t')?;
            Some(GitStashEntry {
                reference: reference.trim().to_owned(),
                summary: summary.trim().to_owned(),
            })
        })
        .collect())
}

/// Resolve an unmerged (conflicted) workspace file by taking `"ours"`, taking `"theirs"`, or
/// marking a manually edited file `"resolved"` (refusing if conflict markers remain).
pub fn git_resolve_conflict(root: &Path, path: &str, strategy: &str) -> Result<(), String> {
    let canonical = root
        .canonicalize()
        .map_err(|e| format!("The project folder is unavailable: {e}"))?;
    let entry = panel_entry(root, path)?;
    require_workspace_index(&canonical, &entry)?;
    let rel = relative(&canonical, &entry);
    if rel.is_empty() {
        return Err(outside(path));
    }
    if !super::diff::is_path_conflicted(&canonical, &rel) {
        return Err(format!("{path} does not have an active merge conflict"));
    }
    match strategy {
        "ours" | "theirs" => {
            let flag = if strategy == "ours" { "--ours" } else { "--theirs" };
            let out = git_in(&canonical, &["checkout", flag, "--", &rel])?;
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
                return Err(if err.is_empty() {
                    format!("Could not select `{strategy}` for {path}")
                } else {
                    err
                });
            }
            let add = git_in(&canonical, &["add", "--", &rel])?;
            if !add.status.success() {
                return Err(format!(
                    "git add failed: {}",
                    String::from_utf8_lossy(&add.stderr).trim()
                ));
            }
            Ok(())
        }
        "resolved" => {
            if entry.is_file() {
                let bytes = fs::read(&entry).map_err(|e| format!("Cannot read {path}: {e}"))?;
                let text = String::from_utf8_lossy(&bytes);
                let has_markers = text.lines().any(|line| {
                    line.starts_with("<<<<<<< ")
                        || line == "<<<<<<<"
                        || line == "======="
                        || line.starts_with(">>>>>>> ")
                        || line == ">>>>>>>"
                });
                if has_markers {
                    return Err(format!(
                        "Conflict markers (<<<<<<< / ======= / >>>>>>>) remain in {path}. Edit the file first or choose Keep ours / Take theirs."
                    ));
                }
                let add = git_in(&canonical, &["add", "--", &rel])?;
                if !add.status.success() {
                    return Err(format!(
                        "git add failed: {}",
                        String::from_utf8_lossy(&add.stderr).trim()
                    ));
                }
            } else {
                let rm = git_in(&canonical, &["rm", "--quiet", "--", &rel])?;
                if !rm.status.success() {
                    return Err(format!(
                        "git rm failed: {}",
                        String::from_utf8_lossy(&rm.stderr).trim()
                    ));
                }
            }
            Ok(())
        }
        other => Err(format!("Unknown conflict resolution strategy `{other}`")),
    }
}
