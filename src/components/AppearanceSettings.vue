<script setup lang="ts">
// How the app looks on this computer: light, dark or the system's theme, zoom, and the
// background behind sessions.
import { appearance, zoomBy, ZOOM_STEPS, type Theme } from "../appearance";
import Icon from "./Icon.vue";
import BackgroundSettings from "./BackgroundSettings.vue";
import SGroup from "./settings/SGroup.vue";
import SRow from "./settings/SRow.vue";
import SSwitch from "./settings/SSwitch.vue";
import SNumber from "./settings/SNumber.vue";
import { terminalPrefs, TERMINAL_FONT_SIZE, TERMINAL_SCROLLBACK } from "../terminalPrefs";

const THEMES: { id: Theme; name: string }[] = [
  { id: "system", name: "Match system" },
  { id: "light", name: "Light" },
  { id: "dark", name: "Dark" },
];
</script>

<template>
  <SGroup title="Theme">
    <div class="s-tiles" role="radiogroup" aria-label="Theme">
      <button
        v-for="t in THEMES"
        :key="t.id"
        class="s-tile"
        role="radio"
        :aria-checked="appearance.theme === t.id"
        :class="{ on: appearance.theme === t.id }"
        @click="appearance.theme = t.id"
      >
        <span class="art sketch" :class="t.id">
          <span class="half light"><i /><b /><b class="short" /></span>
          <span class="half dark"><i /><b /><b class="short" /></span>
        </span>
        <span>{{ t.name }}</span>
      </button>
    </div>
    <SRow label="Zoom" description="Text and controls everywhere. ⌘+ and ⌘− work anywhere; ⌘0 resets.">
      <div class="zoom">
        <button title="Zoom out (⌘−)" aria-label="Zoom out" :disabled="appearance.zoom === ZOOM_STEPS[0]" @click="zoomBy(-1)">
          <Icon name="minus" :size="12" />
        </button>
        <button class="value mono" title="Actual size (⌘0)" @click="zoomBy(0)">{{ Math.round(appearance.zoom * 100) }}%</button>
        <button
          title="Zoom in (⌘+)"
          aria-label="Zoom in"
          :disabled="appearance.zoom === ZOOM_STEPS[ZOOM_STEPS.length - 1]"
          @click="zoomBy(1)"
        >
          <Icon name="plus" :size="12" />
        </button>
      </div>
    </SRow>
  </SGroup>
  <SGroup title="In answers">
    <SRow label="Interactive cards" description="Calculators, guides, comparisons and more, added when they help. Off stops asking for them and shows their JSON.">
      <SSwitch v-model="appearance.cards" label="Interactive cards" />
    </SRow>
    <SRow label="Diagrams and charts" description="Mermaid diagrams and charts, added when they help. Off stops asking for them and shows their source.">
      <SSwitch v-model="appearance.diagrams" label="Diagrams and charts" />
    </SRow>
  </SGroup>
  <SGroup title="Terminal">
    <SRow label="Font size" description="Your own shells in the Terminal panel. Open terminals change at once.">
      <SNumber v-model="terminalPrefs.fontSize" label="Terminal font size" unit="px" :min="TERMINAL_FONT_SIZE.min" :max="TERMINAL_FONT_SIZE.max" :step="TERMINAL_FONT_SIZE.step" />
    </SRow>
    <SRow label="Scrollback" description="Lines each terminal keeps above the screen. More uses more memory.">
      <SNumber v-model="terminalPrefs.scrollback" label="Terminal scrollback" unit="lines" integer :min="TERMINAL_SCROLLBACK.min" :max="TERMINAL_SCROLLBACK.max" :step="TERMINAL_SCROLLBACK.step" />
    </SRow>
  </SGroup>
  <BackgroundSettings />
</template>

<style scoped>
/* A sketch of the window in each theme; "Match system" shows both halves. */
.sketch {
  display: flex;
}
.half {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 14px 12px;
}
.sketch.light .dark,
.sketch.dark .light {
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
.zoom {
  display: flex;
  align-items: center;
  height: 30px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: var(--bg);
}
.zoom button {
  display: grid;
  place-items: center;
  width: 28px;
  height: 100%;
  color: var(--text-faint);
}
.zoom button:hover:not(:disabled) {
  color: var(--text);
}
.zoom button:disabled {
  opacity: 0.35;
}
.zoom .value {
  width: 56px;
  font-size: 12.5px;
  color: var(--text);
}
</style>
