<script setup lang="ts">
import { computed } from "vue";
import { highlighterVersion, renderMarkdown } from "../markdown";

const props = defineProps<{ text: string }>();
const html = computed(() => {
  // Re-render once code highlighting is available.
  void highlighterVersion.value;
  return renderMarkdown(props.text);
});
</script>

<template>
  <div class="prose" v-html="html" />
</template>
