<script setup lang="ts">
// Edit a TOML-compatible table or list field by field: switches for true/false, number boxes,
// text boxes, and nested groups. Values keep their types; nothing is stringified.
import { computed, ref } from "vue";
import SSwitch from "./settings/SSwitch.vue";
import Icon from "./Icon.vue";

const props = defineProps<{ value: unknown; label: string; stringsOnly?: boolean }>();
const emit = defineEmits<{ change: [value: unknown] }>();

const name = ref("");
const type = ref("Text");
const TYPES = ["Text", "Number", "Switch", "List", "Group"];
const initial = () => ({ Text: "", Number: 0, Switch: false, List: [], Group: {} })[type.value] ?? "";
const object = computed(() =>
  props.value && typeof props.value === "object" && !Array.isArray(props.value) ? (props.value as Record<string, unknown>) : null,
);
const exists = computed(() => !!object.value && Object.hasOwn(object.value, name.value.trim()));
const nested = (v: unknown) => !!v && typeof v === "object";

function set(key: string | number, value: unknown) {
  if (Array.isArray(props.value)) {
    const next = props.value.slice();
    next[Number(key)] = value;
    emit("change", next);
  } else emit("change", { ...object.value, [key]: value });
}
function remove(key: string | number) {
  if (Array.isArray(props.value)) emit("change", props.value.filter((_, i) => i !== key));
  else emit("change", Object.fromEntries(Object.entries(object.value ?? {}).filter(([k]) => k !== key)));
}
function add() {
  if (Array.isArray(props.value)) emit("change", [...props.value, props.stringsOnly ? "" : initial()]);
  else if (name.value.trim() && !exists.value) {
    set(name.value.trim(), initial());
    name.value = "";
  }
}
</script>

<template>
  <div v-if="object || Array.isArray(value)" class="collection">
    <div v-for="(item, key) in object || (value as unknown[])" :key="key" class="entry" :class="{ nested: nested(item) }">
      <div class="key">
        <span class="mono truncate">{{ object ? key : `#${Number(key) + 1}` }}</span>
        <button type="button" class="remove" :aria-label="`Remove ${label} ${key}`" title="Remove" @click="remove(key)">
          <Icon name="trash" :size="12" />
        </button>
      </div>
      <SettingValue :value="item" :label="`${label} ${key}`" @change="set(key, $event)" />
    </div>
    <p v-if="!(object ? Object.keys(object).length : (value as unknown[]).length)" class="empty">Empty</p>
    <div class="add">
      <input
        v-if="object"
        v-model="name"
        class="s-input mono"
        :aria-label="`New field in ${label}`"
        placeholder="New field name"
        @keydown.enter.prevent="add"
      />
      <select v-if="!stringsOnly" v-model="type" class="s-select" :aria-label="`New value type in ${label}`">
        <option v-for="t in TYPES" :key="t">{{ t }}</option>
      </select>
      <button type="button" class="btn btn-secondary" :disabled="!!object && (!name.trim() || exists)" @click="add">
        <Icon name="plus" :size="12" /> {{ object ? "Add field" : "Add item" }}
      </button>
      <small v-if="exists" class="s-error">That field already exists.</small>
    </div>
  </div>
  <SSwitch v-else-if="typeof value === 'boolean'" :model-value="value" :label="label" @update:model-value="emit('change', $event)" />
  <input
    v-else-if="typeof value === 'number'"
    class="s-input mono num"
    type="number"
    step="any"
    required
    :value="value"
    :aria-label="label"
    @input="Number.isFinite(($event.target as HTMLInputElement).valueAsNumber) && emit('change', ($event.target as HTMLInputElement).valueAsNumber)"
  />
  <input
    v-else
    class="s-input text"
    spellcheck="false"
    :value="String(value ?? '')"
    :aria-label="label"
    @input="emit('change', ($event.target as HTMLInputElement).value)"
  />
</template>

<style scoped>
.collection {
  display: flex;
  flex-direction: column;
  min-width: 0;
  width: 100%;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--bg);
}
.entry {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
  padding: 8px 10px;
  border-bottom: 1px solid var(--border);
}
.entry.nested {
  flex-direction: column;
  align-items: stretch;
  gap: 8px;
}
.key {
  display: flex;
  flex: 0 0 150px;
  align-items: center;
  gap: 4px;
  min-width: 0;
  font-size: 12.5px;
  color: var(--text-muted);
}
.entry.nested .key {
  flex-basis: auto;
}
.remove {
  display: grid;
  place-items: center;
  width: 22px;
  height: 22px;
  margin-left: auto;
  border-radius: 5px;
  color: var(--text-faint);
  opacity: 0;
}
.entry:hover > .key .remove,
.remove:focus-visible {
  opacity: 1;
}
.remove:hover {
  background: var(--hover);
  color: var(--danger);
}
.text {
  flex: 1;
  min-width: 0;
}
.num {
  width: 120px;
}
.entry > .s-switch {
  margin-left: auto;
}
.empty {
  margin: 0;
  padding: 10px;
  font-size: 12px;
  color: var(--text-faint);
}
.add {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
}
.add .s-input {
  flex: 1;
  min-width: 140px;
}
.add .btn {
  height: 30px;
}
</style>
