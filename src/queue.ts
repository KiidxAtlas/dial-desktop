// Per-session follow-up prompt queue: prepare next prompts while a turn runs without interrupting it.
import { reactive } from "vue";
import { engine, showToast } from "./bridge";
import { draftFor } from "./drafts";
import { canRetry } from "./sessionActions";
import type { Attachment, OpenSnapshot } from "./types";
import { focusComposer } from "./ui";

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
}

const queues = new Map<string, SessionQueue>();
let observedKey: string | undefined;
let observedRunning = false;
let nextId = 0;

function keyOf(snap: Pick<OpenSnapshot, "workspace" | "sessionId">): string {
  return JSON.stringify([snap.workspace.root, snap.sessionId]);
}

export function queueFor(snap: Pick<OpenSnapshot, "workspace" | "sessionId">): SessionQueue {
  const key = keyOf(snap);
  let queue = queues.get(key);
  if (!queue) {
    queue = reactive({ items: [], paused: false, sending: false });
    queues.set(key, queue);
  }
  return queue;
}

/** Add a follow-up prompt to this session's queue; returns whether anything was queued. */
export function enqueuePrompt(snap: OpenSnapshot, text: string, attachments: Attachment[]): boolean {
  if (!snap.sessionId || !snap.turnRunning || snap.connection.state !== "live") return false;
  if (attachments.length && snap.model?.acceptsImages === false) return false;
  const trimmed = text.trim();
  if (!trimmed && !attachments.length) return false;
  const queue = queueFor(snap);
  queue.items.push({ id: ++nextId, text: trimmed, attachments: [...attachments] });
  return true;
}

export function removeQueued(snap: Pick<OpenSnapshot, "workspace" | "sessionId">, id: number) {
  const queue = queues.get(keyOf(snap));
  if (!queue) return;
  queue.items = queue.items.filter((item) => item.id !== id);
  if (!queue.items.length) queue.paused = false;
}

/** Move a queued prompt back into the session's composer draft so the user can edit it. */
export function editQueued(snap: OpenSnapshot, id: number): boolean {
  const queue = queues.get(keyOf(snap));
  const index = queue?.items.findIndex((item) => item.id === id) ?? -1;
  if (!queue || index < 0) return false;
  const draft = draftFor(snap);
  if (draft.text.trim() || draft.attachments.length) {
    showToast("Send or clear your current draft before editing a queued prompt");
    return false;
  }
  const [item] = queue.items.splice(index, 1);
  if (!queue.items.length) queue.paused = false;
  draft.text = item.text;
  draft.attachments = [...item.attachments];
  focusComposer();
  return true;
}

/** Send the first queued prompt for `snap` when the session is ready. */
export async function sendQueuedNext(snap: OpenSnapshot): Promise<boolean> {
  if (snap.connection.state !== "live" || snap.turnRunning || !snap.canSend) return false;
  const queue = queues.get(keyOf(snap));
  if (!queue || queue.sending || !queue.items.length) return false;
  if (snap.question || snap.approvals.length || (queue.items[0].attachments.length && snap.model?.acceptsImages === false)) {
    queue.paused = true;
    return false;
  }
  const item = queue.items.shift()!;
  queue.sending = true;
  queue.paused = false;
  try {
    const scope = { workspaceRoot: snap.workspace.root, sessionId: snap.sessionId };
    const accepted = await engine.send(item.text, item.attachments, scope);
    if (!accepted) {
      queue.items.unshift(item);
      queue.paused = true;
      return false;
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
  const finished = key === observedKey && observedRunning && !snap.turnRunning;
  if (observedKey && key !== observedKey) {
    const previous = queues.get(observedKey);
    if (previous?.items.length) previous.paused = true;
  }
  observedKey = key;
  if (snap.turnRunning && !observedRunning && !queueFor(snap).items.length) queueFor(snap).paused = false;
  observedRunning = snap.turnRunning;
  if (!finished) return;
  const queue = queues.get(key);
  if (!queue || !queue.items.length) return;

  if (queue.paused || !snap.canSend || snap.connection.state !== "live" || canRetry(snap) || snap.approvals.length > 0 || !!snap.question) {
    queue.paused = true;
    showToast("Queued follow-up paused; send it when ready");
    return;
  }
  if (snap.connection.state === "live" && snap.canSend) {
    void sendQueuedNext(snap);
  }
}
