<script setup lang="ts">
// Shown when no folder is open: pick a project, like starting a terminal agent in one.
import { onMounted, ref } from "vue";
import type { Workspace } from "../types";
import { folders } from "../bridge";
import Icon from "./Icon.vue";
import Logo from "./Logo.vue";

const recent = ref<Workspace[]>([]);
onMounted(async () => {
  recent.value = await folders.recent();
});
</script>

<template>
  <div class="welcome" data-tauri-drag-region>
    <div class="panel">
      <div class="hero">
        <Logo :size="56" />
        <h1>Welcome to Dial</h1>
        <p>Choose a project folder. Dial starts its engine there and works on that code.</p>
      </div>
      <button class="btn btn-primary open" @click="folders.choose()">
        <Icon name="folder" :size="16" /> Open folder…
      </button>
      <div v-if="recent.length" class="recent">
        <div class="section-label">Recent</div>
        <button v-for="w in recent" :key="w.root" class="row" :title="w.root" @click="folders.open(w.root)">
          <Icon name="folder" :size="15" />
          <span class="name truncate">{{ w.name }}</span>
          <span class="path truncate">{{ w.display }}</span>
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.welcome {
  height: 100%;
  display: grid;
  place-items: center;
  padding: var(--space-xxl);
}
.panel {
  width: min(460px, 100%);
  display: flex;
  flex-direction: column;
  gap: var(--space-lg);
}
.hero {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
}
h1 {
  margin: 18px 0 8px;
  font: 400 32px/1.2 ui-serif, "Iowan Old Style", "Palatino Linotype", Georgia, serif;
  letter-spacing: -0.01em;
}
.hero p {
  margin: 0;
  color: var(--text-muted);
}
.open {
  justify-content: center;
  height: 40px;
  font-size: 14px;
}
.recent {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.recent .section-label {
  padding: 0 10px 6px;
}
.row {
  display: grid;
  grid-template-columns: auto minmax(0, auto) minmax(0, 1fr);
  align-items: center;
  gap: 10px;
  padding: 9px 10px;
  border-radius: var(--radius-md);
  text-align: left;
  color: var(--text-faint);
}
.row:hover {
  background: var(--hover);
}
.name {
  color: var(--text);
}
.path {
  font: var(--text-sm) / 1.4 var(--font-mono);
}
</style>
