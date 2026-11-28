<script setup lang="ts">
// Changing mode must not temporarily apply the minimum window (and trigger early compaction).
import { computed, ref, watch } from 'vue';
import SSegmented from './SSegmented.vue';

const props = defineProps<{ value: unknown; min: number | null; max: number | null; label: string }>();
const emit = defineEmits<{ save: [value: number | 'auto'] }>();
const custom = ref(false);
const draft = ref('');
watch(() => props.value, value => {
  custom.value = typeof value === 'number';
  draft.value = custom.value ? String(value) : '';
}, { immediate: true });
const tokens = computed(() => /^\d+$/.test(draft.value) ? Number(draft.value) : NaN);
const valid = computed(() => Number.isSafeInteger(tokens.value) && tokens.value >= (props.min ?? 1) && tokens.value <= (props.max ?? Infinity));
function choose(mode: string) {
  custom.value = mode === 'set';
  if (!custom.value && props.value !== 'auto') {
    // Keep showing the saved value until the engine confirms Auto, including after a refusal.
    custom.value = true;
    emit('save', 'auto');
  }
}
function apply() {
  if (valid.value && tokens.value !== props.value) emit('save', tokens.value);
}
</script>

<template>
  <div class="window-control">
    <SSegmented :model-value="custom ? 'set' : 'auto'" :options="[{ value: 'auto', label: 'Auto' }, { value: 'set', label: 'Custom' }]" :label="`${label} mode`" @update:model-value="choose" />
    <form v-if="custom" @submit.prevent="apply">
      <input v-model="draft" class="s-input mono" inputmode="numeric" :aria-label="`${label} (tokens)`" placeholder="Tokens" />
      <button class="btn btn-secondary" :disabled="!valid || tokens === value" :title="tokens === value ? 'This cap is already in effect' : undefined">{{ tokens === value ? 'Applied' : 'Apply' }}</button>
    </form>
    <small v-if="custom && draft && !valid" role="status">Enter a whole number from {{ min }} to {{ max }} tokens.</small>
  </div>
</template>

<style scoped>
.window-control, form { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; }
.window-control { justify-content: inherit; }
input.s-input { width: 88px; }
small { flex-basis: 100%; color: var(--warning); }
</style>
