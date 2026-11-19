<script lang="ts">
import type { ImageFile } from "../bridge";

// Shared by every row. Generated files are never overwritten, so a path's preview is reused
// across snapshots and re-renders instead of being read again.
const cache = new Map<string, Promise<ImageFile>>();
</script>

<script setup lang="ts">
import { ref, watch } from "vue";
import { files } from "../bridge";

/** One image a tool wrote, shown in the conversation. Click shows it in the file manager. */
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
</script>

<template>
  <figure class="generated">
    <button v-if="image" class="frame" :title="`Show ${image.name} in its folder`" @click="files.reveal(path)">
      <img :src="image.url" :alt="image.name" />
    </button>
    <div v-else-if="error" class="missing">Preview unavailable: {{ error }}</div>
    <div v-else class="loading" />
    <figcaption v-if="image" class="mono truncate">{{ image.name }}</figcaption>
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
}
img {
  display: block;
  max-width: 100%;
  max-height: 420px;
  object-fit: contain;
}
.loading {
  width: 240px;
  height: 160px;
  border-radius: var(--radius-md);
  background: var(--hover);
}
.missing,
figcaption {
  margin-top: 4px;
  font-size: var(--text-sm);
  color: var(--text-faint);
}
</style>
