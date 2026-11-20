<script setup lang="ts">
// A titled card of rows. `plain` drops the card for free-form content (charts, tiles).
defineProps<{ title?: string; description?: string; plain?: boolean; id?: string }>();
</script>

<template>
  <section :id="id" class="s-group">
    <div v-if="title || $slots.aside" class="s-group-head">
      <div>
        <h3 v-if="title">{{ title }}</h3>
        <p v-if="description">{{ description }}</p>
      </div>
      <slot name="aside" />
    </div>
    <slot v-if="plain" />
    <div v-else class="s-card"><slot /></div>
    <slot name="after" />
  </section>
</template>
