// Where each conversation was scrolled to, so returning to a session puts the reader back.
// Persist only bounded numeric positions, not transcript text or attachments.

const LIMIT = 50;

/** The entry at the top of the view and how far its top sat below the view's top (px). Survives
 * older entries being drawn at an estimated height (see {@link renderLazily}). */
export interface ScrollAnchor {
  entry: number;
  offset: number;
}

type Position = { top: number; anchor: ScrollAnchor | null };
const STORAGE_KEY = 'dial.readingPositions.v1';
const positions = new Map<string, Position>();
try {
  const rows: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '[]');
  if (Array.isArray(rows)) for (const row of rows.slice(-LIMIT)) {
    if (!Array.isArray(row)) continue;
    const [key, p] = row;
    if (typeof key !== 'string' || !p || !Number.isFinite(p.top) || p.top < 0) continue;
    const a = p.anchor;
    const anchor = a && Number.isSafeInteger(a.entry) && a.entry >= 0 && Number.isFinite(a.offset)
      ? { entry: a.entry, offset: a.offset } : null;
    positions.set(key, { top: p.top, anchor });
  }
} catch { /* No storage or malformed data: open at the end. */ }

/** The key a session's position is kept under; `null` when it has no id yet. */
export function scrollKey(root: string | undefined, id: string | null | undefined): string | null {
  return root && id ? `${root}\n${id}` : null;
}

/** Remember `top` for `key`; a reader at the end is followed again, so nothing is kept. */
export function rememberScroll(key: string | null, top: number, atEnd: boolean, anchor: ScrollAnchor | null = null) {
  if (!key) return;
  if (atEnd && !positions.has(key)) return;
  positions.delete(key);
  if (!atEnd) positions.set(key, { top: Math.max(0, top), anchor });
  while (positions.size > LIMIT) positions.delete(positions.keys().next().value!);
  try { localStorage.setItem(STORAGE_KEY, JSON.stringify([...positions])); }
  catch { /* Keep positions in memory when storage is unavailable. */ }
}

/** The remembered position, or `null` to open at the end. */
export function savedScroll(key: string | null): number | null {
  return key ? positions.get(key)?.top ?? null : null;
}

/** The remembered anchor entry, when one was recorded with the position. */
export function savedAnchor(key: string | null): ScrollAnchor | null {
  return key ? positions.get(key)?.anchor ?? null : null;
}

/** How many of the newest entries always render fully. */
export const EAGER_ENTRIES = 40;

/**
 * Whether entry `index` of `total` may skip rendering while off screen (`content-visibility:
 * auto`): only older entries, and never one a search or jump is about to scroll to.
 */
export function renderLazily(index: number, total: number, targets: readonly number[] = [], eager = EAGER_ENTRIES): boolean {
  return index < total - eager && !targets.includes(index);
}
