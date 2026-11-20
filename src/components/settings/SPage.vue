<script setup lang="ts">
// One Settings page: a title, a sentence about what it is for, optional actions, then groups.
defineProps<{ title: string; description?: string }>();
</script>

<template>
  <div class="s-page">
    <header class="s-head">
      <div>
        <h2>{{ title }}</h2>
        <p v-if="description">{{ description }}</p>
      </div>
      <div v-if="$slots.actions" class="s-head-actions"><slot name="actions" /></div>
    </header>
    <slot />
  </div>
</template>
