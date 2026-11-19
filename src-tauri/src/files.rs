//! File access for the workspace panels: listing folders, reading text files and diffing changed
//! files. Every path is resolved inside the open workspace; nothing here writes.

use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde::Serialize;

/// Larger files are shown truncated.
const MAX_READ: u64 = 512 * 1024;
/// Entries never listed in the file tree.
const HIDDEN: &[&str] = &[".git", ".DS_Store"];

/// One entry of a listed folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntryView {
    pub name: String,
    /// Relative to the workspace, `/`-separated.
    pub path: String,
    pub dir: bool,
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
    /// Git does not track the file yet; the diff shows all of it as added.
    pub untracked: bool,
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
pub fn list(root: &Path, path: &str) -> Result<Vec<DirEntryView>, String> {
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
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
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
        Command::new("git")
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
    let output = if tracked {
        let has_head = git(&["rev-parse", "--verify", "--quiet", "HEAD"])?
            .status
            .success();
        let mut args = vec!["diff", "--no-color", "--no-ext-diff"];
        if has_head {
            args.push("HEAD");
        }
        args.extend(["--", &rel]);
        git(&args)?
    } else {
        // Exit status 1 just means the files differ.
        git(&[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-index",
            "--",
            null_device(),
            &rel,
        ])?
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
        untracked: !tracked,
    })
}

fn null_device() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}

#[cfg(test)]
mod tests;
