<script setup lang="ts">
// The command palette (⌘K): type to find an action or a session, arrows to move, Enter to run.
import { computed, nextTick, ref, watch } from "vue";
import type { OpenSnapshot } from "../types";
import { ui } from "../ui";
import { commands, filterCommands, highlightMatch, type Command } from "../commands";
import Icon, { PATHS } from "./Icon.vue";
import { workspaceFiles } from "../bridge";
import { showFile } from "../workspace";
import { loadRecentProjects } from "../projects";

const props = defineProps<{ snap: OpenSnapshot }>();

const query = ref("");
const active = ref(0);
const input = ref<HTMLInputElement | null>(null);
const list = ref<HTMLElement | null>(null);

const paths = ref<string[]>([]);
const status = ref("");
watch(() => [ui.filePalette, ui.paletteOpen, props.snap.workspace.root, query.value] as const, ([files, open, , q], _, cleanup) => {
  paths.value = [];
  status.value = "Type part of a file path";
  if (!files || !open || !q.trim()) return;
  status.value = "Searching…";
  let current = true;
  const timer = setTimeout(async () => {
    try {
      const found = await workspaceFiles.search(q.trim());
      if (current) { paths.value = found; status.value = "No matching files"; }
    } catch (e) { if (current) status.value = String(e); }
  }, 100);
  cleanup(() => { current = false; clearTimeout(timer); });
});
const results = computed<Command[]>(() => ui.filePalette ? paths.value.map(path => ({ id: path, group: "Project files", label: path, icon: "file", run: () => showFile(path) })) : filterCommands(commands(props.snap), query.value));
/** Results under group headings, keeping their ranked order; `index` counts across groups. */
const groups = computed(() => {
  const out: { name: string; items: { command: Command; index: number }[] }[] = [];
  results.value.forEach((command, index) => {
    let group = out.find((g) => g.name === command.group);
    if (!group) out.push((group = { name: command.group, items: [] }));
    group.items.push({ command, index });
  });
  return out;
});
/** Display order, which can differ from rank order once grouped. */
const ordered = computed(() => groups.value.flatMap((g) => g.items));

const icon = (name?: string) => (name && name in PATHS ? (name as keyof typeof PATHS) : null);

watch(
  () => [ui.paletteOpen, ui.filePalette] as const,
  async ([open]) => {
    if (!open) return;
    void loadRecentProjects();
    query.value = "";
    active.value = 0;
    await nextTick();
    input.value?.focus();
  },
);
watch(query, () => (active.value = 0));

function close() {
  ui.paletteOpen = false;
  ui.filePalette = false;
}

function run(command: Command | undefined) {
  if (!command) return;
  close();
  command.run();
}

async function move(by: number) {
  const n = ordered.value.length;
  if (!n) return;
  const at = ordered.value.findIndex((item) => item.index === active.value);
  active.value = ordered.value[(at + by + n) % n].index;
  await nextTick();
  list.value?.querySelector(".on")?.scrollIntoView({ block: "nearest" });
}

function onKey(event: KeyboardEvent) {
  if (event.isComposing) return;
  if (event.key === "ArrowDown" || (event.ctrlKey && event.key === "n")) {
    event.preventDefault();
    move(1);
  } else if (event.key === "ArrowUp" || (event.ctrlKey && event.key === "p")) {
    event.preventDefault();
    move(-1);
  } else if (event.key === "Enter") {
    event.preventDefault();
    run(results.value[active.value]);
  } else if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    close();
  }
}
</script>

<template>
  <Transition name="fade">
    <div v-if="ui.paletteOpen" class="overlay" @mousedown.self="close">
      <div class="palette" role="dialog" :aria-label="ui.filePalette ? 'Quick-open project file' : 'Command palette'">
        <label class="field">
          <Icon name="search" :size="16" />
          <input
            ref="input"
            v-model="query"
            :placeholder="ui.filePalette ? 'Find a project file' : 'Search actions and sessions'"
            spellcheck="false"
            role="combobox"
            aria-expanded="true"
            @keydown="onKey"
          />
          <kbd>esc</kbd>
        </label>
        <div ref="list" class="results" role="listbox">
          <p v-if="!results.length" class="none">{{ ui.filePalette ? status : `Nothing matches “${query}”.` }}</p>
          <template v-for="group in groups" :key="group.name">
            <div class="group">{{ group.name }}</div>
            <button
              v-for="{ command, index } in group.items"
              :key="command.id"
              role="option"
              :aria-selected="index === active"
              :class="{ on: index === active }"
              @mousemove="active = index"
              @click="run(command)"
            >
              <Icon v-if="icon(command.icon)" :name="icon(command.icon)!" :size="15" />
              <span v-else class="blank" />
              <span class="label truncate"><template v-for="(part, p) in highlightMatch(command.label, query)" :key="p"><mark v-if="part.hit">{{ part.text }}</mark><template v-else>{{ part.text }}</template></template></span>
              <kbd v-if="command.hint">{{ command.hint }}</kbd>
            </button>
          </template>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  z-index: 70;
  display: flex;
  justify-content: center;
  align-items: flex-start;
  padding-top: 12vh;
  background: rgb(0 0 0 / 32%);
  -webkit-backdrop-filter: blur(2px);
  backdrop-filter: blur(2px);
}
.palette {
  width: min(600px, calc(100vw - 48px));
  max-height: min(460px, 70vh);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border-radius: var(--radius-lg);
  background: var(--raised);
  box-shadow: var(--shadow-pop);
  animation: rise 160ms var(--ease) both;
}
.field {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 50px;
  flex: none;
  padding: 0 16px;
  border-bottom: 1px solid var(--border);
  color: var(--text-faint);
}
.field input {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 15px;
}
.field input::placeholder {
  color: var(--text-faint);
}
.results {
  overflow-y: auto;
  padding: 6px;
}
.group {
  padding: 10px 10px 4px;
  font-size: var(--text-xs);
  color: var(--text-faint);
}
.group:first-child {
  padding-top: 4px;
}
button {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  height: 36px;
  padding: 0 10px;
  border-radius: var(--radius-sm);
  font-size: 13.5px;
  color: var(--text-muted);
  text-align: left;
}
button svg {
  flex: none;
  color: var(--text-faint);
}
button.on {
  background: var(--hover);
  color: var(--text);
}
.blank {
  width: 15px;
  flex: none;
}
.label {
  flex: 1;
}
.label mark {
  background: none;
  color: var(--text-strong);
  font-weight: 600;
  text-decoration: underline;
  text-decoration-color: color-mix(in srgb, var(--accent) 70%, transparent);
  text-underline-offset: 3px;
}
button.on::before {
  content: "";
  position: absolute;
  left: 0;
  top: 8px;
  bottom: 8px;
  width: 2px;
  border-radius: 2px;
  background: var(--accent);
}
button {
  position: relative;
}
kbd {
  flex: none;
}
.none {
  margin: 14px 10px;
  font-size: 13px;
  color: var(--text-faint);
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
