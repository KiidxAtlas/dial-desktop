<script setup lang="ts">
// A small fill bar shows usage when reported (a bar, so it never reads as a loading spinner);
// editing the budget does not require a first turn.
import { computed, ref, watch } from "vue";
import type { ContextView } from "../types";
import { resetSettingRow, saveSettingRow, settings, settingsPage } from "../bridge";
import SettingRow from "./SettingRow.vue";

const props = defineProps<{ context: ContextView | null; live: boolean; turnRunning: boolean; canCompact?: boolean; local?: boolean }>();
const emit = defineEmits<{ compact: [] }>();
const open = ref(false);
const root = ref<HTMLElement | null>(null);
const trigger = ref<HTMLButtonElement | null>(null);
const rows = computed(() => {
  const keys = props.local
    ? ['local_context_window', 'local_runtime.kv_bits', 'context_window']
    : ['context_window', 'local_context_window'];
  const all = settingsPage.value?.settings ?? [];
  return keys.map(k => all.find(s => s.key === k)).filter((s): s is NonNullable<typeof s> => !!s);
});
watch(open, (shown, _, cleanup) => {
  if (!shown) return;
  void settings.load();
  const outside = (event: Event) => { if (!root.value?.contains(event.target as Node)) open.value = false; };
  document.addEventListener('pointerdown', outside);
  document.addEventListener('focusin', outside);
  cleanup(() => {
    document.removeEventListener('pointerdown', outside);
    document.removeEventListener('focusin', outside);
  });
});
watch(() => props.live, () => { open.value = false; });
function close() {
  open.value = false;
  trigger.value?.focus();
}
function escape(event: KeyboardEvent) {
  if (!open.value) return;
  event.stopPropagation();
  close();
}

/** Share of the window in use, 0–1; `null` when the window is unknown. */
const share = computed(() =>
  props.context?.window ? Math.min(1, props.context.tokens / props.context.window) : null,
);
const tone = computed(() => (share.value === null ? "" : share.value >= 0.9 ? "danger" : share.value >= 0.75 ? "warning" : ""));

function count(n: number) {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

const windowMismatch = computed(() => {
  const c = props.context;
  if (!c?.loadedWindow || !c.requestedWindow || c.loadedWindow >= c.requestedWindow) return null;
  return `Server loaded ${count(c.loadedWindow)} (requested ${count(c.requestedWindow)} — capped by RAM or server config)`;
});

const label = computed(() => {
  if (!props.context) return 'Context usage not reported yet';
  const { tokens, window, loadedWindow, estimated } = props.context;
  const about = estimated ? "About " : "";
  const effective = loadedWindow ?? window;
  const suffix = loadedWindow ? " loaded" : "";
  if (!effective || share.value === null) return `${about}${count(tokens)} tokens of context in use`;
  return `${about}${count(tokens)} of ${count(effective)}${suffix} tokens in use (${Math.round(share.value * 100)}%)`;
});
const title = computed(() => {
  const base = windowMismatch.value ? `${label.value} · ${windowMismatch.value}` : label.value;
  return props.live ? `${base}. Open context controls.` : base;
});
</script>

<template>
  <div ref="root" class="context-control" @keydown.esc="escape">
    <button
      ref="trigger"
      type="button"
      class="meter"
      :class="tone"
      :title="title"
      :aria-label="title"
      :aria-expanded="open"
      :disabled="!live"
      @click.stop="open = !open"
    >
      <span class="bar" aria-hidden="true"><span v-if="share !== null" class="fill" :style="{ width: `${Math.max(share * 100, 4)}%` }" /></span>
      <span v-if="share !== null && share >= 0.5" class="pct">{{ Math.round(share * 100) }}%</span>
    </button>
    <section v-if="open" class="context-panel" aria-label="Context window">
      <header>
        <strong>Context window</strong>
        <span>{{ label }}</span>
        <span v-if="windowMismatch" class="mismatch">{{ windowMismatch }}</span>
        <span v-else-if="local && context && !context.loadedWindow" class="faint-note">Configured limit — actual server window is confirmed after the first request.</span>
      </header>
      <template v-if="rows.length">
        <p>Changes here apply to this project.</p>
        <SettingRow v-for="s in rows" :key="s.key" :setting="{ ...s, modified: s.modified && s.source !== 'global' }" page-scope="project"
          @save="value => saveSettingRow(s, value, 'project', turnRunning)" @reset="resetSettingRow(s, 'project', turnRunning)" />
      </template>
      <p v-else>{{ settingsPage ? 'This engine does not offer context window settings.' : 'Waiting for engine settings…' }}</p>
      <button class="btn btn-secondary" :disabled="!canCompact" @click="close(); emit('compact')">Compact conversation</button>
    </section>
  </div>
</template>

<style scoped>
.context-control { display: flex; }
.context-panel {
  position: absolute;
  z-index: 30;
  right: 0;
  bottom: calc(100% + var(--pop-gap));
  width: min(390px, 100%);
  max-height: 60vh;
  overflow-y: auto;
  padding: 14px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--raised);
  box-shadow: var(--shadow-pop);
  cursor: default;
}
.context-panel header { display: grid; gap: 4px; }
.context-panel header span, .context-panel p { color: var(--text-muted); font-size: var(--text-sm); }
.context-panel .mismatch { color: var(--warning); font-size: var(--text-xs); }
.context-panel .faint-note { color: var(--text-faint); font-size: var(--text-xs); }
.context-panel :deep(.s-row) { flex-direction: column; align-items: stretch; gap: 7px; padding: 10px 0; border-top: 1px solid var(--border); }
.context-panel :deep(.s-control) { max-width: none; justify-content: flex-start; flex-wrap: wrap; }
.context-panel .btn { margin-top: 6px; }
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
.bar {
  position: relative;
  width: 18px;
  height: 4px;
  overflow: hidden;
  border-radius: 999px;
  background: var(--border-strong);
}
.fill {
  position: absolute;
  inset: 0 auto 0 0;
  border-radius: inherit;
  background: var(--tone);
  transition: width 300ms var(--ease);
}
.pct {
  font-variant-numeric: tabular-nums;
}
</style>
