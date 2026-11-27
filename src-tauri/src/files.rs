//! File access for the workspace panels: listing folders, reading text files, diffing changed
//! files, Git status and content search. Every path is resolved inside the open workspace. The
//! writes are [`revert`] and [`revert_hunk`], which the user confirms in the UI first, and
//! [`stage`] and [`commit`], which only touch Git's index and history.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde::Serialize;
use sha2::{Digest, Sha256};

mod diff;
mod git_ops;
mod search;
pub use diff::{diff, diff_scoped, revert_hunk, stage_hunk, unstage_hunk};
pub use git_ops::{
    GitSyncCanceller, git_branches, git_checkout, git_resolve_conflict, git_stash, git_sync,
    git_sync_cancellable,
};
pub use search::grep;
#[cfg(test)]
use diff::split_hunks;
#[cfg(test)]
use git_ops::format_git_sync_error;

/// Larger files are shown truncated.
const MAX_READ: u64 = 512 * 1024;
/// Entries never listed in the file tree.
const HIDDEN: &[&str] = &[".git", ".DS_Store"];
/// Common dependency and build folders skipped when falling back to a directory walk.
const SKIP_DIRS: &[&str] = &[
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
    "venv",
];
/// Cap fallback directory walking so very large non-Git folders stay responsive.
const MAX_WALK_FILES: usize = 20_000;

/// One entry of a listed folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntryView {
    pub name: String,
    /// Relative to the workspace, `/`-separated.
    pub path: String,
    pub dir: bool,
    /// Matched by `.gitignore` (only listed when the panel asks for ignored entries).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub ignored: bool,
}

/// One changed path from `git status`, relative to the workspace and `/`-separated. Untracked
/// folders end in `/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileStatus {
    pub path: String,
    /// `M` modified, `A` added, `D` deleted, `R` renamed, `U` untracked, `C` conflicted.
    pub status: char,
    /// Lines added and removed, when counts were asked for and the file is text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed: Option<u64>,
    /// Some of the change is in Git's index (staged for the next commit).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub staged: bool,
    /// Some of the change is only in the working tree (not staged).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unstaged: bool,
}

/// The workspace's Git state for the file panels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusView {
    /// Whether the workspace itself has a Git index (the one used by commit controls).
    pub repo: bool,
    pub files: Vec<GitFileStatus>,
    /// The checked-out branch, `detached at <commit>` without one; `None` outside Git.
    pub branch: Option<String>,
    /// Successfully inspected child worktrees, with project-relative paths.
    pub nested_repos: Vec<String>,
}

/// One line of a content search: the file, its 1-based line number and the line's text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrepMatch {
    /// Relative to the workspace, `/`-separated.
    pub path: String,
    pub line: u64,
    /// The matching line, shortened to [`GREP_TEXT`] characters.
    pub text: String,
}

/// A file opened in the viewer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    /// Relative to the workspace, `/`-separated.
    pub path: String,
    /// `None` for binary files.
    pub text: Option<String>,
    pub size: u64,
    /// Only the first part of a large file is in `text`.
    pub truncated: bool,
    /// SHA-256 hex digest of the complete file on disk, used for optimistic conflict detection on save.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// The uncommitted changes of one file, as a unified diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffView {
    /// Relative to the workspace, `/`-separated.
    pub path: String,
    /// Empty when the file has no uncommitted changes.
    pub diff: String,
    pub base: DiffBase,
    /// Reverting deletes this entry rather than restoring a committed version.
    pub delete_on_revert: bool,
    /// Child repositories can be inspected without authorizing writes to their indexes.
    pub nested: bool,
    /// Whether this file currently has unresolved Git merge conflicts in the index.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub conflicted: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffBase {
    Head,
    Index,
    Untracked,
}

/// Which side of Git's index a file diff inspects.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffScope {
    /// Working tree against `HEAD` (both staged and unstaged changes).
    #[default]
    All,
    /// Working tree against Git's index (unstaged changes only).
    Unstaged,
    /// Git's index against `HEAD` (staged changes only).
    Staged,
}

/// Local branches and upstream sync state for the workspace repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitBranchInfo {
    pub current: Option<String>,
    pub local: Vec<String>,
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
}

/// One saved entry from `git stash list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStashEntry {
    pub reference: String,
    pub summary: String,
}

/// `path` (absolute, or relative to `root`) as an absolute path inside `root`; refuses anything
/// that leaves the workspace, including through `..` or a symlink.
pub fn resolve(root: &Path, path: &str) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("The project folder is unavailable: {e}"))?;
    let given = Path::new(path);
    let relative = if given.is_absolute() {
        let real = given.canonicalize().unwrap_or_else(|_| given.to_path_buf());
        real.strip_prefix(&root)
            .map(Path::to_path_buf)
            .map_err(|_| outside(path))?
    } else {
        given.to_path_buf()
    };
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(outside(path));
    }
    let joined = root.join(&relative);
    // A symlink inside the project may still point out of it.
    let real = joined
        .canonicalize()
        .map_err(|e| format!("Cannot open {path}: {e}"))?;
    if !real.starts_with(&root) {
        return Err(outside(path));
    }
    Ok(real)
}

fn outside(path: &str) -> String {
    format!("{path} is outside the project folder")
}

