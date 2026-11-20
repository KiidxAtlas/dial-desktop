<script setup lang="ts">
// The model switcher in the composer, like `/model` in a terminal agent: search, pick, and set
// how hard the model thinks. The choice applies to this session.
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import type { LocalModelRemoval, ModelOption, OpenSnapshot } from "../types";
import { engine, models } from "../bridge";
import { focusComposer, ui } from "../ui";
import Icon from "./Icon.vue";

const props = defineProps<{ snap: OpenSnapshot }>();

const root = ref<HTMLElement | null>(null);
const search = ref<HTMLInputElement | null>(null);
const query = ref("");
const active = ref(0);
const downloadedOnly = ref(false);
const details = ref<{ row: ModelOption; plan?: LocalModelRemoval; error?: string } | null>(null);
const managing = ref(false);

async function inspectModel(row: ModelOption) {
  const root = props.snap.workspace.root;
  details.value = { row };
  const request = details.value;
  try {
    const plan = await engine.localModelDetails(root, row.modelId);
    if (details.value === request) details.value = { row, plan };
  } catch (error) {
    if (details.value === request) details.value = { row, error: String(error) };
  }
}

async function removeModel() {
  const selected = details.value;
  if (!selected?.plan || managing.value || cannotRemove(selected.row)) return;
  managing.value = true;
  try {
    if (await engine.removeLocalModel(props.snap.workspace.root, selected.row.modelId, selected.plan) && details.value === selected) details.value = null;
  } catch (error) {
    if (details.value === selected) details.value = { ...selected, error: String(error) };
  } finally {
    managing.value = false;
  }
}

function cannotRemove(row: ModelOption) {
  if (props.snap.connection.state !== "live") return "Connect to Dial to remove models.";
  if (props.snap.turnRunning) return "Wait for the running turn to finish.";
  if (isCurrent(row) || row.configured) return "Switch away from this model and remove it from Dial’s configuration before deleting it.";
  return "";
}

watch([() => props.snap.workspace.root, () => props.snap.sessionId, downloadedOnly, () => ui.modelPickerOpen], () => { details.value = null; });

const current = computed(() => props.snap.model);
const usable = computed(() => props.snap.connection.state === "live" || props.snap.connection.state === "preview");
const isCurrent = (m: ModelOption) =>
  !!current.value && m.modelId === current.value.modelId && (!current.value.provider || m.provider === current.value.provider);

interface Group {
  title: string;
  rows: ModelOption[];
}

const groups = computed<Group[]>(() => {
  const words = query.value.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const match = (m: ModelOption) => {
    const text = `${m.label} ${m.modelId} ${m.provider}`.toLowerCase();
    return words.every((w) => text.includes(w));
  };
  const rows = models.value.filter(match);
  const local = rows.filter((m) => m.local);
  if (downloadedOnly.value) return local.length ? [{ title: "Downloaded models", rows: local }] : [];
  const mine = rows.filter((m) => !m.local && (m.configured || isCurrent(m)));
  const byProvider = new Map<string, ModelOption[]>();
  for (const m of rows) {
    if (m.local || mine.includes(m)) continue;
    byProvider.set(m.provider, [...(byProvider.get(m.provider) ?? []), m]);
  }
  const out: Group[] = [];
  if (mine.length) out.push({ title: "Your models", rows: mine });
  if (local.length) out.push({ title: "Downloaded models", rows: local });
  for (const [provider, list] of byProvider) out.push({ title: provider, rows: list });
  return out;
});
const flat = computed(() => groups.value.flatMap((g) => g.rows));

const levels = computed(() => (current.value?.thinkingSettable ? current.value.levels : []));
const thinkingLabel = computed(() => {
  const m = current.value;
  if (!m?.thinkingSettable) return null;
  return m.thinking ?? (m.defaultThinking ? `${m.defaultThinking} (default)` : "default");
});

function context(m: ModelOption) {
  const n = m.contextWindow;
  if (!n) return "";
  return n >= 1_000_000 ? `${+(n / 1_000_000).toFixed(1)}M` : `${Math.round(n / 1000)}K`;
}

function open() {
  if (!usable.value) return;
  ui.modelPickerOpen = true;
}

function close(refocus = true) {
  ui.modelPickerOpen = false;
  query.value = "";
  if (refocus) focusComposer();
}

function choose(m: ModelOption) {
  if (!isCurrent(m)) engine.setModel(m.provider, m.modelId);
  close();
}

function think(level: string) {
  engine.setThinking(level);
}

function onKey(event: KeyboardEvent) {
  const n = flat.value.length;
  if (event.key === "ArrowDown" && n) {
    event.preventDefault();
    active.value = (active.value + 1) % n;
    scrollActive();
  } else if (event.key === "ArrowUp" && n) {
    event.preventDefault();
    active.value = (active.value - 1 + n) % n;
    scrollActive();
  } else if (event.key === "Enter" && n) {
    event.preventDefault();
    const pick = flat.value[active.value];
    if (pick) choose(pick);
  } else if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    close();
  }
}

