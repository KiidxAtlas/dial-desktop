<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { files, type ImageFile } from "../bridge";
import { highlighterVersion, renderMarkdown } from "../markdown";

const props = defineProps<{ text: string; workspaceRoot: string; streaming?: boolean }>();
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
</script>

<template>
  <div ref="prose" class="prose" v-html="html" />
</template>