/// `path` relative to an already-canonicalized `canonical_root`, `/`-separated.
fn relative_to(canonical_root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(canonical_root).unwrap_or(path);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// `path` relative to `root`, `/`-separated.
fn relative(root: &Path, path: &Path) -> String {
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    relative_to(&canonical_root, path)
}

/// The entries of folder `path` (empty for the project root): folders first, then by name.
/// Entries `.gitignore` matches are left out.
pub fn list(root: &Path, path: &str) -> Result<Vec<DirEntryView>, String> {
    list_with(root, path, false)
}

/// Like [`list`]; with `show_ignored`, ignored entries are kept and marked instead.
pub fn list_with(root: &Path, path: &str, show_ignored: bool) -> Result<Vec<DirEntryView>, String> {
    // This also applies when reloading a directory remembered by an older tree.
    let dir = panel_entry(root, path)?;
    let meta = fs::symlink_metadata(&dir).map_err(|e| format!("Cannot list {path}: {e}"))?;
    if !meta.is_dir() || directory_reparse_point(&meta) { return Err(format!("{path} is not a plain folder")); }
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let entries = fs::read_dir(&dir).map_err(|e| format!("Cannot list {path}: {e}"))?;
    let mut out: Vec<DirEntryView> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if HIDDEN.contains(&name.as_str()) {
                return None;
            }
            // Do not recurse through directory links/junctions (including links to ancestors).
            let meta = entry.metadata().ok()?;
            let is_dir = meta.is_dir() && !directory_reparse_point(&meta);
            Some(DirEntryView {
                path: relative_to(&canonical_root, &dir.join(&name)),
                name,
                dir: is_dir,
                ignored: false,
            })
        })
        .collect();
    let ignored = ignored_paths(root, &out);
    if show_ignored {
        for entry in &mut out {
            entry.ignored = ignored.contains(&entry.path);
        }
    } else {
        out.retain(|entry| !ignored.contains(&entry.path));
    }
    out.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

/// Run `git -C root args…`.
fn git_in(root: &Path, args: &[&str]) -> Result<Output, String> {
    git_command(root, args)
        .output()
        .map_err(|e| format!("Could not run git: {e}"))
}

fn git_command(root: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    crate::launcher::without_console(&mut cmd).arg("-C").arg(root);
    // Inspecting a child worktree must not start a monitor configured by that worktree.
    cmd.args(["-c", "core.fsmonitor=false"]);
    // Panel paths are filenames, never user-entered patterns. Do not inherit a shell's
    // pathspec mode (including case folding); check-ignore requires plain ./-prefixed names.
    if matches!(args.first().copied(), Some("add" | "restore" | "rm" | "diff" | "ls-files" | "ls-tree" | "check-ignore")) {
        for key in ["GIT_LITERAL_PATHSPECS", "GIT_GLOB_PATHSPECS", "GIT_NOGLOB_PATHSPECS", "GIT_ICASE_PATHSPECS"] {
            cmd.env_remove(key);
        }
        if args[0] != "check-ignore" { cmd.arg("--literal-pathspecs"); }
    }
    cmd.args(args);
    cmd
}

/// The workspace's path inside its Git repository (`""` at the top, else ending in `/`);
/// `None` outside a work tree or without Git.
fn git_prefix(root: &Path) -> Option<String> {
    let out = git_in(root, &["rev-parse", "--show-prefix"]).ok()?;
    if !out.status.success() { return None; }
    Some(std::str::from_utf8(&out.stdout).ok()?.trim_end_matches(['\n', '\r']).to_owned())
}

/// The paths among `entries` that `.gitignore` (or Git's other exclude files) matches. Empty
/// outside Git, and when the workspace itself sits in an ignored folder of an enclosing
/// repository, where everything would match.
fn ignored_paths(root: &Path, entries: &[DirEntryView]) -> HashSet<String> {
    let mut out = HashSet::new();
    if entries.is_empty() {
        return out;
    }
    let Some(prefix) = git_prefix(root) else {
        return out;
    };
    if let Some(name) = prefix.trim_end_matches('/').rsplit('/').next().filter(|n| !n.is_empty())
        && let Some(parent) = root.canonicalize().ok().and_then(|r| r.parent().map(Path::to_path_buf))
        && git_in(&parent, &["check-ignore", "-q", "--", &format!("./{name}/")])
            .is_ok_and(|o| o.status.success())
    {
        return out;
    }
    // Folders get a trailing `/` so folder-only patterns such as `build/` match them.
    let mut input = Vec::new();
    for entry in entries {
        // check-ignore does not support :(literal); ./ also disarms leading colon magic.
        input.extend_from_slice(b"./");
        input.extend_from_slice(entry.path.as_bytes());
        if entry.dir {
            input.push(b'/');
        }
        input.push(0);
    }
    let Ok(output) = git_with_input(root, &["check-ignore", "--stdin", "-z"], input) else {
        return out;
    };
    // Exit 1: nothing matched; anything else but 0 is an error, treated the same.
    if !output.status.success() {
        return out;
    }
    for raw in output.stdout.split(|&b| b == 0).filter(|raw| !raw.is_empty()) {
        let path = String::from_utf8_lossy(raw);
        out.insert(path.strip_prefix("./").unwrap_or(&path).trim_end_matches('/').to_owned());
    }
    out
}

/// Run `git -C root args…` with `input` on its standard input (written from a thread so a full
/// output pipe cannot stall the write).
fn git_with_input(root: &Path, args: &[&str], input: Vec<u8>) -> std::io::Result<Output> {
    with_input(&mut git_command(root, args), input)
}

fn with_input(command: &mut Command, input: Vec<u8>) -> std::io::Result<Output> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take();
    let writer = std::thread::spawn(move || {
        if let Some(stdin) = stdin.as_mut() {
            let _ = stdin.write_all(&input);
        }
    });
    let output = child.wait_with_output();
    let _ = writer.join();
    output
}

/// Look for child worktrees, including submodules and linked worktrees (`.git` files).
/// Never descend through links, Git metadata or common dependency/build folders.
fn nested_worktrees(root: &Path) -> Vec<PathBuf> {
    let mut queue = std::collections::VecDeque::from([root.to_owned()]);
    let mut repos = Vec::new();
    let mut visited = 0;
    while let Some(dir) = queue.pop_front() {
        let Ok(entries) = fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            if !kind.is_dir() { continue; }
            let Ok(name) = entry.file_name().into_string() else { continue };
            if name.eq_ignore_ascii_case(".git") || SKIP_DIRS.contains(&name.as_str()) { continue; }
            if !entry.metadata().is_ok_and(|meta| !directory_reparse_point(&meta)) { continue; }
            let path = entry.path();
            if worktree_marker(&path) { repos.push(path.clone()); }
            queue.push_back(path);
            visited += 1;
            if visited >= MAX_WALK_FILES { return repos; }
        }
    }
    repos
}

