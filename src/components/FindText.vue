<script setup lang="ts">
import type { TextMatch } from '../chatFind';
defineProps<{ text: string; match: TextMatch }>();
</script>

<template><span>{{ text.slice(0, match.start) }}<mark class="find-current">{{ text.slice(match.start, match.end) }}</mark>{{ text.slice(match.end) }}</span></template>

<style scoped>
.find-current { color: var(--text); background: color-mix(in srgb, var(--accent) 50%, var(--bg)); border-radius: 2px; }
</style>
