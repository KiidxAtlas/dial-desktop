<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { Setting } from "../types";
import Icon from "./Icon.vue";

const props = defineProps<{ setting: Setting }>();
const emit = defineEmits<{ save: [value: unknown]; reset: [] }>();

const s = computed(() => props.setting);
const c = computed(() => props.setting.control);
const error = ref("");

/** The editable text of the current value, for text-like controls. */
function asText(value: unknown): string {
  switch (c.value.type) {
    case "list":
      return Array.isArray(value) ? value.join("\n") : "";
    case "table":
      return value && typeof value === "object" && Object.keys(value).length ? JSON.stringify(value, null, 2) : "";
    default:
      return value === null || value === undefined ? "" : String(value);
  }
}

const draft = ref("");
watch(
  () => props.setting.value,
  (value) => {
    draft.value = asText(value);
    error.value = "";
  },
  { immediate: true },
);

const isAuto = computed(() => props.setting.value === "auto");

function save(value: unknown) {
  error.value = "";
  if (JSON.stringify(value) !== JSON.stringify(props.setting.value)) emit("save", value);
}

/** Validate and save the drafted text. */
function commit() {
  // `v-model` on a number input yields a number; the checks below want its text.
  const text = String(draft.value ?? "").trim();
  const control = c.value;
  if (control.type === "number") {
    if (text === "" && control.auto) return save("auto");
    const n = Number(text);
    if (text === "" || !Number.isFinite(n)) return (error.value = "Enter a number");
    if (control.integer && !Number.isInteger(n)) return (error.value = "Enter a whole number");
    if (control.min !== null && n < control.min) return (error.value = `At least ${control.min}`);
    if (control.max !== null && n > control.max) return (error.value = `At most ${control.max}`);
    return save(n);
  }
  if (control.type === "list") {
    return save(
      text
        .split("\n")
        .map((line) => line.trim())
        .filter(Boolean),
    );
  }
  if (control.type === "table") {
    if (!text) return save({});
    try {
      const parsed = JSON.parse(text);
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return (error.value = "Enter a JSON object");
      return save(parsed);
    } catch (e) {
      return (error.value = `Not valid JSON: ${(e as Error).message}`);
    }
  }
  save(String(draft.value ?? ""));
}

/** `auto | on | off` for an automatic toggle, whose values are `"auto" | true | false`. */
const selected = computed(() => {
  const v = props.setting.value;
  return v === true ? "on" : v === false ? "off" : String(v ?? "");
});

function choose(event: Event) {
  const v = (event.target as HTMLSelectElement).value;
  const autoBool = c.value.type === "select" && c.value.options.some((o) => o.value === "on");
  save(autoBool && v === "on" ? true : autoBool && v === "off" ? false : v);
}

function toggleAuto(on: boolean) {
  if (on) save("auto");
  else {
    const control = c.value;
    const fallback = typeof props.setting.default === "number" ? props.setting.default : control.type === "number" ? (control.min ?? 0) : 0;
    save(fallback);
  }
}

const defaultText = computed(() => {
  const d = props.setting.default;
  if (d === null || d === undefined || d === "") return "empty";
  if (Array.isArray(d)) return d.length ? d.join(", ") : "none";
  if (typeof d === "object") return Object.keys(d).length ? "table" : "empty";
  if (d === true) return "on";
  if (d === false) return "off";
  return String(d);
});
const listId = computed(() => `suggest-${props.setting.key}`);
</script>

