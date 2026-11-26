<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref, computed, watch } from "vue";

// `waiting`: the turn is paused on the user (an approval or a question), so it reads as a prompt
// to act rather than as work in progress, and Esc-to-stop is not offered as the next step.
const props = defineProps<{ label: string; detail?: string; waiting?: boolean }>();

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
    <span v-if="detail" class="metric">{{ detail }}</span>
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
@media (prefers-reduced-motion: reduce) {
  .glyph { visibility: hidden; }
  .waiting .glyph { visibility: visible; }
}
</style>
