// Picking several sidebar sessions, as in Finder: ⌘/Ctrl-click toggles one row, Shift-click takes
// the range from the anchor (the last row clicked) to the clicked row, a plain click opens a
// session and drops the pick.

export interface PickState {
  picked: Set<string>;
  anchor: string | null;
}

export type ClickKind = "toggle" | "range" | "open";

export function clickKind(event: { metaKey: boolean; ctrlKey: boolean; shiftKey: boolean }): ClickKind {
  if (event.metaKey || event.ctrlKey) return "toggle";
  if (event.shiftKey) return "range";
  return "open";
}

/**
 * The pick after clicking row `id` of `ids` (the rows in display order). A range without an
 * anchor starts at `fallback` (the open session), else at the clicked row.
 */
export function pickAfter(state: PickState, ids: string[], id: string, kind: ClickKind, fallback: string | null): PickState {
  if (kind === "open") return { picked: new Set(), anchor: id };
  if (kind === "toggle") {
    const picked = new Set(state.picked);
    if (!picked.delete(id)) picked.add(id);
    return { picked, anchor: id };
  }
  const to = ids.indexOf(id);
  const from = ids.indexOf(state.anchor ?? fallback ?? id);
  const [a, b] = from < 0 ? [to, to] : [Math.min(from, to), Math.max(from, to)];
  // The anchor stays, so the range can be adjusted with further Shift-clicks.
  return { picked: new Set(ids.slice(a, b + 1)), anchor: state.anchor ?? (from < 0 ? id : ids[from]) };
}
