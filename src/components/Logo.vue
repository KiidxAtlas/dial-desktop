<script setup lang="ts">
// The Dial mark: a dial ring with its pointer, in the accent colour.
defineProps<{ size?: number }>();
</script>

<template>
  <svg :width="size ?? 22" :height="size ?? 22" viewBox="0 0 32 32" aria-hidden="true" style="flex: none">
    <circle cx="16" cy="16" r="11" fill="none" stroke="var(--accent)" stroke-width="3" />
    <path d="M16 16l5.6-6.6" stroke="var(--accent)" stroke-width="3" stroke-linecap="round" />
    <circle cx="16" cy="16" r="2.6" fill="var(--accent)" />
  </svg>
</template>
