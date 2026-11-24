import assert from 'node:assert/strict';
import test from 'node:test';
import { createServer } from 'vite';

test('the agent sees an open app, its console, and the outcome of scripts run inside it', async () => {
  const vite = await createServer({ configFile: false, optimizeDeps: { noDiscovery: true }, server: { middlewareMode: true, hmr: false, watch: null } });
  try {
    const { attach, update, log, settle, host } = await vite.ssrLoadModule('/src/typeHost.ts');
    assert.deepEqual(JSON.parse(host.status('cook')), { open: false });
    const posted = [];
    const detach = attach('cook', () => null);
    assert.match(host.run('cook', 'return 1'), /not open/);
    update('cook', { version: 'a1', loaded: true, post: (m) => posted.push(m) });
    log('cook', { level: 'error', text: 'boom' });
    assert.deepEqual(JSON.parse(host.logs('cook')), [{ level: 'error', text: 'boom' }]);

    const { key } = JSON.parse(host.run('cook', 'return 2'));
    assert.deepEqual(posted, [{ kind: 'eval', id: key, script: 'return 2' }]);
    assert.deepEqual(JSON.parse(host.result(key)), { pending: true });
    log('cook', { level: 'log', text: 'clicked' });
    settle(key, { ok: true, value: 2 });
    assert.deepEqual(JSON.parse(host.result(key)), { ok: true, value: 2, logs: [{ level: 'log', text: 'clicked' }] });
    assert.match(host.result(key), /lost/);

    // A reload fails a pending script at once and starts a fresh console.
    const second = JSON.parse(host.run('cook', 'await new Promise(() => {})')).key;
    update('cook', { loaded: false, version: 'b2' });
    assert.equal(JSON.parse(host.result(second)).ok, false);
    assert.deepEqual(JSON.parse(host.logs('cook')), []);
    detach();
    assert.deepEqual(JSON.parse(host.status('cook')), { open: false });
  } finally {
    await vite.close();
  }
});
