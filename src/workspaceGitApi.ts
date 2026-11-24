// Backend calls for Git-aware file panels (`files.rs`), beside the shared bridge.
import { invoke } from "@tauri-apps/api/core";
import type { GitStatusView, GrepMatch } from "./workspaceGit";
import type { DirEntry } from "./types";

/** A listed entry; `ignored` is set only when ignored entries were asked for. */
export interface GitDirEntry extends DirEntry {
  ignored?: boolean;
}

export interface CommitReview {
  paths: string[];
  branch: string | null;
  fingerprint: string;
}

export const workspaceGitApi = {
  /** Folder `path` (`""` is the root); `.gitignore`d entries are left out unless `showIgnored`. */
  list: (path: string, workspaceRoot: string, showIgnored: boolean) =>
    invoke<GitDirEntry[]>("list_dir", { path, workspaceRoot, showIgnored }),
  /** Changed paths from one `git status`; `counts` adds +/- lines per file. */
  status: (workspaceRoot: string, counts = false) => invoke<GitStatusView>("file_git_status", { workspaceRoot, counts }),
  /** Discard every uncommitted change to `path`. Confirm with the user first. */
  revert: (path: string, workspaceRoot: string) => invoke<void>("file_revert", { path, workspaceRoot }),
  /** Undo one hunk (as the diff showed it: header and lines) in the working tree. Confirm first. */
  revertHunk: (path: string, hunk: string, workspaceRoot: string) =>
    invoke<void>("file_revert_hunk", { path, hunk, workspaceRoot }),
  /** Stage (`staged`) or unstage `paths`; unstaging keeps the working tree. */
  stage: (paths: string[], staged: boolean, workspaceRoot: string) =>
    invoke<void>("file_stage", { paths, staged, workspaceRoot }),
  review: (workspaceRoot: string) => invoke<CommitReview>("file_commit_review", { workspaceRoot }),
  commit: (message: string, workspaceRoot: string, review: CommitReview) => invoke<string>("file_commit", { message, workspaceRoot, review }),
  /** Lines containing `query` (case-insensitive, literal) in the project's files. */
  grep: (query: string, workspaceRoot: string, limit = 200) =>
    invoke<GrepMatch[]>("file_grep", { query, workspaceRoot, limit }),
  /** Open in the system's default app, or via VS Code's line-aware handler if requested. */
  open: (path: string, workspaceRoot: string, line?: number) => invoke<void>("file_open", { path, workspaceRoot, line }),
};
