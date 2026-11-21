<script setup lang="ts">
// A linear walkthrough with a progress bar and checklists that remember what was ticked.
import { computed, reactive, ref, watch, watchEffect } from 'vue';
import type { ChatApp } from '../../chatApp';
import Markdown from '../Markdown.vue';

const props = defineProps<{ app: Extract<ChatApp, { kind: 'guide' }>; workspaceRoot: string }>();
const emit = defineEmits<{ meta: [string] }>();
const index = ref(0);
const step = computed(() => props.app.steps[index.value]);
const total = computed(() => props.app.steps.length);

// Ticks are kept per guide on this computer, keyed by the guide's content.
function hash(text: string) {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 0x01000193);
  return (h >>> 0).toString(36);
}
const key = `dial.guide.${hash(JSON.stringify([props.app.title, props.app.steps]))}`;
function load(): Record<string, boolean> {
  try {
    const value = JSON.parse(localStorage.getItem(key) ?? '{}');
    return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
  } catch {
    return {};
  }
}
const ticked = reactive<Record<string, boolean>>(load());
watch(ticked, value => {
  try { localStorage.setItem(key, JSON.stringify(value)); } catch { /* Storage full or unavailable: ticks last this session. */ }
});
const items = computed(() => props.app.steps.reduce((n, s) => n + s.checklist.length, 0));
const done = computed(() => props.app.steps.reduce((n, s, i) => n + s.checklist.filter((_, j) => ticked[`${i}.${j}`]).length, 0));
watchEffect(() => emit('meta', `step ${index.value + 1} of ${total.value}${items.value ? ` · ${done.value}/${items.value} done` : ''}`));
</script>

<template>
  <div class="guide">
    <ol class="progress" aria-label="Steps">
      <li v-for="(s, i) in app.steps" :key="i">
        <button type="button" :class="{ on: i === index, past: i < index }" :aria-current="i === index ? 'step' : undefined" :title="s.title" @click="index = i">
          <span class="sr">{{ s.title }}</span>
        </button>
      </li>
    </ol>
    <Transition name="swap" mode="out-in">
      <div :key="index" class="step">
        <h4><span class="num">{{ String(index + 1).padStart(2, '0') }}</span> {{ step.title }}</h4>
        <Markdown class="text" :text="step.text" :workspace-root="workspaceRoot" nested />
        <ul v-if="step.checklist.length" class="checklist">
          <li v-for="(item, j) in step.checklist" :key="j">
            <label :class="{ ticked: ticked[`${index}.${j}`] }">
              <input v-model="ticked[`${index}.${j}`]" type="checkbox" />
              <span class="box" aria-hidden="true">✓</span>
              <span>{{ item }}</span>
            </label>
          </li>
        </ul>
      </div>
    </Transition>
    <nav class="nav">
      <button type="button" :disabled="index === 0" @click="index--">← prev</button>
      <button type="button" class="next" :disabled="index === total - 1" @click="index++">next →</button>
    </nav>
  </div>
</template>

<style scoped>
.progress { display: flex; gap: 4px; margin: 8px 16px 0; padding: 0; list-style: none; }
.progress li { flex: 1; }
.progress button { display: block; width: 100%; height: 14px; padding: 5px 0; background: none; }
.progress button::before { content: ""; display: block; height: 4px; border-radius: 2px; background: var(--border-strong); transition: background 0.2s var(--ease); }
.progress button.past::before { background: color-mix(in srgb, var(--accent) 50%, transparent); }
.progress button.on::before { background: var(--accent); }
.progress button:hover::before { background: color-mix(in srgb, var(--accent) 75%, transparent); }
.sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
.step { padding: 10px 16px 14px; }
h4 { display: flex; align-items: baseline; gap: 10px; margin: 0 0 6px; font-size: 1em; font-weight: 600; }
.num { font: 500 var(--text-xs) / 1 var(--font-mono); color: var(--accent); }
.text { color: var(--text-muted); }
.checklist { display: grid; gap: 4px; margin: 10px 0 0; padding: 0; list-style: none; }
.checklist label { display: flex; align-items: center; gap: 10px; padding: 6px 8px; border-radius: var(--radius-sm); cursor: pointer; }
.checklist label:hover { background: var(--hover); }
.checklist input { position: absolute; opacity: 0; pointer-events: none; }
.box {
  flex: none; display: grid; place-items: center; width: 16px; height: 16px; border-radius: 5px;
  font-size: 11px; color: transparent; box-shadow: inset 0 0 0 1.5px var(--border-strong);
  transition: background 0.15s var(--ease), color 0.15s var(--ease), box-shadow 0.15s var(--ease);
}
.checklist input:focus-visible + .box { outline: 2px solid var(--focus-border); outline-offset: 2px; }
.ticked .box { color: var(--on-accent); background: var(--accent); box-shadow: none; }
.ticked span:last-child { color: var(--text-faint); text-decoration: line-through; text-decoration-color: var(--border-strong); }
.nav { display: flex; justify-content: space-between; padding: 4px 8px; border-top: 1px solid var(--border); font: var(--text-xs) / 1.4 var(--font-mono); }
.nav button { padding: 5px 8px; border-radius: var(--radius-sm); color: var(--text-muted); }
.nav button.next { color: var(--accent); }
.nav button:hover:not(:disabled) { background: var(--hover); }
.nav button:disabled { color: var(--text-faint); opacity: 0.5; cursor: default; }
.swap-enter-active, .swap-leave-active { transition: opacity 0.16s var(--ease), transform 0.16s var(--ease); }
.swap-enter-from { opacity: 0; transform: translateX(6px); }
.swap-leave-to { opacity: 0; }
@media (prefers-reduced-motion: reduce) {
  .swap-enter-active, .swap-leave-active, .box, .progress button::before { transition: none; }
}
</style>
