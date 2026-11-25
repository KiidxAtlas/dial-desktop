// Git status for the file panels: markers in the file tree and +/- counts in the Changes list.
// Pure helpers only; the backend calls are in workspaceGitApi.ts.

/** `M` modified, `A` added, `D` deleted, `R` renamed, `U` untracked, `C` conflicted. */
export type GitCode = "M" | "A" | "D" | "R" | "U" | "C";

/** One changed path, relative to the project and `/`-separated (`files.rs` `GitFileStatus`). */
export interface GitFileStatus {
  path: string;
  status: GitCode;
  added?: number;
  removed?: number;
  /** Some of the change is in Git's index, staged for the next commit. */
  staged?: boolean;
  /** Some of the change is only in the working tree. */
  unstaged?: boolean;
}

export interface GitStatusView {
  repo: boolean;
  files: GitFileStatus[];
  /** The checked-out branch, or `detached at <commit>`; null outside Git or before a first commit. */
  branch?: string | null;
}

/** One line a content search found (`files.rs` `GrepMatch`). */
export interface GrepMatch {
  path: string;
  /** 1-based. */
  line: number;
  text: string;
}

/** Whether a changed file's checkbox shows staged: all of it (`true`), part (`"mixed"`) or none. */
export function stageState(git: GitFileStatus | null): boolean | "mixed" {
  if (!git?.staged) return false;
  return git.unstaged ? "mixed" : true;
}

/** How many changed files have something staged. */
export function stagedCount(files: GitFileStatus[]): number {
  return files.filter((file) => file.staged).length;
}

const LABELS: Record<GitCode, string> = {
  M: "Modified",
  A: "Added",
  D: "Deleted",
  R: "Renamed",
  U: "Untracked (new)",
  C: "Conflicted",
};

/** A tree row's marker: a status letter for a file, `•` for a folder holding changes. */
export interface GitMarker {
  text: string;
  code: GitCode | "dir";
  title: string;
}

/** Marker lookup for tree rows, built once per status refresh. */
export function gitMarkers(files: GitFileStatus[]): (path: string, dir: boolean) => GitMarker | null {
  const exact = new Map<string, GitCode>();
  const untrackedDirs: string[] = [];
  const changedDirs = new Set<string>();
  for (const file of files) {
    const path = file.path.replace(/\/$/, "");
    if (file.path.endsWith("/")) untrackedDirs.push(path + "/");
    exact.set(path, file.status);
    const parts = path.split("/");
    for (let i = 1; i < parts.length; i++) changedDirs.add(parts.slice(0, i).join("/"));
  }
  return (path, dir) => {
    const code = exact.get(path) ?? (untrackedDirs.some((d) => path.startsWith(d)) ? "U" : undefined);
    if (code) return { text: code, code, title: LABELS[code] };
    if (dir && changedDirs.has(path)) return { text: "•", code: "dir", title: "Contains changes" };
    return null;
  };
}

/** The status entry for project-relative `path`, if it changed. */
export function gitStatusOf(files: GitFileStatus[], path: string): GitFileStatus | null {
  return files.find((file) => file.path === path) ?? null;
}

export function gitLabel(code: GitCode): string {
  return LABELS[code];
}

/** A session-touched file with its current git status; `settled` when git reports no diff for it
 *  any more (committed or reverted since). */
export interface ChangeRow<T> {
  item: T;
  git: GitFileStatus | null;
  settled: boolean;
}

/** The Changes list: files that still differ first, in session order, then settled ones.
 *  Without git status (`repo` false or unknown) nothing counts as settled. */
export function changeRows<T>(items: T[], pathOf: (item: T) => string, files: GitFileStatus[], repo: boolean): ChangeRow<T>[] {
  const rows = items.map((item) => {
    const git = gitStatusOf(files, pathOf(item));
    return { item, git, settled: repo && !git };
  });
  return [...rows.filter((r) => !r.settled), ...rows.filter((r) => r.settled)];
}
