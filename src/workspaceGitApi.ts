// Backend calls for Git-aware file panels (`files.rs`), beside the shared bridge.
import { invoke } from "@tauri-apps/api/core";
import type { GitStatusView, GrepMatch } from "./workspaceGit";
import type { DirEntry, FileView } from "./types";
/** A listed entry; `ignored` is set only when ignored entries were asked for. */
export interface GitDirEntry extends DirEntry {
  ignored?: boolean;
}

export interface CommitReview {
  paths: string[];
  branch: string | null;
  fingerprint: string;
}

/** The commit author Git has for a project; a part it has no value for is empty. */
export interface GitIdentity {
  name: string;
  email: string;
}

export interface GitBranchInfo {
  current: string | null;
  local: string[];
  upstream: string | null;
  ahead: number;
  behind: number;
}

export interface GitStashEntry {
  reference: string;
  summary: string;
}

export const workspaceGitApi = {
  /** Folder `path` (`""` is the root); `.gitignore`d entries are left out unless `showIgnored`. */
  list: (path: string, workspaceRoot: string, showIgnored: boolean) =>
    invoke<GitDirEntry[]>("list_dir", { path, workspaceRoot, showIgnored }),
  /** Workspace-relative changed paths across worktrees; `counts` adds +/- lines per file. */
  status: (workspaceRoot: string, counts = false) => invoke<GitStatusView>("file_git_status", { workspaceRoot, counts }),
  /** Discard every uncommitted change to `path`. Confirm with the user first. */
  revert: (path: string, workspaceRoot: string) => invoke<void>("file_revert", { path, workspaceRoot }),
  /** Save inline edits atomically to a workspace text file and return the refreshed file view. */
  write: (path: string, content: string, workspaceRoot: string, expectedSha256?: string) =>
    invoke<FileView>("write_file", { path, content, workspaceRoot, expectedSha256: expectedSha256 ?? null }),
  /** Create a new text file inside the workspace and return its file view. */
  createFile: (path: string, content: string, workspaceRoot: string) =>
    invoke<FileView>("file_create", { path, content, workspaceRoot }),
  /** Rename or move a file or folder inside the workspace; returns the new relative path. */
  renameFile: (from: string, to: string, workspaceRoot: string) =>
    invoke<string>("file_rename", { from, to, workspaceRoot }),
  /** Delete a plain file or empty folder inside the workspace. Confirm first. */
  deleteFile: (path: string, workspaceRoot: string) =>
    invoke<void>("file_delete", { path, workspaceRoot }),
  /** Undo one hunk (as the diff showed it: header and lines) in the working tree. Confirm first. */
  revertHunk: (path: string, hunk: string, workspaceRoot: string) =>
    invoke<void>("file_revert_hunk", { path, hunk, workspaceRoot }),
  /** Stage one hunk (as the diff showed it: header and lines) into Git's index. */
  stageHunk: (path: string, hunk: string, workspaceRoot: string) =>
    invoke<void>("file_stage_hunk", { path, hunk, workspaceRoot }),
  /** Unstage one hunk (as the staged diff showed it) from Git's index. */
  unstageHunk: (path: string, hunk: string, workspaceRoot: string) =>
    invoke<void>("file_unstage_hunk", { path, hunk, workspaceRoot }),
  /** Local branches and upstream ahead/behind counts for the repository. */
  branches: (workspaceRoot: string) => invoke<GitBranchInfo>("file_git_branches", { workspaceRoot }),
  /** Switch to a local branch, or create and switch to a new one. */
  checkout: (branch: string, create: boolean, workspaceRoot: string) =>
    invoke<string>("file_git_checkout", { branch, create, workspaceRoot }),
  /** Run `fetch`, `pull` (`--ff-only`) or `push` for the repository. */
  sync: (action: "fetch" | "pull" | "push", workspaceRoot: string) =>
    invoke<string>("file_git_sync", { action, workspaceRoot }),
  /** Cancel an in-flight `fetch`, `pull`, or `push` for the repository. */
  cancelSync: (workspaceRoot: string) =>
    invoke<boolean>("file_git_sync_cancel", { workspaceRoot }),
  stash: (action: "list" | "push" | "pop", workspaceRoot: string, message?: string) =>
    invoke<GitStashEntry[]>("file_git_stash", { action, message: message ?? null, workspaceRoot }),
  /** Resolve an unmerged Git conflict using `ours`, `theirs`, or `resolved`. */
  resolveConflict: (path: string, strategy: "ours" | "theirs" | "resolved", workspaceRoot: string) =>
    invoke<void>("file_git_resolve_conflict", { path, strategy, workspaceRoot }),
  /** Stage (`staged`) or unstage `paths`; unstaging keeps the working tree. */
  stage: (paths: string[], staged: boolean, workspaceRoot: string) =>
    invoke<void>("file_stage", { paths, staged, workspaceRoot }),
  review: (workspaceRoot: string) => invoke<CommitReview>("file_commit_review", { workspaceRoot }),
  commit: (message: string, workspaceRoot: string, review: CommitReview) => invoke<string>("file_commit", { message, workspaceRoot, review }),
  /** The newest commits of the current branch. */
  log: (workspaceRoot: string, limit = 100) => invoke<GitLogEntry[]>("file_git_log", { workspaceRoot, limit }),
  /** `dial review`, read-only: the engine's report on the working tree, a branch range or a commit. Runs a model. */
  aiReview: (workspaceRoot: string, target: ReviewTarget, focus = "") => invoke<string>("review_changes", { workspaceRoot, target, focus }),
  /** The engine's AI message for what is staged (`dial commit --dry-run`); commits nothing. */
  aiMessage: (workspaceRoot: string) => invoke<{ message: string; warning?: string | null }>("file_commit_message", { workspaceRoot }),
  identity: (workspaceRoot: string) => invoke<GitIdentity>("file_git_identity", { workspaceRoot }),
  /** Save the commit author for this repository, or with `everywhere` for all of the user's. */
  setIdentity: (name: string, email: string, everywhere: boolean, workspaceRoot: string) =>
    invoke<GitIdentity>("file_git_identity_set", { name, email, everywhere, workspaceRoot }),
  /** Lines containing `query` (case-insensitive, literal) in the project's files. */
  grep: (query: string, workspaceRoot: string, limit = 200) =>
    invoke<GrepMatch[]>("file_grep", { query, workspaceRoot, limit }),
  /** Open in the system's default app, or via VS Code's line-aware handler if requested. */
  open: (path: string, workspaceRoot: string, line?: number) => invoke<void>("file_open", { path, workspaceRoot, line }),
};

export interface GitLogEntry { sha: string; subject: string; author: string; date: string; refs: string }
export type ReviewTarget = { kind: "uncommitted" } | { kind: "base"; value: string } | { kind: "commit"; value: string };
