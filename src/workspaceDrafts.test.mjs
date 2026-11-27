import test from 'node:test';
import assert from 'node:assert/strict';
import { DraftStore } from './workspaceDrafts.ts';

test('DraftStore preserves edits and original baseSha256 across updates and cleans up when reverted', () => {
  const store = new DraftStore();
  store.update('/ws/a', 'src/main.ts', 'const x = 2;', 'const x = 1;', 'sha-orig');
  assert.equal(store.isDirty('/ws/a', 'src/main.ts'), true);
  assert.equal(store.get('/ws/a', 'src/main.ts')?.baseSha256, 'sha-orig');

  // Subsequent edits keep the original baseline text and SHA-256 even if a newer read passes different args.
  store.update('/ws/a', 'src/main.ts', 'const x = 3;', 'stale', 'sha-other');
  assert.equal(store.get('/ws/a', 'src/main.ts')?.text, 'const x = 3;');
  assert.equal(store.get('/ws/a', 'src/main.ts')?.baseText, 'const x = 1;');
  assert.equal(store.get('/ws/a', 'src/main.ts')?.baseSha256, 'sha-orig');

  // Reverting text back to original baseline drops the draft automatically.
  store.update('/ws/a', 'src/main.ts', 'const x = 1;', 'const x = 1;', 'sha-orig');
  assert.equal(store.isDirty('/ws/a', 'src/main.ts'), false);
  assert.equal(store.has('/ws/a', 'src/main.ts'), false);
});

test('DraftStore isolates workspaces and handles rename and clear', () => {
  const store = new DraftStore();
  store.update('/ws/a', 'README.md', 'edited a', 'orig a', 'sha-a');
  store.update('/ws/b', 'README.md', 'edited b', 'orig b', 'sha-b');
  assert.deepEqual(store.dirtyPaths('/ws/a'), ['README.md']);
  assert.deepEqual(store.dirtyPaths('/ws/b'), ['README.md']);
  assert.equal(store.count('/ws/a'), 1);

  store.rename('/ws/a', 'README.md', 'DOCS.md');
  assert.equal(store.has('/ws/a', 'README.md'), false);
  assert.equal(store.get('/ws/a', 'DOCS.md')?.text, 'edited a');
  assert.equal(store.get('/ws/b', 'README.md')?.text, 'edited b');

  assert.equal(store.clear('/ws/a', 'DOCS.md'), true);
  assert.equal(store.count('/ws/a'), 0);
  assert.equal(store.count('/ws/b'), 1);
});
