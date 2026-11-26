import test from 'node:test';
import assert from 'node:assert/strict';
import { changeRows, gitMarkers, gitStatusOf, otherNestedChanges } from './workspaceGit.ts';

const files = [
  { path: 'src/a.ts', status: 'M' },
  { path: 'src/new/', status: 'U' },
  { path: 'README.md', status: 'D' },
];

test('tree markers cover files, untracked folders and their parents', () => {
  const marker = gitMarkers(files);
  const table = [
    ['src/a.ts', false, 'M'],
    ['README.md', false, 'D'],
    ['src/new', true, 'U'],
    ['src/new/deep/x.ts', false, 'U'],
    ['src', true, '•'],
    ['src/b.ts', false, null],
    ['docs', true, null],
    ['src/newer.ts', false, null],
  ];
  for (const [path, dir, text] of table) assert.equal(marker(path, dir)?.text ?? null, text, path);
});

test('status lookup is exact', () => {
  assert.equal(gitStatusOf(files, 'src/a.ts')?.status, 'M');
  assert.equal(gitStatusOf(files, 'src'), null);
});

test('change rows list files that still differ first; settled only with git status', () => {
  const items = ['src/a.ts', 'docs/gone.md', 'src/c.ts'];
  const status = [{ path: 'src/a.ts', status: 'M' }, { path: 'src/c.ts', status: 'U' }];
  const rows = changeRows(items, (p) => p, status, true);
  assert.deepEqual(rows.map((r) => [r.item, r.settled, r.git?.status ?? null]), [
    ['src/a.ts', false, 'M'],
    ['src/c.ts', false, 'U'],
    ['docs/gone.md', true, null],
  ]);
  // Not a repository (or status unknown): keep session order, nothing settled.
  assert.deepEqual(changeRows(items, (p) => p, [], false).map((r) => [r.item, r.settled]), items.map((p) => [p, false]));
});

test('nested changes are visible without a root repository and not duplicated as session edits', () => {
  const repos = ['pkg', 'pkg/sub'];
  const paths = ['pkg/file', 'pkg/sub/file', 'pkg/sub', 'pkg2/file'];
  const files = paths.map(path => ({ path, status: 'M' }));
  assert.deepEqual(otherNestedChanges(files, repos, ['pkg/file']).map(f => f.path), ['pkg/sub/file']);
  assert.deepEqual(changeRows(['pkg/clean', 'pkg2/clean'], p => p, files, false, repos).map(r => [r.item, r.settled]),
    [['pkg2/clean', false], ['pkg/clean', true]]);
});

test('stage state and count follow the index flags', async () => {
  const { stageState, stagedCount } = await import('./workspaceGit.ts');
  const table = [
    [null, false],
    [{ path: 'a', status: 'M', unstaged: true }, false],
    [{ path: 'a', status: 'M', staged: true }, true],
    [{ path: 'a', status: 'M', staged: true, unstaged: true }, 'mixed'],
  ];
  for (const [git, want] of table) assert.equal(stageState(git), want, JSON.stringify(git));
  assert.equal(stagedCount(table.map(([git]) => git).filter(Boolean)), 2);
});
