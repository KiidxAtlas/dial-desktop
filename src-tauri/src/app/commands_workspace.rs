//! Workspace file, diff, search, and Git Tauri commands extracted from `app.rs`.

use super::{
    Shared, engine_program, main_only, panel_root, rpc_method_unsupported, rpc_reply, workspace_rpc,
};
use crate::files::{self, DiffView, DirEntryView, FileView};
use std::time::Duration;
use tauri::State;

/// The entries of folder `path` in the workspace (`""` for its root).
#[tauri::command]
pub(super) async fn list_dir(
    shared: State<'_, Shared>,
    path: String,
    workspace_root: String,
    show_ignored: Option<bool>,
) -> Result<Vec<DirEntryView>, String> {
    let root = panel_root(&shared, &workspace_root)?;
    let show_ignored = show_ignored.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || files::list_with(&root, &path, show_ignored))
        .await
        .map_err(|e| e.to_string())?
}

/// File `path` in the workspace, for the viewer.
#[tauri::command]
pub(super) async fn read_file(
    shared: State<'_, Shared>,
    path: String,
    workspace_root: String,
) -> Result<FileView, String> {
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::read(&root, &path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(super) async fn write_file(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    path: String,
    content: String,
    expected_sha256: Option<String>,
    workspace_root: String,
) -> Result<FileView, String> {
    if webview.label() != "main" {
        return Err("Editing files is only available in the app UI".into());
    }
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || {
        files::write(&root, &path, &content, expected_sha256.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(super) async fn file_create(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    path: String,
    content: Option<String>,
    workspace_root: String,
) -> Result<FileView, String> {
    if webview.label() != "main" {
        return Err("Creating files is only available in the app UI".into());
    }
    let root = panel_root(&shared, &workspace_root)?;
    let body = content.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || files::create_file(&root, &path, &body))
        .await
        .map_err(|e| e.to_string())?
}

/// Rename or move a workspace file or folder. App UI only.
#[tauri::command]
pub(super) async fn file_rename(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    from: String,
    to: String,
    workspace_root: String,
) -> Result<String, String> {
    if webview.label() != "main" {
        return Err("Renaming files is only available in the app UI".into());
    }
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::rename_file(&root, &from, &to))
        .await
        .map_err(|e| e.to_string())?
}

/// Delete a plain file or empty folder inside the workspace. App UI only.
#[tauri::command]
pub(super) async fn file_delete(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    path: String,
    workspace_root: String,
) -> Result<(), String> {
    if webview.label() != "main" {
        return Err("Deleting files is only available in the app UI".into());
    }
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::delete_file(&root, &path))
        .await
        .map_err(|e| e.to_string())?
}

/// The uncommitted changes of file `path` in the workspace, optionally scoped to `unstaged` or `staged`.
#[tauri::command]
pub(super) async fn file_diff(
    shared: State<'_, Shared>,
    path: String,
    workspace_root: String,
    scope: Option<files::DiffScope>,
) -> Result<DiffView, String> {
    let root = panel_root(&shared, &workspace_root)?;
    let scope = scope.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || files::diff_scoped(&root, &path, scope))
        .await
        .map_err(|e| e.to_string())?
}

/// The workspace's changed paths from `git status`; with `counts`, each file's +/- line counts.
#[tauri::command]
pub(super) async fn file_git_status(
    shared: State<'_, Shared>,
    workspace_root: String,
    counts: Option<bool>,
) -> Result<files::GitStatusView, String> {
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::status(&root, counts.unwrap_or(false)))
        .await
        .map_err(|e| e.to_string())?
}

/// Discard every uncommitted change to file `path` (the UI confirms first). App UI only.
#[tauri::command]
pub(super) async fn file_revert(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    path: String,
    workspace_root: String,
) -> Result<(), String> {
    if webview.label() != "main" {
        return Err("Reverting files is only available in the app UI".into());
    }
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::revert(&root, &path))
        .await
        .map_err(|e| e.to_string())?
}

/// Undo one hunk of file `path`'s uncommitted changes (the UI confirms first). App UI only.
#[tauri::command]
pub(super) async fn file_revert_hunk(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    path: String,
    hunk: String,
    workspace_root: String,
) -> Result<(), String> {
    if webview.label() != "main" {
        return Err("Reverting changes is only available in the app UI".into());
    }
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::revert_hunk(&root, &path, &hunk))
        .await
        .map_err(|e| e.to_string())?
}

