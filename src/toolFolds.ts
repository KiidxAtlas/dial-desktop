// Keep a burst of related activity (the tool calls between two messages, with the coordination
// messages and status lines that arrive among them) on one line, including while it runs. The
// line summarizes the burst; its rows open beneath it.
import type { Entry, ToolEntry } from "./types";

type ReceivedEntry = Extract<Entry, { kind: "received" }>;
type NoticeEntry = Extract<Entry, { kind: "notice" }>;

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

/**
 * What a call is doing, as a present-tense phase for the live line: "Reading", "Editing",
 * "Verifying". Unknown tools read as "Working".
 */
export function phaseOf(tool: Pick<ToolEntry, "label" | "check">): string {
  if (tool.check) return "Verifying";
  switch (tool.label.toLowerCase()) {
    case "read": case "view": case "open":
      return "Reading";
    case "search": case "find": case "grep": case "glob": case "ast_search": case "list":
      return "Searching";
    case "write": case "create":
      return "Writing";
    case "edit": case "ast_edit":
      return "Editing";
    case "delete":
      return "Deleting files";
    case "move":
      return "Moving files";
    case "verify": case "check": case "test":
      return "Verifying";
    case "run": case "bash": case "bg": case "background":
      return "Running commands";
    case "web search": case "web_search":
      return "Searching the web";
    case "web": case "fetch":
      return "Browsing the web";
    case "generate image": case "image_gen":
      return "Generating an image";
    case "todo": case "todos": case "plan": case "update_plan": case "todo_write":
      return "Making the plan";
    case "task": case "agent": case "delegate":
      return "Delegating";
    case "ask": case "question":
      return "Asking you";
  }
  return "Working";
}

/** One transcript row a run folds away, with its index in the conversation. */
export interface RunItem {
  index: number;
  entry: ToolEntry | ReceivedEntry | NoticeEntry;
}

export interface ToolRun {
  /** Index of the first and last entry of the run; every entry between them is one of `items`. */
  start: number;
  end: number;
  /** Key the run's open state is stored under in `ui.expanded`; kept while the run grows. */
  key: string;
  tools: ToolEntry[];
  /** The calls, received messages and status lines of the run, in transcript order. */
  items: RunItem[];
}

/** Status lines that are part of the work around them. Alerts and dividers (a compaction, a
 * context reset) always stay in view. */
function incidental(entry: NoticeEntry) {
  return entry.lane === null || entry.lane === "context";
}

/**
 * Consecutive calls and received messages form one run. Status lines between them join it;
 * ones before or after it stay in view. Prompts, answers, errors, alerts, dividers and calls
 * that ask the user break it. `session` scopes the key of a run that opens with a message.
 */
export function toolRuns(entries: readonly Entry[], session: string | null = null): Map<number, ToolRun> {
  const out = new Map<number, ToolRun>();
  let previous: ToolRun | undefined;
  /** Status lines since the run's last call or message; they join it only if it continues. */
  let asides: RunItem[] = [];
  let asked = false;
  entries.forEach((entry, index) => {
    if (entry.kind === "notice" && incidental(entry)) {
      if (previous) asides.push({ index, entry });
      return;
    }
    if (entry.kind !== "tool" && entry.kind !== "received") {
      previous = undefined;
      asides = [];
      return;
    }
    const asks = entry.kind === "tool" && breaksRun(entry);
    if (previous && !asks && !asked) {
      previous.items.push(...asides, { index, entry });
      previous.end = index;
    } else {
      const key = entry.kind === "tool" ? entry.key : `${session ?? "new"}:received:${index}`;
      previous = { start: index, end: index, key: `fold:${key}`, tools: [], items: [{ index, entry }] };
      out.set(index, previous);
    }
    if (entry.kind === "tool") previous.tools.push(entry);
    asides = [];
    asked = asks;
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

type Part = { kind: Kind | "message"; text: string };

function summaryParts(run: ToolRun): Part[] {
  const order: (Kind | "message")[] = [];
  const calls = new Map<Kind, ToolEntry[]>();
  let messages = 0;
  for (const { entry } of run.items) {
    if (entry.kind === "received") {
      if (!messages++) order.push("message");
      continue;
    }
    if (entry.kind !== "tool") continue;
    const k = kind(entry);
    if (!calls.has(k)) { order.push(k); calls.set(k, []); }
    calls.get(k)!.push(entry);
  }
  const distinct = (values: string[]) => new Set(values.filter(Boolean)).size;
  return order.map((k) => {
    if (k === "message") return { kind: k, text: `received ${plural(messages, "message")}` };
    const list = calls.get(k)!;
    const text = (() => {
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
    })();
    return { kind: k, text };
  });
}

const capital = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

/** "Read 3 files, 2 searches, edited 2 files, ran 1 command, received 1 message", in the order the work happened. */
export function runSummary(run: ToolRun): string {
  return capital(summaryParts(run).map((p) => p.text).join(", "));
}

/** What a run changed, before what it looked at. */
const WEIGHT: Record<Kind | "message", number> = { edit: 0, image: 1, run: 2, other: 3, web: 4, search: 5, read: 6, message: 7 };

/** The run's headline: its weightiest `max` parts, the rest as a count.
 *  "Edited 2 files, generated 1 image · 2 more". The full line is `runSummary`. */
export function runHeadline(run: ToolRun, max = 2): string {
  const parts = summaryParts(run).sort((a, b) => WEIGHT[a.kind] - WEIGHT[b.kind]);
  const head = capital(parts.slice(0, max).map((p) => p.text).join(", "));
  return parts.length > max ? `${head} · ${parts.length - max} more` : head;
}

/** "8 of 12 tests passed", "3/10" → 0.67, 0.3: a progress line's fraction, if it states one. */
export function progressFraction(text: string | null | undefined): number | null {
  const m = /(\d+)\s*(?:of|\/)\s*(\d+)/.exec(text ?? "");
  if (!m) return null;
  const [done, total] = [Number(m[1]), Number(m[2])];
  return total > 0 && done <= total ? done / total : null;
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
  return toolFiles(run.tools);
}

/** Files `tools` changed, in first-change order, with the line counts the calls reported. */
export function toolFiles(tools: ToolEntry[]): FileChange[] {
  const files = new Map<string, FileChange>();
  for (const tool of tools) {
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
