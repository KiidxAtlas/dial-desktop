<script lang="ts">
import type { ImageFile } from "../bridge";

// Shared by every row. Generated files are never overwritten, so a path's preview is reused
// across snapshots and re-renders instead of being read again.
const cache = new Map<string, Promise<ImageFile>>();
</script>

<script setup lang="ts">
import { ref, watch } from "vue";
import { files } from "../bridge";
import { viewImage } from "../imageViewer";
import Icon from "./Icon.vue";

/** One image a tool wrote, shown in the conversation. Click opens it larger in the viewer. */
const props = defineProps<{ path: string }>();

const image = ref<ImageFile | null>(null);
const error = ref<string | null>(null);

watch(
  () => props.path,
  async (path) => {
    image.value = null;
    error.value = null;
    let load = cache.get(path);
    if (!load) {
      load = files.readImage(path);
      cache.set(path, load);
      // A failed read (the file is not there yet, or was removed) may succeed next time.
      load.catch(() => cache.delete(path));
    }
    try {
      const loaded = await load;
      if (props.path === path) image.value = loaded;
    } catch (failure) {
      if (props.path === path) error.value = String(failure);
    }
  },
  { immediate: true },
);

function openInViewer() {
  viewImage(props.path);
}
</script>

<template>
  <figure class="generated">
    <button v-if="image" class="frame" :title="`View ${image.name}`" @click.stop="openInViewer">
      <img :src="image.url" :alt="image.name" draggable="false" />
    </button>
    <div v-else-if="error" class="missing">Preview unavailable: {{ error }}</div>
    <div v-else class="loading" />
    <figcaption v-if="image">
      <button class="name mono truncate" :title="path" @click.stop="openInViewer">{{ image.name }}</button>
      <button class="act" title="Show in Finder" aria-label="Show in Finder" @click.stop="files.reveal(path)">
        <Icon name="folder" :size="13" />
      </button>
    </figcaption>
  </figure>
</template>

<style scoped>
.generated {
  margin: 0;
  max-width: 100%;
}
.frame {
  display: block;
  max-width: 100%;
  padding: 0;
  border-radius: var(--radius-md);
  overflow: hidden;
  box-shadow: inset 0 0 0 1px var(--border);
  cursor: zoom-in;
  -webkit-user-select: none;
  user-select: none;
}
img {
  display: block;
  max-width: 100%;
  max-height: 280px;
  object-fit: contain;
  -webkit-user-drag: none;
  pointer-events: none;
}
.loading {
  width: 240px;
  height: 160px;
  border-radius: var(--radius-md);
  background: var(--hover);
}
.missing {
  margin-top: 4px;
  font-size: var(--text-sm);
  color: var(--text-faint);
}
figcaption {
  display: flex;
  align-items: center;
  gap: 2px;
  max-width: 100%;
  margin-top: 4px;
}
.name {
  min-width: 0;
  padding: 2px 4px;
  margin-left: -4px;
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
  color: var(--accent);
  text-align: left;
  cursor: pointer;
}
.name:hover {
  text-decoration: underline;
}
.act {
  display: grid;
  flex: none;
  place-items: center;
  width: 24px;
  height: 24px;
  border-radius: var(--radius-sm);
  color: var(--text-faint);
}
.act:hover,
.name:hover {
  background: var(--hover);
}
.act:hover {
  color: var(--text);
}
</style>
