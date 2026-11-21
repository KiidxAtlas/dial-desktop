<script setup lang="ts">
// A generated or mentioned image, shown large, with Open / Show in Finder / Copy path.
import { computed, ref, watch } from "vue";
import { files, type ImageFile } from "../bridge";
import { viewedImage } from "../imageViewer";
import { copyText } from "../sessionActions";
import Icon from "./Icon.vue";
import { vFocus } from "./Shortcuts.vue";

const file = computed(() => viewedImage.value && 'path' in viewedImage.value ? viewedImage.value : null);
const loadedImage = ref<ImageFile | null>(null);
const image = computed(() => viewedImage.value && 'attachment' in viewedImage.value ? viewedImage.value.attachment : loadedImage.value);
const title = computed(() => file.value?.path ?? image.value?.name);
const error = ref<string | null>(null);

watch(
  file,
  async (viewed) => {
    loadedImage.value = null;
    error.value = null;
    if (!viewed) return;
    try {
      const loaded = await files.readTranscriptImage(viewed.root, viewed.path);
      if (viewedImage.value === viewed) loadedImage.value = loaded;
    } catch (failure) {
      if (viewedImage.value === viewed) error.value = String(failure);
    }
  },
  { immediate: true },
);

function close() {
  viewedImage.value = null;
}

function open() {
  const v = file.value;
  if (v) void files.openTranscriptImage(v.root, v.path);
}

function reveal() {
  const v = file.value;
  if (v) void files.reveal(v.absolute);
}

function copyPath() {
  const v = file.value;
  if (v) void copyText(v.absolute, "Path copied");
}

function onKey(event: KeyboardEvent) {
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    close();
  } else if (!file.value) return;
  else if (event.key === "Enter" || ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "o")) {
    event.preventDefault();
    open();
  } else if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "c" && !getSelection()?.toString()) {
    event.preventDefault();
    copyPath();
  }
}
</script>

<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="viewedImage" class="overlay" @mousedown.self="close">
        <div v-focus class="viewer" role="dialog" :aria-label="title" tabindex="-1" @keydown="onKey">
          <header>
            <span class="name mono truncate" :title="file?.absolute ?? title">{{ title }}</span>
            <button v-if="file" class="btn btn-secondary" title="Open in the default viewer (Enter)" @click="open">
              <Icon name="external" :size="13" /> Open
            </button>
            <button v-if="file" class="btn btn-secondary" title="Show in Finder" @click="reveal">
              <Icon name="folder" :size="13" /> Show in Finder
            </button>
            <button v-if="file" class="icon-btn" title="Copy path (⌘C)" aria-label="Copy path" @click="copyPath">
              <Icon name="copy" :size="14" />
            </button>
            <button class="icon-btn" title="Close (Esc)" aria-label="Close" @click="close">
              <Icon name="cross" :size="14" />
            </button>
          </header>
          <div class="stage" @dblclick="open">
            <img v-if="image" :src="image.url" :alt="image.name" draggable="false" />
            <p v-else-if="error" class="error">Preview unavailable: {{ error }}</p>
            <div v-else class="loading" />
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  z-index: 75;
  display: grid;
  place-items: center;
  padding: 28px;
  background: rgb(0 0 0 / 55%);
}
.viewer {
  display: flex;
  flex-direction: column;
  gap: 10px;
  max-width: min(1100px, 100%);
  max-height: 100%;
  padding: 12px;
  border-radius: var(--radius-lg);
  background: var(--raised);
  box-shadow: var(--shadow-pop);
  outline: none;
  animation: rise 160ms var(--ease) both;
}
header {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}
.name {
  flex: 1;
  min-width: 120px;
  margin-right: 8px;
  font-size: 12.5px;
  color: var(--text-muted);
}
header .btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  white-space: nowrap;
}
.stage {
  display: grid;
  place-items: center;
  min-height: 0;
  overflow: hidden;
}
img {
  display: block;
  max-width: 100%;
  max-height: calc(100vh - 140px);
  border-radius: var(--radius-md);
  object-fit: contain;
  -webkit-user-drag: none;
}
.loading {
  width: 360px;
  height: 260px;
  border-radius: var(--radius-md);
  background: var(--hover);
}
.error {
  margin: 0;
  padding: 24px;
  color: var(--text-faint);
  font-size: 13px;
}
.fade-enter-active,
.fade-leave-active {
  transition: opacity 120ms var(--ease);
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
