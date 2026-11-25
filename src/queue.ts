// Per-session follow-up prompt queue: prepare next prompts while a turn runs without interrupting it.
import { reactive } from "vue";
import { engine, showToast, snapshot } from './bridge';
import { draftFor, draftRecord } from './drafts';
import { draftReady, readyForFollowUp, restoreInput } from './draftInput';
import { DRAFTS, QUEUES, localKeys, readLocal, writeLocal } from './localPrompts';
import { canRetry } from "./sessionActions";
import type { Attachment, OpenSnapshot } from "./types";
import { focusComposer } from "./ui";
import { typeIdFor } from "./savedTypes";

export interface QueuedPrompt {
  id: number;
  text: string;
  attachments: Attachment[];
}

export interface SessionQueue {
  items: QueuedPrompt[];
  /** Set when a turn fails, is aborted, or a send is rejected so queued prompts do not fire blindly. */
  paused: boolean;
  sending: boolean;
  loading: boolean;
  saving: boolean;
  recovered: boolean;
}

const queues = new Map<string, SessionQueue>();
const queuedKeys = reactive(new Set<string>());
let queuedIndex: Promise<void> | undefined;

/** Sidebar badges need only keys, not every saved follow-up's attachment bytes. */
export function hasQueuedFollowUp(root: string, id: string): boolean {
  const key = JSON.stringify([root, id]);
  queuedIndex ??= localKeys(QUEUES).then(keys => {
    for (const k of keys) if (typeof k === 'string') queuedKeys.add(k);
  }).catch(() => { /* In-memory queues remain visible even without storage. */ });
  const saved = queuedKeys.has(key);
  const queue = queues.get(key);
  return queue && !queue.loading ? queue.items.length > 0 : saved;
}
let observedKey: string | undefined;
let observedRunning = false;
let turnEpoch = 0;
let nextId = 0;

function keyOf(snap: Pick<OpenSnapshot, "workspace" | "sessionId">): string {
  return JSON.stringify([snap.workspace.root, snap.sessionId]);
}

async function restore(key: string, queue: SessionQueue) {
  try {
    const saved = await readLocal(QUEUES, key) as { text: string; attachments: Attachment[] }[] | undefined;
    if (!Array.isArray(saved) || !saved.length) return;
    if (saved.some((item) => typeof item?.text !== "string" || !Array.isArray(item.attachments))) {
      throw new Error("Invalid saved queue");
    }
    queue.paused = true;
    queue.recovered = true;
    queuedKeys.add(key);
    queue.items = saved.map(({ text, attachments }) => ({ id: ++nextId, text, attachments }));
  } catch {
    showToast("Could not restore saved follow-ups for this session.");
  } finally {
    queue.loading = false;
  }
}

async function save(key: string, items: readonly QueuedPrompt[], extra: Record<string, unknown> = {}): Promise<boolean> {
  const saved = items.map(({ text, attachments }) => ({ text, attachments: attachments.map((a) => ({ ...a })) }));
  try {
    await writeLocal(key, { ...extra, [QUEUES]: saved.length ? saved : undefined });
    if (items.length) queuedKeys.add(key); else queuedKeys.delete(key);
    return true;
  } catch {
    showToast("Could not save changes to the follow-up queue.");
    return false;
  }
}

async function changeItems(key: string, queue: SessionQueue, items: QueuedPrompt[], extra?: Record<string, unknown>): Promise<boolean> {
  queue.saving = true;
  try {
    if (!(await save(key, items, extra))) return false;
    queue.items = items;
    if (!items.length) {
      queue.paused = false;
      queue.recovered = false;
    }
    return true;
  } finally {
    queue.saving = false;
  }
}

export function queueFor(snap: Pick<OpenSnapshot, "workspace" | "sessionId">): SessionQueue {
  const key = keyOf(snap);
  let queue = queues.get(key);
  if (!queue) {
    queue = reactive({ items: [], paused: false, sending: false, loading: !!snap.sessionId, saving: false, recovered: false });
    queues.set(key, queue);
    if (snap.sessionId) void restore(key, queue);
  }
  return queue;
}

