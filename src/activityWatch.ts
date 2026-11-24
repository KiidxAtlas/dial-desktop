// Worker ids are local to their parent session; peer ids belong to the project's coordination board.
export interface WatchWorker {
  id: string; watchToken: string; name?: string; status: string; agent?: string; role?: string; model?: string;
  phase?: string; health?: string; steps?: number; tools?: number; started_ms?: number;
  task?: string; result?: string;
}
export interface WatchPeer {
  id: string; summary: string; phase: string; scopes: string[]; model: string; overlapping: string[];
  messages: { outgoing: boolean; text: string; unread: boolean; receipt_available: boolean; at_ms: number; by_user: boolean }[];
}
export interface WatchPeerBoard {
  session_id: string; scopes: string[]; overlapping: string[]; queued: number; rows: WatchPeer[];
}
export type WatchTarget = { kind: 'worker'; row: WatchWorker } | { kind: 'peer'; row: WatchPeer };

/** Resolve only the originally opened recipient. A refresh can reorder, remove or replace rows. */
export function resolveRecipient(selected: WatchTarget, workers: WatchWorker[], peers: WatchPeer[]): WatchTarget | null {
  if (selected.kind === 'worker') {
    const found = workers.filter(w => w.id === selected.row.id);
    if (found.length === 1 && found[0].watchToken === selected.row.watchToken && found[0].started_ms === selected.row.started_ms) return { kind: 'worker', row: found[0] };
  } else {
    const found = peers.filter(p => p.id === selected.row.id);
    if (found.length === 1) return { kind: 'peer', row: found[0] };
  }
  return null;
}

export function canAddress(target: WatchTarget): boolean {
  return target.kind === 'worker' ? !!target.row.watchToken && /^task-[1-9][0-9]*$/.test(target.row.id) && target.row.status === 'running'
    : /^[0-9a-f]{32}$/i.test(target.row.id);
}

const utf8 = new TextEncoder();
export const MESSAGE_LIMIT = 4096;
export const messageTooLong = (text: string) => text.length > MESSAGE_LIMIT || utf8.encode(text).length > MESSAGE_LIMIT;

/** RPC success can still mean the mailbox refused the message. Never acknowledge a different id. */
export function checkWorkerReceipt(id: string, reply: unknown): void {
  const rows = (reply as { receipts?: unknown } | null)?.receipts;
  const receipt = Array.isArray(rows) && rows.length === 1 && rows[0]?.to === id ? rows[0] : null;
  if (receipt?.status === 'queued') return;
  if (receipt?.status === 'refused') throw new Error(typeof receipt.reason === 'string' ? receipt.reason : 'The worker refused this message.');
  if (receipt?.status === 'finished') throw new Error('The worker finished before this message was queued.');
  throw new Error('The engine did not confirm that this message was queued.');
}

/** A queued inbox item is not a read receipt; a stale peer/sender echo is not a confirmation. */
export function checkPeerReceipt(sessionId: string, id: string, reply: unknown): void {
  const receipt = reply as { session_id?: string; to?: string; queued?: boolean } | null;
  if (receipt?.session_id !== sessionId || receipt.to !== id || receipt.queued !== true)
    throw new Error('The engine did not confirm that this message was queued.');
}
