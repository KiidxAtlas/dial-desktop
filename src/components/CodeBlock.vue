<script setup lang="ts">
import { computed } from "vue";
import { codeBlock, highlighterVersion } from "../markdown";

// A captioned code block for tool details; tall output scrolls inside it.
const props = defineProps<{ caption: string; text: string; lang?: string; maxHeight?: number }>();
const html = computed(() => {
  void highlighterVersion.value;
  return codeBlock(props.text, props.lang ?? "", props.caption);
});
</script>

<template>
  <div class="detail-code" :style="{ '--max': `${maxHeight ?? 260}px` }" v-html="html" />
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
</style>
