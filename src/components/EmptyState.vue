<script setup lang="ts">
import { computed } from "vue";
import type { OpenSnapshot } from "../types";
import { canUseTaskRecipes, insertTaskRecipe, taskRecipes } from "../taskRecipes";
import Logo from "./Logo.vue";
import { ui } from "../ui";

const props = defineProps<{ snap: OpenSnapshot }>();

const enabled = computed(() => canUseTaskRecipes(props.snap));
</script>

<template>
  <div class="empty">
    <div class="hero">
      <h2><Logo :size="30" /> {{ snap.workspace.name }}</h2>
      <p class="mono">{{ snap.workspace.display }}</p>
    </div>
    <div class="chips">
      <button
        v-for="(s, i) in taskRecipes().slice(0, 3)"
        :key="s.id"
        class="chip"
        :style="{ animationDelay: `${80 + i * 35}ms` }"
        :disabled="!enabled"
        :title="s.detail"
        @click="insertTaskRecipe(s.prompt)"
      >
        {{ s.title }}
      </button>
    </div>
    <button class="tips" @click="ui.shortcutsOpen = true">Keyboard shortcuts</button>
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
}
.chip {
  height: 32px;
  padding: 0 14px;
  border-radius: 999px;
  background: var(--glass-soft);
  -webkit-backdrop-filter: var(--glass-blur);
  backdrop-filter: var(--glass-blur);
  box-shadow: inset 0 0 0 1px var(--border-strong);
  font-size: 13px;
  color: var(--text-muted);
  animation: rise 360ms var(--ease) both;
  transition:
    background 120ms var(--ease),
    color 120ms var(--ease),
    box-shadow 120ms var(--ease),
    transform 120ms var(--ease);
}
.chip:hover:not(:disabled) {
  background: var(--hover);
  color: var(--text);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 55%, transparent);
  transform: translateY(-1px);
}
.chip:active:not(:disabled) {
  transform: scale(0.97);
}
.chip:disabled {
  opacity: 0.45;
}
.tips {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 6px 16px;
  max-width: 560px;
  font-size: var(--text-xs);
  color: var(--text-muted);
  opacity: 0.85;
  text-shadow: var(--hero-shadow);
  animation: rise 420ms 260ms var(--ease) both;
}
.tips:hover { color: var(--text); text-decoration: underline; }
</style>
