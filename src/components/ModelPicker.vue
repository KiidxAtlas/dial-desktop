<script setup lang="ts">
// The model switcher in the composer, like `/model` in a terminal agent: search, pick, and set
// how hard the model thinks. The choice applies to this session.
import { computed, nextTick, onBeforeUnmount, ref, toRef, watch } from "vue";
import type { LocalModelRemoval, ModelOption, OpenSnapshot } from "../types";
import { engine, models, settings, settingsPage, showToast } from "../bridge";
import { engineCall, loadModes, modes } from "../engineTools";
import { focusComposer, ui } from "../ui";
import { buildPickerGroups, pickerProviders, usableModels } from "../modelPicker";
import { lastRequestLabel, selectedRoute, type RequestObservation } from '../setupState';
import { normalizeArrowKey } from "../macKeys";
import Icon from "./Icon.vue";
import ModesMenu from "./ModesMenu.vue";
const props = defineProps<{ snap: OpenSnapshot }>();

const root = ref<HTMLElement | null>(null);
const search = ref<HTMLInputElement | null>(null);
const query = ref("");
const active = ref(0);
const filter = toRef(ui, 'modelPickerFilter');
const expandedProviders = ref<Set<string>>(new Set());
const downloadedOnly = computed(() => filter.value === "downloaded");
const details = ref<{ row: ModelOption; plan?: LocalModelRemoval; error?: string } | null>(null);
const managing = ref(false);
const modesPage = ref(false);
function showModes() {
  if (props.snap.connection.state !== 'live') return;
  modesPage.value = true;
  open();
}
defineExpose({ showModes });
/** Read-only session modes, named next to the model so it is clear Dial won't change files. */
const activeModes = computed(() => {
  const m = props.snap.connection.state === "live" ? modes.value : null;
  if (!m) return [];
  return [m.plan && "Plan", m.modes.vibe && "Vibe"].filter((x): x is string => !!x);
});
// Modes can change from the menu, a slash command or another client; re-read between turns.
watch(
  () => [props.snap.sessionId, props.snap.connection.state, props.snap.turnRunning] as const,
  ([, state, running]) => state === "live" && !running && void loadModes(),
  { immediate: true },
);

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

watch([() => props.snap.workspace.root, () => props.snap.sessionId, filter, () => ui.modelPickerOpen], () => { details.value = null; });

const current = computed(() => props.snap.model);
/** The `model` setting: what new sessions start with (Settings › Overview › Default model). */
const defaultModel = computed(() => {
  const value = settingsPage.value?.settings.find((s) => s.key === "model")?.value;
  return typeof value === "string" && value ? value : null;
});
watch(() => ui.modelPickerOpen, (open) => {
  if (open && !settingsPage.value && props.snap.connection.state === "live") void settings.load();
});
function changeDefault() {
  close(false);
  ui.settingsTab = "";
  ui.settingsOpen = true;
}
const lastRequest = ref<RequestObservation | null>(null);
watch([() => ui.modelPickerOpen, () => props.snap.workspace.root, () => props.snap.sessionId, () => current.value?.provider, () => current.value?.modelId, () => props.snap.turnRunning, () => props.snap.connection.state], async (_, __, cleanup) => {
  let active = true;
  cleanup(() => { active = false; });
  lastRequest.value = null;
  if (!ui.modelPickerOpen || props.snap.turnRunning || props.snap.connection.state !== 'live') return;
  try {
    const data = await engineCall<{session_id: string; last_request: RequestObservation | null}>('evidence', {action: 'model_request'}, props.snap.workspace.root);
    if (active && data.session_id === props.snap.sessionId) lastRequest.value = data.last_request;
  } catch { /* Older engines have no request evidence. */ }
}, { immediate: true });
const usable = computed(() => props.snap.connection.state === "live" || props.snap.connection.state === "preview");
const isCurrent = (m: ModelOption) =>
  !!current.value && m.modelId === current.value.modelId && (!current.value.provider || m.provider === current.value.provider);

const cleanedModels = computed(() => usableModels(models.value, isCurrent));
const providers = computed(() => pickerProviders(cleanedModels.value));

watch(providers, (list) => {
  if (filter.value !== "all" && filter.value !== "downloaded" && !list.some((p) => p.provider === filter.value)) {
    filter.value = "all";
  }
});

function expandProvider(provider: string) {
  const next = new Set(expandedProviders.value);
  next.add(provider);
  expandedProviders.value = next;
}

