<script setup lang="ts">
// Branching screens: each choice opens another screen or offers a follow-up prompt.
import { computed, ref, watchEffect } from 'vue';
import type { ChatApp } from '../../chatApp';
import { addToPrompt } from '../../workspace';
import Markdown from '../Markdown.vue';
import Choice from './Choice.vue';

const props = defineProps<{ app: Extract<ChatApp, { kind: 'screens' }>; workspaceRoot: string }>();
const emit = defineEmits<{ meta: [string] }>();
const trail = ref<string[]>([]);
const screen = computed(() => props.app.screens.find(s => s.id === trail.value.at(-1)) ?? props.app.screens[0]);
const panel = ref<HTMLElement | null>(null);
watchEffect(() => emit('meta', trail.value.length ? `step ${trail.value.length + 1}` : ''));
function navigate(trip: string[]) {
  trail.value = trip.slice(-100);
}
// The screen swaps out-in, so focus the new one once it has entered.
function focusPanel() {
  panel.value?.focus({ preventScroll: true });
}
</script>

<template>
  <Transition name="swap" mode="out-in" @after-enter="focusPanel">
    <div :key="trail.length + ':' + screen.id" ref="panel" class="body" tabindex="-1" role="group" :aria-label="`Screen ${trail.length + 1}`">
      <Markdown class="text" :text="screen.text" :workspace-root="workspaceRoot" nested />
      <div v-if="screen.choices.length" class="choices">
        <template v-for="(choice, i) in screen.choices" :key="i">
          <Choice v-if="'to' in choice" :index="i + 1" :label="choice.label" @click="navigate([...trail, choice.to])" />
          <Choice v-else :index="i + 1" :label="choice.label" ask @click="addToPrompt(choice.ask)" />
        </template>
      </div>
    </div>
  </Transition>
  <nav v-if="trail.length" class="nav">
    <button type="button" @click="navigate(trail.slice(0, -1))">← back</button>
    <button type="button" @click="navigate([])">↺ start over</button>
  </nav>
</template>

<style scoped>
.body { padding: 6px 16px 16px; }
.body:focus-visible { outline-offset: -3px; }
.text { color: var(--text-muted); }
.choices { display: grid; gap: 6px; margin-top: 14px; }
.nav { display: flex; justify-content: space-between; padding: 4px 8px; border-top: 1px solid var(--border); font: var(--text-xs) / 1.4 var(--font-mono); }
.nav button { padding: 5px 8px; border-radius: var(--radius-sm); color: var(--text-faint); }
.nav button:hover { background: var(--hover); color: var(--accent); }
.swap-enter-active, .swap-leave-active { transition: opacity 0.16s var(--ease), transform 0.16s var(--ease); }
.swap-enter-from { opacity: 0; transform: translateY(4px); }
.swap-leave-to { opacity: 0; }
@media (prefers-reduced-motion: reduce) { .swap-enter-active, .swap-leave-active { transition: none; } }
</style>
