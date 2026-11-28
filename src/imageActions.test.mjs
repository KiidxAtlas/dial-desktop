import assert from 'node:assert/strict';
import test from 'node:test';
import { createServer } from 'vite';

const png = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aX1kAAAAASUVORK5CYII=';

test('Copy image writes the displayed PNG even when connect-src blocks data URLs', async t => {
  const vite = await createServer({ configFile: false, server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  t.after(() => vite.close());
  const { imageMenuItems } = await vite.ssrLoadModule('/src/imageActions.ts');
  t.mock.method(globalThis, 'fetch', async () => { throw new TypeError('Blocked by connect-src'); });
  let payload;
  globalThis.ClipboardItem = class { constructor(data) { this.data = data; } };
  navigator.clipboard = { write([item]) { payload = item.data['image/png']; return payload.then(() => {}); } };
  t.after(() => { delete globalThis.ClipboardItem; delete navigator.clipboard; });

  const actions = imageMenuItems({ name: 'preview.png', url: `data:image/png;base64,${png}` });
  actions.find(item => item.label === 'Copy image').run();
  assert.ok(payload, 'clipboard write starts during the click');
  const blob = await payload;
  assert.equal(blob.type, 'image/png');
  assert.deepEqual(Buffer.from(await blob.arrayBuffer()), Buffer.from(png, 'base64'));
});