const groups = computed(() =>
  buildPickerGroups(cleanedModels.value, {
    query: query.value,
    filter: filter.value,
    expandedProviders: expandedProviders.value,
    isCurrent,
  }),
);
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
  expandedProviders.value = new Set();
  if (refocus) focusComposer();
}

const changingEngine = ref(false);
const engineNames: Record<string, string> = { auto: 'Automatic', mlx: 'MLX', 'llama-cpp': 'llama.cpp', atomic: 'Atomic Engine' };

async function changeEngine(m: ModelOption, event: Event) {
  const select = event.target as HTMLSelectElement;
  const choice = select.value;
  const pin = m.localEngine;
  if (!pin) return;
  select.value = pin.choices.includes(pin.selected) ? pin.selected : '';
  if (changingEngine.value || !pin.choices.includes(choice)) return;
  const root = props.snap.workspace.root;
  changingEngine.value = true;
  try {
    if (choice === 'atomic' && !await engine.setupAtomic()) return;
    if (props.snap.workspace.root !== root) return;
    await engineCall('set_setting', { key: pin.setting, value: choice, scope: 'global' }, root);
    if (props.snap.workspace.root === root) {
      await engine.refreshModels();
      showToast(`${m.label} will use ${engineNames[choice]} on load.`);
    }
  } catch (error) { showToast(String(error)); }
  finally { changingEngine.value = false; }
}

function choose(m: ModelOption) {
  if (changingEngine.value) return;
  if (props.snap.turnRunning || !isCurrent(m) || m.local || m.provider === 'mlx') {
    const snap = props.snap;
    void engine.setModel(snap, m.provider, m.modelId).then(ok => {
      if (ok && snap.turnRunning) showToast(`Next turn in this session: ${m.label}`);
    });
  }
  close();
}

function think(level: string) {
  const snap = props.snap;
  void engine.setThinking(snap, level).then(ok => {
    if (ok && snap.turnRunning) showToast(`Next turn in this session: ${level} effort`);
  });
}

function onKey(event: KeyboardEvent) {
  if (event.target instanceof HTMLSelectElement) return; // Let the native engine menu handle arrows and Enter.
  const key = normalizeArrowKey(event.key);
  const n = flat.value.length;
  if (key === "ArrowDown" && n) {
    event.preventDefault();
    active.value = (active.value + 1) % n;
    scrollActive();
  } else if (key === "ArrowUp" && n) {
    event.preventDefault();
    active.value = (active.value - 1 + n) % n;
    scrollActive();
  } else if (key === "Enter" && n) {
    event.preventDefault();
    const pick = flat.value[active.value];
    if (pick) choose(pick);
  } else if (key === "Escape") {
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
      void engine.discoverModels();
      const index = flat.value.findIndex(isCurrent);
      active.value = index < 0 ? 0 : index;
      nextTick(() => {
        search.value?.focus();
        scrollActive();
      });
      document.addEventListener("mousedown", onOutside);
    } else {
      modesPage.value = false;
      document.removeEventListener("mousedown", onOutside);
    }
  },
);
watch([query, filter], () => (active.value = 0));
onBeforeUnmount(() => document.removeEventListener("mousedown", onOutside));
</script>

