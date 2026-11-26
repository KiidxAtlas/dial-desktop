<script lang="ts">
import type { ImageFile } from "../bridge";

// Shared by every row. Generated files are never overwritten, so a path's preview is reused
// across snapshots and re-renders instead of being read again.
const cache = new Map<string, Promise<ImageFile>>();
</script>

<script setup lang="ts">
import { ref, watch } from "vue";
import { files, snapshot } from "../bridge";
import { openImageMenu } from "../imageActions";
import { viewAttachment, viewImage } from "../imageViewer";
import Icon from "./Icon.vue";

/** One image a tool wrote, shown in the conversation. Click opens it larger in the viewer;
 * right-click copies or saves it. `compact`: a thumbnail under a folded run of calls. */
const props = defineProps<{ path: string; compact?: boolean }>();
/** A file name cut in the middle to fit under a thumbnail, keeping its start and its extension:
 *  "128x128@2x.png" → "128…x.png". */
function shortName(name: string, room = 9) {
  if (name.length <= room) return name;
  return `${name.slice(0, room - 6)}…${name.slice(-5)}`;
}

const image = ref<ImageFile | null>(null);
const error = ref<string | null>(null);

watch(
  [() => props.path, () => snapshot.value?.workspace?.root],
  async ([path, root], _, onCleanup) => {
    let current = true;
    onCleanup(() => { current = false; });
    image.value = null;
    error.value = null;
    if (!root) return;
    const key = JSON.stringify([root, path]);
    let load = cache.get(key);
    if (!load) {
      load = files.readTranscriptImage(root, path);
      if (cache.size >= 16) cache.delete(cache.keys().next().value!);
      cache.set(key, load);
      // A failed read (the file is not there yet, or was removed) may succeed next time.
      load.catch(() => { if (cache.get(key) === load) cache.delete(key); });
    }
    try {
      const loaded = await load;
      if (!current) return;
      // An empty reply would otherwise leave the placeholder shimmering for good.
      if (loaded?.url) image.value = loaded;
      else error.value = "the file could not be read";
    } catch (failure) {
      if (current) error.value = String(failure);
    }
  },
  { immediate: true },
);

function openInViewer() {
  if (props.path.startsWith("artifact://") && image.value) viewAttachment(image.value);
  else viewImage(props.path);
}

function menu(event: MouseEvent) {
  const shown = image.value;
  if (!shown) return;
  openImageMenu(event, {
    name: shown.name,
    url: shown.url,
    absolute: props.path.startsWith("artifact://") ? undefined : shown.path,
    open: { label: "View larger", run: openInViewer },
  });
}
</script>

<template>
  <figure class="generated" :class="{ compact }">
    <button v-if="image" class="frame" :title="compact ? `${image.name} — click to view` : `View ${image.name}`" @click.stop="openInViewer" @contextmenu="menu">
      <img :src="image.url" :alt="image.name" draggable="false" />
    </button>
    <div v-else-if="error" class="missing" :title="error">{{ compact ? "No preview" : `Preview unavailable: ${error}` }}</div>
    <div v-else class="loading" />
    <figcaption v-if="image && !compact">
      <button class="name mono truncate" :title="path" @click.stop="openInViewer">{{ image.name }}</button>
      <button v-if="!path.startsWith('artifact://')" class="act" title="Show in Finder" aria-label="Show in Finder" @click.stop="files.reveal(path)">
        <Icon name="folder" :size="13" />
      </button>
    </figcaption>
    <figcaption v-if="image && compact" class="chip-name mono" :title="path">{{ shortName(image.name) }}</figcaption>
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
  max-height: 240px;
  object-fit: contain;
  -webkit-user-drag: none;
  pointer-events: none;
  animation: fade-in 240ms var(--ease) both;
}
@keyframes fade-in { from { opacity: 0; } }
.loading {
  width: 240px;
  height: 160px;
  border-radius: var(--radius-md);
  background: linear-gradient(90deg, var(--hover) 0%, var(--active) 50%, var(--hover) 100%) 0 0 / 200% 100%;
  animation: shimmer 1.4s linear infinite;
}
@keyframes shimmer { to { background-position: -200% 0; } }
/* Under a folded run: a small thumbnail named like the file chips beside it. */
.compact {
  display: flex;
  flex-direction: column;
  gap: 4px;
  width: 64px;
}
.compact .frame {
  transition: transform 160ms var(--ease), box-shadow 160ms var(--ease);
}
.compact .frame:hover {
  transform: translateY(-1px);
  box-shadow: inset 0 0 0 1px var(--border-strong), 0 6px 18px rgb(0 0 0 / 22%);
}
.compact img {
  width: 64px;
  height: 48px;
  object-fit: cover;
}
.compact .loading,
.compact .missing {
  width: 64px;
  height: 48px;
}
.chip-name {
  font-size: 11px;
  color: var(--text-muted);
}
.compact .missing {
  display: grid;
  place-items: center;
  margin: 0;
  padding: 6px;
  border-radius: var(--radius-md);
  box-shadow: inset 0 0 0 1px var(--border);
  font-size: 11px;
  text-align: center;
}
@media (prefers-reduced-motion: reduce) {
  img, .loading { animation: none; }
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
