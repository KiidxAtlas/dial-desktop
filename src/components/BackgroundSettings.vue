<script setup lang="ts">
// The Background tab of the settings page: pick built-in artwork or an image of your own, and
// how it sits behind sessions.
import { computed } from "vue";
import { files, showToast } from "../bridge";
import { background, customUrl, forgetImage, MAX_DIM, PRESETS, useImage } from "../backgrounds";
import BackgroundThumb from "./BackgroundThumb.vue";
import Icon from "./Icon.vue";

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
</script>

<template>
  <div class="bg-settings">
    <div class="toolbar">
      <h2>Background</h2>
    </div>
    <p class="note">Artwork behind your sessions. Saved on this computer, for every project.</p>

    <h3>Artwork</h3>
    <div class="grid" role="radiogroup" aria-label="Artwork">
      <button
        v-for="p in still"
        :key="p.id"
        class="tile"
        role="radio"
        :aria-checked="background.id === p.id"
        :class="{ on: background.id === p.id }"
        @click="background.id = p.id"
      >
        <BackgroundThumb :preset="p" class="art" />
        <span>{{ p.name }}</span>
      </button>
      <button
        class="tile"
        role="radio"
        :aria-checked="background.id === 'custom'"
        :class="{ on: background.id === 'custom' }"
        :title="background.path ?? 'Choose an image'"
        @click="pickCustom"
      >
        <img v-if="customUrl" class="art" :src="customUrl" alt="" />
        <span v-else class="art add"><Icon name="image" :size="20" /></span>
        <span>{{ customUrl ? "Your image" : "Choose image…" }}</span>
      </button>
      <button
        class="tile"
        role="radio"
        :aria-checked="background.id === 'none'"
        :class="{ on: background.id === 'none' }"
        @click="background.id = 'none'"
      >
        <span class="art plain" />
        <span>None</span>
      </button>
    </div>

    <h3>Animated</h3>
    <div class="grid" role="radiogroup" aria-label="Animated">
      <button
        v-for="p in moving"
        :key="p.id"
        class="tile"
        role="radio"
        :aria-checked="background.id === p.id"
        :class="{ on: background.id === p.id }"
        @click="background.id = p.id"
      >
        <BackgroundThumb :preset="p" class="art" />
        <span>{{ p.name }}</span>
      </button>
    </div>

    <div v-if="background.path" class="row">
      <div class="about">
        <div class="label">Your image</div>
        <div class="meta mono truncate" :title="background.path">{{ fileName(background.path) }}</div>
      </div>
      <div class="buttons">
        <button class="btn btn-secondary" @click="chooseImage">Change…</button>
        <button class="btn btn-secondary" @click="forgetImage">Remove</button>
      </div>
    </div>

    <div class="row">
      <div class="about">
        <div class="label">Show behind</div>
        <p class="desc">Every session, or only a new one before the first prompt.</p>
      </div>
      <div class="segmented" role="radiogroup" aria-label="Show behind">
        <button
          role="radio"
          :aria-checked="background.placement === 'always'"
          :class="{ on: background.placement === 'always' }"
          @click="background.placement = 'always'"
        >
          Every session
        </button>
        <button
          role="radio"
          :aria-checked="background.placement === 'empty'"
          :class="{ on: background.placement === 'empty' }"
          @click="background.placement = 'empty'"
        >
          New sessions
        </button>
      </div>
    </div>

    <div class="row">
      <div class="about">
        <div class="label">Dim behind conversations</div>
        <p class="desc">Darkens the background while messages are on screen, so they stay easy to read.</p>
      </div>
      <label class="slider">
        <input v-model.number="background.dim" type="range" min="0" :max="MAX_DIM" step="0.05" aria-label="Dim" />
        <span class="mono">{{ Math.round(background.dim * 100) }}%</span>
      </label>
    </div>

    <div class="row">
      <div class="about">
        <div class="label">Motion</div>
        <p class="desc">
          Animated backgrounds move slowly; off holds them still, which saves battery.
          <template v-if="reduced">Your system asks to reduce motion, so they are held still anyway.</template>
        </p>
      </div>
      <button
        class="switch"
        :class="{ on: background.motion }"
        role="switch"
        :aria-checked="background.motion"
        aria-label="Motion"
        @click="background.motion = !background.motion"
      >
        <i />
      </button>
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
.switch {
  position: relative;
  flex: none;
  width: 34px;
  height: 20px;
  border-radius: 999px;
  background: var(--raised);
  border: 1px solid var(--border-strong);
  transition: background 150ms var(--ease);
}
.switch i {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: var(--text-muted);
  transition:
    transform 150ms var(--ease),
    background 150ms var(--ease);
}
.switch.on {
  background: var(--accent);
  border-color: var(--accent);
}
.switch.on i {
  transform: translateX(14px);
  background: var(--on-accent);
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
.art {
  display: block;
  width: 100%;
  aspect-ratio: 16 / 10;
  border-radius: var(--radius-md);
  object-fit: cover;
  box-shadow: 0 0 0 1px var(--border-strong);
  transition: box-shadow 140ms var(--ease);
}
.add {
  display: grid;
  place-items: center;
  border: 1px dashed var(--border-strong);
  box-shadow: none;
  color: var(--text-faint);
}
.plain {
  background: var(--bg);
}
.tile:hover {
  color: var(--text);
}
.tile:hover .art {
  box-shadow: 0 0 0 1px var(--text-faint);
}
.tile.on {
  color: var(--text);
}
.tile.on .art {
  box-shadow:
    0 0 0 2px var(--bg),
    0 0 0 4px var(--accent);
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
.desc,
.meta {
  margin: 3px 0 0;
  font-size: var(--text-sm);
  color: var(--text-muted);
}
.meta {
  font-size: var(--text-xs);
  color: var(--text-faint);
}
.buttons {
  display: flex;
  gap: 6px;
  flex: none;
}
.segmented {
  display: flex;
  flex: none;
  padding: 2px;
  border-radius: var(--radius-sm);
  background: var(--sidebar);
  border: 1px solid var(--border);
}
.segmented button {
  height: 24px;
  padding: 0 10px;
  border-radius: 5px;
  font-size: var(--text-sm);
  color: var(--text-muted);
}
.segmented button.on {
  background: var(--raised);
  color: var(--text);
}
.slider {
  display: flex;
  align-items: center;
  gap: 10px;
  flex: none;
  font-size: var(--text-sm);
  color: var(--text-muted);
}
.slider input {
  width: 160px;
  accent-color: var(--accent);
}
.slider span {
  width: 34px;
  text-align: right;
}
</style>