<template>
  <div ref="root" class="picker">
    <button
      class="trigger"
      :disabled="!usable"
      :title="snap.loadingLocalModel ? `Loading ${snap.loadingLocalModel}` : current && snap.connection.state === 'live' ? `${current.label} · ${current.provider || 'unknown provider'}/${current.modelId}\n${selectedRoute(current, models)}. ${lastRequestLabel(current, lastRequest)}.` : `Model and session modes · ${current?.label ?? 'Choose model'}`" :aria-expanded="ui.modelPickerOpen"
      @click.stop="ui.modelPickerOpen ? close() : open()"
    >
      <Icon name="chip" :size="14" />
      <span class="truncate" aria-live="polite">{{ snap.loadingLocalModel ? `Loading ${snap.loadingLocalModel}…` : current?.label ?? 'Model' }}</span>
      <span v-if="thinkingLabel && !snap.loadingLocalModel" class="level">· {{ thinkingLabel }}</span>
      <Icon name="chevronDown" :size="12" />
    </button>
    <button
      v-for="name in activeModes"
      :key="name"
      class="mode-chip"
      :title="`${name} mode is on: Dial explores and plans without changing files. Click to change.`"
      @click.stop="showModes"
    >
      {{ name }}
    </button>

    <Transition name="pop">
      <div v-if="ui.modelPickerOpen" class="menu" @click.stop @keydown.esc.stop="close()">
        <div v-if="snap.connection.state === 'live'" class="filters tabs" role="group" aria-label="Session settings">
          <button :aria-pressed="!modesPage" @click="modesPage = false; nextTick(() => search?.focus())">Model</button>
          <button :aria-pressed="modesPage" @click="showModes">Session modes</button>
        </div>
        <ModesMenu v-if="modesPage && snap.connection.state === 'live'" :snap="snap" />
        <template v-else>
        <p v-if="snap.turnRunning" class="empty">Changes apply to this session after the running turn.</p>
        <label class="search">
          <Icon name="search" :size="14" />
          <input ref="search" v-model="query" placeholder="Search models" spellcheck="false" @keydown="onKey" />
          <button class="refresh" title="Reload the model list" @click="engine.refreshModels()">
            <Icon name="refresh" :size="13" />
          </button>
        </label>
        <!-- One row of filters: where models come from, then each provider. -->
        <div class="providers" role="group" aria-label="Show models from">
          <button :aria-pressed="filter === 'all'" @click="filter = 'all'">All</button>
          <button :aria-pressed="filter === 'downloaded'" @click="filter = 'downloaded'">Downloaded</button>
          <template v-if="providers.length > 1">
            <button
              v-for="p in providers"
              :key="p.provider"
              :aria-pressed="filter === p.provider"
              @click="filter = filter === p.provider ? 'all' : p.provider"
            >
              {{ p.provider }} <span class="count">{{ p.count }}</span>
            </button>
          </template>
        </div>

        <div class="list">
          <p v-if="downloadedOnly && cleanedModels.length && !cleanedModels.some((m) => m.local)" class="empty">
            No downloaded models found by Dial. After adding a local model, reload this list.
          </p>
          <div v-else-if="!cleanedModels.length" class="empty none">
            <span>Couldn’t find any models yet. Sign in to a provider or download a model, then reload.</span>
            <button class="btn btn-secondary" @click="engine.refreshModels()"><Icon name="refresh" :size="13" /> Reload</button>
          </div>
          <p v-else-if="!groups.length" class="empty">No model matches “{{ query }}”.</p>
          <template v-for="group in groups" :key="group.title">
            <div class="group">{{ group.title }}</div>
            <button
              v-if="group.hiddenCount && group.provider"
              class="expand-row"
              @click="expandProvider(group.provider)"
            >
              Show all {{ group.hiddenCount }} {{ group.title }} models…
            </button>
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
              <select v-if="m.local && m.localEngine && snap.connection.state === 'live'" class="engine-select" :aria-label="`Inference engine for ${m.label}`" title="Remember an inference engine for this model" :value="m.localEngine.choices.includes(m.localEngine.selected) ? m.localEngine.selected : ''" :disabled="changingEngine || snap.turnRunning" @change="changeEngine(m, $event)">
                <option v-if="!m.localEngine.choices.includes(m.localEngine.selected)" value="" disabled>Choose an engine…</option>
                <option v-for="id in m.localEngine.choices" :key="id" :value="id">{{ engineNames[id] ?? id }}</option>
              </select>
              <button v-if="downloadedOnly && snap.connection.state === 'live'" class="refresh" :aria-label="`Details for ${m.label}`" title="Model details" @click="inspectModel(m)">···</button>
            </div>
          </template>
        </div>
        <p v-if="changingEngine" class="empty" role="status">Preparing the engine choice…</p>
        <div v-if="downloadedOnly" class="model-details">
          <p v-if="!details">Choose a model above to load it, or get more from the model library.</p>
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
        <div class="more-links">
          <button class="link" @click="close(false); ui.settingsTab = 'providers'; ui.settingsOpen = true"><Icon name="globe" :size="13" /> Providers…</button>
          <button class="link" @click="close(false); ui.settingsTab = 'library'; ui.settingsOpen = true"><Icon name="download" :size="13" /> Get models…</button>
          <button class="link" title="Helper models for background work, and saved model sets" @click="close(false); ui.settingsTab = 'roles'; ui.settingsOpen = true"><Icon name="list" :size="13" /> Roles &amp; presets…</button>
        </div>
        <div class="foot">
          <span v-if="snap.connection.state === 'live'" class="default-model">
            New sessions use <span :class="{ mono: defaultModel }" :title="defaultModel ?? undefined">{{ defaultModel ?? "the engine default" }}</span> ·
            <button class="link" title="Change the default in Settings" @click="changeDefault">Change</button>
          </span>
          <span class="keys"><kbd>↑↓</kbd> <kbd>↵</kbd> <kbd>esc</kbd></span>
        </div>
        </template>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.engine-select { max-width: 9em; align-self: center; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--surface); color: var(--text); padding: 3px; font-size: var(--text-xs); }
