<script setup lang="ts">
// Pick one value from a long list (models, themes, providers) by clicking or by filtering.
// `custom` also accepts a value that is not listed, typed into the filter.
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import Icon from "../Icon.vue";

export interface ComboOption {
  value: string;
  label: string;
  group?: string;
  detail?: string;
}

const props = defineProps<{
  modelValue: string;
  options: ComboOption[];
  label: string;
  /** What the empty value means ("Default", "Follow main model"); omitted: no empty choice. */
  emptyLabel?: string;
  custom?: boolean;
  placeholder?: string;
  disabled?: boolean;
}>();
const emit = defineEmits<{ "update:modelValue": [value: string] }>();

const open = ref(false);
const query = ref("");
const active = ref(0);
const root = ref<HTMLElement | null>(null);
const input = ref<HTMLInputElement | null>(null);
const list = ref<HTMLElement | null>(null);

const current = computed(() => props.options.find((o) => o.value === props.modelValue));
const shown = computed(() => {
  if (props.modelValue === "") return props.emptyLabel ?? "";
  return current.value?.label ?? props.modelValue;
});

const filtered = computed<ComboOption[]>(() => {
  const terms = query.value.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const hits = props.options.filter((o) => {
    const hay = `${o.label} ${o.value} ${o.group ?? ""} ${o.detail ?? ""}`.toLowerCase();
    return terms.every((t) => hay.includes(t));
  });
  const out: ComboOption[] = [];
  if (props.emptyLabel !== undefined && !terms.length) out.push({ value: "", label: props.emptyLabel });
  out.push(...hits.slice(0, 300));
  const typed = query.value.trim();
  if (props.custom && typed && !props.options.some((o) => o.value === typed))
    out.push({ value: typed, label: `Use “${typed}”`, group: "\u0000custom" });
  return out;
});
const more = computed(() => Math.max(0, props.options.length - 300));

/** Rows with a heading before the first option of each group. */
const rows = computed(() => {
  let last: string | undefined;
  return filtered.value.map((o, index) => {
    const heading = o.group && o.group !== last && !o.group.startsWith("\u0000") ? o.group : "";
    last = o.group;
    return { o, index, heading };
  });
});

function toggle() {
  if (props.disabled) return;
  open.value = !open.value;
}
watch(open, async (now) => {
  if (!now) return;
  query.value = "";
  active.value = Math.max(0, filtered.value.findIndex((o) => o.value === props.modelValue));
  await nextTick();
  input.value?.focus();
  scrollActive();
});
watch(query, () => (active.value = 0));

function pick(o: ComboOption) {
  open.value = false;
  if (o.value !== props.modelValue) emit("update:modelValue", o.value);
}
function scrollActive() {
  nextTick(() => list.value?.querySelector<HTMLElement>(".opt.active")?.scrollIntoView({ block: "nearest" }));
}
function key(event: KeyboardEvent) {
  const n = filtered.value.length;
  if (event.key === "ArrowDown" && n) {
    active.value = (active.value + 1) % n;
    scrollActive();
  } else if (event.key === "ArrowUp" && n) {
    active.value = (active.value - 1 + n) % n;
    scrollActive();
  } else if (event.key === "Enter") {
    const o = filtered.value[active.value];
    if (o) pick(o);
  } else if (event.key === "Escape") {
    open.value = false;
  } else return;
  event.preventDefault();
  event.stopPropagation();
}

function outside(event: MouseEvent) {
  if (open.value && root.value && !root.value.contains(event.target as Node)) open.value = false;
}
document.addEventListener("mousedown", outside, true);
onBeforeUnmount(() => document.removeEventListener("mousedown", outside, true));
</script>

<template>
  <div ref="root" class="combo">
    <button
      class="s-select face"
      :class="{ empty: modelValue === '' }"
      :aria-label="label"
      aria-haspopup="listbox"
      :aria-expanded="open"
      :disabled="disabled"
      :title="modelValue || emptyLabel"
      @click="toggle"
    >
      <span class="truncate">{{ shown || placeholder || "Choose…" }}</span>
    </button>
    <div v-if="open" class="pop" @keydown="key">
      <label class="find">
        <Icon name="search" :size="13" />
        <input ref="input" v-model="query" :placeholder="custom ? 'Search, or type a value' : 'Search'" spellcheck="false" />
      </label>
      <div ref="list" class="list" role="listbox" :aria-label="label">
        <template v-for="r in rows" :key="`${r.o.group ?? ''}:${r.o.value}`">
          <div v-if="r.heading" class="heading">{{ r.heading }}</div>
          <button
            class="opt"
            role="option"
            :aria-selected="r.o.value === modelValue"
            :class="{ active: r.index === active, custom: r.o.group === '\u0000custom' }"
            @mouseenter="active = r.index"
            @click="pick(r.o)"
          >
            <span class="truncate">{{ r.o.label }}</span>
            <span v-if="r.o.detail" class="detail truncate">{{ r.o.detail }}</span>
            <Icon v-if="r.o.value === modelValue" name="check" :size="13" class="tick" />
          </button>
        </template>
        <p v-if="!filtered.length" class="none">Nothing matches.</p>
        <p v-else-if="more && !query" class="none">Type to search {{ options.length }} options.</p>
      </div>
    </div>
  </div>
</template>

<style scoped>
.combo {
  position: relative;
  min-width: 0;
}
.face {
  display: flex;
  align-items: center;
  width: 240px;
  max-width: 100%;
  text-align: left;
}
.face.empty {
  color: var(--text-muted);
}
.pop {
  position: absolute;
  z-index: 30;
  top: calc(100% + 6px);
  right: 0;
  width: 340px;
  border-radius: 10px;
  background: var(--raised);
  box-shadow: var(--shadow-pop);
  overflow: hidden;
}
.find {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 12px;
  height: 38px;
  border-bottom: 1px solid var(--border);
  color: var(--text-faint);
}
.find input {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 13px;
}
.list {
  max-height: 300px;
  overflow-y: auto;
  padding: 4px;
}
.heading {
  padding: 8px 8px 4px;
  font-size: 11px;
  font-weight: 600;
  color: var(--text-faint);
}
.opt {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  min-height: 30px;
  padding: 0 8px;
  border-radius: 6px;
  font-size: 13px;
  color: var(--text);
  text-align: left;
}
.opt.active {
  background: var(--active);
}
.opt.custom {
  color: var(--accent);
}
.detail {
  flex: none;
  max-width: 40%;
  margin-left: auto;
  font-size: 11.5px;
  color: var(--text-faint);
}
.tick {
  flex: none;
  margin-left: auto;
  color: var(--accent);
}
.detail + .tick {
  margin-left: 0;
}
.none {
  margin: 0;
  padding: 10px 8px;
  font-size: 12px;
  color: var(--text-faint);
}
</style>
