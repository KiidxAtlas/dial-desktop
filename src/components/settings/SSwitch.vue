<script setup lang="ts">
defineProps<{ modelValue: boolean; label: string; disabled?: boolean }>();
const emit = defineEmits<{ "update:modelValue": [on: boolean] }>();
</script>

<template>
  <button
    class="s-switch"
    :class="{ on: modelValue }"
    role="switch"
    :aria-checked="modelValue"
    :aria-label="label"
    :disabled="disabled"
    @click="emit('update:modelValue', !modelValue)"
  >
    <i />
  </button>
</template>
