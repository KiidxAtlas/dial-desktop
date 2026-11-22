import assert from 'node:assert/strict';
import test from 'node:test';
import { createServer } from 'vite';

// Load the same TypeScript module and dependencies the desktop webview uses.
test('a closed rich fence renders in a live reply; a partial fence stays source', async () => {
  const vite = await createServer({
    configFile: false,
    optimizeDeps: { noDiscovery: true },
    server: { middlewareMode: true, hmr: false, watch: null },
  });
  try {
    const { renderMarkdown, renderMarkdownParts } = await vite.ssrLoadModule('/src/markdown.ts');
    const { appearance } = await vite.ssrLoadModule('/src/appearance.ts');
    for (const [language, body, marker] of [
      ['mermaid', 'flowchart LR\n  A --> B', 'class="reply-diagram"'],
      ['dial-app', '{"title":"Choices","kind":"compare","tabs":[{"label":"A","text":"One"},{"label":"B","text":"Two"}]}', 'class="reply-app"'],
    ]) {
      for (const fence of ['```', '~~~~']) {
        const partial = `${fence}${language}\n${body}\n`;
        assert.ok(!renderMarkdown(partial, true).includes(marker));
        assert.ok(!renderMarkdown(partial + fence.slice(1), true).includes(marker));
        const closed = partial + fence;
        assert.ok(renderMarkdown(closed, true).includes(marker), `${language} should render at its closing fence`);
        assert.equal(renderMarkdown(closed, true), renderMarkdown(closed));
        const parts = renderMarkdownParts(closed + '\n\nNext paragraph', true);
        assert.deepEqual(parts, [renderMarkdown(closed), '<p>Next paragraph</p>\n']);
        const pref = language === 'mermaid' ? 'diagrams' : 'cards';
        appearance[pref] = false;
        assert.ok(!renderMarkdown(closed, true).includes(marker));
        appearance[pref] = true;
      }
    }
  } finally {
    await vite.close();
  }
});
