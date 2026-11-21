<script lang="ts">
import type { ImageFile } from "../../bridge";

// Assets are named by their hash and never rewritten, so a path's preview is read once.
const cache = new Map<string, Promise<ImageFile>>();

export function loadAsset(path: string): Promise<ImageFile> {
  let load = cache.get(path);
  if (!load) {
    load = import("../../bridge").then(({ files }) => files.readImage(path));
    cache.set(path, load);
    load.catch(() => cache.delete(path));
  }
  return load;
}
</script>

<script setup lang="ts">
import { ref, watch } from "vue";

/** One image saved in a type, from its asset path; a soft placeholder while it loads. */
const props = defineProps<{ path: string | null; alt?: string; fit?: "cover" | "contain" | "natural" }>();

const url = ref<string | null>(null);
const failed = ref(false);

watch(
  () => props.path,
  async (path) => {
    url.value = null;
    failed.value = false;
    if (!path) return;
    try {
      const image = await loadAsset(path);
      if (props.path === path) url.value = image.url;
    } catch {
      if (props.path === path) failed.value = true;
    }
  },
  { immediate: true },
);
</script>

<template>
  <img v-if="url" class="ty-img" :class="fit ?? 'cover'" :src="url" :alt="alt ?? ''" draggable="false" />
  <div v-else class="ty-img placeholder" :class="[fit ?? 'cover', { failed }]" :title="failed ? 'Image missing' : undefined" />
</template>

<style scoped>
.ty-img {
  display: block;
  width: 100%;
  -webkit-user-drag: none;
}
.cover {
  height: 100%;
  object-fit: cover;
}
.contain {
  height: 100%;
  object-fit: contain;
}
.natural {
  height: auto;
}
.placeholder {
  min-height: 120px;
  background:
    linear-gradient(110deg, transparent 30%, rgb(255 255 255 / 6%) 50%, transparent 70%) 0 0 / 200% 100%,
    color-mix(in srgb, var(--ty-ink, var(--text)) 7%, transparent);
  animation: shimmer 1.4s linear infinite;
}
.placeholder.failed {
  animation: none;
}
@keyframes shimmer {
  to {
    background-position: -200% 0;
  }
}
</style>
