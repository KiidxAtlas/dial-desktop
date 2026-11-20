<script setup lang="ts">
// Artwork behind sessions: built-in or an image of your own, and how it sits behind them.
import { computed } from "vue";
import { files, showToast } from "../bridge";
import { background, customUrl, forgetImage, MAX_DIM, PRESETS, useImage } from "../backgrounds";
import BackgroundThumb from "./BackgroundThumb.vue";
import Icon from "./Icon.vue";
import SGroup from "./settings/SGroup.vue";
import SRow from "./settings/SRow.vue";
import SSwitch from "./settings/SSwitch.vue";
import SSegmented from "./settings/SSegmented.vue";

const still = computed(() => PRESETS.filter((p) => !p.moving));
const moving = computed(() => PRESETS.filter((p) => p.moving));
const reduced = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;

async function chooseImage() {
  try {
    const path = await files.chooseBackground();
    if (path) await useImage(path);
  } catch (error) {
    showToast(String(error));
  }
}

function pickCustom() {
  if (customUrl.value) background.id = "custom";
  else chooseImage();
}

const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;
const PLACEMENTS = [
  { value: "always", label: "Every session" },
  { value: "empty", label: "New sessions" },
];
</script>

<template>
  <SGroup title="Background" description="Artwork behind your sessions, on this computer for every project.">
    <div class="s-tiles" role="radiogroup" aria-label="Background">
      <button
        class="s-tile"
        role="radio"
        :aria-checked="background.id === 'none'"
        :class="{ on: background.id === 'none' }"
        @click="background.id = 'none'"
      >
        <span class="art plain" />
        <span>None</span>
      </button>
      <button
        v-for="p in still"
        :key="p.id"
        class="s-tile"
        role="radio"
        :aria-checked="background.id === p.id"
        :class="{ on: background.id === p.id }"
        @click="background.id = p.id"
      >
        <BackgroundThumb :preset="p" class="art" />
        <span>{{ p.name }}</span>
      </button>
      <button
        v-for="p in moving"
        :key="p.id"
        class="s-tile"
        role="radio"
        :aria-checked="background.id === p.id"
        :class="{ on: background.id === p.id }"
        @click="background.id = p.id"
      >
        <BackgroundThumb :preset="p" class="art" />
        <span>{{ p.name }} <span class="s-muted">· animated</span></span>
      </button>
      <button
        class="s-tile"
        role="radio"
        :aria-checked="background.id === 'custom'"
        :class="{ on: background.id === 'custom' }"
        :title="background.path ?? 'Choose an image'"
        @click="pickCustom"
      >
        <img v-if="customUrl" class="art" :src="customUrl" alt="" />
        <span v-else class="art add"><Icon name="image" :size="20" /></span>
        <span>{{ customUrl ? "Your image" : "Your image…" }}</span>
      </button>
    </div>

    <SRow v-if="background.path" label="Your image" :description="fileName(background.path)">
      <button class="btn btn-secondary" @click="chooseImage">Change…</button>
      <button class="btn btn-secondary" @click="forgetImage">Remove</button>
    </SRow>
    <template v-if="background.id !== 'none'">
      <SRow label="Show behind" description="Every session, or only a new one before the first prompt.">
        <SSegmented v-model="background.placement" :options="PLACEMENTS" label="Show behind" />
      </SRow>
      <SRow label="Dim behind conversations" description="Darkens the background while messages are on screen.">
        <label class="slider">
          <input v-model.number="background.dim" type="range" min="0" :max="MAX_DIM" step="0.05" aria-label="Dim" />
          <span class="mono">{{ Math.round(background.dim * 100) }}%</span>
        </label>
      </SRow>
      <SRow
        label="Motion"
        :description="reduced ? 'Your system asks to reduce motion, so animated artwork holds still anyway.' : 'Animated artwork moves slowly; off holds it still and saves battery.'"
      >
        <SSwitch v-model="background.motion" label="Motion" />
      </SRow>
    </template>
  </SGroup>
</template>

<style scoped>
.add {
  display: grid;
  place-items: center;
  border: 1px dashed var(--border-strong);
  box-shadow: none !important;
  color: var(--text-faint);
}
.plain {
  background: var(--bg);
}
.slider {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 12.5px;
  color: var(--text-muted);
}
.slider input {
  width: 150px;
  accent-color: var(--accent);
}
.slider span {
  width: 34px;
  text-align: right;
}
</style>
