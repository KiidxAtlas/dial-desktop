import test from 'node:test';
import assert from 'node:assert/strict';
import { findPathLinks } from './terminalLinks.ts';
import { relativeTo } from './paths.ts';

test('project paths in terminal lines become links', () => {
  const root = '/Users/me/app';
  const table = [
    ['error at src/main.ts:12:5 here', [['src/main.ts', 12, 'src/main.ts:12:5']]],
    ['  ./lib/a.rs', [['lib/a.rs', null, './lib/a.rs']]],
    ['/Users/me/app/docs/x.md:3', [['docs/x.md', 3, '/Users/me/app/docs/x.md:3']]],
    ['/etc/hosts.conf and ../up/a.ts', []],
    ['see https://example.com/a/b.js', []],
    ['plain words and file.txt', []],
    ['(src/a.vue) "b/c.json"', [['src/a.vue', null, 'src/a.vue'], ['b/c.json', null, 'b/c.json']]],
  ];
  for (const [line, want] of table) {
    const got = findPathLinks(line, (p) => relativeTo(root, p)).map((l) => [l.path, l.line, line.slice(l.start, l.end)]);
    assert.deepEqual(got, want, line);
  }
});

test('windows roots accept backslash paths inside the project', () => {
  const got = findPathLinks('C:\\work\\app\\src\\x.ts:4', (p) => relativeTo('C:\\work\\app', p));
  assert.deepEqual(got.map((l) => [l.path, l.line]), [['src/x.ts', 4]]);
  assert.deepEqual(findPathLinks('D:\\other\\x.ts', (p) => relativeTo('C:\\work\\app', p)), []);
});