fn worktree_marker(dir: &Path) -> bool {
    fs::symlink_metadata(dir.join(".git")).is_ok_and(|m| (m.is_dir() || m.is_file()) && !directory_reparse_point(&m))
}

/// A gitfile can redirect Git. Only report a child whose worktree is actually this folder.
fn is_worktree_root(dir: &Path) -> bool {
    git_checked(dir, &["rev-parse", "--show-toplevel"]).ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|top| Path::new(top.trim_end_matches(['\r', '\n'])).canonicalize().ok())
        .is_some_and(|top| top == dir)
}

/// Aggregate files, not branches or indexes: commit controls always describe the workspace.
pub fn status(root: &Path, counts: bool) -> Result<GitStatusView, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("The project folder is unavailable: {e}"))?;
    let prefix = git_prefix(&root);
    let children = nested_worktrees(&root);
    let mut files = match &prefix {
        Some(prefix) => repo_status(&root, prefix, counts, !children.is_empty())?,
        None => Vec::new(),
    };
    let mut nested_repos = Vec::new();
    for child in children {
        if !is_worktree_root(&child) { continue; }
        let Ok(changes) = repo_status(&child, "", counts, true) else { continue };
        let path = relative(&root, &child);
        let prefix = format!("{path}/");
        // Git lists an embedded repository as an untracked directory. Its own status
        // supersedes that placeholder (but not an actual staged submodule pointer).
        files.retain(|f| !f.path.starts_with(&prefix));
        files.extend(changes.into_iter().map(|mut f| { f.path.insert_str(0, &prefix); f }));
        nested_repos.push(path);
    }
    Ok(GitStatusView { repo: prefix.is_some(), branch: prefix.and_then(|_| current_branch(&root)), files, nested_repos })
}

fn repo_status(root: &Path, prefix: &str, counts: bool, nested: bool) -> Result<Vec<GitFileStatus>, String> {
    let untracked = if nested { "--untracked-files=all" } else { "--untracked-files=normal" };
    let output = git_in(
        root,
        &["status", "--porcelain=v1", "-z", untracked, "--", "."],
    )?;
    if !output.status.success() {
        return Err(format!(
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut files = parse_status(&output.stdout, prefix);
    if counts && !files.is_empty() {
        let has_head = git_in(root, &["rev-parse", "--verify", "--quiet", "HEAD"])?
            .status
            .success();
        let stats = if has_head {
            git_in(root, &["diff", "--numstat", "--no-ext-diff", "--no-textconv", "-z", "--relative", "--no-color", "HEAD", "--"])
                .ok()
                .filter(|o| o.status.success())
                .map(|o| parse_numstat(&o.stdout))
                .unwrap_or_default()
        } else {
            HashMap::new()
        };
        for file in &mut files {
            if let Some(&(added, removed)) = stats.get(&file.path) {
                (file.added, file.removed) = (added, removed);
            } else if file.status == 'U' && !file.path.ends_with('/') {
                file.added = untracked_lines(root, &file.path);
                file.removed = file.added.map(|_| 0);
            }
        }
    }
    Ok(files)
}

/// The checked-out branch's short name, or `detached at <commit>`; `None` before the first commit
/// on a detached head or when Git cannot say.
fn current_branch(root: &Path) -> Option<String> {
    let line = |args: &[&str]| {
        git_in(root, args)
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .filter(|s| !s.is_empty())
    };
    line(&["symbolic-ref", "--short", "-q", "HEAD"])
        .or_else(|| line(&["rev-parse", "--short", "HEAD"]).map(|sha| format!("detached at {sha}")))
}

/// Entries of `git status --porcelain=v1 -z`, whose paths are relative to the repository top,
/// re-rooted at the workspace (`prefix`, from `git rev-parse --show-prefix`).
fn parse_status(raw: &[u8], prefix: &str) -> Vec<GitFileStatus> {
    let mut out = Vec::new();
    let mut fields = raw.split(|&b| b == 0);
    while let Some(field) = fields.next() {
        if field.len() < 4 {
            continue;
        }
        let (x, y) = (field[0], field[1]);
        // A rename or copy is followed by its original path.
        if matches!(x, b'R' | b'C') {
            fields.next();
        }
        // The JSON UI cannot identify a non-Unicode file without changing its name.
        let Ok(path) = std::str::from_utf8(&field[3..]) else { continue };
        let Some(path) = path.strip_prefix(prefix).filter(|p| !p.is_empty()) else {
            continue;
        };
        let status = match (x, y) {
            (b'?', b'?') => 'U',
            (b'!', b'!') => continue,
            (b'U', _) | (_, b'U') | (b'A', b'A') | (b'D', b'D') => 'C',
            (b'A', _) => 'A',
            (b'R' | b'C', _) => 'R',
            (b'D', _) | (_, b'D') => 'D',
            _ => 'M',
        };
        let staged = !matches!(x, b' ' | b'?');
        let unstaged = !matches!(y, b' ') || status == 'U';
        out.push(GitFileStatus {
            path: path.to_owned(),
            status,
            added: None,
            removed: None,
            staged,
            unstaged,
        });
    }
    out
}

/// `git diff --numstat -z` as path → (added, removed); binary files have no counts.
fn parse_numstat(raw: &[u8]) -> HashMap<String, (Option<u64>, Option<u64>)> {
    let mut out = HashMap::new();
    let mut fields = raw.split(|&b| b == 0);
    while let Some(field) = fields.next() {
        let Ok(text) = std::str::from_utf8(field) else { continue };
        let mut parts = text.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        // A rename has an empty path here, then its old and new paths as separate fields.
        let path = if path.is_empty() {
            fields.next();
            match fields.next().and_then(|new| std::str::from_utf8(new).ok()) {
                Some(new) => new.to_owned(),
                None => continue,
            }
        } else {
            path.to_owned()
        };
        out.insert(path, (added.parse().ok(), removed.parse().ok()));
    }
    out
}

/// The line count of untracked text file `path`, up to [`MAX_READ`]; `None` for binary or larger files.
fn untracked_lines(root: &Path, path: &str) -> Option<u64> {
    let file = resolve(root, path).ok()?;
    if fs::metadata(&file).ok()?.len() > MAX_READ {
        return None;
    }
    let text = decode_text(&fs::read(&file).ok()?, false)?;
    Some(text.lines().count() as u64)
}

/// Windows junctions and other directory reparse points need not report a symlink file type.
fn directory_reparse_point(_meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const DIRECTORY_REPARSE: u32 = 0x10 | 0x400;
        return _meta.file_attributes() & DIRECTORY_REPARSE == DIRECTORY_REPARSE;
    }
    #[cfg(not(windows))]
    false
}

/// The selected directory entry, without following a link and losing its Git name.
fn workspace_entry(root: &Path, path: &str) -> Result<PathBuf, String> {
    let canonical = root.canonicalize().map_err(|e| format!("The project folder is unavailable: {e}"))?;
    let given = Path::new(path);
    let rel = if given.is_absolute() {
        given.strip_prefix(root).or_else(|_| given.strip_prefix(&canonical)).or_else(|_| {
            // Windows may spell the same root with a different case or verbatim prefix.
            let ancestor = given.ancestors().filter(|a| a.canonicalize().is_ok_and(|p| p == canonical)).last();
            given.strip_prefix(ancestor.ok_or_else(|| outside(path))?).map_err(|_| outside(path))
        })?
    } else {
        given
    };
    if rel.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
        return Err(outside(path));
    }
    Ok(canonical.join(rel))
}

/// Address a missing entry (deletion) or an in-project link (the link, not its target).
/// Refuse traversal through links, dangling or escaping links, and Git's own metadata.
fn panel_entry(root: &Path, path: &str) -> Result<PathBuf, String> {
    let entry = workspace_entry(root, path)?;
    let canonical = root.canonicalize().map_err(|e| e.to_string())?;
    let rel = entry.strip_prefix(&canonical).map_err(|_| outside(path))?;
    let mut current = canonical.clone();
    for part in rel.components() {
        if part.as_os_str().eq_ignore_ascii_case(".git") {
            return Err("Cannot change Git metadata".into());
        }
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) if directory_reparse_point(&meta) => return Err(format!("Cannot address {path} through a directory reparse point")),
            Ok(meta) if meta.is_symlink() => {
                let target = current.canonicalize().map_err(|e| format!("Cannot resolve {path}: {e}"))?;
                if !target.starts_with(&canonical) { return Err(outside(path)); }
                if current != entry { return Err(format!("Cannot address {path} through a directory link")); }
            }
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(format!("Cannot open {path}: {e}")),
            _ => {}
        }
    }
    Ok(entry)
}

