// Drafts are stored locally, keyed by project and session. Never sync them to the engine.
import { reactive, watch } from "vue";
import type { Attachment, OpenSnapshot } from "./types";
import { showToast } from "./bridge";
import { DRAFTS, localKeys, readLocal, writeLocal } from "./localPrompts";
import { combinedInput, restoreInput, type Draft } from './draftInput';
export { beginSend } from './draftInput';

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
  const combined = combinedInput(p, draft);
  return draftRecord(combined.text, combined.attachments);
}

async function attachPersistence(key: string, draft: Draft) {
  // Read before the first write. Text typed or files prepared during startup belong
  // beside the restored draft, and must not replace it on disk before it has loaded.
  let stored: { text?: unknown; attachments?: Attachment[] } | undefined;
  try {
    stored = await readLocal(DRAFTS, key) as typeof stored;
  } catch { storageError(); }
  const edited = !!(draft.text || draft.attachments.length || draft.pending);
  if (typeof stored?.text === 'string' && Array.isArray(stored.attachments)) {
    restoreInput(draft, { text: stored.text, attachments: stored.attachments });
  }
  // Created after the async read: this watcher belongs to the cached draft, not
  // the component that first opened it. Batch changes to a pending send into one write.
  watch(() => recoverable(draft), (record) => {
    if (record) storedKeys.add(key); else storedKeys.delete(key);
    void writeLocal(key, { [DRAFTS]: record }).catch(storageError);
  }, { immediate: edited });
}

type DraftScope = Pick<OpenSnapshot, 'workspace' | 'sessionId' | 'startupDraftKey'>;
const starting = new Map<string, Draft>();

export function draftFor(snap: DraftScope): Draft {
  const startup = snap.startupDraftKey;
  const unbound = startup?.startsWith('startup:');
  const launch = JSON.stringify([snap.workspace.root, startup]);
  // A resume already knows its journal id, even while the worker is briefly in its initial session.
  const id = startup && !unbound ? startup : snap.sessionId;
  if (unbound && !id) {
    let draft = starting.get(launch);
    if (!draft) {
      draft = reactive<Draft>({ text: '', attachments: [], sending: false, loading: false, reading: 0 });
      starting.set(launch, draft);
    }
    return draft;
  }
  const key = JSON.stringify([snap.workspace.root, id]);
  let draft = drafts.get(key);
  if (!draft) {
    // Keep the object itself: a picker, paste or unacknowledged send may still own it.
    const created = (unbound && starting.get(launch)) || reactive<Draft>({ text: '', attachments: [], sending: false, loading: false, reading: 0 });
    created.loading = true;
    starting.delete(launch);
    drafts.set(key, created);
    void attachPersistence(key, created).catch(storageError).finally(() => { created.loading = false; });
    draft = created;
  } else if (unbound) {
    // A launch can reconnect to a journal whose draft is already cached.
    const earlier = starting.get(launch);
    if (earlier) {
      starting.delete(launch);
      const destination = draft;
      const move = () => restoreInput(destination, earlier);
      if (earlier.sending || earlier.reading) {
        const stop = watch(() => !earlier.sending && !earlier.reading, ready => { if (ready) { stop(); move(); } });
      } else move();
    }
  }
  return draft;
}

export type { Draft };
