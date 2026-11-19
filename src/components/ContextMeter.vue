<script setup lang="ts">
// How full the conversation's context is: a small ring by the send button, with the numbers on hover.
// Clicking it compacts the conversation when that is possible.
import { computed } from "vue";
import type { ContextView } from "../types";

const props = defineProps<{ context: ContextView; canCompact?: boolean }>();
const emit = defineEmits<{ compact: [] }>();

const R = 7;
const CIRCUMFERENCE = 2 * Math.PI * R;

/** Share of the window in use, 0–1; `null` when the window is unknown. */
const share = computed(() =>
  props.context.window ? Math.min(1, props.context.tokens / props.context.window) : null,
);
const tone = computed(() => (share.value === null ? "" : share.value >= 0.9 ? "danger" : share.value >= 0.75 ? "warning" : ""));

function count(n: number) {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

const label = computed(() => {
  const { tokens, window, estimated } = props.context;
  const about = estimated ? "About " : "";
  if (!window || share.value === null) return `${about}${count(tokens)} tokens of context in use`;
  return `${about}${count(tokens)} of ${count(window)} tokens of context in use (${Math.round(share.value * 100)}%)`;
});
const title = computed(() => (props.canCompact ? `${label.value}. Click to compact the conversation.` : label.value));
</script>

<template>
  <button
    type="button"
    class="meter"
    :class="tone"
    :title="title"
    :aria-label="title"
    :disabled="!canCompact"
    @click.stop="emit('compact')"
  >
    <svg width="18" height="18" viewBox="0 0 18 18">
      <circle cx="9" cy="9" :r="R" class="track" />
      <circle
        v-if="share !== null"
        cx="9"
        cy="9"
        :r="R"
        class="fill"
        :stroke-dasharray="CIRCUMFERENCE"
        :stroke-dashoffset="CIRCUMFERENCE * (1 - share)"
      />
    </svg>
    <span v-if="share !== null && share >= 0.5" class="pct">{{ Math.round(share * 100) }}%</span>
  </button>
</template>

<style scoped>
.meter {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 30px;
  padding: 0 6px;
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  color: var(--text-faint);
  --tone: var(--text-muted);
}
.meter:hover:not(:disabled) {
  background: var(--hover);
}
.meter:disabled {
  cursor: default;
}
.meter.warning {
  --tone: var(--warning);
  color: var(--warning);
}
.meter.danger {
  --tone: var(--danger);
  color: var(--danger);
}
svg {
  transform: rotate(-90deg);
}
circle {
  fill: none;
  stroke-width: 2;
}
.track {
  stroke: var(--border-strong);
}
.fill {
  stroke: var(--tone);
  stroke-linecap: round;
  transition: stroke-dashoffset 300ms var(--ease);
}
.pct {
  font-variant-numeric: tabular-nums;
}
</style>
