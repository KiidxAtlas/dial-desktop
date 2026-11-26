import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'vite';

test('quoted arguments preserve drive and UNC paths and still support escaped quotes', async t => {
  const vite = await createServer({ configFile: false, optimizeDeps: { noDiscovery: true }, server: { middlewareMode: true, ws: false } });
  t.after(() => vite.close());
  const { words } = await vite.ssrLoadModule('/src/commandRunner.ts');
  for (const [text, expected] of [
    [String.raw`move "C:\My Projects\dial" --flag`, ['move', String.raw`C:\My Projects\dial`, '--flag']],
    [String.raw`"\\server\share name\file.txt"`, [String.raw`\\server\share name\file.txt`]],
    [String.raw`"D:\work\\" next`, ['D:\\work\\', 'next']],
    [String.raw`set "say \"hello\"" ''`, ['set', 'say "hello"', '']],
    [String.raw`'C:\app dir' /etc/file`, [String.raw`C:\app dir`, '/etc/file']],
  ]) assert.deepEqual(words(text), expected, text);
});
