export interface DiffLine {
  kind: "add" | "del" | "ctx" | "note";
  text: string;
  old: number | null;
  next: number | null;
}

export interface DiffHunk {
  header: string;
  lines: DiffLine[];
}

/** Parse ordinary Git unified diffs, including zero-length ranges and missing newlines. */
export function diffHunks(diff: string): DiffHunk[] {
  const hunks: DiffHunk[] = [];
  let hunk: DiffHunk | undefined;
  let old = 0, next = 0;
  for (const text of diff.split("\n")) {
    const range = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(text);
    if (range) {
      old = Number(range[1]);
      next = Number(range[2]);
      hunk = { header: text, lines: [] };
      hunks.push(hunk);
    } else if (hunk) {
      if (text.startsWith("+")) hunk.lines.push({ kind: "add", text, old: null, next: next++ });
      else if (text.startsWith("-")) hunk.lines.push({ kind: "del", text, old: old++, next: null });
      else if (text.startsWith(" ")) hunk.lines.push({ kind: "ctx", text, old: old++, next: next++ });
      else if (text.startsWith("\\")) hunk.lines.push({ kind: "note", text, old: null, next: null });
      else hunk = undefined;
    }
  }
  return hunks;
}