/// Stage one hunk of file `path`'s uncommitted changes into Git's index. App UI only.
#[tauri::command]
pub(super) async fn file_stage_hunk(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    path: String,
    hunk: String,
    workspace_root: String,
) -> Result<(), String> {
    if webview.label() != "main" {
        return Err("Staging changes is only available in the app UI".into());
    }
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::stage_hunk(&root, &path, &hunk))
        .await
        .map_err(|e| e.to_string())?
}

/// Unstage one hunk of file `path`'s staged changes from Git's index. App UI only.
#[tauri::command]
pub(super) async fn file_unstage_hunk(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    path: String,
    hunk: String,
    workspace_root: String,
) -> Result<(), String> {
    if webview.label() != "main" {
        return Err("Unstaging changes is only available in the app UI".into());
    }
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::unstage_hunk(&root, &path, &hunk))
        .await
        .map_err(|e| e.to_string())?
}

/// Local branches and upstream sync state for the workspace repository.
#[tauri::command]
pub(super) async fn file_git_branches(
    shared: State<'_, Shared>,
    workspace_root: String,
) -> Result<files::GitBranchInfo, String> {
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::git_branches(&root))
        .await
        .map_err(|e| e.to_string())?
}

/// Switch to an existing local branch or create a new one. App UI only.
#[tauri::command]
pub(super) async fn file_git_checkout(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    branch: String,
    create: bool,
    workspace_root: String,
) -> Result<String, String> {
    if webview.label() != "main" {
        return Err("Switching branches is only available in the app UI".into());
    }
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::git_checkout(&root, &branch, create))
        .await
        .map_err(|e| e.to_string())?
}

/// Run `fetch`, `pull` (`--ff-only`) or `push` for the workspace repository. App UI only.
#[tauri::command]
pub(super) async fn file_git_sync(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    action: String,
    workspace_root: String,
) -> Result<String, String> {
    if webview.label() != "main" {
        return Err("Git sync is only available in the app UI".into());
    }
    let root = panel_root(&shared, &workspace_root)?;
    let guard = shared.git_syncs.begin(&root);
    tauri::async_runtime::spawn_blocking(move || {
        let _keep = &guard;
        files::git_sync_cancellable(&root, &action, guard.flag())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Cancel an in-flight `file_git_sync` for the workspace repository. App UI only.
#[tauri::command]
pub(super) fn file_git_sync_cancel(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    workspace_root: String,
) -> Result<bool, String> {
    if webview.label() != "main" {
        return Err("Git sync is only available in the app UI".into());
    }
    let root = panel_root(&shared, &workspace_root)?;
    Ok(shared.git_syncs.cancel(&root))
}

/// List, push or pop entries in Git's stash. App UI only for mutations.
#[tauri::command]
pub(super) async fn file_git_stash(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    action: String,
    message: Option<String>,
    workspace_root: String,
) -> Result<Vec<files::GitStashEntry>, String> {
    if action != "list" && webview.label() != "main" {
        return Err("Stashing changes is only available in the app UI".into());
    }
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::git_stash(&root, &action, message.as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

/// Resolve an unmerged Git conflict in `path` using `"ours"`, `"theirs"`, or `"resolved"`. App UI only.
#[tauri::command]
pub(super) async fn file_git_resolve_conflict(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    path: String,
    strategy: String,
    workspace_root: String,
) -> Result<(), String> {
    main_only(&webview, "resolve Git conflicts")?;
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::git_resolve_conflict(&root, &path, &strategy))
        .await
        .map_err(|e| e.to_string())?
}

/// Stage (`staged`) or unstage project files `paths` in Git's index. App UI only.
#[tauri::command]
pub(super) async fn file_stage(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    paths: Vec<String>,
    staged: bool,
    workspace_root: String,
) -> Result<(), String> {
    if webview.label() != "main" {
        return Err("Staging files is only available in the app UI".into());
    }
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::stage(&root, &paths, staged))
        .await
        .map_err(|e| e.to_string())?
}

/// Read the exact staged paths for a user-confirmed commit, independently of session changes.
#[tauri::command]
pub(super) async fn file_commit_review(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    workspace_root: String,
) -> Result<files::CommitReview, String> {
    if webview.label() != "main" {
        return Err("Commit review is only available in the app UI".into());
    }
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::commit_review(&root))
        .await
        .map_err(|e| e.to_string())?
}

/// The newest commits of the current branch. App UI only.
#[tauri::command]
pub(super) async fn file_git_log(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    workspace_root: String,
    limit: Option<usize>,
) -> Result<Vec<files::LogEntry>, String> {
    main_only(&webview, "read the Git history")?;
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::log(&root, limit.unwrap_or(100)))
        .await
        .map_err(|e| e.to_string())?
}

/// `dial review` over the project's changes, read-only; its report. App UI only.
#[tauri::command]
pub(super) async fn review_changes(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    workspace_root: String,
    target: crate::engine_cli::ReviewTarget,
    focus: Option<String>,
) -> Result<String, String> {
    main_only(&webview, "review changes")?;
    let root = panel_root(&shared, &workspace_root)?;
    let program = engine_program(&shared)?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::engine_cli::review(&program, &root, &target, focus.as_deref().unwrap_or(""))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The engine's AI commit message for the staged changes (`commit_preview` RPC, falling back to
/// `dial commit --dry-run`). App UI only.
#[tauri::command]
pub(super) async fn file_commit_message(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    workspace_root: String,
) -> Result<crate::engine_cli::CommitPreview, String> {
    main_only(&webview, "write a commit message")?;
    let root = panel_root(&shared, &workspace_root)?;
    if let Ok(reply) = workspace_rpc(
        &shared,
        &workspace_root,
        "commit_preview",
        serde_json::json!({}),
    ) {
        match rpc_reply(reply, Some(Duration::from_secs(5 * 60))).await {
            Ok(value) => {
                if let Ok(parsed) = crate::engine_cli::parse_commit_preview_json(&value.to_string()) {
                    return parsed;
                }
            }
            Err(error) if rpc_method_unsupported(&error) => {}
            Err(error) => return Err(error),
        }
    }
    let program = engine_program(&shared)?;
    tauri::async_runtime::spawn_blocking(move || crate::engine_cli::commit_message(&program, &root))
        .await
        .map_err(|e| e.to_string())?
}

/// Commit the staged changes with `message`; the new commit's short hash. App UI only.
#[tauri::command]
pub(super) async fn file_commit(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    message: String,
    review: files::CommitReview,
    workspace_root: String,
) -> Result<String, String> {
    if webview.label() != "main" {
        return Err("Committing is only available in the app UI".into());
    }
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::commit(&root, &message, &review))
        .await
        .map_err(|e| e.to_string())?
}

/// The commit author Git has for this project; a part it has no value for is empty.
#[tauri::command]
pub(super) async fn file_git_identity(
    shared: State<'_, Shared>,
    workspace_root: String,
) -> Result<files::GitIdentity, String> {
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || files::identity(&root))
        .await
        .map_err(|e| e.to_string())?
}

