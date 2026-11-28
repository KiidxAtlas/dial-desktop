import assert from 'node:assert/strict';
import test from 'node:test';
import { createServer } from 'vite';
import vue from '@vitejs/plugin-vue';
import { createSSRApp, h } from 'vue';
import { renderToString } from 'vue/server-renderer';

test('reported reasoning stays visible across a tool call and final answer unless folded', async t => {
  const vite = await createServer({ configFile: false, plugins: [vue()], server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  t.after(() => vite.close());
  const { default: Timeline } = await vite.ssrLoadModule('/src/components/Timeline.vue');
  const thought = { kind: 'assistant', text: '', reasoning: 'Compare the two versions.', streaming: true, abandoned: false };
  const tool = { kind: 'tool', key: 'r', label: 'Read', target: 'versions.txt', state: 'succeeded', changedPaths: [], images: [], facts: [] };
  const answer = { ...thought, text: 'The versions match.', reasoning: '', streaming: false };
  const render = (entries, showThinking, turnRunning) => renderToString(createSSRApp({ render: () => h(Timeline, { snap: {
    entries, showThinking, turnRunning, workspace: { root: '/fixture' }, sessionId: 's', connection: { state: 'preview' },
    voice: { reading: false }, approvals: [], question: null, stats: { running: 0 },
  } }) }));
  for (const [entries, running] of [[[{ ...thought, streaming: false }, tool], true], [[thought], true], [[{ ...thought, streaming: false }, tool, answer], false]]) {
    assert.equal((await render(entries, true, running)).includes(thought.reasoning), true, 'on: reasoning remains readable');
    assert.equal((await render(entries, false, running)).includes(thought.reasoning), false, 'off: only the disclosure is shown');
  }
  assert.doesNotMatch(await render([tool, answer], true, false), /class="thinking"/, 'no fabricated reasoning for a provider that sends none');
});

test('local models default reasoning to collapsed with a concise preview unless showThinking is off', async t => {
  const vite = await createServer({ configFile: false, plugins: [vue()], server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  t.after(() => vite.close());
  const { default: Timeline } = await vite.ssrLoadModule('/src/components/Timeline.vue');
  const thought = { kind: 'assistant', text: 'Done.', reasoning: 'First line.\nCheck Cargo.toml.', streaming: false, abandoned: false };
  const render = (showThinking) => renderToString(createSSRApp({ render: () => h(Timeline, { snap: {
    entries: [thought], showThinking, turnRunning: false, workspace: { root: '/fixture' }, sessionId: 's', connection: { state: 'preview' },
    model: { provider: 'mlx', modelId: 'local-qwen', label: 'Local Qwen', local: true, thinking: null, defaultThinking: null, levels: [], thinkingSettable: false, acceptsImages: null },
    voice: { reading: false }, approvals: [], question: null, stats: { running: 0 },
  } }) }));
  const onHtml = await render(true);
  assert.doesNotMatch(onHtml, /class="thought"/, 'local reasoning stays collapsed by default');
  assert.match(onHtml, /Check Cargo\.toml\./, 'collapsed local reasoning shows a concise preview');
  const offHtml = await render(false);
  assert.doesNotMatch(offHtml, /Check Cargo\.toml\./, 'showThinking=false hides preview text too');
});
