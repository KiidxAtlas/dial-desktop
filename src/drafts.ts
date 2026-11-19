// Drafts are stored locally, keyed by project and session. Never sync them to the engine.
import { reactive, watch } from "vue";
import type { Attachment, OpenSnapshot } from "./types";
import { showToast } from "./bridge";
import { DRAFTS, readLocal, writeLocal } from "./localPrompts";

interface Draft {
  text: string;
  attachments: Attachment[];
  sending: boolean;
  loading: boolean;
}

const drafts = new Map<string, Draft>();
let warned = false;

function storageError() {
  if (warned) return;
  warned = true;
  showToast("Could not save or restore a local draft. Drafts still work while the app is open.");
}

export function draftRecord(text: string, attachments: readonly Attachment[]) {
  return text || attachments.length ? { text, attachments: attachments.map((a) => ({ ...a })) } : undefined;
}

async function attachPersistence(key: string, draft: Draft) {
  // Install the watcher before loading so an edit during startup wins over stored data.
  let edited = false;
  let restoring = false;
  watch(() => [draft.text, [...draft.attachments]] as const, ([text, attachments]) => {
    if (restoring) return;
    edited = true;
    void writeLocal(key, { [DRAFTS]: draftRecord(text, attachments) }).catch(storageError);
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