/// Save the commit author for this project, or with `everywhere` for all of the user's
/// repositories. App UI only.
#[tauri::command]
pub(super) async fn file_git_identity_set(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    name: String,
    email: String,
    everywhere: bool,
    workspace_root: String,
) -> Result<files::GitIdentity, String> {
    main_only(&webview, "change the Git author")?;
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || {
        files::set_identity(&root, &name, &email, everywhere)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Lines of project files containing `query` (case-insensitive), for the Files panel.
#[tauri::command]
pub(super) async fn file_grep(
    shared: State<'_, Shared>,
    query: String,
    workspace_root: String,
    limit: Option<usize>,
) -> Result<Vec<files::GrepMatch>, String> {
    let root = panel_root(&shared, &workspace_root)?;
    let cap = limit.unwrap_or(200);
    tauri::async_runtime::spawn_blocking(move || files::grep(&root, &query, cap))
        .await
        .map_err(|e| e.to_string())?
}

/// Open a checked project file in its default app, or at `line` via the VS Code URI handler.
#[tauri::command]
pub(super) async fn file_open(
    webview: tauri::Webview,
    shared: State<'_, Shared>,
    path: String,
    line: Option<u32>,
    workspace_root: String,
) -> Result<(), String> {
    if webview.label() != "main" {
        return Err("Opening files is only available in the app UI".into());
    }
    let root = panel_root(&shared, &workspace_root)?;
    tauri::async_runtime::spawn_blocking(move || {
        let file = files::openable(&root, &path)?;
        if let Some(line) = line {
            let uri = files::vscode_line_uri(&file, line)?;
            return tauri_plugin_opener::open_url(uri, None::<&str>)
                .map_err(|e| format!("Could not open the line in VS Code. Is it installed? {e}"));
        }
        tauri_plugin_opener::open_path(&file, None::<&str>)
            .map_err(|e| format!("Could not open {path}: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Fuzzy-search files inside the workspace for `@` mentions in the composer.
#[tauri::command]
pub(super) async fn search_files(
    shared: State<'_, Shared>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<String>, String> {
    let root = shared.root()?;
    let cap = limit.unwrap_or(20);
    tauri::async_runtime::spawn_blocking(move || files::search(&root, &query, cap))
        .await
        .map_err(|error| error.to_string())?
}
