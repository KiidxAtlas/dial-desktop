import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'vite';

// Exercise the real bridge and dialog plugin against a fake native IPC boundary. No window,
// executable, model or network download is needed. Middleware mode opens no server port.
test('a confirmed model selection survives helper setup, but never bypasses either consent', async t => {
  const server = await createServer({ configFile: false, appType: 'custom', server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  t.after(() => server.close());
  const { modelDownloads } = await server.ssrLoadModule('/src/bridge.ts');
  for (const [name, modelConsent, missing, helperConsent, starts, setups, running] of [
    ['ready', true, false, false, 1, 0, true],
    ['setup accepted', true, true, true, 2, 1, true],
    ['setup declined', true, true, false, 1, 1, false],
    ['model declined', false, true, true, 0, 0, false],
  ]) {
    await t.test(name, async s => {
      const selections = [];
      let prompts = 0, helpers = 0;
      const previous = globalThis.window;
      s.after(() => { if (previous === undefined) delete globalThis.window; else globalThis.window = previous; });
      globalThis.window = { __TAURI_INTERNALS__: { async invoke(cmd, args) {
        if (cmd === 'plugin:dialog|message') { prompts++; return modelConsent ? 'Yes' : 'No'; }
        if (cmd === 'model_hub_setup') { helpers++; return helperConsent; }
        assert.equal(cmd, 'model_download_start'); selections.push(args);
        return missing && !helpers ? null : { running: true };
      } } };
      const selection = { repo: 'org/model', file: 'nested/q4.gguf' };
      const result = await modelDownloads.start(selection.repo, '5 GB', selection.file);
      assert.equal(!!result?.running, running);
      assert.equal(prompts, 1); assert.equal(helpers, setups);
      assert.deepEqual(selections, Array(starts).fill(selection));
    });
  }
});
