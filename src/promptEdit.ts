import type { Attachment, Entry, Snapshot } from './types';
import { shallowReactive } from 'vue';

type UserEntry = Extract<Entry, { kind: 'user' }>;
type Scope = { workspaceRoot: string; sessionId: string | null };
type Receipt = { scope: Scope; index: number; row?: UserEntry; wireText?: string; text: string; attachments: Attachment[] };
const previewPrefix = /^data:image\/(?:png|jpeg|gif|webp);base64,/i;

/** Chosen images already have a full data URL from the native picker. Restore those bytes,
 * not a mutable path that may point at a different image by the time the user resends. */
function restoreAttachment(file: Attachment): Attachment {
  const match = file.path && file.url?.match(previewPrefix);
  return match ? { ...file, path: null, data: file.url!.slice(match[0].length) } : { ...file };
}

/** Only retain the last local submission, in memory. Never reconstruct attachments from paths
 * mentioned by an engine or replayed conversation. If identity is uncertain, warn instead. */
export class LastSubmission {
  // Track receipt and row/ack changes without proxying the transcript row or image bytes.
  private readonly state = shallowReactive<{ recent?: Receipt }>({});

  start(snap: Snapshot | null, scope: Scope, text: string, attachments: Attachment[]) {
    this.state.recent = undefined;
    if (!attachments.length || !scope.sessionId || snap?.workspace?.root !== scope.workspaceRoot || snap.sessionId !== scope.sessionId) return;
    const receipt = this.state.recent = shallowReactive<Receipt>({ scope, index: snap.entries.length, text, attachments: attachments.slice() });
    return (wireText?: string) => {
      if (this.state.recent !== receipt) return;
      if (wireText === undefined) this.state.recent = undefined;
      else receipt.wireText = wireText;
    };
  }

  observe(snap: Snapshot) {
    const r = this.state.recent;
    if (!r) return;
    const row = snap.entries[r.index];
    if (snap.workspace?.root !== r.scope.workspaceRoot || snap.sessionId !== r.scope.sessionId || snap.historyLoading || snap.connection.state !== 'live' ||
        snap.entries.length < r.index || (r.row && r.row !== row) || (row && row.kind !== 'user')) this.state.recent = undefined;
    else if (row?.kind === 'user') r.row = row;
  }

  private match(row: UserEntry) {
    const r = this.state.recent;
    return r?.row === row && r.wireText === row.text ? r : undefined;
  }

  imagesFor(row: UserEntry): readonly { name: string; url: string }[] {
    if (!row.images) return [];
    const r = this.match(row);
    return r?.attachments.flatMap(({ name, url }) => url && previewPrefix.test(url) ? [{ name, url }] : []) ?? [];
  }

  forRow(row: UserEntry) {
    const r = this.match(row);
    if (!r) return;
    // Selected documents don't have preview bytes. The displayed prompt already points at
    // their private staged copies; keep those references rather than reopening the originals.
    if (r.attachments.some(file => file.path && !file.url)) {
      return { text: row.text, attachments: row.images ? r.attachments.filter(file => file.url).map(restoreAttachment) : [] };
    }
    return { text: r.text, attachments: r.attachments.map(restoreAttachment) };
  }
}
