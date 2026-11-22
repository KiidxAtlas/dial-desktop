// Drafts are stored locally, keyed by project and session. Never sync them to the engine.
import { reactive, watch } from "vue";
import type { Attachment, OpenSnapshot } from "./types";
import { showToast } from "./bridge";
import { DRAFTS, localKeys, readLocal, writeLocal } from "./localPrompts";

interface Draft {
  text: string;
  attachments: Attachment[];
  sending: boolean;
  /** Unacknowledged input stays recoverable alongside the editable next draft. */
  pending?: { text: string; attachments: Attachment[] };
  loading: boolean;
}

const drafts = new Map<string, Draft>();
let warned = false;
const storedKeys = reactive(new Set<string>());
let indexLoaded: Promise<void> | undefined;

export function hasDraft(root: string, id: string): boolean {
  const key = JSON.stringify([root, id]);
  indexLoaded ??= localKeys(DRAFTS).then(keys => {
    for (const k of keys) if (typeof k === "string") storedKeys.add(k);
  }).catch(storageError);
  const draft = drafts.get(key);
  return draft && !draft.loading ? !!(draft.text || draft.attachments.length || draft.pending) : storedKeys.has(key);
}

function storageError() {
  if (warned) return;
  warned = true;
  showToast("Could not save or restore a local draft. Drafts still work while the app is open.");
}

export function draftRecord(text: string, attachments: readonly Attachment[]) {
  return text || attachments.length ? { text, attachments: attachments.map((a) => ({ ...a })) } : undefined;
}

function recoverable(draft: Draft) {
  const p = draft.pending;
  if (!p) return draftRecord(draft.text, draft.attachments);
  const keys = new Set(p.attachments.map(a => a.key));
  return draftRecord([p.text, draft.text].filter(Boolean).join("\n\n"), [...p.attachments, ...draft.attachments.filter(a => !keys.has(a.key))]);
}

export function beginSend(draft: Draft) {
  const pending = { text: draft.text, attachments: draft.attachments };
  draft.sending = true;
  draft.text = "";
  draft.attachments = [];
  draft.pending = pending;
  return (accepted: boolean) => {
    const restored = !accepted && recoverable(draft);
    draft.pending = undefined;
    if (restored) {
      draft.text = restored.text;
      draft.attachments = restored.attachments;
    }
    draft.sending = false;
  };
}

async function attachPersistence(key: string, draft: Draft) {
  // Install the watcher before loading so an edit during startup wins over stored data.
  let edited = false;
  let restoring = false;
  watch(() => recoverable(draft), (record) => {
    if (restoring) return;
    edited = true;
    if (record) storedKeys.add(key); else storedKeys.delete(key);
    void writeLocal(key, { [DRAFTS]: record }).catch(storageError);
  }, { flush: "sync" });
  const stored = await readLocal(DRAFTS, key) as { text?: unknown; attachments?: Attachment[] } | undefined;
  if (edited || typeof stored?.text !== "string" || !Array.isArray(stored.attachments)) return;
  restoring = true;
  draft.text = stored.text;
  draft.attachments = stored.attachments;
  restoring = false;
}

export function draftFor(snap: Pick<OpenSnapshot, "workspace" | "sessionId">): Draft {
  const key = JSON.stringify([snap.workspace.root, snap.sessionId]);
  let draft = drafts.get(key);
  if (!draft) {
    const created = reactive<Draft>({ text: "", attachments: [], sending: false, loading: true });
    drafts.set(key, created);
    void attachPersistence(key, created).catch(storageError).finally(() => { created.loading = false; });
    draft = created;
  }
  return draft;
}

export type { Draft };
