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
    /// `false` outside a Git work tree (or when Git is not installed); `files` is then empty.
    pub repo: bool,
    pub files: Vec<GitFileStatus>,
    /// The checked-out branch, `detached at <commit>` without one; `None` outside Git.
    pub branch: Option<String>,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffBase {
    Head,
    Index,
    Untracked,
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

/// `path` relative to `root`, `/`-separated.
fn relative(root: &Path, path: &Path) -> String {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let rel = path.strip_prefix(&root).unwrap_or(path);
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// The entries of folder `path` (empty for the project root): folders first, then by name.
/// Entries `.gitignore` matches are left out.
pub fn list(root: &Path, path: &str) -> Result<Vec<DirEntryView>, String> {
    list_with(root, path, false)
}

/// Like [`list`]; with `show_ignored`, ignored entries are kept and marked instead.
pub fn list_with(root: &Path, path: &str, show_ignored: bool) -> Result<Vec<DirEntryView>, String> {
    let dir = resolve(root, if path.is_empty() { "." } else { path })?;
    let entries = fs::read_dir(&dir).map_err(|e| format!("Cannot list {path}: {e}"))?;
    let mut out: Vec<DirEntryView> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if HIDDEN.contains(&name.as_str()) {
                return None;
            }
            // Follows symlinks, so a linked folder expands like a folder.
            let is_dir = entry.path().is_dir();
            Some(DirEntryView {
                path: relative(root, &dir.join(&name)),
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
    crate::launcher::without_console(&mut cmd).arg("-C").arg(root).args(args);
    cmd
}

/// The workspace's path inside its Git repository (`""` at the top, else ending in `/`);
/// `None` outside a work tree or without Git.
fn git_prefix(root: &Path) -> Option<String> {
    let out = git_in(root, &["rev-parse", "--show-prefix"]).ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .trim_end_matches(['\n', '\r'])
            .to_owned()
    })
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
        && git_in(&parent, &["check-ignore", "-q", "--", &format!("{name}/")])
            .is_ok_and(|o| o.status.success())
    {
        return out;
    }
    // Folders get a trailing `/` so folder-only patterns such as `build/` match them.
    let mut input = Vec::new();
    for entry in entries {
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
        out.insert(path.trim_end_matches('/').to_owned());
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

/// The workspace's changed paths from one `git status`; with `counts`, also each file's added and
/// removed lines (`git diff --numstat` against `HEAD`, line counts for untracked files).
pub fn status(root: &Path, counts: bool) -> Result<GitStatusView, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("The project folder is unavailable: {e}"))?;
    let Some(prefix) = git_prefix(&root) else {
        return Ok(GitStatusView { repo: false, files: Vec::new(), branch: None });
    };
    let output = git_in(
        &root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=normal", "--", "."],
    )?;
    if !output.status.success() {
        return Err(format!(
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut files = parse_status(&output.stdout, &prefix);
    if counts {
        let has_head = git_in(&root, &["rev-parse", "--verify", "--quiet", "HEAD"])?
            .status
            .success();
        let stats = if has_head {
            git_in(&root, &["diff", "--numstat", "-z", "--relative", "--no-color", "HEAD", "--"])
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
                file.added = untracked_lines(&root, &file.path);
                file.removed = file.added.map(|_| 0);
            }
        }
    }
    Ok(GitStatusView { repo: true, files, branch: current_branch(&root) })
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
        let path = String::from_utf8_lossy(&field[3..]);
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
        let text = String::from_utf8_lossy(field);
        let mut parts = text.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        // A rename has an empty path here, then its old and new paths as separate fields.
        let path = if path.is_empty() {
            fields.next();
            match fields.next() {
                Some(new) => String::from_utf8_lossy(new).into_owned(),
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

/// `path` inside `root`, which need not exist (a deleted file): the nearest existing ancestor is
/// resolved like [`resolve`] and the missing tail, plain names only, is appended.
fn resolve_missing(root: &Path, path: &str) -> Result<PathBuf, String> {
    if let Ok(found) = resolve(root, path) {
        return Ok(found);
    }
    let canonical = root
        .canonicalize()
        .map_err(|e| format!("The project folder is unavailable: {e}"))?;
    let given = Path::new(path);
    let relative = if given.is_absolute() {
        given.strip_prefix(&canonical).or_else(|_| given.strip_prefix(root)).map_err(|_| outside(path))?
    } else {
        given
    };
    if relative.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(outside(path));
    }
    let mut base = relative.to_path_buf();
    let mut tail = Vec::new();
    while !canonical.join(&base).exists() {
        let Some(name) = base.file_name() else { break };
        tail.push(name.to_owned());
        base.pop();
    }
    let mut real = if base.as_os_str().is_empty() {
        canonical.clone()
    } else {
        resolve(&canonical, &base.to_string_lossy())?
    };
    if !real.starts_with(&canonical) {
        return Err(outside(path));
    }
    real.extend(tail.iter().rev());
    Ok(real)
}

/// Discard every uncommitted change to file `path`: a tracked file goes back to `HEAD` in both the
/// working tree and the index; a file added since `HEAD` (or untracked) is unstaged and deleted.
/// This also drops the user's own edits to that file; the UI confirms before calling it.
pub fn revert(root: &Path, path: &str) -> Result<(), String> {
    let file = resolve_missing(root, path)?;
    if file.is_dir() {
        return Err(format!("{path} is a folder"));
    }
    let canonical = root
        .canonicalize()
        .map_err(|e| format!("The project folder is unavailable: {e}"))?;
    let rel = relative(&canonical, &file);
    if rel.is_empty() {
        return Err(outside(path));
    }
    if git_prefix(&canonical).is_none() {
        return Err("Reverting needs the project to be a Git repository".into());
    }
    let in_head = git_in(&canonical, &["cat-file", "-e", &format!("HEAD:./{rel}")])?
        .status
        .success();
    let failed = |what: &str, output: Output| {
        format!("{what} failed: {}", String::from_utf8_lossy(&output.stderr).trim())
    };
    if in_head {
        let output = git_in(
            &canonical,
            &["restore", "--source=HEAD", "--worktree", "--staged", "--", &rel],
        )?;
        return if output.status.success() { Ok(()) } else { Err(failed("git restore", output)) };
    }
    let staged = git_in(&canonical, &["ls-files", "--error-unmatch", "--", &rel])?
        .status
        .success();
    if staged {
        let output = git_in(&canonical, &["rm", "--cached", "--quiet", "--force", "--", &rel])?;
        if !output.status.success() {
            return Err(failed("git rm", output));
        }
    }
    match fs::remove_file(&file) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("Cannot delete {path}: {e}")),
        _ => Ok(()),
    }
}

/// Undo one hunk of file `path`'s uncommitted changes in the working tree. `hunk` is the hunk as
/// the diff panel showed it (its `@@` header and lines); it must still be in the file's current
/// diff, so a stale or edited hunk is refused rather than guessed at. The UI confirms first.
pub fn revert_hunk(root: &Path, path: &str, hunk: &str) -> Result<(), String> {
    let canonical = root
        .canonicalize()
        .map_err(|e| format!("The project folder is unavailable: {e}"))?;
    let file = resolve(&canonical, path)?;
    if file.is_dir() {
        return Err(format!("{path} is a folder"));
    }
    let rel = relative(&canonical, &file);
    if git_prefix(&canonical).is_none() {
        return Err("Reverting needs the project to be a Git repository".into());
    }
    let tracked = git_in(&canonical, &["ls-files", "--error-unmatch", "--", &rel])?
        .status
        .success();
    if !tracked {
        return Err(format!("{path} is a new file; revert the whole file instead"));
    }
    let has_head = git_in(&canonical, &["rev-parse", "--verify", "--quiet", "HEAD"])?
        .status
        .success();
    // Fixed prefixes and repository-relative paths, whatever the user's diff settings, so the
    // patch applies from the top of the work tree.
    let mut args = vec![
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--no-relative",
        "--src-prefix=a/",
        "--dst-prefix=b/",
    ];
    if has_head {
        args.push("HEAD");
    }
    args.extend(["--", &rel]);
    let output = git_in(&canonical, &args)?;
    if !output.status.success() {
        return Err(format!("git diff failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let (head, hunks) = split_hunks(&text);
    let wanted = hunk.trim_end_matches('\n');
    let Some(found) = hunks.into_iter().find(|h| h.trim_end_matches('\n') == wanted) else {
        return Err("That change is no longer in the file. Refresh the diff and try again.".into());
    };
    let top = git_in(&canonical, &["rev-parse", "--show-toplevel"])?;
    let top = PathBuf::from(String::from_utf8_lossy(&top.stdout).trim_end_matches(['\n', '\r']));
    let mut patch = format!("{head}{found}");
    if !patch.ends_with('\n') {
        patch.push('\n');
    }
    let output = git_with_input(&top, &["apply", "-R", "--whitespace=nowarn", "-"], patch.into_bytes())
        .map_err(|e| format!("Could not run git: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "The change could not be undone cleanly: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// A one-file unified diff split into its header (everything before the first `@@`) and hunks,
/// each with its trailing newline.
fn split_hunks(diff: &str) -> (&str, Vec<&str>) {
    let Some(first) = diff.find("\n@@ ").map(|i| i + 1).or_else(|| diff.starts_with("@@ ").then_some(0))
    else {
        return (diff, Vec::new());
    };
    let mut hunks = Vec::new();
    let mut start = first;
    while let Some(next) = diff[start + 1..].find("\n@@ ") {
        let end = start + 1 + next + 1;
        hunks.push(&diff[start..end]);
        start = end;
    }
    hunks.push(&diff[start..]);
    (&diff[..first], hunks)
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
        let file = resolve_missing(&canonical, path.trim_end_matches('/'))?;
        let rel = relative(&canonical, &file);
        if rel.is_empty() {
            return Err(outside(path));
        }
        rels.push(rel);
    }
    if rels.is_empty() {
        return Ok(());
    }
    let has_head = git_in(&canonical, &["rev-parse", "--verify", "--quiet", "HEAD"])?
        .status
        .success();
    let mut args: Vec<&str> = match (staged, has_head) {
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

/// Longest line text a content search returns.
const GREP_TEXT: usize = 200;
/// Most matching lines a content search returns from one file.
const GREP_PER_FILE: usize = 20;
/// Bytes the non-Git content search reads at most, across all files.
const GREP_WALK_BYTES: u64 = 64 * 1024 * 1024;

/// Lines containing `query` (case-insensitive, literal), up to `limit`: `git grep` over tracked and
/// non-ignored files in a repository, else a bounded walk that skips build folders and reads text
/// files up to [`MAX_READ`].
pub fn grep(root: &Path, query: &str, limit: usize) -> Result<Vec<GrepMatch>, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("The project folder is unavailable: {e}"))?;
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let cap = limit.clamp(1, 500);
    if git_prefix(&root).is_some()
        && let Some(found) = git_grep(&root, query, cap)
    {
        return Ok(found);
    }
    Ok(walk_grep(&root, query, cap))
}

fn grep_match(path: String, line: u64, text: &str) -> GrepMatch {
    let text = text.trim_end_matches('\r').trim_start();
    let text = match text.char_indices().nth(GREP_TEXT) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_owned(),
    };
    GrepMatch { path, line, text }
}

/// `git grep`, read as it streams and stopped at `cap`; `None` when Git fails.
fn git_grep(root: &Path, query: &str, cap: usize) -> Option<Vec<GrepMatch>> {
    use std::io::BufRead;
    let mut child = crate::launcher::without_console(&mut Command::new("git"))
        .arg("-C")
        .arg(root)
        // Paths relative to the workspace and no column field, whatever the user's settings.
        .args(["-c", "grep.fullName=false", "-c", "grep.column=false"])
        .args(["grep", "-n", "-I", "-i", "-F", "--untracked", "--no-color", "-z", "-e", query, "--", "."])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let reader = std::io::BufReader::new(child.stdout.take()?);
    let mut out = Vec::new();
    let mut per_file: HashMap<String, usize> = HashMap::new();
    for line in reader.split(b'\n') {
        let Ok(line) = line else { break };
        let mut parts = line.splitn(3, |&b| b == 0);
        let (Some(path), Some(number), Some(text)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let (Ok(path), Some(number)) = (
            std::str::from_utf8(path),
            std::str::from_utf8(number).ok().and_then(|n| n.parse().ok()),
        ) else {
            continue;
        };
        let seen = per_file.entry(path.to_owned()).or_default();
        if *seen >= GREP_PER_FILE || resolve(root, path).is_err() {
            continue;
        }
        *seen += 1;
        out.push(grep_match(path.to_owned(), number, &String::from_utf8_lossy(text)));
        if out.len() >= cap {
            break;
        }
    }
    let _ = child.kill();
    let status = child.wait().ok()?;
    // 1 means no match; anything else without results is a failure the walk may cover.
    if out.is_empty() && !matches!(status.code(), Some(0 | 1)) {
        return None;
    }
    Some(out)
}

/// The content search for folders outside Git.
fn walk_grep(root: &Path, query: &str, cap: usize) -> Vec<GrepMatch> {
    let needle = query.to_lowercase();
    let mut out = Vec::new();
    let mut budget = GREP_WALK_BYTES;
    for path in walk_files(root) {
        let Ok(file) = resolve(root, &path) else { continue };
        let mut bytes = Vec::new();
        let Ok(read) = fs::File::open(&file).and_then(|f| f.take(MAX_READ.min(budget)).read_to_end(&mut bytes)) else {
            continue;
        };
        budget = budget.saturating_sub(read as u64);
        if let Some(text) = decode_text(&bytes, true) {
            let mut found = 0;
            for (i, line) in text.lines().enumerate() {
                if line.to_lowercase().contains(&needle) {
                    out.push(grep_match(path.clone(), i as u64 + 1, line));
                    found += 1;
                    if out.len() >= cap {
                        return out;
                    }
                    if found >= GREP_PER_FILE {
                        break;
                    }
                }
            }
        }
        if budget == 0 {
            break;
        }
    }
    out
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
    Ok(FileView {
        path: relative(root, &file),
        text,
        size,
        truncated,
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

/// The uncommitted changes of file `path` against Git's `HEAD` (or the index in a repository
/// without commits). Untracked files show as entirely added.
pub fn diff(root: &Path, path: &str) -> Result<DiffView, String> {
    let file = resolve(root, path)?;
    if file.is_dir() {
        return Err(format!("{path} is a folder"));
    }
    let rel = relative(root, &file);
    let git = |args: &[&str]| {
        crate::launcher::without_console(&mut Command::new("git"))
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .map_err(|e| format!("Could not run git: {e}"))
    };
    let inside = git(&["rev-parse", "--is-inside-work-tree"])?;
    if !inside.status.success() {
        return Err("Diffs need the project to be a Git repository".into());
    }
    let tracked = git(&["ls-files", "--error-unmatch", "--", &rel])?
        .status
        .success();
    let (output, base) = if tracked {
        let has_head = git(&["rev-parse", "--verify", "--quiet", "HEAD"])?
            .status
            .success();
        let mut args = vec!["diff", "--no-color", "--no-ext-diff"];
        if has_head {
            args.push("HEAD");
        }
        args.extend(["--", &rel]);
        (
            git(&args)?,
            if has_head {
                DiffBase::Head
            } else {
                DiffBase::Index
            },
        )
    } else {
        // Exit status 1 just means the files differ.
        (
            git(&[
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--no-index",
                "--",
                null_device(),
                &rel,
            ])?,
            DiffBase::Untracked,
        )
    };
    if !output.status.success() && output.status.code() != Some(1) {
        return Err(format!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(DiffView {
        path: rel,
        diff: String::from_utf8_lossy(&output.stdout).into_owned(),
        base,
    })
}

fn null_device() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
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
            let name = entry.file_name().to_string_lossy().into_owned();
            if HIDDEN.contains(&name.as_str()) {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
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
