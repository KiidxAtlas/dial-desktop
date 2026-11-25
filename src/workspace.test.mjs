import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'vite';
import { nextTick } from 'vue';

test('Back from a citation restores its panel, file selection and activating link, only in its session', async t => {
  const vite = await createServer({ configFile: false, optimizeDeps: { noDiscovery: true }, server: { middlewareMode: true, ws: false } });
  t.after(() => vite.close());
  const { showFile, canGoBackFromFile, goBackFromFile } = await vite.ssrLoadModule('/src/workspace.ts');
  const { ui } = await vite.ssrLoadModule('/src/ui.ts');
  const { snapshot } = await vite.ssrLoadModule('/src/bridge.ts');
  snapshot.value = { workspace: { root: '/project' }, sessionId: 'first' };
  Object.assign(ui, { inspectorTab: 'terminal', inspectorOpen: false, filePath: 'previous.md', fileLines: null });
  let focused = false;
  const link = { isConnected: true, focus() { focused = true; }, scrollIntoView() {} };
  const range = { start: 12, end: 13 };
  showFile('src/app.ts', range, link);
  showFile('notes.md', null, link);
  goBackFromFile();
  assert.deepEqual([ui.inspectorTab, ui.filePath, ui.fileLines], ['files', 'src/app.ts', range]);
  goBackFromFile();
  await nextTick();
  assert.deepEqual([ui.inspectorTab, ui.inspectorOpen, ui.filePath, ui.fileLines], ['terminal', false, 'previous.md', null]);
  assert.equal(focused, true);
  assert.equal(canGoBackFromFile(), false);
  showFile('src/app.ts', range, link);
  snapshot.value = { ...snapshot.value, sessionId: 'different' };
  assert.equal(canGoBackFromFile(), false);
});
