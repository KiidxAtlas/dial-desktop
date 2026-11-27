//! Unified diff generation and hunk-level revert/stage/unstage operations.

use std::path::{Path, PathBuf};

use super::{
    DiffBase, DiffScope, DiffView, git_file, git_in, git_with_input, is_worktree_root,
    nested_owner, panel_entry, relative, worktree_marker,
};

/// Whether `rel` currently has unmerged (conflicted) stages in Git's index.
pub(super) fn is_path_conflicted(root: &Path, rel: &str) -> bool {
    git_in(root, &["ls-files", "--unmerged", "-z", "--", rel])
        .is_ok_and(|out| out.status.success() && !out.stdout.is_empty())
}

/// The uncommitted changes of file `path` against Git's `HEAD` (or the index in a repository
/// without commits). Untracked files show as entirely added.
pub fn diff(root: &Path, path: &str) -> Result<DiffView, String> {
    diff_scoped(root, path, DiffScope::All)
}

/// The uncommitted changes of file `path` for a chosen [`DiffScope`] (`All`, `Unstaged` or `Staged`).
pub fn diff_scoped(root: &Path, path: &str, scope: DiffScope) -> Result<DiffView, String> {
    let workspace = root.canonicalize().map_err(|e| e.to_string())?;
    let entry = panel_entry(root, path)?;
    let nested = nested_owner(&workspace, &entry);
    let root = nested.as_deref().unwrap_or(&workspace);
    if nested.is_some() && !(worktree_marker(root) && is_worktree_root(root)) {
        return Err("Cannot identify this nested Git worktree".into());
    }
    let file = git_file(root, &relative(root, &entry))?;
    let conflicted = is_path_conflicted(root, &file.rel);
    let base_args = [
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--no-renames",
        "--no-relative",
        "--src-prefix=a/",
        "--dst-prefix=b/",
    ];
    let (output, base) = match scope {
        DiffScope::All => {
            if file.in_head || file.in_index {
                let mut args = base_args.to_vec();
                if file.has_head {
                    args.push("HEAD");
                }
                args.extend(["--", &file.rel]);
                (
                    git_in(root, &args)?,
                    if file.has_head { DiffBase::Head } else { DiffBase::Index },
                )
            } else {
                if file.entry.is_dir() {
                    return Err(format!("Cannot diff {path} as an untracked directory link"));
                }
                (
                    git_in(
                        root,
                        &[
                            "diff",
                            "--no-color",
                            "--no-ext-diff",
                            "--no-textconv",
                            "--no-index",
                            "--",
                            null_device(),
                            &file.rel,
                        ],
                    )?,
                    DiffBase::Untracked,
                )
            }
        }
        DiffScope::Unstaged => {
            if file.in_index {
                let mut args = base_args.to_vec();
                args.extend(["--", &file.rel]);
                (git_in(root, &args)?, DiffBase::Index)
            } else if !file.in_head {
                if file.entry.is_dir() {
                    return Err(format!("Cannot diff {path} as an untracked directory link"));
                }
                (
                    git_in(
                        root,
                        &[
                            "diff",
                            "--no-color",
                            "--no-ext-diff",
                            "--no-textconv",
                            "--no-index",
                            "--",
                            null_device(),
                            &file.rel,
                        ],
                    )?,
                    DiffBase::Untracked,
                )
            } else {
                return Ok(DiffView {
                    path: relative(&workspace, &entry),
                    nested: nested.is_some(),
                    delete_on_revert: !file.in_head,
                    conflicted,
                    diff: String::new(),
                    base: DiffBase::Index,
                });
            }
        }
        DiffScope::Staged => {
            if file.in_head || file.in_index {
                let mut args = base_args.to_vec();
                args.push("--cached");
                args.extend(["--", &file.rel]);
                (
                    git_in(root, &args)?,
                    if file.has_head { DiffBase::Head } else { DiffBase::Index },
                )
            } else {
                return Ok(DiffView {
                    path: relative(&workspace, &entry),
                    nested: nested.is_some(),
                    delete_on_revert: !file.in_head,
                    conflicted,
                    diff: String::new(),
                    base: DiffBase::Head,
                });
            }
        }
    };
    let differs =
        base == DiffBase::Untracked && output.status.code() == Some(1) && !output.stdout.is_empty();
    if !output.status.success() && !differs {
        return Err(format!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(DiffView {
        path: relative(&workspace, &entry),
        nested: nested.is_some(),
        delete_on_revert: !file.in_head,
        conflicted,
        diff: String::from_utf8_lossy(&output.stdout).into_owned(),
        base,
    })
}

/// Undo one hunk of file `path`'s uncommitted changes in the working tree. `hunk` is the hunk as
/// the diff panel showed it (its `@@` header and lines); it must still be in the file's current
/// diff, so a stale or edited hunk is refused rather than guessed at. The UI confirms first.
pub fn revert_hunk(root: &Path, path: &str, hunk: &str) -> Result<(), String> {
    let shown = diff(root, path)?;
    if shown.nested {
        return Err("Open the nested repository as a project to change it".into());
    }
    if shown.base == DiffBase::Untracked {
        return Err(format!("{path} is a new file; revert the whole file instead"));
    }
    let (head, hunks) = split_hunks(&shown.diff);
    let wanted = hunk.trim_end_matches('\n');
    let Some(found) = hunks.into_iter().find(|h| h.trim_end_matches('\n') == wanted) else {
        return Err("That change is no longer in the file. Refresh the diff and try again.".into());
    };
    let top = git_in(root, &["rev-parse", "--show-toplevel"])?;
    let top = std::str::from_utf8(&top.stdout).map_err(|_| "Cannot address the Git work tree")?;
    let top = PathBuf::from(top.trim_end_matches(['\n', '\r']));
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

/// Stage one hunk of file `path`'s uncommitted changes into Git's index. `hunk` is the hunk as
/// shown in the diff panel (`@@` header and lines); stale hunks and nested repos are refused.
pub fn stage_hunk(root: &Path, path: &str, hunk: &str) -> Result<(), String> {
    let shown = diff(root, path)?;
    if shown.nested {
        return Err("Open the nested repository as a project to change it".into());
    }
    if shown.base == DiffBase::Untracked {
        return Err(format!("{path} is a new file; stage the whole file instead"));
    }
    let wanted = hunk.trim_end_matches('\n');
    let unstaged = git_in(
        root,
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--",
            &shown.path,
        ],
    )?;
    if !unstaged.status.success() {
        return Err(format!(
            "git diff failed: {}",
            String::from_utf8_lossy(&unstaged.stderr).trim()
        ));
    }
    let unstaged_text = String::from_utf8_lossy(&unstaged.stdout);
    let (head, unstaged_hunks) = split_hunks(&unstaged_text);
    let (_, shown_hunks) = split_hunks(&shown.diff);
    let seen = shown_hunks
        .iter()
        .chain(unstaged_hunks.iter())
        .any(|h| h.trim_end_matches('\n') == wanted);
    if !seen {
        return Err("That change is no longer in the file. Refresh the diff and try again.".into());
    }
    fn hunk_body(text: &str) -> &str {
        text.split_once('\n').map_or("", |(_, rest)| rest.trim_end_matches('\n'))
    }
    let wanted_body = hunk_body(wanted);
    let chosen = unstaged_hunks
        .iter()
        .copied()
        .find(|h| h.trim_end_matches('\n') == wanted)
        .or_else(|| {
            let mut matches = unstaged_hunks
                .iter()
                .copied()
                .filter(|h| !wanted_body.is_empty() && hunk_body(h) == wanted_body);
            let first = matches.next()?;
            matches.next().is_none().then_some(first)
        });
    let Some(found) = chosen else {
        return Err("That change is already staged.".into());
    };
    let top = git_in(root, &["rev-parse", "--show-toplevel"])?;
    let top = std::str::from_utf8(&top.stdout).map_err(|_| "Cannot address the Git work tree")?;
    let top = PathBuf::from(top.trim_end_matches(['\n', '\r']));
    let mut patch = format!("{head}{found}");
    if !patch.ends_with('\n') {
        patch.push('\n');
    }
    let output = git_with_input(
        &top,
        &["apply", "--cached", "--whitespace=nowarn", "-"],
        patch.into_bytes(),
    )
    .map_err(|e| format!("Could not run git: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "The change could not be staged cleanly: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// Unstage one hunk of file `path`'s staged changes from Git's index while keeping the working tree.
pub fn unstage_hunk(root: &Path, path: &str, hunk: &str) -> Result<(), String> {
    let staged = diff_scoped(root, path, DiffScope::Staged)?;
    if staged.nested {
        return Err("Open the nested repository as a project to change it".into());
    }
    let wanted = hunk.trim_end_matches('\n');
    let (head, staged_hunks) = split_hunks(&staged.diff);
    let Some(found) = staged_hunks
        .iter()
        .copied()
        .find(|h| h.trim_end_matches('\n') == wanted)
    else {
        return Err("That change is no longer staged. Refresh the diff and try again.".into());
    };
    let top = git_in(root, &["rev-parse", "--show-toplevel"])?;
    let top = std::str::from_utf8(&top.stdout).map_err(|_| "Cannot address the Git work tree")?;
    let top = PathBuf::from(top.trim_end_matches(['\n', '\r']));
    let mut patch = format!("{head}{found}");
    if !patch.ends_with('\n') {
        patch.push('\n');
    }
    let output = git_with_input(
        &top,
        &["apply", "--cached", "--reverse", "--whitespace=nowarn", "-"],
        patch.into_bytes(),
    )
    .map_err(|e| format!("Could not run git: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "The change could not be unstaged cleanly: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// A one-file unified diff split into its header (everything before the first `@@`) and hunks,
/// each with its trailing newline.
pub(super) fn split_hunks(diff: &str) -> (&str, Vec<&str>) {
    let Some(first) = diff
        .find("\n@@ ")
        .map(|i| i + 1)
        .or_else(|| diff.starts_with("@@ ").then_some(0))
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

fn null_device() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}
