// Drafts are stored locally, keyed by project and session. Never sync them to the engine.
import { reactive, watch } from "vue";
import type { Attachment, OpenSnapshot } from "./types";
import { showToast } from "./bridge";

interface Draft {
  text: string;
  attachments: Attachment[];
  sending: boolean;
}

const drafts = new Map<string, Draft>();
const storeName = "drafts";
let database: Promise<IDBDatabase> | undefined;
let warned = false;

function storageError() {
  if (warned) return;
  warned = true;
  showToast("Could not save or restore a local draft. Drafts still work while the app is open.");
}

function store() {
  return database ??= new Promise((resolve, reject) => {
    const request = indexedDB.open("dial-desktop-drafts", 1);
    request.onupgradeneeded = () => request.result.createObjectStore(storeName);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function attachPersistence(key: string, draft: Draft) {
  // Install the watcher before loading so an edit during startup wins over stored data.
  let edited = false;
  let restoring = false;
  const db = store();
  watch(() => [draft.text, [...draft.attachments]] as const, ([text, attachments]) => {
    if (restoring) return;
    edited = true;
    void db.then((database) => {
      const tx = database.transaction(storeName, "readwrite");
      tx.onabort = storageError;
      const records = tx.objectStore(storeName);
      if (text || attachments.length) records.put({ text, attachments: attachments.map((a) => ({ ...a })) }, key);
      else records.delete(key);
    }).catch(storageError);
  }, { flush: "sync" });
  const records = (await db).transaction(storeName).objectStore(storeName);
  const request = records.get(key);
  request.onerror = storageError;
  request.onsuccess = () => {
    if (edited || !request.result) return;
    const { text, attachments } = request.result;
    if (typeof text !== "string" || !Array.isArray(attachments)) return;
    restoring = true;
    draft.text = text;
    draft.attachments = attachments;
    restoring = false;
  };
}

export function draftFor(snap: Pick<OpenSnapshot, "workspace" | "sessionId">): Draft {
  const key = JSON.stringify([snap.workspace.root, snap.sessionId]);
  let draft = drafts.get(key);
  if (!draft) {
    draft = reactive({ text: "", attachments: [], sending: false });
    drafts.set(key, draft);
    void attachPersistence(key, draft).catch(storageError);
  }
  return draft;
}

export type { Draft };
