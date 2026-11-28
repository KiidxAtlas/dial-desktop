import assert from 'node:assert/strict';
import test from 'node:test';
import { shallowRef } from 'vue';
import { createServer } from 'vite';

test('startup input stays with its launch; only a ready destination can receive it', async t => {
  const vite = await createServer({ configFile: false, optimizeDeps: { noDiscovery: true }, server: { middlewareMode: true, ws: false } });
  t.after(() => vite.close());
  const { draftFor, beginSend } = await vite.ssrLoadModule('/src/drafts.ts');
  const { startupDestination } = await vite.ssrLoadModule('/src/startupPrompt.ts');
  const start = { workspace: { root: 'fixture' }, startupDraftKey: 'startup:1', sessionId: null, connection: { state: 'starting' }, canSend: false, approvals: [] };
  const live = { ...start, sessionId: 'journal', connection: { state: 'live' }, canSend: true };
  const draft = draftFor(start);
  draft.text = 'first';
  const finish = beginSend(draft);
  draft.text = 'second';
  assert.notEqual(draftFor({ ...start, startupDraftKey: 'startup:2' }), draft);
  assert.equal(draftFor(live), draft);
  finish(false);
  assert.equal(draftFor({ ...live, startupDraftKey: null }).text, 'first\n\nsecond');
  for (const interrupt of [null, { workspace: { root: 'another' } }, { startupDraftKey: 'startup:2' }, { connection: { state: 'failed' } }, { turnRunning: true }]) {
    const current = shallowRef(start);
    const pending = startupDestination(start, () => current.value, new AbortController().signal);
    current.value = { ...live, historyLoading: true };
    if (interrupt) current.value = { ...live, ...interrupt };
    current.value = live;
    assert.equal(await pending, interrupt ? null : 'journal');
  }
  const abort = new AbortController();
  const pending = startupDestination(start, () => start, abort.signal);
  abort.abort();
  assert.equal(await pending, null);
});
