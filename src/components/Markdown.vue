<script setup lang="ts">
import { computed, getCurrentInstance, h, ref, render, watch } from "vue";
import { files, type ImageFile } from "../bridge";
import { highlighterVersion, renderMarkdown } from "../markdown";
import { parseChatApp } from "../chatApp";
import { diagramName, renderDiagram, shownTheme } from "../diagram";
import ChatApp from "./ChatApp.vue";
import { viewAttachment } from "../imageViewer";
import { trackOverflow } from "../scrollFade";
/** `nested`: Markdown inside an interactive card, which does not mount cards of its own. */
const props = defineProps<{ text: string; workspaceRoot: string; streaming?: boolean; nested?: boolean }>();
const appContext = getCurrentInstance()?.appContext ?? null;
const prose = ref<HTMLDivElement | null>(null);
const html = computed(() => {
  // Re-render once code highlighting is available.
  void highlighterVersion.value;
  return renderMarkdown(props.text, props.streaming);
});

// Preserve in-flight reads while a reply streams, but drop images removed from the message.
let cacheRoot = "";
let previews = new Map<string, Promise<ImageFile>>();
watch([html, prose, () => props.workspaceRoot], ([, el, root], _, cleanup) => {
  if (!el) return;
  const previous = root === cacheRoot ? previews : new Map<string, Promise<ImageFile>>();
  previews = new Map();
  cacheRoot = root;
  let current = true;
  cleanup(() => { current = false; });
  const stops: (() => void)[] = [];
  cleanup(() => { for (const stop of stops) stop(); });
  const mounts: HTMLElement[] = [];
  for (const details of props.nested ? [] : el.querySelectorAll<HTMLDetailsElement>('.reply-app > details')) {
    const app = parseChatApp(details.querySelector('pre')?.textContent ?? '');
    if (!app) {
      details.querySelector('summary')!.textContent = 'Interactive example unavailable · View source';
      continue;
    }
    const mount = document.createElement('div');
    details.before(mount);
    // Cards render outside this component's tree; give them the app's context all the same.
    const card = h(ChatApp, { app, workspaceRoot: root });
    card.appContext = appContext;
    render(card, mount);
    mounts.push(mount);
    details.open = false;
  }
  cleanup(() => { for (const mount of mounts) { render(null, mount); mount.remove(); } });
  for (const figure of el.querySelectorAll<HTMLElement>('figure.reply-diagram')) {
    void displayDiagram(figure, () => current, stops);
  }
  for (const img of el.querySelectorAll<HTMLImageElement>("img[data-workspace-image]")) {
    const src = img.dataset.workspaceImage!;
    const load = previews.get(src) ?? previous.get(src) ?? Promise.resolve().then(() =>
      files.readTranscriptImage(root, decodeURIComponent(src)),
    );
    previews.set(src, load);
    void load.then(({ url }) => {
      if (current) img.src = url;
    }, () => {
      if (!current) return;
      const note = document.createElement("span");
      note.textContent = `Preview unavailable: ${img.alt || src}`;
      img.replaceWith(note);
    });
  }
}, { flush: "post" });

async function displayDiagram(figure: HTMLElement, current: () => boolean, stops: (() => void)[]) {
  const details = figure.querySelector('details');
  const source = details?.querySelector('code')?.textContent;
  if (!source || !details) return;
  const name = diagramName(source);
  try {
    // Transparent, so the diagram sits on the conversation itself rather than on a panel.
    const diagram = await renderDiagram(source, name);
    if (!current()) return;
    const button = document.createElement('button');
    button.type = 'button';
    button.className = 'scroll-x';
    button.setAttribute('aria-label', `Enlarge ${name.toLowerCase()}`);
    button.title = `Enlarge ${name.toLowerCase()}`;
    // The viewer has its own surface, so enlarge an opaque rendering of the palette on screen.
    button.onclick = () => viewAttachment({ name, url: diagram.opaqueURL(shownTheme(diagram.images)) });
    button.append(...diagram.images);
    details.before(button);
    stops.push(trackOverflow(button, wide => figure.classList.toggle('wide', wide)));
    details.open = false;
  } catch {
    if (current()) figure.querySelector('figcaption')!.textContent = `${name} preview unavailable`;
  }
}
</script>

<template>
  <div ref="prose" class="prose" v-html="html" />
</template>
