<script setup lang="ts">
import { computed, ref, watchPostEffect } from "vue";
import type { LineRange } from "../ui";
import { codeBlock, highlighterVersion } from "../markdown";

// A captioned code block for tool details; tall output scrolls inside it.
const props = defineProps<{ caption: string; text: string; lang?: string; maxHeight?: number; lines?: LineRange | null }>();
const block = ref<HTMLElement | null>(null);
const unavailable = ref(false);
const html = computed(() => {
  void highlighterVersion.value;
  return codeBlock(props.text, props.lang ?? "", props.caption, false, !!props.lines);
});
watchPostEffect((cleanup) => {
  void html.value; // Reapply after syntax highlighting replaces the rendered nodes.
  const range = props.lines;
  unavailable.value = false;
  if (!block.value || !range) return;
  const lines = block.value.querySelectorAll('pre code > .line');
  const marked = Array.from(lines).slice(range.start - 1, range.end);
  unavailable.value = !marked.length || range.end > lines.length;
  for (const line of marked) line.classList.add('cited');
  marked[0]?.scrollIntoView({ block: 'center', inline: 'nearest' });
  cleanup(() => { for (const line of marked) line.classList.remove('cited'); });
});
</script>

<template>
  <div ref="block" class="detail-code" :class="{ anchored: lines }" :style="{ '--max': `${maxHeight ?? 260}px` }" v-html="html" />
  <small v-if="unavailable" role="status">The cited range extends beyond the displayed file.</small>
</template>

<style scoped>
.detail-code :deep(.code-block) {
  margin: 0;
  background: var(--bg);
}
.detail-code :deep(pre) {
  max-height: var(--max);
  overflow: auto;
  user-select: text;
  -webkit-user-select: text;
  white-space: pre-wrap;
  word-break: break-word;
}
.anchored :deep(pre code) { display: grid; white-space: normal; }
.anchored :deep(pre code > .line) { white-space: pre-wrap; min-height: 1lh; }
.anchored :deep(.cited) { background: color-mix(in srgb, var(--accent) 15%, transparent); box-shadow: inset 2px 0 var(--accent); }
</style>