/** Add a follow-up prompt to this session's queue; returns whether anything was queued. */
export async function enqueuePrompt(snap: OpenSnapshot, text: string, attachments: Attachment[]): Promise<boolean> {
  if (!snap.sessionId || !snap.turnRunning || snap.connection.state !== "live") return false;
  const trimmed = text.trim();
  if (!trimmed && !attachments.length) return false;
  const queue = queueFor(snap);
  if (queue.loading || queue.saving || queue.sending) return false;
  const started = turnEpoch;
  const key = keyOf(snap);
  const saved = await changeItems(key, queue, [...queue.items, { id: ++nextId, text: trimmed, attachments: [...attachments] }]);
  if (saved && (observedKey !== key || !observedRunning || started !== turnEpoch)) queue.paused = true;
  return saved;
}

export async function removeQueued(snap: Pick<OpenSnapshot, "workspace" | "sessionId">, id: number) {
  const key = keyOf(snap);
  const queue = queues.get(key);
  if (!queue || queue.loading || queue.saving || queue.sending) return;
  await changeItems(key, queue, queue.items.filter((item) => item.id !== id));
}

/** Move a queued prompt back into the session's composer draft so the user can edit it. */
export async function editQueued(snap: OpenSnapshot, id: number): Promise<boolean> {
  const key = keyOf(snap);
  const queue = queues.get(key);
  const item = queue?.items.find((entry) => entry.id === id);
  if (!queue || !item || queue.loading || queue.saving || queue.sending) return false;
  const draft = draftFor(snap);
  if (!draftReady(draft)) return false;
  if (draft.text || draft.attachments.length) {
    showToast("Send or clear your current draft before editing a queued prompt");
    return false;
  }
  const remaining = queue.items.filter((entry) => entry.id !== id);
  draft.sending = true;
  try {
    const saved = await changeItems(key, queue, remaining, { [DRAFTS]: draftRecord(item.text, item.attachments) });
    if (!saved) return false;
    // Preserve input added by another control while the local transaction finished.
    restoreInput(draft, item);
    if (key === observedKey) focusComposer();
    return true;
  } finally {
    draft.sending = false;
  }
}

/** Send the first queued prompt for `snap` when the session is ready. */
export async function sendQueuedNext(snap: OpenSnapshot): Promise<boolean> {
  const key = keyOf(snap);
  const queue = queues.get(key);
  if (!queue || queue.loading || queue.saving || queue.sending || !queue.items.length) return false;
  if (!snap.sessionId || !readyForFollowUp(snapshot.value, snap)) {
    queue.paused = true;
    return false;
  }
  const [item, ...remaining] = queue.items;
  queue.sending = true;
  queue.paused = false;
  // Keep the restart warning on a rejected or unconfirmed send.
  try {
    const scope = { workspaceRoot: snap.workspace.root, sessionId: snap.sessionId, typeId: typeIdFor(snap.sessionId) };
    const accepted = await engine.send(item.text, item.attachments, scope);
    if (!accepted) {
      // No acceptance: leave the persisted copy intact and require manual review before retry.
      queue.paused = true;
      return false;
    }
    if (!remaining.length) queue.recovered = false;
    queue.items = remaining;
    if (!(await save(key, remaining))) {
      queue.paused = true;
      showToast("Check the transcript if a sent follow-up reappears after restarting.");
    }
    return true;
  } finally {
    queue.sending = false;
  }
}

/** Pause this session's queue when the user stops the running turn. */
export function markTurnAborted(snap: Pick<OpenSnapshot, "workspace" | "sessionId">) {
  const queue = queues.get(keyOf(snap));
  if (queue) queue.paused = true;
}

/** Observe snapshots so a finished turn advances the queue (or pauses on error/abort). */
export function syncQueue(snap: OpenSnapshot) {
  const key = keyOf(snap);
  const switched = key !== observedKey;
  const finished = !switched && observedRunning && !snap.turnRunning;
  if (switched || observedRunning !== snap.turnRunning) turnEpoch++;
  if (observedKey && switched) {
    const previous = queues.get(observedKey);
    if (previous?.items.length && !previous.paused) {
      previous.paused = true;
      showToast('Follow-ups stay with their conversation. Reopen it to resume the queue.');
    }
  }
  observedKey = key;
  if (snap.turnRunning && (switched || !observedRunning) && !queueFor(snap).items.length) queueFor(snap).paused = false;
  observedRunning = snap.turnRunning;
  if (!finished) return;
  const queue = queues.get(key);
  if (!queue || !queue.items.length) return;
  if (queue.saving || queue.sending) {
    queue.paused = true;
    return;
  }

  if (queue.paused || !readyForFollowUp(snapshot.value, snap) || canRetry(snap)) {
    queue.paused = true;
    showToast("Queued follow-up paused; send it when ready");
    return;
  }
  void sendQueuedNext(snap);
}
