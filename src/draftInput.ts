// Input state belongs to one draft, even while the user is viewing another session.
import type { Attachment, OpenSnapshot, Snapshot } from './types';

export interface DraftContent { text: string; attachments: Attachment[] }
export interface Draft extends DraftContent {
  loading: boolean;
  sending: boolean;
  /** Number of outstanding selection / paste / drop operations for this draft. */
  reading: number;
  pending?: DraftContent;
}

export function draftReady(draft: Draft): boolean {
  return !draft.loading && !draft.sending && !draft.reading;
}

/** Background summaries don't confirm the session can accept a prompt. Re-read
 * the active snapshot before any delayed resend or follow-up dispatch. */
export function readyForFollowUp(current: Snapshot | null, target: Pick<OpenSnapshot, 'workspace' | 'sessionId'>): current is OpenSnapshot {
  return !!current && current.workspace?.root === target.workspace.root && current.sessionId === target.sessionId &&
    current.connection.state === 'live' && current.canSend && !current.turnRunning && !current.historyLoading &&
    !current.question && !current.approvals.length;
}

export async function readingFiles(draft: Draft, read: () => Promise<void>): Promise<void> {
  draft.reading++;
  try { await read(); } finally { draft.reading--; }
}

/** Recover the earlier prompt ahead of any work already in the box, without dropping files. */
export function combinedInput(earlier: DraftContent, later: DraftContent): DraftContent {
  const keys = new Set(earlier.attachments.map(file => file.key));
  return {
    text: [earlier.text, later.text].filter(Boolean).join('\n\n'),
    attachments: [...earlier.attachments, ...later.attachments.filter(file => !keys.has(file.key))],
  };
}

export function restoreInput(draft: Draft, earlier: DraftContent) {
  Object.assign(draft, combinedInput(earlier, draft));
}

/** Keep unacknowledged input recoverable. A regenerate has its own payload and
 * must never clear the draft the user had prepared for the next turn. */
export function beginSend(draft: Draft, resend?: DraftContent) {
  const pending = resend ?? { text: draft.text, attachments: draft.attachments };
  draft.sending = true;
  if (!resend) {
    draft.text = '';
    draft.attachments = [];
  }
  draft.pending = pending;
  return (accepted: boolean) => {
    if (!accepted) restoreInput(draft, pending);
    draft.pending = undefined;
    draft.sending = false;
  };
}
