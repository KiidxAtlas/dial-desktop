// Persistent workspace-level inline editor drafts.
//
// Drafts are keyed by `(workspaceRoot, relativePath)` so switching files, switching
// inspector panels, or unmounting FilesPane never silently drops unsaved edits or
// loses the original baseline SHA-256 conflict guard.

export interface FileDraft {
  root: string;
  path: string;
  /** Latest edited buffer text. */
  text: string;
  /** Original file text when editing began; used to compute dirty state. */
  baseText: string;
  /** Original disk SHA-256 when editing began; preserved for optimistic save conflict checks. */
  baseSha256?: string;
}

export function draftKey(root: string, path: string): string {
  return `${root}\0${path}`;
}

export class DraftStore {
  readonly entries = new Map<string, FileDraft>();

  get(root: string, path: string): FileDraft | undefined {
    return this.entries.get(draftKey(root, path));
  }

  has(root: string, path: string): boolean {
    return this.entries.has(draftKey(root, path));
  }

  isDirty(root: string, path: string): boolean {
    const draft = this.get(root, path);
    return !!draft && draft.text !== draft.baseText;
  }

  /** Store or update a draft. If `text === baseText`, cleans up the entry so unmodified buffers don't linger. */
  set(draft: FileDraft): void {
    const key = draftKey(draft.root, draft.path);
    if (draft.text === draft.baseText) {
      this.entries.delete(key);
      return;
    }
    this.entries.set(key, { ...draft });
  }

  /** Update only the text of an existing draft (or create it using the provided baseline). */
  update(root: string, path: string, text: string, baseText: string, baseSha256?: string): void {
    const existing = this.get(root, path);
    const resolvedBaseText = existing ? existing.baseText : baseText;
    const resolvedSha = existing ? existing.baseSha256 : baseSha256;
    this.set({
      root,
      path,
      text,
      baseText: resolvedBaseText,
      baseSha256: resolvedSha,
    });
  }

  clear(root: string, path: string): boolean {
    return this.entries.delete(draftKey(root, path));
  }

  /** Migrate an unsaved draft when a file is renamed inside `root`. */
  rename(root: string, from: string, to: string): void {
    const existing = this.get(root, from);
    if (!existing) return;
    this.entries.delete(draftKey(root, from));
    this.entries.set(draftKey(root, to), { ...existing, path: to });
  }

  /** Return all paths with unsaved drafts in `root`. */
  dirtyPaths(root: string): string[] {
    const out: string[] = [];
    for (const draft of this.entries.values()) {
      if (draft.root === root && draft.text !== draft.baseText) {
        out.push(draft.path);
      }
    }
    return out;
  }

  count(root: string): number {
    let n = 0;
    for (const draft of this.entries.values()) {
      if (draft.root === root && draft.text !== draft.baseText) n++;
    }
    return n;
  }
}
