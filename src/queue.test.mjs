import assert from 'node:assert/strict';
import test from 'node:test';
import { createServer } from 'vite';


// Minimal in-memory IndexedDB shim so queue persistence works in plain Node tests.
function installMemoryIdb() {
  const stores = new Map([['drafts', new Map()], ['queues', new Map()]]);
  const db = {
    objectStoreNames: { contains: (n) => stores.has(n) },
    createObjectStore: (n) => { if (!stores.has(n)) stores.set(n, new Map()); },
    close: () => {},
    transaction(names) {
      const tx = { oncomplete: null, onabort: null, onerror: null, abort() {} };
      tx.objectStore = (name) => {
        const map = stores.get(name) ?? new Map();
        return {
          get(key) {
            const req = { result: map.get(key), onsuccess: null, onerror: null };
            queueMicrotask(() => req.onsuccess?.());
            return req;
          },
          getAllKeys() {
            const req = { result: [...map.keys()], onsuccess: null, onerror: null };
            queueMicrotask(() => req.onsuccess?.());
            return req;
          },
          put(value, key) { map.set(key, value); },
          delete(key) { map.delete(key); },
        };
      };
      queueMicrotask(() => tx.oncomplete?.());
      return tx;
    },
  };
  globalThis.indexedDB = {
    open() {
      const req = { result: db, onupgradeneeded: null, onsuccess: null, onerror: null };
      queueMicrotask(() => { req.onupgradeneeded?.(); req.onsuccess?.(); });
      return req;
    },
  };
}
installMemoryIdb();
function snap(over = {}) {
  return {
    workspace: { root: '/proj', name: 'proj', display: 'proj' },
    sessionId: 's1',
    connection: { state: 'live', model: 'm', engineVersion: '0.4.2' },
    turnRunning: false,
    canSend: true,
    historyLoading: false,
    approvals: [],
    question: null,
    entries: [{ kind: 'user', text: 'initial', images: 0 }],
    sessions: [],
    ...over,
  };
}

test('queued follow-up sends automatically when turn finishes, even if canSend settles a frame later', async t => {
  const vite = await createServer({ configFile: false, optimizeDeps: { noDiscovery: true }, server: { middlewareMode: true, ws: false } });
  t.after(() => vite.close());
  const bridge = await vite.ssrLoadModule('/src/bridge.ts');
  const queueMod = await vite.ssrLoadModule('/src/queue.ts');

  const sent = [];
  const origSend = bridge.engine.send;
  bridge.engine.send = async (text, attachments, scope) => {
    sent.push({ text, attachments, scope });
    return true;
  };
  t.after(() => { bridge.engine.send = origSend; });

  // 1. Running turn, queue two prompts.
  const running = snap({ turnRunning: true, canSend: false });
  bridge.snapshot.value = running;
  queueMod.syncQueue(running);
  const q = queueMod.queueFor(running);
  q.loading = false;
  assert.equal(await queueMod.enqueuePrompt(running, 'first follow-up', []), true);
  assert.equal(await queueMod.enqueuePrompt(running, 'second follow-up', []), true);
  assert.equal(q.items.length, 2);

  // 2. Turn ends, but canSend is transiently false (e.g. deferred apply_settings).
  const settling = snap({ turnRunning: false, canSend: false });
  bridge.snapshot.value = settling;
  queueMod.syncQueue(settling);
  await new Promise(r => setTimeout(r, 10));
  assert.equal(sent.length, 0, 'must wait until canSend settles');
  assert.equal(q.paused, false, 'must not permanently pause while settling');
  assert.equal(q.pendingDispatch, true);

  // 3. Next snapshot: canSend becomes true -> dispatches first queued prompt automatically.
  const ready = snap({ turnRunning: false, canSend: true });
  bridge.snapshot.value = ready;
  queueMod.syncQueue(ready);
  await new Promise(r => setTimeout(r, 10));
  assert.equal(sent.length, 1);
  assert.equal(sent[0].text, 'first follow-up');
  assert.equal(q.items.length, 1);

  // 4. Second turn runs and finishes -> dispatches second queued prompt automatically.
  const running2 = snap({ turnRunning: true, canSend: false });
  bridge.snapshot.value = running2;
  queueMod.syncQueue(running2);
  bridge.snapshot.value = ready;
  queueMod.syncQueue(ready);
  await new Promise(r => setTimeout(r, 10));
  assert.equal(sent.length, 2);
  assert.equal(sent[1].text, 'second follow-up');
  assert.equal(q.items.length, 0);
});

