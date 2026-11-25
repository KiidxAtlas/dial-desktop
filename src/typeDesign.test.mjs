import assert from 'node:assert/strict';
import test from 'node:test';
import { createServer } from 'vite';

test('build steps accrue only for the matching type and never invent progress', async () => {
  const vite = await createServer({ configFile: false, optimizeDeps: { noDiscovery: true }, server: { middlewareMode: true, hmr: false, watch: null } });
  try {
    const { applyTypeChange, typeState, types } = await vite.ssrLoadModule('/src/savedTypes.ts');
    const saves = typeState.firstSaves;
    saves.set('book', {});
    applyTypeChange(null);
    applyTypeChange('book'); // A list or item refresh is not a build step.
    applyTypeChange({ id: 'other', choices: { fields: ['Name'] } });
    assert.deepEqual(saves.get('book'), {});
    assert.equal(saves.has('other'), false);
    applyTypeChange({ id: 'book', choices: { fields: ['Title', 'Ingredients'] } });
    applyTypeChange({ id: 'book', choices: { saved: 1 } });
    applyTypeChange({ id: 'book', choices: { app: 14 } });
    applyTypeChange({ id: 'book', choices: { checked: true } });
    assert.deepEqual(saves.get('book'), { fields: ['Title', 'Ingredients'], saved: 1, app: 14, checked: true });
    assert.equal(types.revision.book, 5);
    saves.delete('book');
    applyTypeChange({ id: 'book', choices: { app: 15 } });
    assert.equal(saves.size, 0);
  } finally {
    await vite.close();
  }
});

test('answers name the local images they show', async () => {
  const vite = await createServer({ configFile: false, optimizeDeps: { noDiscovery: true }, server: { middlewareMode: true, hmr: false, watch: null } });
  try {
    const { markdownImages } = await vite.ssrLoadModule('/src/typeExtract.ts');
    const text = '![a](/tmp/a.png) ![b](./out/b%20c.jpg) ![w](https://x/y.png) ![a](/tmp/a.png)';
    assert.deepEqual(markdownImages(text, '/proj/'), ['/tmp/a.png', '/proj/out/b c.jpg']);
  } finally {
    await vite.close();
  }
});

test('failed Types lookup and stale workspace replies never hide ordinary sessions', async t => {
  const vite = await createServer({ configFile: false, optimizeDeps: { noDiscovery: true }, server: { middlewareMode: true, ws: false } });
  t.after(() => vite.close());
  const { typeApi, types, refreshTypes, followTypeSession, isTypeSession } = await vite.ssrLoadModule('/src/savedTypes.ts');
  const { sessionGroups } = await vite.ssrLoadModule('/src/sessionActions.ts');
  t.mock.method(typeApi, 'list', async () => { throw Error('offline'); });
  t.mock.method(typeApi, 'location', async () => ({}));
  const previous = globalThis.isTauri;
  globalThis.isTauri = true;
  t.after(() => { if (previous === undefined) delete globalThis.isTauri; else globalThis.isTauri = previous; });
  const rows = ['ordinary', 'classified'].map(id => ({ id, title: id, empty: false }));
  const groups = () => sessionGroups({ sessions: rows, workspace: { root: 'project' } }).flatMap(g => g.sessions.map(s => s.id));
  await refreshTypes();
  assert.equal(types.listError, true);
  assert.deepEqual(groups(), ['ordinary', 'classified']);
  assert.equal(isTypeSession('ordinary'), false);
  typeApi.list.mock.mockImplementation(async () => [{ sessions: ['classified'] }]);
  await refreshTypes();
  assert.equal(types.listError, false);
  assert.deepEqual(groups(), ['ordinary']);
  let resolveStale;
  typeApi.list.mock.mockImplementationOnce(() => new Promise(resolve => { resolveStale = resolve; }));
  const stale = refreshTypes();
  typeApi.list.mock.mockImplementation(async () => []);
  followTypeSession('next', null, 'project', null);
  resolveStale([{ sessions: ['ordinary'] }]);
  await stale;
  assert.deepEqual(groups(), ['ordinary', 'classified']);
});