.picker {
  position: relative;
  display: flex;
  align-items: center;
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
/* In a narrow composer the level goes first, so the model name keeps its room rather than both clipping. */
@container composer-bar (max-width: 500px) {
  .level { display: none; }
}
.mode-chip {
  height: 22px;
  margin-left: 2px;
  padding: 0 8px;
  border-radius: 999px;
  border: 1px solid color-mix(in srgb, var(--accent) 45%, transparent);
  color: var(--accent);
  font-size: var(--text-xs);
  font-weight: 600;
  white-space: nowrap;
}
.mode-chip:hover {
  background: color-mix(in srgb, var(--accent) 12%, transparent);
}
.trigger > :deep(svg) {
  flex: none;
}
.menu {
  position: absolute;
  left: 0;
  bottom: calc(100% + var(--pop-gap));
  z-index: 30;
  width: 420px;
  display: flex;
  flex-direction: column;
  border-radius: var(--radius-lg);
  background: var(--raised);
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
/* Model / Session modes: one segmented switch, the only navigation in the menu. */
.filters.tabs {
  padding: 8px 10px;
}
.filters.tabs button {
  flex: 1;
  padding: 5px 8px;
}
.filters.tabs button[aria-pressed="true"] {
  background: var(--active);
  font-weight: 500;
}
.providers {
  display: flex;
  gap: 4px;
  padding: 5px 8px;
  border-bottom: 1px solid var(--border);
  overflow-x: auto;
  scrollbar-width: none;
}
.providers::-webkit-scrollbar { display: none; }
.providers button {
  flex: none;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2px 7px;
  border-radius: 999px;
  border: 1px solid var(--border);
  font-size: var(--text-xs);
  color: var(--text-muted);
  white-space: nowrap;
}
.providers button:hover,
.providers button[aria-pressed="true"] {
  color: var(--text);
  background: var(--hover);
  border-color: var(--border-strong);
}
.providers .count { color: var(--text-faint); }
.expand-row {
  display: block;
  width: 100%;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  text-align: left;
  font-size: var(--text-sm);
  color: var(--text-muted);
}
.expand-row:hover {
  background: var(--hover);
  color: var(--text);
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
.empty.none {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 10px;
  line-height: 1.5;
}
.more-links {
  display: flex;
  flex-wrap: wrap;
  gap: 2px 4px;
  padding: 6px 8px;
  border-top: 1px solid var(--border);
}
.more-links .link {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 4px 8px;
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
  color: var(--text-muted);
}
.more-links .link:hover {
  background: var(--hover);
  color: var(--text);
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
  border-radius: 999px;
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
/* Reasoning: a segmented control, like every other one-of-several choice. */
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 2px;
  padding: 2px;
  border-radius: var(--radius-sm);
  background: var(--active);
}
.chip {
  height: 24px;
  padding: 0 10px;
  border-radius: 6px;
  font-size: var(--text-sm);
  color: var(--text-muted);
  text-transform: capitalize;
  transition: background 120ms var(--ease), color 120ms var(--ease);
}
.chip:hover {
  color: var(--text);
}
.chip.on {
  background: var(--raised);
  color: var(--text);
  box-shadow: 0 1px 2px rgb(0 0 0 / 14%), 0 0 0 1px var(--border);
}
.faint {
  color: var(--text-faint);
  text-transform: none;
}
.foot {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 7px 12px;
  border-top: 1px solid var(--border);
  font-size: var(--text-xs);
  color: var(--text-faint);
}
.foot .default-model {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.foot .mono {
  font-family: var(--font-mono);
}
.foot .link {
  color: var(--accent);
  font: inherit;
}
.foot .link:hover {
  text-decoration: underline;
}
.foot .keys {
  flex: none;
  margin-left: auto;
}
.foot kbd {
  padding: 0 4px;
  border-radius: 4px;
  background: var(--hover);
  font: inherit;
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
