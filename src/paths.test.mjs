import test from 'node:test';
import assert from 'node:assert/strict';
import { joinPath, relativeTo, separatorOf } from './paths.ts';

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
