<script setup lang="ts">
// The frame every interactive answer card shares: mono kind label, title, the kind's own body, and
// follow-up prompts the reader can put in the prompt box.
import { computed, ref } from 'vue';
import type { ChatApp } from '../chatApp';
import { addToPrompt } from '../workspace';
import Screens from './chat-app/Screens.vue';
import Guide from './chat-app/Guide.vue';
import Compare from './chat-app/Compare.vue';
import Calculator from './chat-app/Calculator.vue';
import Explore from './chat-app/Explore.vue';

const props = defineProps<{ app: ChatApp; workspaceRoot: string }>();
const KINDS = {
  screens: { label: 'interactive', mark: '✻', body: Screens },
  guide: { label: 'guide', mark: '☰', body: Guide },
  compare: { label: 'compare', mark: '⇄', body: Compare },
  calculator: { label: 'calculator', mark: '∑', body: Calculator },
  explore: { label: 'explore', mark: '◎', body: Explore },
} as const;
const kind = computed(() => KINDS[props.app.kind]);
/** Progress or state the body reports for the header, such as "step 2 of 5". */
const meta = ref('');
</script>

<template>
  <section class="mini-app" :class="`kind-${app.kind}`" :aria-label="app.title">
    <header>
      <div class="meta">
        <span class="kind"><span aria-hidden="true">{{ kind.mark }}</span> {{ kind.label }}</span>
        <span v-if="meta" class="state">{{ meta }}</span>
      </div>
      <strong>{{ app.title }}</strong>
    </header>
    <component :is="kind.body" :app="app as never" :workspace-root="workspaceRoot" @meta="meta = $event" />
    <footer v-if="app.followUps.length" class="follow-ups">
      <span class="lead">ask dial</span>
      <button v-for="(f, i) in app.followUps" :key="i" type="button" :title="`Add to your prompt: ${f.ask}`" @click="addToPrompt(f.ask)">
        {{ f.label }} <span aria-hidden="true">↗</span>
      </button>
    </footer>
  </section>
</template>

<style scoped>
/* Same glass frame and mono labels as the other answer cards, with a clay hairline on top. */
.mini-app {
  position: relative;
  margin: 0 0 0.5em;
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  overflow: hidden;
  background: var(--glass-soft);
}
.mini-app::before {
  content: "";
  position: absolute;
  inset: 0 0 auto;
  height: 2px;
  background: linear-gradient(90deg, var(--accent), color-mix(in srgb, var(--accent) 0%, transparent) 70%);
}
header { display: grid; gap: 6px; padding: 14px 16px 4px; }
.meta { display: flex; align-items: center; gap: 10px; font: 500 var(--text-xs) / 1.4 var(--font-mono); }
.kind { color: var(--accent); }
.state { margin-left: auto; color: var(--text-faint); font-variant-numeric: tabular-nums; }
header strong { font-size: 1.1em; font-weight: 600; letter-spacing: -0.01em; color: var(--text-strong); }
.follow-ups {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  padding: 8px 12px 10px;
  border-top: 1px solid var(--border);
}
.lead { margin-right: 2px; font: var(--text-xs) / 1 var(--font-mono); color: var(--text-faint); }
.follow-ups button {
  padding: 4px 10px;
  border-radius: 999px;
  font-size: var(--text-sm);
  color: var(--text-muted);
  box-shadow: inset 0 0 0 1px var(--border-strong);
  transition: color 0.15s var(--ease), box-shadow 0.15s var(--ease), background 0.15s var(--ease);
}
.follow-ups button span { color: var(--accent); }
.follow-ups button:hover { color: var(--text); background: color-mix(in srgb, var(--accent) 9%, transparent); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 55%, transparent); }
@media (prefers-reduced-motion: reduce) { .follow-ups button { transition: none; } }
</style>
