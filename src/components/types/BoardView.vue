<script setup lang="ts">
import { computed } from "vue";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { showToast } from "../../bridge";
import { viewImage } from "../../imageViewer";
import { assetPath, fieldText, typeApi, type Full, type Item } from "../../savedTypes";
import TypeImage from "./TypeImage.vue";
import Icon from "../Icon.vue";

/** A board: a masonry wall of images, with notes as cards among them. */
const props = defineProps<{ full: Full }>();
const emit = defineEmits<{ edit: [item: Item] }>();

const def = computed(() => props.full.def);
const imageKey = computed(() => def.value.view.imageField);
const titleKey = computed(() => def.value.view.titleField);
const noteKey = computed(() => def.value.fields.find((f) => (f.kind === "longtext" || f.kind === "markdown"))?.key ?? null);

function image(item: Item): string | null {
  const key = imageKey.value;
  if (!key) return null;
  const value = item.fields[key];
  return assetPath(props.full, Array.isArray(value) ? value[0] : value);
}

const tilt = (item: Item) => {
  let h = 0;
  for (const c of item.id) h = (h * 31 + c.charCodeAt(0)) | 0;
  return `${((h % 7) - 3) * 0.35}deg`;
};

async function addImages() {
  const key = imageKey.value;
  if (!key) return showToast("This board has no image field; add one in Design");
  const picked = await openDialog({ multiple: true, filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "gif", "webp"] }] });
  const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
  for (const path of paths) {
    try {
      await typeApi.addItem(def.value.id, { [key]: path });
    } catch (error) {
      showToast(String(error));
    }
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
  <div class="board">
    <div class="wall">
      <figure v-for="item in full.items" :key="item.id" class="tile" :class="{ note: !image(item) }" :style="{ '--tilt': tilt(item) }">
        <button v-if="image(item)" class="shot" :title="'View larger'" @click="viewImage(image(item)!)">
          <TypeImage :path="image(item)" fit="natural" :alt="titleKey ? fieldText(item.fields[titleKey]) : ''" />
        </button>
        <div v-else class="note-body">
          <strong v-if="titleKey && item.fields[titleKey]">{{ fieldText(item.fields[titleKey]) }}</strong>
          <p v-if="noteKey && item.fields[noteKey]">{{ fieldText(item.fields[noteKey]) }}</p>
        </div>
        <figcaption v-if="image(item) && titleKey && item.fields[titleKey]">{{ fieldText(item.fields[titleKey]) }}</figcaption>
        <div class="tools">
          <button class="tool" title="Edit" @click="emit('edit', item)"><Icon name="pencil" :size="12" /></button>
          <button class="tool" title="Remove" @click="remove(item)"><Icon name="trash" :size="12" /></button>
        </div>
      </figure>
      <button v-if="!full.items.length" class="tile add" @click="addImages">
        <Icon name="image" :size="22" />
        <span>Add images</span>
      </button>
    </div>
  </div>
</template>

<style scoped>
.board {
  padding: 22px 28px 60px;
}
.wall {
  columns: 230px;
  column-gap: 16px;
}
.tile {
  position: relative;
  display: block;
  width: 100%;
  margin: 0 0 16px;
  break-inside: avoid;
  border-radius: var(--ty-radius);
  background: var(--ty-raised);
  box-shadow: 0 1px 0 var(--ty-line), 0 10px 24px rgb(0 0 0 / 14%);
  overflow: hidden;
  transform: rotate(var(--tilt));
  transition: transform 200ms var(--ease), box-shadow 200ms var(--ease);
}
.tile:hover {
  transform: rotate(0deg) translateY(-3px);
  box-shadow: 0 1px 0 var(--ty-line), 0 18px 34px rgb(0 0 0 / 22%);
  z-index: 1;
}
.shot {
  display: block;
  width: 100%;
  cursor: zoom-in;
}
figcaption {
  padding: 10px 12px 12px;
  font-size: 13.5px;
  line-height: 1.4;
}
.note {
  background: color-mix(in srgb, var(--ty-accent) 16%, var(--ty-paper));
}
.note-body {
  display: grid;
  gap: 6px;
  padding: 16px 16px 18px;
}
.note-body strong {
  font-size: 16px;
}
.note-body p {
  margin: 0;
  color: var(--ty-muted);
  font-size: 13.5px;
  line-height: 1.55;
  white-space: pre-wrap;
}
.tools {
  position: absolute;
  top: 8px;
  right: 8px;
  display: flex;
  gap: 4px;
  opacity: 0;
  transition: opacity 120ms var(--ease);
}
.tile:hover .tools {
  opacity: 1;
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
  display: grid;
  place-items: center;
  gap: 6px;
  min-height: 150px;
  background: none;
  box-shadow: inset 0 0 0 2px var(--ty-line);
  color: var(--ty-faint);
  font: 600 12.5px var(--font-ui);
  transform: none;
}
.add:hover {
  color: var(--ty-accent);
  box-shadow: inset 0 0 0 2px var(--ty-accent);
}
</style>
