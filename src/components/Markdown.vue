<script setup lang="ts">
import { computed } from 'vue';
import { highlighterVersion, renderMarkdownParts } from '../markdown';
import MarkdownPart from './MarkdownPart.vue';

/** `nested`: Markdown inside an interactive card, which does not mount cards of its own. */
const props = defineProps<{ text: string; workspaceRoot: string; streaming?: boolean; nested?: boolean }>();
const parts = computed(() => {
  void highlighterVersion.value;
  return renderMarkdownParts(props.text, props.streaming);
});
// Ordinary fragments contain only renderer-produced HTML, with no cards or assets to mount.
const interactive = (html: string) => /(?:class="reply-(?:app|diagram)"|<img [^>]*data-workspace-image=)/.test(html);
</script>

<template>
  <div class="prose">
    <template v-for="(html, index) in parts" :key="index">
      <MarkdownPart v-if="interactive(html)" :html="html" :workspace-root="workspaceRoot" :nested="nested" />
      <div v-else class="markdown-part" v-html="html" />
    </template>
  </div>
</template>
