<script setup lang="ts">
// A few mutually exclusive choices, all visible.
defineProps<{ modelValue: string; options: { value: string; label: string }[]; label: string }>();
const emit = defineEmits<{ "update:modelValue": [value: string] }>();
</script>

<template>
  <div class="s-seg" role="radiogroup" :aria-label="label">
    <button
      v-for="o in options"
      :key="o.value"
      role="radio"
      :aria-checked="modelValue === o.value"
      :class="{ on: modelValue === o.value }"
      @click="emit('update:modelValue', o.value)"
    >
      {{ o.label }}
    </button>
  </div>
</template>
