// Raw errors from the file, diff, terminal and browser panels turned into what happened and what to
// do next. The backend's messages stay as the detail; nothing here hides a failure.

export interface PaneError {
  /** What went wrong, in a few words. */
  title: string;
  /** The original message, cleaned of prefixes such as `Error: `. */
  detail: string;
  /** The next step, when there is a clear one. */
  hint?: string;
}

const NO_AUTHOR = /please tell me who you are|author identity unknown|unable to auto-detect email|empty ident name/i;

/** Git refused a commit because it has no author name or email; the Changes panel asks for them. */
export function needsGitIdentity(error: unknown): boolean {
  return NO_AUTHOR.test(rawText(error));
}

const RULES: { test: RegExp; title: string; hint?: string }[] = [
  {
    test: /could not run git|git: (command )?not found|'git' is not recognized|xcrun: error/i,
    title: "Git is not installed",
    hint: "Install Git (on a Mac run xcode-select --install; elsewhere see git-scm.com), then refresh.",
  },
  {
    test: /not a git repository|needs? the project to be a git repository/i,
    title: "This project is not a Git repository",
    hint: "Diffs and reverts need Git. Run git init in the project folder to start tracking changes.",
  },
  {
    test: NO_AUTHOR,
    title: "Git doesn't know who you are",
    hint: "Enter your name and email under the commit message in the Changes panel, then commit again.",
  },
  {
    test: /nothing is staged|nothing to commit|no changes added to commit/i,
    title: "Nothing to commit",
    hint: "Stage files with the checkbox beside them first.",
  },
  {
    test: /write a commit message/i,
    title: "No commit message",
    hint: "Describe the change in a line, then commit.",
  },
  {
    test: /staged too|outside this project folder are staged/i,
    title: "Other files are staged",
    hint: "Files outside this project folder are staged as well. Unstage them or commit from a terminal.",
  },
  {
    test: /no longer in the file|could not be undone cleanly|patch does not apply/i,
    title: "The file changed meanwhile",
    hint: "Refresh the diff and try again.",
  },
  {
    test: /is a new file; revert the whole file/i,
    title: "New file",
    hint: "A new file has no earlier version per hunk. Use Revert file to delete it instead.",
  },
  {
    test: /outside the project folder/i,
    title: "Outside the project",
    hint: "Only files inside the open project folder can be shown here.",
  },
  {
    test: /project changed/i,
    title: "The project changed",
    hint: "Another folder was opened meanwhile. Pick the file again.",
  },
  {
    test: /permission denied|operation not permitted|access is denied|os error (1|5|13)\b/i,
    title: "Permission denied",
    hint: "Dial is not allowed to open it. Check the file's permissions; on a Mac also System Settings → Privacy & Security → Files and Folders.",
  },
  {
    test: /is a folder|is a directory/i,
    title: "That is a folder",
    hint: "Expand it in the file tree instead.",
  },
  {
    test: /too large|too big|file size exceeds/i,
    title: "The file is too large",
    hint: "Open it in another app with Open in editor.",
  },
  {
    test: /binary/i,
    title: "Binary file",
    hint: "It cannot be shown as text. Open it in its own app with Open in editor.",
  },
  {
    test: /no such file|not found|cannot find the (file|path)|os error (2|3)\b|enoent/i,
    title: "Not found",
    hint: "It may have been moved or deleted. Refresh and try again.",
  },
];

/** The raw text of `error`, whatever was thrown. */
function rawText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  if (error && typeof error === "object" && typeof (error as { message?: unknown }).message === "string") {
    return (error as { message: string }).message;
  }
  return error == null ? "" : String(error);
}

/** `error` as a title, its original detail and, when known, the next step. */
export function paneError(error: unknown): PaneError {
  const detail = rawText(error).replace(/^(Error|TypeError):\s*/, "").trim() || "Unknown error";
  const rule = RULES.find((r) => r.test.test(detail));
  if (!rule) return { title: "Something went wrong", detail };
  return { title: rule.title, detail, hint: rule.hint };
}

/** One line for a toast: the title, the next step, and the original detail when it adds anything. */
export function paneErrorText(error: unknown): string {
  const { title, detail, hint } = paneError(error);
  if (!hint) return detail;
  return `${title}. ${hint} (${detail})`;
}
