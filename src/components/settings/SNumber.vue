<script setup lang="ts">
// A number with − and + buttons and its unit; typing works too, and is checked on commit.
import { computed, ref, watch } from "vue";
import Icon from "../Icon.vue";

const props = defineProps<{
  modelValue: number;
  min?: number | null;
  max?: number | null;
  step?: number | null;
  integer?: boolean;
  unit?: string | null;
  label: string;
}>();
const emit = defineEmits<{ "update:modelValue": [value: number] }>();

const draft = ref("");
const error = ref("");
watch(
  () => props.modelValue,
  (v) => {
    draft.value = String(v);
    error.value = "";
  },
  { immediate: true },
);

const stepBy = computed(() => props.step ?? 1);
const decimals = computed(() => (String(stepBy.value).split(".")[1] ?? "").length);

function clamp(n: number) {
  if (props.min != null) n = Math.max(props.min, n);
  if (props.max != null) n = Math.min(props.max, n);
  return Number(n.toFixed(decimals.value));
}

function commit() {
  const text = draft.value.trim();
  const n = Number(text);
  if (text === "" || !Number.isFinite(n)) return (error.value = "Enter a number");
  if (props.integer && !Number.isInteger(n)) return (error.value = "Enter a whole number");
  if (props.min != null && n < props.min) return (error.value = `At least ${props.min}`);
  if (props.max != null && n > props.max) return (error.value = `At most ${props.max}`);
  error.value = "";
  if (n !== props.modelValue) emit("update:modelValue", n);
}

function nudge(direction: 1 | -1) {
  const base = Number.isFinite(Number(draft.value)) ? Number(draft.value) : props.modelValue;
  const next = clamp(base + direction * stepBy.value);
  draft.value = String(next);
  commit();
}

/** A friendlier reading of large values: 8192 bytes → 8 KB, 3600 s → 1 h. */
const reading = computed(() => {
  const v = props.modelValue;
  if (props.unit === "bytes" && v >= 1024) {
    const units = ["KB", "MB", "GB", "TB"];
    let n = v / 1024;
    let i = 0;
    while (n >= 1024 && i < units.length - 1) {
      n /= 1024;
      i++;
    }
    return `${Number(n.toFixed(1))} ${units[i]}`;
  }
  if (props.unit === "s" && v >= 120) {
    if (v >= 86400) return `${Number((v / 86400).toFixed(1))} days`;
    if (v >= 3600) return `${Number((v / 3600).toFixed(1))} h`;
    return `${Number((v / 60).toFixed(1))} min`;
  }
  if (props.unit === "tokens" && v >= 10000) return `${Number((v / 1000).toFixed(1))}k`;
  return "";
});
</script>

<template>
  <div class="wrap">
    <div class="stepper" :class="{ bad: error }">
      <button tabindex="-1" :aria-label="`Decrease ${label}`" :disabled="min != null && modelValue <= min" @click="nudge(-1)">
        <Icon name="minus" :size="12" />
      </button>
      <input
        v-model="draft"
        class="mono"
        inputmode="decimal"
        :aria-label="label"
        @change="commit"
        @keydown.enter="commit"
        @keydown.up.prevent="nudge(1)"
        @keydown.down.prevent="nudge(-1)"
      />
      <button tabindex="-1" :aria-label="`Increase ${label}`" :disabled="max != null && modelValue >= max" @click="nudge(1)">
        <Icon name="plus" :size="12" />
      </button>
    </div>
    <span v-if="unit" class="unit" :title="reading">{{ reading ? `${unit} · ${reading}` : unit }}</span>
    <p v-if="error" class="s-error">{{ error }}</p>
  </div>
</template>

<style scoped>
.wrap {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: flex-end;
  gap: 4px 8px;
}
.stepper {
  display: flex;
  align-items: center;
  height: 30px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: var(--bg);
  transition: border-color 120ms var(--ease);
}
.stepper:focus-within {
  border-color: var(--accent);
}
.stepper.bad {
  border-color: var(--danger);
}
.stepper button {
  display: grid;
  place-items: center;
  width: 26px;
  height: 100%;
  color: var(--text-faint);
}
.stepper button:hover:not(:disabled) {
  color: var(--text);
}
.stepper button:disabled {
  opacity: 0.35;
}
input {
  width: 76px;
  height: 100%;
  border: none;
  outline: none;
  background: none;
  color: var(--text);
  font-size: var(--text-sm);
  text-align: center;
}
.unit {
  min-width: 0;
  font-size: 12px;
  color: var(--text-faint);
  white-space: nowrap;
}
.s-error {
  flex-basis: 100%;
  text-align: right;
}
</style>
