import test from 'node:test';
import assert from 'node:assert/strict';
import { joinPath, projectFileLink, relativeTo, separatorOf } from './paths.ts';

const cases = [
  // root, relative, absolute
  ['/Users/me/app', 'src/main.ts', '/Users/me/app/src/main.ts'],
  ['/Users/me/app/', 'src/main.ts', '/Users/me/app/src/main.ts'],
  ['/', 'etc/config', '/etc/config'],
  [String.raw`/home/a\b`, String.raw`src/a\b`, String.raw`/home/a\b/src/a\b`],
  ['/home/a\\', '\\notes', '/home/a\\/\\notes'],
  ['C:\\Users\\me\\app', 'src/main.ts', 'C:\\Users\\me\\app\\src\\main.ts'],
  ['C:\\', 'a.txt', 'C:\\a.txt'],
  ['\\\\server\\share\\app', 'x/y', '\\\\server\\share\\app\\x\\y'],
];

test('join and strip round-trip on both platforms', () => {
  for (const [root, rel, abs] of cases) {
    assert.equal(joinPath(root, rel), abs, `${root} + ${rel}`);
    assert.equal(relativeTo(root, abs), rel, `${abs} - ${root}`);
  }
});

test('windows roots compare case-insensitively with either separator', () => {
  assert.equal(separatorOf('D:/work/app'), '\\');
  assert.equal(relativeTo('C:\\Users\\Me\\App', 'c:/users/me/app/src/a.ts'), 'src/a.ts');
});

test('paths outside the root or already relative are kept', () => {
  assert.equal(relativeTo('/Users/me/app', '/Users/me/apple/a.ts'), '/Users/me/apple/a.ts');
  assert.equal(relativeTo('/Users/me/app', 'src/a.ts'), 'src/a.ts');
  assert.equal(relativeTo('/Users/me/app', '/Users/me/app/a.ts'), 'a.ts');
  assert.equal(joinPath('/r', ''), '/r');
});

test('file citations accept Windows drive paths without admitting URL schemes', () => {
  const win = 'C:\\Work\\App';
  for (const [root, href, want] of [
    [win, 'c:%5Cwork%5Capp%5Csrc%5Ca%20b.ts#L3-L5', { path: 'src/a b.ts', lines: { start: 3, end: 5 } }],
    [win, 'C:/WORK/APP/img/a.png', { path: 'img/a.png', lines: null }],
    [win, String.raw`C:\Work\App\src\a.ts`, { path: 'src/a.ts', lines: null }],
    [win, 'D:/other/a.ts', { path: 'D:/other/a.ts', lines: null }], // never rebase outside paths into the project
    ['/app', '/app/a.ts#L3', { path: 'a.ts', lines: { start: 3, end: 3 } }],
    ['/app', './a%23b.ts?view=1', { path: 'a#b.ts', lines: null }],
    ['/app', 'a%5Cb.ts', { path: 'a\\b.ts', lines: null }],
    ['/app', 'draft%3A2.md', { path: 'draft:2.md', lines: null }],
    ...['#anchor', 'file:///tmp/a', 'data:text/plain,x', 'javascript:alert(1)', 'https://site', 'x://host/a', 'C:relative'].map(href => [win, href, null]),
  ]) assert.deepEqual(projectFileLink(root, href), want, href);
  assert.throws(() => projectFileLink(win, 'bad%escape'), URIError);
});
