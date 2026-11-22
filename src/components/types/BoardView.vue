<script lang="ts">
/** Tauri drop coordinates are physical; hit-test the visible canvas in CSS pixels. */
export function imageBoardAt(position: { x: number; y: number }): boolean {
  const scale = window.devicePixelRatio;
  return !!document.elementFromPoint(position.x / scale, position.y / scale)?.closest("[data-image-board]");
}
</script>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { IMAGE_FILE } from "../../attachments";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { showToast } from "../../bridge";
import { viewTypeImage } from "../../imageViewer";
import { assetPath, fieldText, typeApi, type Full, type Item } from "../../savedTypes";
import TypeImage from "./TypeImage.vue";
import Icon from "../Icon.vue";

/** A board: a masonry wall of images, with notes as cards among them. */
const props = defineProps<{ full: Full }>();
const emit = defineEmits<{ edit: [item: Item] }>();

const def = computed(() => props.full.def);
const imageKey = computed(() => def.value.view.imageField);
const titleKey = computed(() => def.value.view.titleField);
const noteKey = computed(() => def.value.view.subtitleField
  || def.value.fields.find((f) => f.key !== titleKey.value && (f.kind === "longtext" || f.kind === "markdown"))?.key);
const dropping = ref(false);
let unlisten: (() => void) | undefined;
let disposed = false;

onMounted(async () => {
  const stop = await getCurrentWebview().onDragDropEvent(({ payload }) => {
    const overBoard = payload.type !== "leave" && imageBoardAt(payload.position);
    dropping.value = !!(overBoard && (payload.type === "over"
      || (payload.type === "enter" && payload.paths.some((path) => IMAGE_FILE.test(path)))));
    if (overBoard && payload.type === "drop") void importImages(payload.paths);
  }).catch(() => null);
  if (disposed) stop?.();
  else unlisten = stop ?? undefined;
});
onUnmounted(() => { disposed = true; unlisten?.(); });

function image(item: Item): string | null {
  const key = imageKey.value;
  if (!key) return null;
  const value = item.fields[key];
  return assetPath(props.full, Array.isArray(value) ? value[0] : value);
}

async function importImages(paths: string[]) {
  const key = imageKey.value;
  if (!key) return;
  const id = def.value.id;
  const multiple = def.value.fields.find((field) => field.key === key)?.kind === "images";
  const images = [...new Set(paths.filter((path) => IMAGE_FILE.test(path)))];
  if (paths.length && !images.length) showToast("Drop PNG, JPEG, GIF or WebP images");
  for (const path of images) {
    try {
      await typeApi.addItem(id, { [key]: multiple ? [path] : path });
    } catch (error) {
      showToast(String(error));
    }
  }
}

async function addImages() {
  if (!imageKey.value) return showToast("This board has no image field; add one in Design");
  try {
    const picked = await openDialog({ multiple: true, filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "gif", "webp"] }] });
    await importImages(Array.isArray(picked) ? picked : picked ? [picked] : []);
  } catch (error) {
    showToast(String(error));
  }
}

async function remove(item: Item) {
  try {
    await typeApi.removeItem(def.value.id, item.id);
  } catch (error) {
    showToast(String(error));
  }
}

defineExpose({ addImages });
</script>

<template>
  <div class="board" :class="{ dropping }">
    <div v-if="full.items.length" class="wall">
      <figure v-for="item in full.items" :key="item.id" class="tile" :class="{ note: !image(item) }">
        <button v-if="image(item)" class="shot" aria-label="View larger" @click="viewTypeImage(image(item)!, titleKey ? fieldText(item.fields[titleKey]) : undefined)">
          <TypeImage :path="image(item)" fit="natural" :alt="titleKey ? fieldText(item.fields[titleKey]) : ''" />
        </button>
        <figcaption v-if="(titleKey && item.fields[titleKey]) || (noteKey && item.fields[noteKey])">
          <strong v-if="titleKey && item.fields[titleKey]">{{ fieldText(item.fields[titleKey]) }}</strong>
          <p v-if="noteKey && noteKey !== titleKey && item.fields[noteKey]">{{ fieldText(item.fields[noteKey]) }}</p>
        </figcaption>
        <div class="tools">
          <button class="tool" title="Edit" aria-label="Edit" @click="emit('edit', item)"><Icon name="pencil" :size="12" /></button>
          <button class="tool" title="Remove" aria-label="Remove" @click="remove(item)"><Icon name="trash" :size="12" /></button>
        </div>
      </figure>
    </div>
    <button v-else-if="imageKey" class="add" @click="addImages">
      <Icon name="image" :size="24" />
      <strong>Drop images here</strong>
      <span>or browse for files</span>
    </button>
    <p v-else class="no-images">Add an image field in Design to put pictures on this board.</p>
    <div v-if="dropping" class="drop-cue" aria-hidden="true"><span>Drop to add images</span></div>
  </div>
</template>

<style scoped>
.board {
  padding: clamp(14px, 2.2cqi, 28px);
}
.wall {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(min(100%, 245px), 1fr));
  align-items: start;
  gap: 18px;
}
.tile {
  position: relative;
  min-width: 0;
  max-width: 380px;
  margin: 0;
  border-radius: var(--ty-radius);
  background: var(--ty-raised);
  box-shadow: 0 0 0 1px var(--ty-line);
  overflow: hidden;
}
.shot {
  display: block;
  width: 100%;
  cursor: zoom-in;
}
figcaption {
  display: grid;
  gap: 5px;
  padding: 13px 14px 15px;
  overflow-wrap: anywhere;
}
figcaption strong {
  font: 650 15px/1.35 var(--ty-font);
}
figcaption p {
  margin: 0;
  color: var(--ty-muted);
  font: 13px/1.5 var(--font-ui);
  white-space: pre-wrap;
}
.note {
  background: color-mix(in srgb, var(--ty-accent) 10%, var(--ty-paper));
}
.note figcaption {
  padding-inline-end: 74px;
}
.tools {
  position: absolute;
  top: 8px;
  right: 8px;
  display: flex;
  gap: 4px;
}
@media (hover: hover) {
  .tools { opacity: 0; transition: opacity 120ms var(--ease); }
  .tile:hover .tools, .tile:focus-within .tools { opacity: 1; }
}
.tool {
  display: grid;
  place-items: center;
  width: 26px;
  height: 26px;
  border-radius: 8px;
  background: rgb(0 0 0 / 55%);
  color: #fff;
  backdrop-filter: blur(6px);
}
.add {
  display: flex;
  flex-direction: column;
  justify-content: center;
  align-items: center;
  gap: 8px;
  width: 100%;
  min-height: 200px;
  border: 1px dashed var(--ty-muted);
  border-radius: var(--ty-radius);
  background: var(--ty-raised);
  color: var(--ty-ink);
  font: 14px var(--font-ui);
}
.add svg { color: var(--ty-accent); }
.add span, .no-images { color: var(--ty-muted); }
.add:hover { border-color: var(--ty-accent); }
.drop-cue {
  position: absolute;
  inset: 8px;
  z-index: 4;
  display: grid;
  place-items: center;
  border: 2px dashed var(--ty-accent);
  border-radius: var(--ty-radius);
  background: color-mix(in srgb, var(--ty-paper) 75%, transparent);
  color: var(--ty-ink);
  font: 650 16px var(--font-ui);
  pointer-events: none;
}
</style>