/// Find the nearest repository boundary without walking outside the authorized workspace.
/// The entry itself may be deleted or a symlink; ownership is determined by its parents.
fn nested_owner(root: &Path, entry: &Path) -> Option<PathBuf> {
    entry.parent()?.ancestors().take_while(|p| p.starts_with(root) && *p != root)
        .find(|p| fs::symlink_metadata(p.join(".git")).is_ok()).map(Path::to_path_buf)
}

fn require_workspace_index(root: &Path, entry: &Path) -> Result<(), String> {
    if nested_owner(root, entry).is_some() {
        return Err("Open the nested repository as a project to change it".into());
    }
    Ok(())
}

/// Exact state of one file across the worktree, index and HEAD. Even a literal pathspec
/// selects descendants of a directory; a single-file operation must refuse such a selection.
struct GitFile {
    entry: PathBuf,
    rel: String,
    in_head: bool,
    has_head: bool,
    in_index: bool,
}

fn git_checked(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = git_in(root, args)?;
    if out.status.success() { Ok(out.stdout) } else {
        Err(format!("git {} failed: {}", args[0], String::from_utf8_lossy(&out.stderr).trim()))
    }
}

fn has_head(root: &Path) -> Result<bool, String> {
    let head = git_in(root, &["rev-parse", "--verify", "--quiet", "HEAD"])?;
    match head.status.code() {
        Some(0) => Ok(true),
        Some(1) if head.stderr.is_empty() => {
            // Unborn HEAD points at a named branch with no ref yet, not an unreadable commit.
            let branch = git_checked(root, &["symbolic-ref", "HEAD"])?;
            let branch = std::str::from_utf8(&branch).map_err(|_| "Cannot read the current branch")?.trim_end_matches(['\r', '\n']);
            let reference = git_in(root, &["show-ref", "--verify", "--quiet", branch])?;
            if reference.status.code() != Some(1) || !reference.stderr.is_empty() { return Err("Cannot verify the current HEAD".into()); }
            Ok(false)
        }
        _ => Err("Cannot read the current HEAD".into()),
    }
}