test('queued follow-up resumes if user returns while turn still runs, but pauses on abort or error', async t => {
  const vite = await createServer({ configFile: false, optimizeDeps: { noDiscovery: true }, server: { middlewareMode: true, ws: false } });
  t.after(() => vite.close());
  const bridge = await vite.ssrLoadModule('/src/bridge.ts');
  const queueMod = await vite.ssrLoadModule('/src/queue.ts');

  const sent = [];
  const origSend = bridge.engine.send;
  bridge.engine.send = async (text) => { sent.push(text); return true; };
  t.after(() => { bridge.engine.send = origSend; });

  // Switch away and back while turn still runs -> still dispatches when turn completes.
  const runningA = snap({ sessionId: 'sa', turnRunning: true, canSend: false });
  bridge.snapshot.value = runningA;
  queueMod.syncQueue(runningA);
  const qA = queueMod.queueFor(runningA);
  qA.loading = false;
  assert.equal(await queueMod.enqueuePrompt(runningA, 'after glance', []), true);

  const other = snap({ sessionId: 'sb', turnRunning: false, canSend: true });
  bridge.snapshot.value = other;
  queueMod.syncQueue(other);
  assert.equal(qA.paused, true);

  bridge.snapshot.value = runningA;
  queueMod.syncQueue(runningA);
  assert.equal(qA.paused, false, 'returning while turn still runs resumes auto-dispatch');

  const doneA = snap({ sessionId: 'sa', turnRunning: false, canSend: true });
  bridge.snapshot.value = doneA;
  queueMod.syncQueue(doneA);
  await new Promise(r => setTimeout(r, 10));
  assert.deepEqual(sent, ['after glance']);

  // Aborted turn pauses queue and does not send.
  const runningErr = snap({ sessionId: 'sc', turnRunning: true, canSend: false });
  bridge.snapshot.value = runningErr;
  queueMod.syncQueue(runningErr);
  const qC = queueMod.queueFor(runningErr);
  qC.loading = false;
  assert.equal(await queueMod.enqueuePrompt(runningErr, 'do not auto send', []), true);
  queueMod.markTurnAborted(runningErr);
  const doneC = snap({ sessionId: 'sc', turnRunning: false, canSend: true });
  bridge.snapshot.value = doneC;
  queueMod.syncQueue(doneC);
  await new Promise(r => setTimeout(r, 10));
  assert.deepEqual(sent, ['after glance']);
  assert.equal(qC.paused, true);

  // Failed turn (ends in error entry -> canRetry) pauses queue and does not send.
  const runningFail = snap({ sessionId: 'sd', turnRunning: true, canSend: false });
  bridge.snapshot.value = runningFail;
  queueMod.syncQueue(runningFail);
  const qD = queueMod.queueFor(runningFail);
  qD.loading = false;
  assert.equal(await queueMod.enqueuePrompt(runningFail, 'after error', []), true);
  const failedD = snap({
    sessionId: 'sd',
    turnRunning: false,
    canSend: true,
    entries: [{ kind: 'user', text: 'hi', images: 0 }, { kind: 'error', message: 'boom' }],
  });
  bridge.snapshot.value = failedD;
  queueMod.syncQueue(failedD);
  await new Promise(r => setTimeout(r, 10));
  assert.deepEqual(sent, ['after glance']);
  assert.equal(qD.paused, true);
});