function scrollActive() {
  nextTick(() => root.value?.querySelector(".row.active")?.scrollIntoView({ block: "nearest" }));
}

function onOutside(event: MouseEvent) {
  if (root.value && !root.value.contains(event.target as Node)) close(false);
}

watch(
  () => ui.modelPickerOpen,
  (isOpen) => {
    if (isOpen) {
      engine.refreshModels();
      const index = flat.value.findIndex(isCurrent);
      active.value = index < 0 ? 0 : index;
      nextTick(() => {
        search.value?.focus();
        scrollActive();
      });
      document.addEventListener("mousedown", onOutside);
    } else {
      document.removeEventListener("mousedown", onOutside);
    }
  },
);
watch([query, downloadedOnly], () => (active.value = 0));
onBeforeUnmount(() => document.removeEventListener("mousedown", onOutside));
</script>

<template>
  <div ref="root" class="picker">
    <button
      class="trigger"
      :disabled="!usable"
      :title="current ? `${current.provider ? current.provider + '/' : ''}${current.modelId} · switch model (/model)` : 'Model'"
      @click.stop="ui.modelPickerOpen ? close() : open()"
    >
      <Icon name="chip" :size="14" />
      <span class="truncate">{{ current?.label ?? "Model" }}</span>
      <span v-if="thinkingLabel" class="level">· {{ thinkingLabel }}</span>
      <Icon name="chevronDown" :size="12" />
    </button>

    <Transition name="pop">
      <div v-if="ui.modelPickerOpen" class="menu" @click.stop>
        <label class="search">
          <Icon name="search" :size="14" />
          <input ref="search" v-model="query" placeholder="Search models" spellcheck="false" @keydown="onKey" />
          <button class="refresh" title="Reload the model list" @click="engine.refreshModels()">
            <Icon name="refresh" :size="13" />
          </button>
        </label>
        <div class="filters" role="group" aria-label="Model source">
          <button :aria-pressed="!downloadedOnly" @click="downloadedOnly = false">All models</button>
          <button :aria-pressed="downloadedOnly" @click="downloadedOnly = true">Downloaded</button>
          <button @click="close(false); ui.settingsTab = 'library'; ui.settingsOpen = true">Get models…</button>
          <button title="Helper models for background work, and saved model sets" @click="close(false); ui.settingsTab = 'roles'; ui.settingsOpen = true">Roles &amp; presets…</button>
        </div>

        <div class="list">
          <p v-if="downloadedOnly && models.length && !models.some((m) => m.local)" class="empty">
            No downloaded models found by Dial. After adding a local model, reload this list.
          </p>
          <p v-else-if="!models.length" class="empty">No models listed yet. Use reload to check again.</p>
          <p v-else-if="!flat.length" class="empty">No model matches “{{ query }}”.</p>
          <template v-for="group in groups" :key="group.title">
            <div class="group">{{ group.title }}</div>
            <div v-for="m in group.rows" :key="`${m.provider}/${m.modelId}`" class="model-line">
              <button
                class="row"
                :class="{ active: flat[active] === m, current: isCurrent(m) }"
                @mouseenter="active = flat.indexOf(m)"
                @click="choose(m)"
              >
                <span class="name truncate">{{ m.label }}</span>
                <span class="id truncate">{{ m.provider }}/{{ m.modelId }}</span>
                <span class="tags">
                  <span v-if="m.reasoning" class="tag" title="Supports reasoning"><Icon name="brain" :size="11" /></span>
                  <span v-if="context(m)" class="tag">{{ context(m) }}</span>
                </span>
                <Icon v-if="isCurrent(m)" class="tick" name="check" :size="14" />
              </button>
              <button v-if="downloadedOnly && snap.connection.state === 'live'" class="refresh" :aria-label="`Details for ${m.label}`" title="Model details" @click="inspectModel(m)">···</button>
            </div>
          </template>
        </div>
        <div v-if="downloadedOnly" class="model-details">
          <p v-if="!details">Add models outside Desktop, then reload this list.</p>
          <template v-else>
            <strong>{{ details.row.label }}</strong>
            <p v-if="details.error" role="alert">{{ details.error }}</p>
            <p v-else-if="!details.plan" role="status">Looking up model details…</p>
            <template v-if="details.plan">
              <p>{{ details.plan.source }} · {{ (details.plan.bytes / 1024 ** 3).toFixed(2) }} GiB</p>
              <p class="mono" :title="details.plan.folder">{{ details.plan.folder }}</p>
              <p v-if="cannotRemove(details.row)">{{ cannotRemove(details.row) }}</p>
              <button v-else class="btn btn-danger" :disabled="managing" @click="removeModel">Remove from disk…</button>
            </template>
          </template>
        </div>

        <div v-if="levels.length" class="thinking">
          <span class="section-label">Reasoning</span>
          <div class="chips">
            <button class="chip" :class="{ on: !current?.thinking }" @click="think('default')">
              Default<span v-if="current?.defaultThinking" class="faint"> ({{ current.defaultThinking }})</span>
            </button>
            <button
              v-for="level in levels"
              :key="level"
              class="chip"
              :class="{ on: current?.thinking === level }"
              @click="think(level)"
            >
              {{ level }}
            </button>
          </div>
        </div>
        <div class="foot">↑↓ to move · Enter to switch · Esc to close · applies to this session</div>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.picker {
  position: relative;
  min-width: 0;
}
.trigger {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  max-width: min(320px, 100%);
  overflow: hidden;
  height: 28px;
  padding: 0 8px;
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
  color: var(--text-muted);
  transition:
    background 120ms var(--ease),
    color 120ms var(--ease);
}
.trigger:hover:not(:disabled) {
  background: var(--hover);
  color: var(--text);
}
.trigger:disabled {
  opacity: 0.5;
}
.level {
  color: var(--text-faint);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  min-width: 0;
  flex-shrink: 3;
}
.trigger > :deep(svg) {
  flex: none;
}
.menu {
  position: absolute;
  left: -6px;
  bottom: calc(100% + 10px);
  z-index: 30;
  width: 420px;
  display: flex;
  flex-direction: column;
  border-radius: var(--radius-lg);
  background: var(--raised);
  border: 1px solid var(--border-strong);
  box-shadow: var(--shadow-pop);
  overflow: hidden;
}
.search {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  border-bottom: 1px solid var(--border);
  color: var(--text-faint);
}
.search input {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 13.5px;
}
.refresh {
  display: grid;
  place-items: center;
  width: 24px;
  height: 24px;
  border-radius: var(--radius-sm);
}
.refresh:hover {
  background: var(--hover);
  color: var(--text);
}
.filters {
  display: flex;
  gap: 4px;
  padding: 6px 8px;
  border-bottom: 1px solid var(--border);
}
.filters button {
  padding: 4px 8px;
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.filters button:hover,
.filters button[aria-pressed="true"] {
  color: var(--text);
  background: var(--hover);
}
.model-line { display: flex; align-items: center; gap: 4px; }
.model-line .row { flex: 1; min-width: 0; }
.model-details { border-top: 1px solid var(--border); padding: 8px 12px; font-size: var(--text-sm); overflow-wrap: anywhere; }
.model-details p { margin: 4px 0; color: var(--text-faint); }
.list {
  max-height: 340px;
  overflow-y: auto;
  padding: 4px 6px 6px;
}
.empty {
  margin: 14px 8px;
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.group {
  padding: 10px 8px 4px;
  font-size: var(--text-xs);
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-faint);
}
.row {
  position: relative;
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto 18px;
  grid-template-rows: auto auto;
  column-gap: 10px;
  width: 100%;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  text-align: left;
}
.row.active {
  background: var(--hover);
}
.name {
  font-size: 13.5px;
}
.current .name {
  color: var(--text);
  font-weight: 500;
}
.id {
  grid-row: 2;
  font: var(--text-xs) / 1.4 var(--font-mono);
  color: var(--text-faint);
}
.tags {
  grid-row: 1 / 3;
  grid-column: 2;
  display: flex;
  align-items: center;
  gap: 4px;
}
.tag {
  display: inline-flex;
  align-items: center;
  height: 18px;
  padding: 0 6px;
  border-radius: 9px;
  background: var(--surface);
  font-size: var(--text-xs);
  color: var(--text-faint);
}
.tick {
  grid-row: 1 / 3;
  grid-column: 3;
  align-self: center;
  color: var(--text);
}
.thinking {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  border-top: 1px solid var(--border);
}
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}
.chip {
  height: 24px;
  padding: 0 9px;
  border-radius: 7px;
  border: 1px solid var(--border);
  font-size: var(--text-sm);
  color: var(--text-muted);
  text-transform: capitalize;
}
.chip:hover {
  color: var(--text);
}
.chip.on {
  border-color: transparent;
  background: var(--active);
  color: var(--text);
}
.faint {
  color: var(--text-faint);
  text-transform: none;
}
.foot {
  padding: 7px 12px;
  border-top: 1px solid var(--border);
  font-size: var(--text-xs);
  color: var(--text-faint);
}
.pop-enter-active,
.pop-leave-active {
  transition:
    opacity 140ms var(--ease),
    transform 140ms var(--ease);
}
.pop-enter-from,
.pop-leave-to {
  opacity: 0;
  transform: translateY(4px);
}
</style>
