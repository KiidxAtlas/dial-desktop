// Keep a burst of related tool calls (the calls between two messages) on one line, including
// while they run. The line summarizes the burst; its calls open beneath it.
import type { Entry, ToolEntry } from "./types";

type Kind = "read" | "search" | "edit" | "run" | "web" | "image" | "other";

function kind(tool: ToolEntry): Kind {
  if (tool.check) return "run";
  switch (tool.label.toLowerCase()) {
    case "read": case "view": case "open":
      return "read";
    case "search": case "find": case "grep": case "glob": case "ast_search": case "list":
      return "search";
    case "write": case "edit": case "ast_edit": case "create": case "delete": case "move":
      return "edit";
    case "run": case "bash": case "bg": case "background": case "verify": case "check": case "test":
      return "run";
    case "web": case "web search": case "web_search": case "fetch":
      return "web";
    case "generate image": case "image_gen":
      return "image";
  }
  return "other";
}

/** Calls that wait on the user stand on their own instead of joining a burst. */
function breaksRun(tool: ToolEntry) {
  return ["ask", "question"].includes(tool.label.toLowerCase());
}

export interface ToolRun {
  /** Index of the first and last entry of the run. */
  start: number;
  end: number;
  /** Key the run's open state is stored under in `ui.expanded`; kept while the run grows. */
  key: string;
  tools: ToolEntry[];
}

/** Consecutive calls form one run; messages, notices and calls that ask the user break it. */
export function toolRuns(entries: readonly Entry[]): Map<number, ToolRun> {
  const out = new Map<number, ToolRun>();
  let previous: ToolRun | undefined;
  entries.forEach((entry, index) => {
    if (entry.kind !== "tool") {
      previous = undefined;
    } else if (previous && !breaksRun(entry) && !breaksRun(previous.tools[previous.tools.length - 1])) {
      previous.end = index;
      previous.tools.push(entry);
    } else {
      previous = { start: index, end: index, key: `fold:${entry.key}`, tools: [entry] };
      out.set(index, previous);
    }
  });
  return out;
}

export function runActivity(run: ToolRun) {
  let active: ToolEntry | undefined;
  let failed = 0;
  let unknown = 0;
  for (const tool of run.tools) {
    if (tool.state === "running") active = tool;
    else if (tool.state === "failed") failed++;
    else if (tool.state === "unknown") unknown++;
  }
  return { current: active ?? run.tools[run.tools.length - 1], running: !!active, failed, unknown };
}

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** "Read 3 files, 2 searches, edited 2 files, ran 1 command", in the order the work happened. */
export function runSummary(run: ToolRun): string {
  const order: Kind[] = [];
  const calls = new Map<Kind, ToolEntry[]>();
  for (const tool of run.tools) {
    const k = kind(tool);
    if (!calls.has(k)) { order.push(k); calls.set(k, []); }
    calls.get(k)!.push(tool);
  }
  const distinct = (values: string[]) => new Set(values.filter(Boolean)).size;
  const parts = order.map((k) => {
    const list = calls.get(k)!;
    switch (k) {
      case "read": return `read ${plural(distinct(list.map((t) => t.target)) || list.length, "file")}`;
      case "search": return plural(list.length, "search", "searches");
      case "edit": {
        const files = distinct(list.flatMap((t) => (t.changedPaths.length ? t.changedPaths : [t.target])));
        return `edited ${plural(files || list.length, "file")}`;
      }
      case "run": return `ran ${plural(list.length, "command")}`;
      case "web": return plural(list.length, "web lookup");
      case "image": return `generated ${plural(list.reduce((n, t) => n + Math.max(1, t.images.length), 0), "image")}`;
      case "other": return `used ${plural(list.length, "tool")}`;
    }
  });
  const text = parts.join(", ");
  return text.charAt(0).toUpperCase() + text.slice(1);
}

export interface FileChange {
  path: string;
  /** Lines added and removed, when the calls reported them. */
  added: number | null;
  removed: number | null;
}

/** Line counts one call reports for its single changed file: a diff, or "(+4 -1)" in its receipt. */
function lineCounts(tool: ToolEntry): [number, number] | null {
  const out = tool.output;
  if (!out) return null;
  if (out.caption === "diff") {
    let added = 0, removed = 0;
    for (const line of out.text.split("\n")) {
      if (line.startsWith("+") && !line.startsWith("+++")) added++;
      else if (line.startsWith("-") && !line.startsWith("---")) removed++;
    }
    return [added, removed];
  }
  const m = /\(\+(\d+)(?:\s*[-−](\d+))?\)|\([-−](\d+)\)/.exec(out.text);
  if (!m) return null;
  return [Number(m[1] ?? 0), Number(m[2] ?? m[3] ?? 0)];
}

/** Files the run changed, in first-change order, with the line counts its calls reported. */
export function runFiles(run: ToolRun): FileChange[] {
  const files = new Map<string, FileChange>();
  for (const tool of run.tools) {
    const counts = tool.changedPaths.length === 1 ? lineCounts(tool) : null;
    for (const path of tool.changedPaths) {
      const file = files.get(path) ?? { path, added: null, removed: null };
      if (counts) {
        file.added = (file.added ?? 0) + counts[0];
        file.removed = (file.removed ?? 0) + counts[1];
      }
      files.set(path, file);
    }
  }
  return [...files.values()];
}

/** Images the run's calls wrote or viewed, each once. */
export function runImages(run: ToolRun): string[] {
  return [...new Set(run.tools.flatMap((t) => t.images))];
}
