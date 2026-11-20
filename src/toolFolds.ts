// Long runs of finished tool calls fold into one summary line, so a busy turn reads as
// "what happened" rather than a wall of rows. Runs that are still moving stay open.

import type { ToolEntry } from "./types";

/** Fewest consecutive tool calls worth folding. */
export const FOLD_MIN = 4;

export interface ToolRun {
  /** Index of the first and last entry of the run. */
  start: number;
  end: number;
  /** Key the fold's open state is stored under in `ui.expanded`. */
  key: string;
  tools: ToolEntry[];
}

type Entry = { kind: string };

/** Foldable runs, by the index of their first entry. */
export function toolRuns(entries: readonly Entry[], turnRunning: boolean): Map<number, ToolRun> {
  const out = new Map<number, ToolRun>();
  let i = 0;
  while (i < entries.length) {
    if (entries[i].kind !== "tool") {
      i++;
      continue;
    }
    let j = i;
    while (j + 1 < entries.length && entries[j + 1].kind === "tool") j++;
    const tools = entries.slice(i, j + 1) as unknown as ToolEntry[];
    // The run at the end of a running turn is still growing: keep it open.
    const live = turnRunning && j === entries.length - 1;
    if (tools.length >= FOLD_MIN && !live && tools.every((t) => t.state !== "running")) {
      out.set(i, { start: i, end: j, key: `fold:${tools[0].key}`, tools });
    }
    i = j + 1;
  }
  return out;
}

/** One line describing a run: "6 tool calls · Edit ×3, Run ×2, Read · 1 failed · 4 files". */
export function runSummary(run: ToolRun): string {
  const counts = new Map<string, number>();
  for (const t of run.tools) counts.set(t.label, (counts.get(t.label) ?? 0) + 1);
  const kinds = [...counts]
    .sort((a, b) => b[1] - a[1])
    .slice(0, 4)
    .map(([label, n]) => (n > 1 ? `${label} ×${n}` : label))
    .join(", ");
  const failed = run.tools.filter((t) => t.state === "failed").length;
  const files = new Set(run.tools.flatMap((t) => t.changedPaths)).size;
  return [
    `${run.tools.length} tool calls`,
    kinds,
    failed ? `${failed} failed` : "",
    files ? (files === 1 ? "1 file changed" : `${files} files changed`) : "",
  ]
    .filter(Boolean)
    .join(" · ");
}
