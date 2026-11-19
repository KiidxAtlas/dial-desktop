// Markdown for answers, with code blocks highlighted by Shiki once its grammars load.

import MarkdownIt from "markdown-it";
import { ref } from "vue";
import { createHighlighterCore, type HighlighterCore } from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";

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
  highlighterVersion.value++;
}

function escape(text: string) {
  return text.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

/** Highlighted `<pre>` for `code`, or `null` when the language is unknown. */
export function highlight(code: string, lang: string): string | null {
  const name = lang.trim().toLowerCase();
  if (!highlighter || !name) return null;
  const loaded = highlighter.getLoadedLanguages();
  if (!loaded.includes(name)) return null;
  try {
    return highlighter.codeToHtml(code, { lang: name, themes: THEMES, defaultColor: false });
  } catch {
    return null;
  }
}

/** A code block with a header (`label`, default the language, and a copy button). */
export function codeBlock(code: string, lang: string, label = lang.trim() || "text"): string {
  const body = highlight(code, lang) ?? `<pre class="plain"><code>${escape(code)}</code></pre>`;
  return `<div class="code-block"><div class="code-head"><span>${escape(label)}</span><button type="button" class="copy" data-copy>Copy</button></div>${body}</div>`;
}

const md = new MarkdownIt({ html: false, linkify: true, breaks: false });
md.renderer.rules.fence = (tokens, idx) => {
  const token = tokens[idx];
  const lang = token.info.trim().split(/\s+/)[0] ?? "";
  return codeBlock(token.content.replace(/\n$/, ""), lang);
};

export function renderMarkdown(text: string): string {
  return md.render(text);
}
