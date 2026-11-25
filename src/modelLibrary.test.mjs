import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'vite';
import vue from '@vitejs/plugin-vue';
import { createSSRApp, h } from 'vue';
import { renderToString } from 'vue/server-renderer';
import { downloadProgress } from './modelDownloadProgress.ts';

test('model download meter uses only measured values and keeps wire bytes separate from selection size', () => {
  const start = { running: true, expectedBytes: 1e9, percent: null, completedFiles: null, totalFiles: null, receivedBytes: null, completedBytes: null, totalBytes: null };
  for (const [update, value, max, detail] of [
    [{}, undefined, 100, /Waiting for the download helper/],
    [{ receivedBytes: 0 }, undefined, 100, /^0 B received$/],
    [{ completedBytes: 100e6, receivedBytes: 60e6 }, undefined, 100, /^100 MB processed · 60 MB received$/],
    [{ percent: 40, completedBytes: 500e6, totalBytes: 1.25e9 }, 40, 100, /^40% · 500 MB of 1.25 GB processed$/],
    [{ completedFiles: 2, totalFiles: 12, receivedBytes: 60e6 }, 2, 12, /^60 MB received · 2 of 12 files ready$/],
  ]) {
    const progress = downloadProgress({ ...start, ...update });
    assert.equal(progress.value, value);
    assert.equal(progress.max, max);
    assert.match(progress.detail, detail);
  }
  assert.equal(downloadProgress({ ...start, running: false, percent: 80, receivedBytes: 670e6 }), null);
});

test('local suggestions and default Hub format follow the native host capability', async t => {
  const server = await createServer({ configFile: false, plugins: [vue()], server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  t.after(() => server.close());
  const { snapshot } = await server.ssrLoadModule('/src/bridge.ts');
  const { default: Library } = await server.ssrLoadModule('/src/components/ModelLibrary.vue');
  for (const [platform, appleSilicon] of [['macos', true], ['macos', false], ['windows', false], ['linux', false]]) {
    snapshot.value = { platform, appleSilicon };
    const html = await renderToString(createSSRApp({ render: () => h(Library, { preview: true }) }));
    assert.equal(html.includes('Inkling 2 Mini IQ2_M'), appleSilicon, platform);
    assert.equal(html.includes('mlx-community/'), appleSilicon, platform);
    assert.match(html, /SmolLM2 1.7B Instruct/);
    assert.match(html, new RegExp(`aria-checked="true"[^>]*>${appleSilicon ? 'MLX' : 'GGUF'}</button>`));
  }
});

// Exercise the real bridge and dialog plugin against a fake native IPC boundary. No window,
// executable, model or network download is needed. Middleware mode opens no server port.
function fakeIpc(t, invoke) {
  const previous = globalThis.window;
  t.after(() => { if (previous === undefined) delete globalThis.window; else globalThis.window = previous; });
  globalThis.window = { __TAURI_INTERNALS__: { invoke } };
}

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
      fakeIpc(s, async (cmd, args) => {
        if (cmd === 'plugin:dialog|message') { prompts++; return modelConsent ? 'Yes' : 'No'; }
        if (cmd === 'model_hub_setup') { helpers++; return helperConsent; }
        assert.equal(cmd, 'model_download_start'); selections.push(args);
        return missing && !helpers ? null : { running: true };
      });
      const selection = { repo: 'org/model', file: 'nested/q4.gguf', expectedBytes: 5e9 };
      const result = await modelDownloads.start(selection.repo, '5 GB', selection.expectedBytes, selection.file);
      assert.equal(!!result?.running, running);
      assert.equal(prompts, 1); assert.equal(helpers, setups);
      assert.deepEqual(selections, Array(starts).fill(selection));
    });
  }
});

test('file removal requires consent and only sends the selected snapshot entry to native', async t => {
  const server = await createServer({ configFile: false, appType: 'custom', server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  t.after(() => server.close());
  const { modelDownloads } = await server.ssrLoadModule('/src/bridge.ts');
  for (const consent of [false, true]) {
    await t.test(consent ? 'confirmed' : 'declined', async s => {
      const selected = { repo: 'org/m', revision: 'abc', file: 'nested/q4.gguf' };
      const calls = [];
      fakeIpc(s, async (cmd, args) => {
        if (cmd === 'plugin:dialog|message') return consent ? 'Remove file' : 'No';
        calls.push([cmd, args]);
      });
      const removed = await modelDownloads.removeFile(selected.repo, { revision: selected.revision, path: selected.file, bytes: 42 });
      assert.equal(removed, consent);
      assert.deepEqual(calls, consent ? [['model_cache_remove_file', selected]] : []);
    });
  }
});
