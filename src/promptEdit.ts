import type { Attachment, Entry, Snapshot } from './types';

type UserEntry = Extract<Entry, { kind: 'user' }>;
type Scope = { workspaceRoot: string; sessionId: string | null };

/** Chosen images already have a full data URL from the native picker. Restore those bytes,
 * not a mutable path that may point at a different image by the time the user resends. */
function restoreAttachment(file: Attachment): Attachment {
  const match = file.path && file.url?.match(/^data:image\/(?:png|jpeg|gif|webp);base64,/i);
  return match ? { ...file, path: null, data: file.url!.slice(match[0].length) } : { ...file };
}

/** Only retain the last local submission, in memory. Never reconstruct attachments from paths
 * mentioned by an engine or replayed conversation. If identity is uncertain, warn instead. */
export class LastSubmission {
  private recent?: { scope: Scope; index: number; row?: UserEntry; wireText?: string; text: string; attachments: Attachment[] };

  start(snap: Snapshot | null, scope: Scope, text: string, attachments: Attachment[]) {
    this.recent = undefined;
    if (!attachments.length || !scope.sessionId || snap?.workspace?.root !== scope.workspaceRoot || snap.sessionId !== scope.sessionId) return;
    const receipt = this.recent = { scope, index: snap.entries.length, text, attachments: attachments.slice() };
    return (wireText?: string) => {
      if (this.recent !== receipt) return;
      if (wireText === undefined) this.recent = undefined;
      else this.recent.wireText = wireText;
    };
  }

  observe(snap: Snapshot) {
    const r = this.recent;
    if (!r) return;
    const row = snap.entries[r.index];
    if (snap.workspace?.root !== r.scope.workspaceRoot || snap.sessionId !== r.scope.sessionId || snap.historyLoading || snap.connection.state !== 'live' ||
        snap.entries.length < r.index || (r.row && r.row !== row) || (row && row.kind !== 'user')) this.recent = undefined;
    else if (row?.kind === 'user') r.row = row;
  }

  forRow(row: UserEntry) {
    const r = this.recent;
    if (!r || r.row !== row || r.wireText !== row.text) return;
    // Selected documents don't have preview bytes. The displayed prompt already points at
    // their private staged copies; keep those references rather than reopening the originals.
    if (r.attachments.some(file => file.path && !file.url)) {
      return { text: row.text, attachments: row.images ? r.attachments.filter(file => file.url).map(restoreAttachment) : [] };
    }
    return { text: r.text, attachments: r.attachments.map(restoreAttachment) };
  }
}
