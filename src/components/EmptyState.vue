<script setup lang="ts">
import { computed } from "vue";
import type { OpenSnapshot } from "../types";
import { focusComposer, ui } from "../ui";
import Logo from "./Logo.vue";

const props = defineProps<{ snap: OpenSnapshot }>();

const SUGGESTIONS = [
  {
    title: "Map this codebase",
    detail: "Explain the structure and main flows",
    prompt:
      "Explain how this project is structured: the main modules, how they fit together and where to start reading.",
  },
  {
    title: "Fix a failing test",
    detail: "Run the suite, find the cause, fix it",
    prompt: "Run the test suite, find the first failing test, diagnose the cause and fix it.",
  },
  {
    title: "Review my changes",
    detail: "Check uncommitted work for problems",
    prompt: "Review my uncommitted changes for bugs, missing tests and anything risky.",
  },
  {
    title: "Add tests",
    detail: "Cover the most important module",
    prompt: "Find the most important untested behaviour in this project and add focused tests for it.",
  },
];

const enabled = computed(() => props.snap.canSend || props.snap.connection.state === "preview");

function pick(prompt: string) {
  ui.draft = prompt;
  focusComposer();
}
</script>

<template>
  <div class="empty">
    <div class="hero">
      <h2><Logo :size="30" /> What should we build?</h2>
      <p class="mono">{{ snap.workspace.display }}</p>
    </div>
    <div class="chips">
      <button
        v-for="s in SUGGESTIONS"
        :key="s.title"
        class="chip"
        :disabled="!enabled"
        :title="s.detail"
        @click="pick(s.prompt)"
      >
        {{ s.title }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.empty {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 22px;
  padding: var(--space-xl);
  padding-bottom: 6vh;
}
.hero {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  text-align: center;
  animation: rise 360ms var(--ease) both;
}
h2 {
  display: flex;
  align-items: center;
  gap: 14px;
  margin: 0;
  font: 400 34px/1.2 ui-serif, "Iowan Old Style", "Palatino Linotype", Georgia, serif;
  letter-spacing: -0.01em;
  color: var(--text);
  text-shadow: var(--hero-shadow);
}
p {
  margin: 0;
  font-size: 12.5px;
  color: var(--text-faint);
  text-shadow: var(--hero-shadow);
}
.chips {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 8px;
  max-width: 560px;
  animation: rise 420ms 60ms var(--ease) both;
}
.chip {
  height: 32px;
  padding: 0 14px;
  border-radius: var(--radius-sm);
  background: var(--glass-soft);
  -webkit-backdrop-filter: var(--glass-blur);
  backdrop-filter: var(--glass-blur);
  box-shadow: inset 0 0 0 1px var(--border-strong);
  font-size: 13px;
  color: var(--text-muted);
  transition:
    background 120ms var(--ease),
    color 120ms var(--ease);
}
.chip:hover:not(:disabled) {
  background: var(--hover);
  color: var(--text);
}
.chip:disabled {
  opacity: 0.45;
}
</style>
