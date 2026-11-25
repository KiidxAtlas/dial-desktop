<script setup lang="ts">
// Options side by side as tabs, each with Markdown (prose, code, tables).
import { nextTick, ref } from 'vue';
import type { ChatApp } from '../../chatApp';
import Markdown from '../Markdown.vue';

const props = defineProps<{ app: Extract<ChatApp, { kind: 'compare' }>; workspaceRoot: string }>();
const active = ref(0);
const uid = Math.random().toString(36).slice(2, 8);

// Arrow keys move between tabs, as in a native tab list.
function onKey(event: KeyboardEvent) {
  const n = props.app.tabs.length;
  const next = ({ ArrowRight: active.value + 1, ArrowLeft: active.value - 1, Home: 0, End: n - 1 } as Record<string, number>)[event.key];
  if (next === undefined) return;
  event.preventDefault();
  active.value = (next + n) % n;
  const list = event.currentTarget as HTMLElement;
  void nextTick(() => list.querySelectorAll<HTMLElement>('[role="tab"]')[active.value]?.focus());
}
</script>

<template>
  <div class="compare">
    <div class="tabs" role="tablist" @keydown="onKey">
      <button
        v-for="(tab, i) in app.tabs"
        :id="`tab-${uid}-${i}`"
        :key="i"
        type="button"
        role="tab"
        :aria-selected="i === active"
        :aria-controls="`panel-${uid}`"
        :tabindex="i === active ? 0 : -1"
        :class="{ on: i === active }"
        @click="active = i"
      >{{ tab.label }}</button>
    </div>
    <Transition name="swap" mode="out-in">
      <div :id="`panel-${uid}`" :key="active" class="panel" role="tabpanel" :aria-labelledby="`tab-${uid}-${active}`">
        <Markdown :text="app.tabs[active].text" :workspace-root="workspaceRoot" nested />
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.tabs {
  display: flex;
  gap: 2px;
  margin: 8px 16px 0;
  padding: 3px;
  overflow-x: auto;
  border-radius: var(--radius-md);
  background: var(--hover);
  box-shadow: inset 0 0 0 1px var(--border);
}
.tabs button {
  flex: 1 0 auto;
  padding: 6px 12px;
  border-radius: var(--radius-sm);
  font: 500 var(--text-sm) / 1.3 var(--font-mono);
  color: var(--text-muted);
  white-space: nowrap;
  transition: background 0.15s var(--ease), color 0.15s var(--ease);
}
.tabs button:hover { color: var(--text); }
.tabs button.on {
  color: var(--text-strong);
  background: var(--surface);
  box-shadow: 0 0 0 1px var(--border-strong), inset 0 -2px 0 var(--accent);
}
.panel { padding: 12px 16px 14px; }
.swap-enter-active, .swap-leave-active { transition: opacity 0.14s var(--ease); }
.swap-enter-from, .swap-leave-to { opacity: 0; }
@media (prefers-reduced-motion: reduce) { .tabs button, .swap-enter-active, .swap-leave-active { transition: none; } }
</style>
