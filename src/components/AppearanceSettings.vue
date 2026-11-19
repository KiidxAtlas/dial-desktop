<script setup lang="ts">
// The Appearance tab of the settings page: light, dark or the system's theme, and the zoom level.
import { appearance, zoomBy, ZOOM_STEPS, type Theme } from "../appearance";
import Icon from "./Icon.vue";

const THEMES: { id: Theme; name: string }[] = [
  { id: "system", name: "System" },
  { id: "light", name: "Light" },
  { id: "dark", name: "Dark" },
];
</script>

<template>
  <div class="appearance">
    <div class="toolbar">
      <h2>Appearance</h2>
    </div>
    <p class="note">How Dial looks on this computer, for every project.</p>

    <h3>Theme</h3>
    <div class="grid" role="radiogroup" aria-label="Theme">
      <button
        v-for="t in THEMES"
        :key="t.id"
        class="tile"
        role="radio"
        :aria-checked="appearance.theme === t.id"
        :class="{ on: appearance.theme === t.id }"
        @click="appearance.theme = t.id"
      >
        <span class="art" :class="t.id">
          <span class="half light"><i /><b /><b class="short" /></span>
          <span class="half dark"><i /><b /><b class="short" /></span>
        </span>
        <span>{{ t.name }}</span>
      </button>
    </div>

    <div class="row">
      <div class="about">
        <div class="label">Zoom</div>
        <p class="desc">Makes text and controls larger or smaller. ⌘+ and ⌘− work anywhere; ⌘0 resets.</p>
      </div>
      <div class="stepper">
        <button
          title="Zoom out (⌘−)"
          aria-label="Zoom out"
          :disabled="appearance.zoom === ZOOM_STEPS[0]"
          @click="zoomBy(-1)"
        >
          <Icon name="minus" :size="14" />
        </button>
        <button class="value mono" title="Actual size (⌘0)" @click="zoomBy(0)">
          {{ Math.round(appearance.zoom * 100) }}%
        </button>
        <button
          title="Zoom in (⌘+)"
          aria-label="Zoom in"
          :disabled="appearance.zoom === ZOOM_STEPS[ZOOM_STEPS.length - 1]"
          @click="zoomBy(1)"
        >
          <Icon name="plus" :size="14" />
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.toolbar {
  display: flex;
  align-items: center;
  min-height: 32px;
}
h2 {
  margin: 0;
  font-family: ui-serif, Georgia, "Times New Roman", serif;
  font-size: 24px;
  font-weight: 400;
}
.note {
  margin: 10px 0 4px;
  font-size: var(--text-sm);
  color: var(--text-faint);
}
h3 {
  margin: 20px 0 10px;
  font-size: var(--text-xs);
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--text-faint);
}
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  gap: 14px;
  padding-bottom: 22px;
  border-bottom: 1px solid var(--border);
}
.tile {
  display: flex;
  flex-direction: column;
  gap: 7px;
  font-size: var(--text-sm);
  color: var(--text-muted);
  text-align: left;
}
.tile:hover,
.tile.on {
  color: var(--text);
}
/* A sketch of the window in each theme; "System" shows both halves. */
.art {
  display: flex;
  width: 100%;
  aspect-ratio: 16 / 10;
  overflow: hidden;
  border-radius: var(--radius-md);
  box-shadow: 0 0 0 1px var(--border-strong);
  transition: box-shadow 140ms var(--ease);
}
.tile:hover .art {
  box-shadow: 0 0 0 1px var(--text-faint);
}
.tile.on .art {
  box-shadow:
    0 0 0 2px var(--bg),
    0 0 0 4px var(--accent);
}
.half {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 14px 12px;
}
.art.light .dark,
.art.dark .light {
  display: none;
}
.half.light {
  background: #faf9f5;
  --line: rgb(31 30 29 / 16%);
  --dot: #c96442;
}
.half.dark {
  background: #262624;
  --line: rgb(243 242 236 / 18%);
  --dot: #d97757;
}
.half i {
  width: 10px;
  height: 10px;
  margin-bottom: 4px;
  border-radius: 50%;
  background: var(--dot);
}
.half b {
  height: 6px;
  border-radius: 3px;
  background: var(--line);
}
.half b.short {
  width: 60%;
}
.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
  padding: 14px 0;
  border-bottom: 1px solid var(--border);
}
.about {
  min-width: 0;
}
.label {
  font-size: var(--text-md);
  color: var(--text);
}
.desc {
  margin: 3px 0 0;
  font-size: var(--text-sm);
  color: var(--text-muted);
}
.stepper {
  display: flex;
  flex: none;
  padding: 2px;
  border-radius: var(--radius-sm);
  background: var(--sidebar);
  border: 1px solid var(--border);
}
.stepper button {
  display: grid;
  place-items: center;
  min-width: 28px;
  height: 24px;
  border-radius: 5px;
  color: var(--text-muted);
}
.stepper button:hover:not(:disabled) {
  background: var(--hover);
  color: var(--text);
}
.stepper button:disabled {
  opacity: 0.35;
}
.stepper .value {
  min-width: 52px;
  font-size: var(--text-sm);
}
</style>