fn git_file(root: &Path, path: &str) -> Result<GitFile, String> {
    let entry = panel_entry(root, path)?;
    if fs::symlink_metadata(&entry).is_ok_and(|m| m.is_dir()) { return Err(format!("{path} is a folder")); }
    if git_prefix(root).is_none() { return Err("File changes need the project to be a Git repository".into()); }
    let rel = relative(root, &entry);
    let has_head = has_head(root)?;
    let tree = if has_head { git_checked(root, &["ls-tree", "-z", "HEAD", "--", &rel])? } else { Vec::new() };
    if !tree.is_empty() && tree.split(|&b| b == b' ').nth(1) != Some(b"blob") { return Err(format!("{path} is not a file in HEAD")); }
    let index = git_checked(root, &["ls-files", "-z", "--", &rel])?;
    if index.split(|&b| b == 0).any(|p| !p.is_empty() && p != rel.as_bytes()) {
        return Err(format!("{path} is not a single file in the index"));
    }
    if tree.is_empty() && index.is_empty() {
        // On a case-insensitive filesystem an alternate spelling may open a tracked file
        // even though HEAD and the index have no such name. Never delete it as 'untracked'.
        let others = git_checked(root, &["ls-files", "--others", "-z", "--", &rel])?;
        if others.strip_suffix(b"\0") != Some(rel.as_bytes()) { return Err(format!("Cannot identify {path} as an untracked file")); }
    }
    Ok(GitFile { entry, rel, has_head, in_head: !tree.is_empty(), in_index: !index.is_empty() })
}

/// Discard every uncommitted change to file `path`: a tracked file goes back to `HEAD` in both the
/// working tree and the index; a file added since `HEAD` (or untracked) is unstaged and deleted.
/// This also drops the user's own edits to that file; the UI confirms before calling it.
pub fn revert(root: &Path, path: &str) -> Result<(), String> {
    require_workspace_index(&root.canonicalize().map_err(|e| e.to_string())?, &panel_entry(root, path)?)?;
    let file = git_file(root, path)?;
    let failed = |what: &str, output: Output| {
        format!("{what} failed: {}", String::from_utf8_lossy(&output.stderr).trim())
    };
    if file.in_head {
        let output = git_in(
            root,
            &["restore", "--source=HEAD", "--worktree", "--staged", "--", &file.rel],
        )?;
        return if output.status.success() { Ok(()) } else { Err(failed("git restore", output)) };
    }
    if file.in_index {
        let output = git_in(root, &["rm", "--cached", "--quiet", "--force", "--", &file.rel])?;
        if !output.status.success() {
            return Err(failed("git rm", output));
        }
    }
    match fs::remove_file(&file.entry) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("Cannot delete {path}: {e}")),
        _ => Ok(()),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

