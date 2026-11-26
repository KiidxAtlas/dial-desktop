import assert from 'node:assert/strict';
import test from 'node:test';
import { createServer } from 'vite';

test('a palette action names its terminal command from the first slash word only', async () => {
  const vite = await createServer({ configFile: false, optimizeDeps: { noDiscovery: true }, server: { middlewareMode: true, hmr: false, watch: null } });
  try {
    const { slashOf } = await vite.ssrLoadModule('/src/commands.ts');
    assert.equal(slashOf({ keywords: 'usage cost tokens /stats /usage /cost' }), '/stats');
    assert.equal(slashOf({ keywords: '/models download local' }), '/models');
    assert.equal(slashOf({ keywords: 'dirs add-dir /dirs' }), '/dirs');
    // A path or flag is not a command; no keywords means no hint.
    assert.equal(slashOf({ keywords: 'open a/b path --from-claude' }), null);
    assert.equal(slashOf({}), null);
  } finally {
    await vite.close();
  }
});
