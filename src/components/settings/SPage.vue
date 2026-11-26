<script setup lang="ts">
// One Settings page: a title, a sentence about what it is for, optional actions, then groups.
// An empty title drops the header (the overview sits under the panel's own "Settings").
defineProps<{ title: string; description?: string }>();
</script>

<template>
  <div class="s-page">
    <header v-if="title || $slots.actions" class="s-head">
      <div>
        <h2 v-if="title">{{ title }}</h2>
        <p v-if="description">{{ description }}</p>
      </div>
      <div v-if="$slots.actions" class="s-head-actions"><slot name="actions" /></div>
    </header>
    <slot />
  </div>
</template>
