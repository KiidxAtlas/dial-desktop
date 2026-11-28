<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref, computed, watch } from "vue";
import type { CacheState } from "../turnProgress";

// `waiting`: the turn is paused on the user (an approval or a question), so it reads as a prompt
// to act rather than as work in progress, and Esc-to-stop is not offered as the next step.
const props = defineProps<{
  label: string;
  detail?: string;
  meta?: string;
  fraction?: number;
  waiting?: boolean;
  canWrapUp?: boolean;
  wrappingUp?: boolean;
  cacheState?: CacheState;
}>();
const emit = defineEmits<{ wrapUp: [] }>();
const statusText = ref<HTMLElement>();
let handoff: Animation | undefined;
watch(() => props.label, () => {
  const el = statusText.value;
  if (!el || matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  // Update the same live text immediately. If phases arrive in a burst, let the current
  // entrance finish instead of resetting it to its blurred first frame on every update.
  if (handoff?.playState === "running") return;
  handoff = el.animate([
    { transform: "translateY(0.3em)", filter: "blur(1px)" },
    { transform: "none", filter: "none" },
  ], { duration: 180, easing: "ease-out" });
}, { flush: "post" });

// Keep animation updates local so they don't revisit the transcript's rows.
const GLYPHS = ["·", "✢", "✳", "✶", "✻", "✽", "✻", "✶", "✳", "✢"];
const frame = ref(0);
const elapsed = ref(0);
let timer: ReturnType<typeof setInterval>;
onMounted(() => {
  const started = Date.now();
  timer = setInterval(() => {
    frame.value = (frame.value + 1) % GLYPHS.length;
    elapsed.value = Math.floor((Date.now() - started) / 1000);
  }, 120);
});
onBeforeUnmount(() => clearInterval(timer));

const elapsedLabel = computed(() => {
  const s = elapsed.value;
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
});
</script>

<template>
  <div class="working" :class="{ waiting }">
    <span class="glyph" aria-hidden="true">{{ waiting ? "●" : GLYPHS[frame] }}</span>
    <span ref="statusText" class="label" role="status">{{ label }}</span>
    <span v-if="fraction !== undefined && !waiting" class="prefill-bar" role="progressbar" :aria-valuenow="Math.round(fraction * 100)" aria-valuemin="0" aria-valuemax="100" :aria-label="label">
      <span class="prefill-fill" :style="{ width: `${Math.max(fraction * 100, 3)}%` }" />
    </span>
    <span v-if="detail" class="metric">{{ detail }}</span>
    <span
      v-if="meta"
      class="meta"
      :class="cacheState ? `cache-${cacheState}` : undefined"
      :title="cacheState === 'low' ? 'Low KV cache hit — prompt prefix was rebuilt for this step' : cacheState === 'high' ? 'High KV cache reuse — prompt prefix stayed warm' : undefined"
    >{{ meta }}</span>
    <button
      v-if="canWrapUp && !waiting"
      type="button"
      class="wrap-up"
      :disabled="wrappingUp"
      title="Stop deliberating and act immediately using what’s been reasoned so far"
      @click="emit('wrapUp')"
    >{{ wrappingUp ? "Wrapping up…" : "Wrap up & act" }}</button>
    <span class="hint">{{ elapsedLabel }}<template v-if="!waiting"> · Esc to stop</template><template v-else> · answer below</template></span>
  </div>
</template>

<style scoped>
.working {
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 18px;
  font-size: 14px;
}
.glyph {
  width: 14px;
  text-align: center;
  color: var(--accent);
}
.label {
  color: var(--text);
  font-weight: 500;
}
@media (prefers-reduced-motion: no-preference) {
  .working:not(.waiting) .label { animation: breathe 2s ease-in-out infinite; }
}
@keyframes breathe {
  50% { opacity: 0.78; }
}
.waiting .glyph,
.waiting .label {
  color: var(--warning);
}
.hint {
  font-variant-numeric: tabular-nums;
  font-size: 13px;
  color: var(--text-faint);
}
.metric {
  font-size: 13px;
  color: var(--text-muted);
  font-variant-numeric: tabular-nums;
}
.meta {
  font-size: 12.5px;
  color: var(--text-faint);
  font-variant-numeric: tabular-nums;
}
.meta.cache-low {
  color: var(--warning);
}
.meta.cache-high {
  color: var(--text-muted);
}
.wrap-up {
  padding: 2px 8px;
  border: 1px solid var(--border-strong);
  border-radius: 999px;
  background: var(--surface);
  color: var(--text);
  font-size: 12px;
  line-height: 1.4;
  cursor: pointer;
}
.wrap-up:hover:not(:disabled) {
  border-color: var(--accent);
  color: var(--accent);
}
.wrap-up:disabled {
  opacity: 0.6;
  cursor: default;
}
.prefill-bar {
  position: relative;
  display: inline-block;
  width: 72px;
  height: 4px;
  align-self: center;
  overflow: hidden;
  border-radius: 999px;
  background: var(--border-strong);
}
.prefill-fill {
  position: absolute;
  inset: 0 auto 0 0;
  border-radius: inherit;
  background: var(--accent);
  transition: width 200ms var(--ease);
}
@media (prefers-reduced-motion: reduce) {
  .glyph { visibility: hidden; }
  .waiting .glyph { visibility: visible; }
}
</style>
