// Markdown for answers, with code blocks highlighted by Shiki once its grammars load.

import MarkdownIt from "markdown-it";
import type Token from "markdown-it/lib/token.mjs";
import { ref } from "vue";
import { createHighlighterCore, type HighlighterCore } from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import { diagramName } from "./diagram";
import { appearance } from "./appearance";

/** Code colours for each theme; the page picks one through the `--shiki-light`/`--shiki-dark` variables. */
export const THEMES = { dark: "vesper", light: "github-light" } as const;

/** Bumped when the highlighter becomes available, so rendered markdown refreshes. */
export const highlighterVersion = ref(0);
let highlighter: HighlighterCore | null = null;

export async function loadHighlighter() {
  highlighter = await createHighlighterCore({
    themes: [import("shiki/themes/vesper.mjs"), import("shiki/themes/github-light.mjs")],
    langs: [
      import("shiki/langs/rust.mjs"),
      import("shiki/langs/typescript.mjs"),
      import("shiki/langs/tsx.mjs"),
      import("shiki/langs/javascript.mjs"),
      import("shiki/langs/vue.mjs"),
      import("shiki/langs/json.mjs"),
      import("shiki/langs/toml.mjs"),
      import("shiki/langs/yaml.mjs"),
      import("shiki/langs/shellscript.mjs"),
      import("shiki/langs/python.mjs"),
      import("shiki/langs/go.mjs"),
      import("shiki/langs/c.mjs"),
      import("shiki/langs/cpp.mjs"),
      import("shiki/langs/java.mjs"),
      import("shiki/langs/swift.mjs"),
      import("shiki/langs/html.mjs"),
      import("shiki/langs/css.mjs"),
      import("shiki/langs/sql.mjs"),
      import("shiki/langs/diff.mjs"),
      import("shiki/langs/markdown.mjs"),
    ],
    // The JavaScript engine needs no WebAssembly, so the strict CSP stays as is.
    engine: createJavaScriptRegexEngine(),
  });
  highlighted.clear();
  highlighterVersion.value++;
}