<template>
  <div class="row" :class="{ wide: c.type === 'list' || c.type === 'table' }">
    <div class="about">
      <div class="title">
        <span class="label">{{ s.label }}</span>
        <i v-if="s.modified" class="dot" title="Changed from the default" />
        <span v-if="s.restart" class="tag" title="Takes effect the next time the engine starts">Restart</span>
      </div>
      <p v-if="s.description" class="desc">{{ s.description }}</p>
      <div class="meta mono">
        <span class="truncate">{{ s.key }}</span>
        <span class="default truncate">default: {{ defaultText }}</span>
      </div>
    </div>

    <div class="control">
      <p v-if="s.redacted" class="hidden">Contains credentials · edit with <code>dial config</code></p>

      <button
        v-else-if="c.type === 'toggle'"
        class="switch"
        :class="{ on: s.value === true }"
        role="switch"
        :aria-checked="s.value === true"
        :aria-label="s.label"
        @click="save(s.value !== true)"
      >
        <i />
      </button>

      <select v-else-if="c.type === 'select'" class="field" :value="selected" :aria-label="s.label" @change="choose">
        <option v-if="!c.options.some((o) => o.value === selected)" :value="selected">{{ selected || "—" }}</option>
        <option v-for="o in c.options" :key="o.value" :value="o.value">{{ o.label }}</option>
      </select>

      <div v-else-if="c.type === 'number'" class="number">
        <label v-if="c.auto" class="auto">
          <input type="checkbox" :checked="isAuto" @change="toggleAuto(($event.target as HTMLInputElement).checked)" />
          Auto
        </label>
        <input
          v-if="!isAuto"
          v-model="draft"
          class="field num mono"
          type="number"
          :min="c.min ?? undefined"
          :max="c.max ?? undefined"
          :step="c.step ?? (c.integer ? 1 : 'any')"
          :aria-label="s.label"
          @change="commit"
          @keydown.enter="commit"
        />
      </div>

      <template v-else-if="c.type === 'text'">
        <input
          v-model="draft"
          class="field"
          spellcheck="false"
          :list="c.suggestions.length ? listId : undefined"
          :placeholder="s.default ? String(s.default) : ''"
          :aria-label="s.label"
          @change="commit"
          @keydown.enter="commit"
        />
        <datalist v-if="c.suggestions.length" :id="listId">
          <option v-for="o in c.suggestions" :key="o.value" :value="o.value">{{ o.label }}</option>
        </datalist>
      </template>

      <textarea
        v-else
        v-model="draft"
        class="field area mono"
        spellcheck="false"
        :rows="Math.min(12, Math.max(3, draft.split('\n').length + 1))"
        :placeholder="c.type === 'list' ? 'One per line' : '{ }'"
        :aria-label="s.label"
        @blur="commit"
        @keydown.meta.enter="commit"
        @keydown.ctrl.enter="commit"
      />

      <p v-if="error" class="error">{{ error }}</p>
    </div>

    <button
      class="reset"
      :class="{ shown: s.modified }"
      :disabled="!s.modified"
      title="Reset to the default"
      aria-label="Reset to the default"
      @click="emit('reset')"
    >
      <Icon name="undo" :size="14" />
    </button>
  </div>
</template>

<style scoped>
.row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(180px, 260px) 28px;
  gap: 8px 20px;
  align-items: start;
  padding: 14px 0;
  border-bottom: 1px solid var(--border);
}
.row.wide {
  grid-template-columns: minmax(0, 1fr) 28px;
}
.row.wide .control {
  grid-column: 1;
  grid-row: 2;
}
.row.wide .reset {
  grid-column: 2;
  grid-row: 1;
}
.about {
  min-width: 0;
}
.title {
  display: flex;
  align-items: center;
  gap: 8px;
}
.label {
  font-size: var(--text-md);
  color: var(--text);
}
.dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--accent);
}
.tag {
  padding: 0 6px;
  border-radius: 999px;
  border: 1px solid var(--border-strong);
  font-size: var(--text-xs);
  color: var(--text-faint);
}
.desc {
  margin: 3px 0 0;
  font-size: var(--text-sm);
  line-height: 1.5;
  color: var(--text-muted);
  user-select: text;
  -webkit-user-select: text;
}
.meta {
  display: flex;
  gap: 12px;
  margin-top: 4px;
  font-size: var(--text-xs);
  color: var(--text-faint);
}
.default {
  opacity: 0.8;
}
.control {
  min-width: 0;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 4px;
}
.row.wide .control {
  align-items: stretch;
}
.field {
  width: 100%;
  min-height: 32px;
  padding: 5px 10px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: var(--surface);
  color: var(--text);
  font: inherit;
  font-size: var(--text-sm);
  outline: none;
  transition: border-color 120ms var(--ease);
}
.field:focus {
  border-color: var(--accent);
}
select.field {
  appearance: none;
  padding-right: 28px;
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%238b8981' stroke-width='2' stroke-linecap='round'%3E%3Cpath d='M6 9l6 6 6-6'/%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 10px center;
}
.area {
  resize: vertical;
  font-size: var(--text-mono);
  line-height: 1.5;
}
.number {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 10px;
  width: 100%;
}
.num {
  max-width: 140px;
  text-align: right;
}
.auto {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: var(--text-sm);
  color: var(--text-muted);
}
.auto input {
  accent-color: var(--accent);
}
.switch {
  position: relative;
  width: 34px;
  height: 20px;
  margin-top: 2px;
  border-radius: 999px;
  background: var(--raised);
  border: 1px solid var(--border-strong);
  transition: background 150ms var(--ease);
}
.switch i {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: var(--text-muted);
  transition:
    transform 150ms var(--ease),
    background 150ms var(--ease);
}
.switch.on {
  background: var(--accent);
  border-color: var(--accent);
}
.switch.on i {
  transform: translateX(14px);
  background: var(--on-accent);
}
.hidden {
  margin: 4px 0 0;
  font-size: var(--text-sm);
  color: var(--text-faint);
  text-align: right;
}
.error {
  margin: 0;
  font-size: var(--text-xs);
  color: var(--danger);
}
.reset {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  border-radius: var(--radius-sm);
  color: var(--text-faint);
  visibility: hidden;
}
.reset.shown {
  visibility: visible;
}
.reset:hover {
  background: var(--hover);
  color: var(--text);
}
</style>
