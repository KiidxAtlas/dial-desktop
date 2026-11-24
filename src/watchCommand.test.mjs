import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'vite';

test('/watch opens Activity from the slash menu or typed command, without launching a terminal', async t => {
  const vite = await createServer({ configFile: false, optimizeDeps: { noDiscovery: true }, server: { middlewareMode: true, ws: false } });
  t.after(() => vite.close());
  const { slashCommands } = await vite.ssrLoadModule('/src/slashCommands.ts');
  const { typedCommand, runTyped, terminalRequest } = await vite.ssrLoadModule('/src/commandRunner.ts');
  const { ui } = await vite.ssrLoadModule('/src/ui.ts');
  const snap = { workspace: { root: '/project' }, connection: { state: 'live' }, voice: { phase: 'off' }, entries: [], sessions: [] };
  const ctx = { snap, showApprovals() {}, showModes() {} };
  const opened = () => assert.deepEqual([ui.settingsOpen, ui.inspectorOpen, ui.inspectorTab, terminalRequest.value], [false, true, 'activity', null]);
  for (const surface of [undefined, 'client', 'terminal']) {
    const commands = surface ? [{ name: 'watch', kind: 'builtin', description: 'Watch', usage: '', rpc: { surface } }] : [];
    const items = slashCommands(ctx, commands).filter(item => item.name === '/watch');
    assert.equal(items.length, 1);
    Object.assign(ui, { settingsOpen: true, inspectorOpen: false, inspectorTab: 'plan' });
    items[0].run();
    opened();
    Object.assign(ui, { settingsOpen: true, inspectorOpen: false, inspectorTab: 'files' });
    const typed = typedCommand('/watch', commands);
    assert.deepEqual(typed, { name: 'watch', rest: '' });
    runTyped(snap, typed.name, typed.rest, commands);
    opened();
  }
});
