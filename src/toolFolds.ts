// Keep consecutive calls in the same category on one line, including while they run.
import type { Entry, ToolEntry } from "./types";

const categories = {
  inspect: "Read & search",
  edit: "Write & edit",
  run: "Run & check",
  web: "Web search",
} as const;

function category(tool: ToolEntry): keyof typeof categories | undefined {
  if (tool.check) return "run";
  switch (tool.label.toLowerCase()) {
    case "read": case "search": case "find": case "grep": case "glob": case "ast_search":
      return "inspect";
    case "write": case "edit": case "ast_edit":
      return "edit";
    case "run": case "bash": case "bg": case "background": case "verify": case "check":
      return "run";
    case "web": case "web search": case "web_search":
      return "web";
  }
}

export interface ToolRun {
  /** Index of the first and last entry of the run. */
  start: number;
  end: number;
  label: string;
  /** Key the fold's open state is stored under in `ui.expanded`. */
  key: string;
  tools: ToolEntry[];
}

/** Unrelated calls, messages and notices keep their own position and break a run. */
export function toolRuns(entries: readonly Entry[]): Map<number, ToolRun> {
  const out = new Map<number, ToolRun>();
  let previous: ToolRun | undefined;
  entries.forEach((entry, index) => {
    const kind = entry.kind === "tool" ? category(entry) : undefined;
    if (entry.kind !== "tool" || !kind) {
      previous = undefined;
    } else if (previous?.label === categories[kind]) {
      previous.end = index;
      previous.tools.push(entry);
    } else {
      previous = { start: index, end: index, label: categories[kind], key: `fold:${entry.key}`, tools: [entry] };
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
