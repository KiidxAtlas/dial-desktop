<script setup lang="ts">
// A list of short strings as removable chips, with a box to add one more, and optionally a
// folder picker for lists of paths.
import { ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { showToast } from "../../bridge";
import Icon from "../Icon.vue";

const props = defineProps<{ modelValue: string[]; label: string; placeholder?: string; folders?: boolean }>();
const emit = defineEmits<{ "update:modelValue": [value: string[]] }>();
const draft = ref("");

async function pickFolder() {
  try {
    const picked = await open({ directory: true, multiple: true, title: props.label });
    const paths = (Array.isArray(picked) ? picked : picked ? [picked] : []).filter((p) => !props.modelValue.includes(p));
    if (paths.length) emit("update:modelValue", [...props.modelValue, ...paths]);
  } catch (error) {
    showToast(`Could not open the picker: ${error}`);
  }
}
function add() {
  const items = draft.value
    .split(/\n/)
    .map((s) => s.trim())
    .filter((s) => s && !props.modelValue.includes(s));
  draft.value = "";
  if (items.length) emit("update:modelValue", [...props.modelValue, ...items]);
}
function remove(item: string) {
  emit("update:modelValue", props.modelValue.filter((x) => x !== item));
}
function backspace() {
  if (!draft.value && props.modelValue.length) remove(props.modelValue[props.modelValue.length - 1]);
}
</script>

<template>
  <div class="chips" @click="($event.currentTarget as HTMLElement).querySelector('input')?.focus()">
    <span v-for="item in modelValue" :key="item" class="chip">
      <span class="truncate mono" :title="item">{{ item }}</span>
      <button :aria-label="`Remove ${item}`" @click.stop="remove(item)"><Icon name="cross" :size="10" /></button>
    </span>
    <input
      v-model="draft"
      :aria-label="`Add to ${label}`"
      :placeholder="modelValue.length ? 'Add…' : placeholder ?? 'Add an item…'"
      spellcheck="false"
      @keydown.enter.prevent="add"
      @keydown.backspace="backspace"
      @blur="add"
    />
    <button v-if="folders" class="pick" type="button" :aria-label="`Add a folder to ${label}`" @click.stop="pickFolder">
      <Icon name="folder" :size="12" /> Add folder…
    </button>
  </div>
</template>

<style scoped>
.chips {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  width: 100%;
  min-height: 34px;
  padding: 4px 6px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: var(--bg);
  cursor: text;
}
.chips:focus-within {
  border-color: var(--accent);
}
.chip {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  max-width: 100%;
  height: 24px;
  padding: 0 4px 0 9px;
  border-radius: 6px;
  background: var(--active);
  font-size: 12px;
  color: var(--text);
}
.chip button {
  display: grid;
  place-items: center;
  width: 18px;
  height: 18px;
  border-radius: 4px;
  color: var(--text-faint);
}
.chip button:hover {
  background: var(--hover);
  color: var(--danger);
}
input {
  flex: 1;
  min-width: 100px;
  height: 24px;
  border: none;
  outline: none;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
}
input::placeholder {
  color: var(--text-faint);
}
.pick {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 24px;
  padding: 0 8px;
  border-radius: 6px;
  font-size: 12px;
  color: var(--text-muted);
}
.pick:hover {
  background: var(--hover);
  color: var(--text);
}
</style>
