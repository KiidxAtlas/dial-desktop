import type { Entry, ToolEntry } from './types';

type Receipt = { index: number; tool: ToolEntry };
interface Review { start: number; checks: Receipt[]; files: { path: string; calls: Receipt[] }[] }

/** Group recorded receipts only. A Git diff taken now is not evidence of a past turn. */
export function turnReviews(entries: Entry[], running: boolean) {
  const byEnd = new Map<number, Review[]>();
  let start = -1, checks: Receipt[] = [], files = new Map<string, Receipt[]>();
  function finish(end: number) {
    if (start >= 0 && end > start && (checks.length || files.size)) {
      byEnd.set(end, [{ start, checks, files: Array.from(files, ([path, calls]) => ({ path, calls })) }]);
    }
  }
  entries.forEach((entry, index) => {
    if (entry.kind === 'user') {
      finish(index - 1);
      start = index; checks = []; files = new Map();
    } else if (start >= 0 && entry.kind === 'tool') {
      const receipt = { index, tool: entry };
      if (entry.check) checks.push(receipt);
      for (const path of new Set(entry.changedPaths)) {
        if (!files.has(path)) files.set(path, []);
        files.get(path)!.push(receipt);
      }
    }
  });
  if (!running) finish(entries.length - 1);
  return byEnd;
}
