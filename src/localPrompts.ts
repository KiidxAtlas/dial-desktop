// Drafts and follow-up queues are local to this app. Keep images in IndexedDB, not localStorage.
export const DRAFTS = "drafts";
export const QUEUES = "queues";
let database: Promise<IDBDatabase> | undefined;

function open() {
  return database ??= new Promise((resolve, reject) => {
    const request = indexedDB.open("dial-desktop-drafts", 2);
    request.onupgradeneeded = () => {
      for (const name of [DRAFTS, QUEUES]) {
        if (!request.result.objectStoreNames.contains(name)) request.result.createObjectStore(name);
      }
    };
    request.onsuccess = () => {
      const db = request.result;
      db.onversionchange = () => { db.close(); database = undefined; };
      resolve(db);
    };
    request.onerror = () => { database = undefined; reject(request.error); };
  });
}

export async function readLocal(store: string, key: string): Promise<unknown> {
  const db = await open();
  return new Promise((resolve, reject) => {
    const request = db.transaction(store).objectStore(store).get(key);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

/** Resolve after commit, not just after put(), so the UI never clears unsaved text. */
export async function writeLocal(key: string, records: Record<string, unknown>): Promise<void> {
  const db = await open();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(Object.keys(records), "readwrite");
    tx.oncomplete = () => resolve();
    tx.onabort = () => reject(tx.error ?? new Error("Local storage transaction aborted"));
    tx.onerror = () => reject(tx.error ?? new Error("Could not write local storage"));
    try {
      for (const [name, value] of Object.entries(records)) {
        const store = tx.objectStore(name);
        if (value === undefined) store.delete(key);
        else store.put(value, key);
      }
    } catch (error) {
      tx.abort();
      reject(error);
    }
  });
}