/// Atomically replace `target` via a sibling temp file + `sync_all` + `rename`.
fn atomic_write(
    target: &Path,
    bytes: &[u8],
    preserve_perms: Option<fs::Permissions>,
    label: &str,
) -> Result<(), String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_TMP: AtomicU64 = AtomicU64::new(0);
    let parent = target
        .parent()
        .ok_or_else(|| format!("Cannot determine parent folder for {label}"))?;
    let pid = std::process::id();
    let mut temp_path = None;
    let mut temp_file = None;
    for _ in 0..16 {
        let seq = NEXT_TMP.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(".dial-tmp-{pid}-{seq}"));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temp_path = Some(candidate);
                temp_file = Some(file);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Cannot stage save for {label}: {e}")),
        }
    }
    let (temp_path, mut file) = match (temp_path, temp_file) {
        (Some(p), Some(f)) => (p, f),
        _ => return Err(format!("Cannot create temporary file for {label}")),
    };
    let result = (|| -> Result<(), String> {
        file.write_all(bytes)
            .map_err(|e| format!("Cannot save {label}: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("Cannot flush {label}: {e}"))?;
        if let Some(perms) = preserve_perms {
            let _ = file.set_permissions(perms);
        }
        drop(file);
        fs::rename(&temp_path, target).map_err(|e| format!("Cannot save {label}: {e}"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

/// Write text `content` atomically to an existing plain file inside the workspace, refusing
/// escapes, `.git` metadata, symlinks, oversized payloads and stale writes when `expected_sha256`
/// no longer matches the file on disk. Returns the refreshed [`FileView`].
pub fn write(
    root: &Path,
    path: &str,
    content: &str,
    expected_sha256: Option<&str>,
) -> Result<FileView, String> {
    if content.len() as u64 > MAX_READ {
        return Err(format!(
            "File is larger than the {} KB inline editor limit",
            MAX_READ / 1024
        ));
    }
    let entry = panel_entry(root, path)?;
    let meta = fs::symlink_metadata(&entry).map_err(|e| format!("Cannot open {path}: {e}"))?;
    if meta.is_symlink() {
        return Err(format!("Cannot edit symlink {path} inline"));
    }
    if meta.is_dir() {
        return Err(format!("{path} is a folder"));
    }
    if let Some(expected) = expected_sha256 {
        let current = fs::read(&entry).map_err(|e| format!("Cannot read {path}: {e}"))?;
        if sha256_hex(&current) != expected {
            return Err(format!(
                "{path} changed on disk since you opened it. Reload the file and try again."
            ));
        }
    }
    atomic_write(&entry, content.as_bytes(), Some(meta.permissions()), path)?;
    read(root, path)
}
/// Create a new text file atomically inside the workspace, refusing escapes, `.git`, symlinks,
/// existing paths and oversized payloads. Creates missing parent directories safely.
pub fn create_file(root: &Path, path: &str, content: &str) -> Result<FileView, String> {
    if path.trim().is_empty() {
        return Err("Enter a file path".into());
    }
    if content.len() as u64 > MAX_READ {
        return Err(format!(
            "File is larger than the {} KB inline editor limit",
            MAX_READ / 1024
        ));
    }
    let entry = panel_entry(root, path)?;
    if fs::symlink_metadata(&entry).is_ok() {
        return Err(format!("{path} already exists"));
    }
    if let Some(parent) = entry.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Cannot create folder for {path}: {e}"))?;
    }
    atomic_write(&entry, content.as_bytes(), None, path)?;
    read(root, path)
}

/// Rename or move a file or folder inside the workspace, refusing escapes, `.git`, symlinks
/// and overwriting an existing destination. Returns the new workspace-relative path.
pub fn rename_file(root: &Path, from: &str, to: &str) -> Result<String, String> {
    if to.trim().is_empty() {
        return Err("Enter a destination path".into());
    }
    let canonical = root
        .canonicalize()
        .map_err(|e| format!("The project folder is unavailable: {e}"))?;
    let src = panel_entry(root, from)?;
    if src == canonical {
        return Err("Cannot rename the project root".into());
    }
    let src_meta = fs::symlink_metadata(&src).map_err(|e| format!("Cannot open {from}: {e}"))?;
    if src_meta.is_symlink() {
        return Err(format!("Cannot rename symlink {from}"));
    }
    let dst = panel_entry(root, to)?;
    if dst == canonical {
        return Err("Cannot overwrite the project root".into());
    }
    if fs::symlink_metadata(&dst).is_ok() {
        return Err(format!("{to} already exists"));
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Cannot create folder for {to}: {e}"))?;
    }
    fs::rename(&src, &dst).map_err(|e| format!("Cannot rename {from}: {e}"))?;
    Ok(relative(&canonical, &dst))
}

/// Delete a plain file or empty folder inside the workspace, refusing escapes, `.git`,
/// symlinks and the project root.
pub fn delete_file(root: &Path, path: &str) -> Result<(), String> {
    let canonical = root
        .canonicalize()
        .map_err(|e| format!("The project folder is unavailable: {e}"))?;
    let entry = panel_entry(root, path)?;
    if entry == canonical {
        return Err("Cannot delete the project root".into());
    }
    let meta = fs::symlink_metadata(&entry).map_err(|e| format!("Cannot open {path}: {e}"))?;
    if meta.is_symlink() {
        return Err(format!("Cannot delete symlink {path}"));
    }
    if meta.is_dir() {
        fs::remove_dir(&entry).map_err(|e| format!("Cannot delete folder {path}: {e}"))
    } else {
        fs::remove_file(&entry).map_err(|e| format!("Cannot delete {path}: {e}"))
    }
}

/// Stage (`git add`) or unstage files `paths`; unstaging keeps the working tree as it is.
pub fn stage(root: &Path, paths: &[String], staged: bool) -> Result<(), String> {
    let canonical = root
        .canonicalize()
        .map_err(|e| format!("The project folder is unavailable: {e}"))?;
    if git_prefix(&canonical).is_none() {
        return Err("Staging needs the project to be a Git repository".into());
    }
    let mut rels = Vec::with_capacity(paths.len());
    for path in paths {
        let file = panel_entry(root, path.trim_end_matches('/'))?;
        require_workspace_index(&canonical, &file)?;
        let rel = relative(&canonical, &file);
        if rel.is_empty() {
            return Err(outside(path));
        }
        rels.push(rel);
    }
    if rels.is_empty() {
        return Ok(());
    }
    let mut args: Vec<&str> = match (staged, has_head(&canonical)?) {
        (true, _) => vec!["add", "--all", "--"],
        (false, true) => vec!["restore", "--staged", "--"],
        // Before the first commit there is nothing to restore from: drop the index entries.
        (false, false) => vec!["rm", "--cached", "-r", "--quiet", "--ignore-unmatch", "--"],
    };
    args.extend(rels.iter().map(String::as_str));
    let output = git_in(&canonical, &args)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "git {} failed: {}",
            args[0],
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// A review is tied to the index entries (including modes and blob IDs), branch and HEAD,
/// not just the names: another writer can restage different content under the same name.
#[derive(Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct CommitReview {
    pub paths: Vec<String>,
    pub branch: Option<String>,
    pub fingerprint: String,
}

pub fn commit_review(root: &Path) -> Result<CommitReview, String> {
    use sha2::{Digest, Sha256};
    let canonical = root
        .canonicalize()
        .map_err(|e| format!("The project folder is unavailable: {e}"))?;
    let Some(prefix) = git_prefix(&canonical) else {
        return Err("Committing needs the project to be a Git repository".into());
    };
    // Without rename detection both ends of a move are visible to the workspace boundary check.
    let diff = git_in(&canonical, &["diff", "--cached", "--raw", "--no-abbrev", "--no-renames", "--no-relative", "--ignore-submodules=none", "-z", "--"])?;
    if !diff.status.success() {
        return Err(format!("git diff failed: {}", String::from_utf8_lossy(&diff.stderr).trim()));
    }
    let mut fields = diff.stdout.split(|&b| b == 0);
    let mut paths = Vec::new();
    while let Some(entry) = fields.next().filter(|entry| !entry.is_empty()) {
        if entry.ends_with(b" U") { return Err("Resolve merge conflicts before committing".into()); }
        let path = std::str::from_utf8(fields.next().unwrap_or_default())
            .map_err(|_| "A staged path cannot be displayed. Review and commit from a terminal.")?;
        let path = path.strip_prefix(&prefix).filter(|p| !p.is_empty()).ok_or(
            "Files outside this project folder are staged too. Unstage them or commit from a terminal."
        )?;
        paths.push(path.to_owned());
    }
    if paths.is_empty() { return Err("Nothing is staged. Stage files first, then commit.".into()); }
    let head = git_in(&canonical, &["rev-parse", "--verify", "--quiet", "HEAD"])?;
    let mut hash = Sha256::new();
    hash.update(head.stdout);
    hash.update(diff.stdout);
    Ok(CommitReview { paths, branch: current_branch(&canonical), fingerprint: format!("{:x}", hash.finalize()) })
}

/// Lock Git's real index across the recheck and commit. Git and its usual hooks work against
/// our copy; successful hook/index updates are published atomically just as Git publishes them.
pub fn commit(root: &Path, message: &str, reviewed: &CommitReview) -> Result<String, String> {
    let message = message.trim();
    if message.is_empty() { return Err("Write a commit message first".into()); }
    let location = git_in(root, &["rev-parse", "--path-format=absolute", "--git-path", "index"])?;
    if !location.status.success() { return Err("Could not locate the Git index".into()); }
    let location = std::str::from_utf8(&location.stdout).map_err(|_| "Cannot address the Git index")?;
    let index = Path::new(location.trim_end_matches(['\n', '\r']));
    if !index.is_absolute() { return Err("Could not resolve the Git index".into()); }
    let mut lock = index.as_os_str().to_owned();
    lock.push(".lock");
    let lock = PathBuf::from(lock);
    fs::File::create_new(&lock).map_err(|e| format!("Could not lock the Git index; another Git operation may be running: {e}"))?;
    let lock = tempfile::TempPath::try_from_path(lock).map_err(|e| e.to_string())?;
    if commit_review(root)? != *reviewed {
        return Err("The staged changes or branch changed. Review the staged files again.".into());
    }
    fs::copy(index, &lock).map_err(|e| format!("Could not copy the Git index: {e}"))?;
    let mut command = git_command(root, &["commit", "--quiet", "--file=-"]);
    command.env("GIT_INDEX_FILE", lock.as_os_str());
    let output = with_input(&mut command, message.as_bytes().to_vec())
        .map_err(|e| format!("Could not run git: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let why = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };
        return Err(format!("git commit failed: {why}"));
    }
    lock.persist(index).map_err(|e| format!("The commit succeeded but the Git index could not be updated: {e}"))?;
    let sha = git_in(root, &["rev-parse", "--short", "HEAD"])?;
    Ok(String::from_utf8_lossy(&sha.stdout).trim().to_owned())
}

/// Who Git records as the author of a commit made in `root`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct GitIdentity {
    pub name: String,
    pub email: String,
}

/// `git config --get key`; empty when unset (Git exits 1 for that, which is an answer).
fn config_value(root: &Path, key: &str) -> Result<String, String> {
    let output = git_in(root, &["config", "--get", key])?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// One commit of the current branch's history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogEntry {
    pub sha: String,
    pub subject: String,
    pub author: String,
    /// Committer date, ISO 8601.
    pub date: String,
    /// Branch and tag names pointing here, as `git log --decorate` shows them.
    pub refs: String,
}

/// Fields of one `git log` record, separated by the unit separator (never in a subject).
const LOG_FORMAT: &str = "--format=%H%x1f%an%x1f%cI%x1f%D%x1f%s%x1e";

/// Parse records written with [`LOG_FORMAT`].
pub fn parse_log(text: &str) -> Vec<LogEntry> {
    text.split('\u{1e}')
        .filter_map(|record| {
            let mut fields = record.trim_start_matches(['\n', '\r']).splitn(5, '\u{1f}');
            let sha = fields.next()?.trim();
            if sha.len() < 7 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            Some(LogEntry {
                sha: sha.to_owned(),
                author: fields.next()?.to_owned(),
                date: fields.next()?.to_owned(),
                refs: fields.next()?.to_owned(),
                subject: fields.next()?.trim_end().to_owned(),
            })
        })
        .collect()
}

/// The newest `limit` commits reachable from HEAD; empty before the first commit.
pub fn log(root: &Path, limit: usize) -> Result<Vec<LogEntry>, String> {
    if git_prefix(root).is_none() {
        return Err("This project is not a Git repository".into());
    }
    let head = git_in(root, &["rev-parse", "--verify", "--quiet", "HEAD"])?;
    if !head.status.success() {
        return Ok(Vec::new());
    }
    let count = format!("--max-count={}", limit.clamp(1, 500));
    let output = git_in(root, &["log", &count, "--no-color", LOG_FORMAT, "HEAD", "--"])?;
    if !output.status.success() {
        return Err(format!("git log failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(parse_log(&String::from_utf8_lossy(&output.stdout)))
}

/// The configured `user.name` and `user.email`; a part Git has no value for is empty.
pub fn identity(root: &Path) -> Result<GitIdentity, String> {
    Ok(GitIdentity {
        name: config_value(root, "user.name")?,
        email: config_value(root, "user.email")?,
    })
}

/// Save the commit author for this repository, or with `everywhere` for every repository of this
/// user (`git config --global`). Answers with what Git now reports.
pub fn set_identity(root: &Path, name: &str, email: &str, everywhere: bool) -> Result<GitIdentity, String> {
    let (name, email) = (name.trim(), email.trim());
    let plain = |text: &str| {
        !text.is_empty()
            && !text.starts_with('-')
            && !text.chars().any(|c| c.is_control() || matches!(c, '<' | '>'))
    };
    if !plain(name) {
        return Err("Enter the name to record on your commits".into());
    }
    if !plain(email) || !email.contains('@') || email.contains(char::is_whitespace) {
        return Err("Enter an email address such as you@example.com".into());
    }
    let scope = if everywhere { "--global" } else { "--local" };
    for (key, value) in [("user.name", name), ("user.email", email)] {
        let output = git_in(root, &["config", scope, key, value])?;
        if !output.status.success() {
            return Err(format!("git config failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
        }
    }
    identity(root)
}



/// Extensions the system would run rather than show, so "Open in editor" refuses them.
const RUNNABLE: &[&str] = &[
    "app", "command", "tool", "exe", "bat", "cmd", "com", "msi", "ps1", "vbs", "scr", "jar", "pkg",
    "mpkg", "workflow", "scpt", "applescript", "terminal", "desktop", "appimage", "lnk",
];

/// File `path` inside the workspace, checked for opening in the system's default app.
pub fn openable(root: &Path, path: &str) -> Result<PathBuf, String> {
    let file = resolve(root, path)?;
    if !file.is_file() {
        return Err(format!("{path} is not a file"));
    }
    let ext = file
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if RUNNABLE.contains(&ext.as_str()) {
        return Err(format!(
            "{path} is a program or script; Dial does not open it. Use Show in file manager instead."
        ));
    }
    Ok(file)
}

/// Use VS Code's registered file URI. The plain system opener cannot request a line number.
pub fn vscode_line_uri(file: &Path, line: u32) -> Result<String, String> {
    let url = tauri::Url::from_file_path(file).map_err(|_| "Could not encode the project file path")?;
    if line == 0 || url.host().is_some() { return Err("Cannot open this location in VS Code".into()); }
    Ok(format!("vscode://file{}:{line}", url.path()))
}

/// Read file `path` for the viewer: text up to [`MAX_READ`], nothing for binary files.
pub fn read(root: &Path, path: &str) -> Result<FileView, String> {
    let file = resolve(root, path)?;
    if file.is_dir() {
        return Err(format!("{path} is a folder"));
    }
    let size = fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    let mut bytes = Vec::new();
    fs::File::open(&file)
        .and_then(|f| f.take(MAX_READ).read_to_end(&mut bytes))
        .map_err(|e| format!("Cannot read {path}: {e}"))?;
    let truncated = size > MAX_READ;
    let text = decode_text(&bytes, truncated);
    let sha256 = (!truncated).then(|| sha256_hex(&bytes));
    Ok(FileView {
        path: relative(root, &workspace_entry(root, path)?),
        text,
        size,
        truncated,
        sha256,
    })
}

/// `bytes` as text, or `None` when they look binary. A truncated read may end inside a
/// multi-byte character; that tail is dropped.
fn decode_text(bytes: &[u8], truncated: bool) -> Option<String> {
    if bytes.iter().take(8192).any(|&b| b == 0) {
        return None;
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => Some(text.to_owned()),
        Err(e) if truncated && e.error_len().is_none() => {
            Some(String::from_utf8_lossy(&bytes[..e.valid_up_to()]).into_owned())
        }
        Err(_) => None,
    }
}

/// Fuzzy-search workspace files for `@` mentions in the composer, returning up to `limit`
/// `/`-separated paths relative to `root`. Prefers `git ls-files` (respecting `.gitignore`) and
/// falls back to a bounded directory walk. Every returned path is verified to stay inside `root`.
pub fn search(root: &Path, query: &str, limit: usize) -> Result<Vec<String>, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("The project folder is unavailable: {e}"))?;
    let cap = limit.clamp(1, 100);
    let candidates = git_files(&root).unwrap_or_else(|| walk_files(&root));
    let q = query.trim().to_lowercase();

    let mut scored: Vec<((u8, usize, usize), String)> = candidates
        .into_iter()
        .filter_map(|path| score_path(&path, &q).map(|score| (score, path)))
        .collect();
    scored.sort_unstable();

    let mut out = Vec::with_capacity(cap.min(scored.len()));
    for (_, path) in scored {
        if out.len() >= cap {
            break;
        }
        // Skip deleted tracked files and any symlink pointing outside the workspace.
        if let Ok(real) = resolve(&root, &path)
            && real.is_file()
        {
            out.push(path);
        }
    }
    Ok(out)
}

/// Tracked and non-ignored untracked files via `git ls-files`; `None` outside a Git work tree.
fn git_files(root: &Path) -> Option<Vec<String>> {
    let output = crate::launcher::without_console(&mut Command::new("git"))
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            ".",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut out = Vec::new();
    for raw in output.stdout.split(|&b| b == 0) {
        if raw.is_empty() {
            continue;
        }
        let Ok(path) = std::str::from_utf8(raw) else {
            continue;
        };
        let path = path.strip_prefix("./").unwrap_or(path);
        if path.is_empty() || path.starts_with(".git/") || path == ".git" {
            continue;
        }
        out.push(path.to_owned());
    }
    Some(out)
}

/// Bounded breadth-first walk for non-Git folders; skips hidden/build folders and symlinked dirs.
fn walk_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(root.to_path_buf());
    while let Some(dir) = queue.pop_front() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(name) = entry.file_name().into_string() else { continue };
            if HIDDEN.contains(&name.as_str()) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if directory_reparse_point(&meta) { continue; }
            let kind = meta.file_type();
            let path = entry.path();
            if kind.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) {
                    queue.push_back(path);
                }
            } else if kind.is_file() || (kind.is_symlink() && path.is_file()) {
                out.push(relative(root, &path));
                if out.len() >= MAX_WALK_FILES {
                    return out;
                }
            }
        }
    }
    out
}

/// Lower tuples rank higher; `None` means `path` does not match `query` (already lowercased).
fn score_path(path: &str, query: &str) -> Option<(u8, usize, usize)> {
    if query.is_empty() {
        let depth = path.bytes().filter(|&b| b == b'/').count();
        return Some((0, depth, path.len()));
    }
    let lower = path.to_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(lower.as_str());
    if name == query {
        return Some((0, 0, path.len()));
    }
    if name.starts_with(query) {
        return Some((1, 0, path.len()));
    }
    if lower.starts_with(query) {
        return Some((2, 0, path.len()));
    }
    if let Some(pos) = name.find(query) {
        return Some((3, pos, path.len()));
    }
    if let Some(pos) = lower.find(query) {
        return Some((4, pos, path.len()));
    }
    if let Some(gaps) = subsequence_gaps(name, query) {
        return Some((5, gaps, path.len()));
    }
    subsequence_gaps(&lower, query).map(|gaps| (6, gaps, path.len()))
}

/// Total character gaps between matched characters of `needle` inside `haystack`, or `None`.
fn subsequence_gaps(haystack: &str, needle: &str) -> Option<usize> {
    let mut chars = needle.chars();
    let mut want = chars.next()?;
    let mut first: Option<usize> = None;
    let mut matched = 0usize;
    for (idx, ch) in haystack.chars().enumerate() {
        if ch == want {
            if first.is_none() {
                first = Some(idx);
            }
            matched += 1;
            match chars.next() {
                Some(next) => want = next,
                None => {
                    let span = idx - first.unwrap_or(0) + 1;
                    return Some(span.saturating_sub(matched));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;
