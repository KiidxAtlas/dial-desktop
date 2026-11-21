<script setup lang="ts">
// One option row: a numbered key, the label, and an arrow (↗ when it adds a follow-up prompt).
defineProps<{ index: number; label: string; ask?: boolean }>();
</script>

<template>
  <button type="button" class="choice" :title="ask ? 'Add to your prompt' : undefined">
    <span class="key" aria-hidden="true">{{ index }}</span>
    <span class="label">{{ label }}</span>
    <span class="arrow" :class="{ ask }" aria-hidden="true">{{ ask ? '↗' : '→' }}</span>
  </button>
</template>

<style scoped>
.choice {
  display: flex; align-items: center; gap: 12px;
  width: 100%; padding: 9px 12px 9px 10px; text-align: start;
  border: 1px solid var(--border); border-radius: var(--radius-md);
  background: var(--hover); color: var(--text);
  transition: border-color 0.15s var(--ease), background 0.15s var(--ease);
}
.key {
  flex: none; display: grid; place-items: center;
  width: 20px; height: 20px; border-radius: 6px;
  font: 500 var(--text-xs) / 1 var(--font-mono);
  color: var(--text-muted);
  box-shadow: inset 0 0 0 1px var(--border-strong);
  transition: color 0.15s var(--ease), background 0.15s var(--ease), box-shadow 0.15s var(--ease);
}
.label { flex: 1; min-width: 0; }
.arrow { color: var(--text-faint); opacity: 0; transform: translateX(-4px); transition: transform 0.15s var(--ease), opacity 0.15s var(--ease), color 0.15s var(--ease); }
.arrow.ask { opacity: 1; transform: none; color: var(--accent); }
.choice:hover { border-color: color-mix(in srgb, var(--accent) 55%, transparent); background: color-mix(in srgb, var(--accent) 9%, transparent); }
.choice:hover .key { color: var(--on-accent); background: var(--accent); box-shadow: none; }
.choice:hover .arrow { color: var(--accent); opacity: 1; transform: none; }
@media (prefers-reduced-motion: reduce) { .choice, .key, .arrow { transition: none; } }
</style>
