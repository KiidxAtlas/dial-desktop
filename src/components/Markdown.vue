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
</script>

<template>
  <div class="prose">
    <MarkdownPart v-for="(html, index) in parts" :key="index" :html="html" :workspace-root="workspaceRoot" :nested="nested" />
  </div>
</template>
