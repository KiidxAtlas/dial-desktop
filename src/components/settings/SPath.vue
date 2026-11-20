<script setup lang="ts">
// A file, program or folder, chosen with the system picker rather than typed.
import { computed } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { showToast } from "../../bridge";
import Icon from "../Icon.vue";

const props = defineProps<{
  modelValue: string;
  kind: "program" | "file" | "folder";
  label: string;
  /** What an empty value means, e.g. "Find on PATH". */
  emptyLabel?: string;
}>();
const emit = defineEmits<{ "update:modelValue": [value: string] }>();

const name = computed(() => props.modelValue.split(/[\\/]/).filter(Boolean).pop() ?? props.modelValue);

async function choose() {
  try {
    const picked = await open({
      directory: props.kind === "folder",
      multiple: false,
      title: props.label,
      defaultPath: props.modelValue || undefined,
    });
    if (typeof picked === "string" && picked !== props.modelValue) emit("update:modelValue", picked);
  } catch (error) {
    showToast(`Could not open the picker: ${error}`);
  }
}
</script>

<template>
  <div class="path">
    <span v-if="modelValue" class="value" :title="modelValue">
      <Icon :name="kind === 'folder' ? 'folder' : kind === 'program' ? 'terminal' : 'file'" :size="13" />
      <span class="truncate mono">{{ name }}</span>
    </span>
    <span v-else class="value empty">{{ emptyLabel ?? "Not set" }}</span>
    <button class="btn btn-secondary" @click="choose">Choose…</button>
    <button v-if="modelValue" class="clear" :title="`Clear (${emptyLabel ?? 'not set'})`" aria-label="Clear" @click="emit('update:modelValue', '')">
      <Icon name="cross" :size="12" />
    </button>
  </div>
</template>

<style scoped>
.path {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}
.value {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  max-width: 200px;
  font-size: 12.5px;
  color: var(--text);
}
.value.empty {
  color: var(--text-faint);
}
.btn {
  height: 28px;
  flex: none;
}
.clear {
  display: grid;
  place-items: center;
  width: 24px;
  height: 24px;
  flex: none;
  border-radius: 6px;
  color: var(--text-faint);
}
.clear:hover {
  background: var(--hover);
  color: var(--text);
}
</style>
