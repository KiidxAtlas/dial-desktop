//! Workspace content search (`git grep` with bounded walk fallback).

use super::{GrepMatch, MAX_READ, decode_text, git_prefix, resolve, walk_files};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

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
