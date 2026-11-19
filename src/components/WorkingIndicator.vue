<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref, computed } from "vue";

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
  <div class="working">
    <span class="glyph">{{ GLYPHS[frame] }}</span>
    <span class="shimmer">Working…</span>
    <span class="hint">({{ elapsedLabel }} · esc to interrupt)</span>
  </div>
</template>

<style scoped>
.working {
  display: flex;
  align-items: baseline;
  gap: 8px;
  margin-top: 18px;
  font-size: 14px;
}
.glyph {
  width: 14px;
  text-align: center;
  color: var(--accent);
}
.shimmer {
  color: var(--accent);
  background: linear-gradient(90deg, var(--accent) 0%, var(--accent-soft) 50%, var(--accent) 100%) 0 0 / 200% 100%;
  -webkit-background-clip: text;
  background-clip: text;
  -webkit-text-fill-color: transparent;
  animation: shimmer 2s linear infinite;
}
.hint {
  font-size: 13px;
  color: var(--text-faint);
}
@keyframes shimmer {
  to {
    background-position: -200% 0;
  }
}
</style>
