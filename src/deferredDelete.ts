// Desktop's Recently Deleted list is local to each project. Only ids are stored;
// the engine owns transcripts, which are removed only on explicit permanent deletion.
export interface Deletable { id: string; title: string }

export function trashKey(root: string): string {
  return `dial.recentlyDeleted.${JSON.stringify(root)}`;
}

export function readTrash(raw: string | null): string[] {
  if (raw === null) return [];
  const ids: unknown = JSON.parse(raw);
  if (!Array.isArray(ids) || !ids.every(id => typeof id === 'string' && id.length > 0)) {
    throw new Error('Invalid Recently Deleted list');
  }
  return [...new Set<string>(ids)];
}

/** Read the latest stored list on every user action, not a previous window's cache.
 * Publish the new state only after persistence succeeds. */
export function updateTrash(store: Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>, root: string, ids: string[], remove: boolean): string[] {
  const key = trashKey(root);
  const list = new Set(readTrash(store.getItem(key)));
  for (const id of ids) { if (remove) list.delete(id); else list.add(id); }
  const next = [...list];
  if (next.length) store.setItem(key, JSON.stringify(next));
  else store.removeItem(key);
  return next;
}