function escape(text: string) {
  return text.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

/** Highlighted code by language and text. A streamed answer is re-rendered about once a frame, and
 * highlighting its finished code blocks again each time was the costliest part of that. */
const highlighted = new Map<string, string | null>();
const HIGHLIGHT_CACHE = 300;

/** Highlighted `<pre>` for `code`, or `null` when the language is unknown. */
export function highlight(code: string, lang: string): string | null {
  const name = lang.trim().toLowerCase();
  if (!highlighter || !name) return null;
  const key = `${name}\0${code}`;
  const cached = highlighted.get(key);
  if (cached !== undefined) {
    // Most recently used last, so the oldest goes first.
    highlighted.delete(key);
    highlighted.set(key, cached);
    return cached;
  }
  let html: string | null = null;
  if (highlighter.getLoadedLanguages().includes(name)) {
    try {
      html = highlighter.codeToHtml(code, { lang: name, themes: THEMES, defaultColor: false });
    } catch {
      html = null;
    }
  }
  highlighted.set(key, html);
  if (highlighted.size > HIGHLIGHT_CACHE) highlighted.delete(highlighted.keys().next().value!);
  return html;
}

/** A code block with a header (`label`, default the language, and a copy button). `plain` skips
 * highlighting, for a block that is still being written. */
export function codeBlock(code: string, lang: string, label = lang.trim() || "text", plain = false, lineAnchors = false): string {
  const body = (plain ? null : highlight(code, lang)) ?? `<pre class="plain"><code>${lineAnchors
    ? code.split("\n").map(line => `<span class="line">${escape(line)}</span>`).join("\n") : escape(code)}</code></pre>`;
  return `<div class="code-block"><div class="code-head"><span>${escape(label)}</span><button type="button" class="copy" data-copy>Copy</button></div>${body}</div>`;
}

const md = new MarkdownIt({ html: false, linkify: true, breaks: false });
const renderImage = md.renderer.rules.image!;
md.renderer.rules.image = (tokens, idx, options, env, renderer) => {
  const token = tokens[idx];
  const src = token.attrGet("src");
  // Relative raster images belong to the workspace and load through the app. The CSP blocks every
  // other image except inline data, so show those as a link or label instead of a broken icon.
  if (src && /^(?![\\/]|[a-z][a-z\d+.-]*:)[^?#]+\.(png|jpe?g|gif|webp)$/i.test(src)) {
    token.attrSet("data-workspace-image", src);
    token.attrs?.splice(token.attrIndex("src"), 1);
    return renderImage(tokens, idx, options, env, renderer);
  }
  if (src && /^data:image\/(png|jpeg|gif|webp);/i.test(src)) return renderImage(tokens, idx, options, env, renderer);
  const alt = renderer.renderInlineAsText(token.children ?? [], options, env) || src || "image";
  const label = escape(`Image: ${alt}`);
  return src && /^https?:/i.test(src) ? `<a href="${escape(src)}">${label}</a>` : `<span class="image-ref">${label}</span>`;
};
// markdown-it accepts EOF as an implicit closing fence. Its source map includes both
// delimiters only for an explicit close; token.content contains just the body lines.
function closedFence(token: Token) {
  if (!token.map) return false;
  const bodyLines = token.content ? token.content.replace(/\n$/, '').split('\n').length : 0;
  return token.map[1] - token.map[0] === bodyLines + 2;
}

md.renderer.rules.fence = (tokens, idx, _options, env: RenderEnv | undefined) => {
  const token = tokens[idx];
  const lang = token.info.trim().split(/\s+/)[0] ?? "";
  const growing = !!env?.streaming && !closedFence(token);
  if (lang.toLowerCase() === 'dial-app' && env?.cards && !growing && token.content.length <= 24000) {
    return `<div class="reply-app"><details open><summary>Interactive example source</summary>${codeBlock(token.content.trimEnd(), 'json')}</details></div>`;
  }
  if (lang.toLowerCase() === 'mermaid' && env?.diagrams && !growing && token.content.length <= 12000) {
    const name = diagramName(token.content);
    return `<figure class="reply-diagram"><figcaption>${name}</figcaption><details open><summary>${name} source</summary>${codeBlock(token.content.trimEnd(), 'mermaid')}</details></figure>`;
  }
  return codeBlock(token.content.replace(/\n$/, ""), lang, undefined, growing);
};

// Image files the answer names (`out.png`, `images/a sunset.png` in code, `./b.jpg` in prose)
// become links that open the image viewer; the click handler in `main.ts` takes `data-image-path`.
const IMAGE_CODE = /^(?:~|\.{0,2}\/)?(?:[^\s`<>"'|*?:]+\/)*[^\s/`<>"'|*?:][^/`<>"'|*?:]*\.(?:png|jpe?g|gif|webp)$/i;
const IMAGE_TEXT = /(^|[\s(["'])((?:\.{1,2}\/|\/)?(?:[\w@.+-]+\/)*[\w@+-][\w@.+-]*\.(?:png|jpe?g|gif|webp))(?=$|[\s)\]"',;:!?]|\.(?:\s|$))/gi;

function imageLink(path: string, inner: string) {
  return `<a href="${escape(encodeURI(path))}" class="image-path" data-image-path="${escape(path)}">${inner}</a>`;
}

function insideLink(tokens: { type: string }[], idx: number) {
  for (let i = idx - 1; i >= 0; i--) {
    if (tokens[i].type === "link_close") return false;
    if (tokens[i].type === "link_open") return true;
  }
  return false;
}

md.renderer.rules.code_inline = (tokens, idx) => {
  const code = tokens[idx].content;
  const html = `<code>${escape(code)}</code>`;
  return IMAGE_CODE.test(code.trim()) && !/^[a-z][a-z\d+.-]*:/i.test(code.trim()) && !insideLink(tokens, idx)
    ? imageLink(code.trim(), html)
    : html;
};
md.renderer.rules.text = (tokens, idx) => {
  const text = tokens[idx].content;
  if (insideLink(tokens, idx)) return escape(text);
  let out = "";
  let last = 0;
  for (const m of text.matchAll(IMAGE_TEXT)) {
    const start = m.index! + m[1].length;
    out += escape(text.slice(last, start)) + imageLink(m[2], escape(m[2]));
    last = start + m[2].length;
  }
  return out + escape(text.slice(last));
};

interface RenderEnv { streaming: boolean; cards: boolean; diagrams: boolean }

/** Keep top-level rich fences in separate DOM fragments. Later streamed prose can then update
 * without unmounting a card or cancelling a diagram's pending image decode. Parse the whole
 * document first so Markdown references still resolve across fragments. */
export function renderMarkdownParts(text: string, streaming = false): string[] {
  const env: RenderEnv = { streaming, cards: appearance.cards, diagrams: appearance.diagrams };
  const tokens = md.parse(text, env);
  const parts: string[] = [];
  let from = 0;
  const flush = (end: number) => {
    if (end > from) parts.push(md.renderer.render(tokens.slice(from, end), md.options, env));
    from = end;
  };
  for (const [i, token] of tokens.entries()) {
    if (token.level === 0 && token.type === 'fence' && /^(mermaid|dial-app)(\s|$)/i.test(token.info.trim())) {
      flush(i);
      flush(i + 1);
    }
  }
  flush(tokens.length);
  return parts;
}

export function renderMarkdown(text: string, streaming = false): string {
  return renderMarkdownParts(text, streaming).join('');
}
