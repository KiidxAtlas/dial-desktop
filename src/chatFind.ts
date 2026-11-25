import type { Entry, TranscriptHit } from './types';

type Field = 'text' | 'reasoning' | 'label' | 'target' | 'input' | 'output' | 'message' | 'sender';
export interface TextMatch { entry: number; field: Field; start: number; end: number }

/** Search fields independently: a match must exist in one piece of text the reader can reveal. */
function fields(entry: Entry): [Field, string][] {
  switch (entry.kind) {
    case 'tool': return [['label', entry.label], ['target', entry.target], ['input', entry.input?.text ?? ''], ['output', entry.output?.text ?? '']];
    case 'received': return [['sender', entry.sender], ['text', entry.text]];
    case 'notice': case 'error': return [['message', entry.message]];
    case 'assistant': return [['reasoning', entry.reasoning], ['text', entry.text]];
    default: return [['text', entry.text]];
  }
}

export function findOccurrences(entries: Entry[], query: string): TextMatch[] {
  const term = query.trim();
  if (!term) return [];
  // Match against the original UTF-16 string. Lowercasing the haystack can shift later offsets.
  const pattern = new RegExp(term.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'giu');
  const hits: TextMatch[] = [];
  entries.forEach((row, entry) => {
    for (const [field, text] of fields(row)) {
      for (const m of text.matchAll(pattern)) hits.push({ entry, field, start: m.index!, end: m.index! + m[0].length });
    }
  });
  return hits;
}

/** Saved search results have storage entry IDs; dial-client's displayed messages do not. Never
 * treat an opaque ID (or the rank of a search result) as a displayed row index. */
export function transcriptCandidates(entries: Entry[], hit: Pick<TranscriptHit, 'role' | 'preview'>): number[] {
  const needle = hit.preview.trim();
  if (!needle) return [];
  return entries.flatMap((row, i) => {
    if (row.kind === 'received' && row.source === 'journal') return hit.role === 'user' && row.text.trim() === needle ? [i] : [];
    return (row.kind === 'user' || row.kind === 'assistant') && row.kind === hit.role && row.text.trim() === needle ? [i] : [];
  });
}
