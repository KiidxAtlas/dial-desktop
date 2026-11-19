<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from "vue";
import type { OpenSnapshot, Setting, SettingScope } from "../types";
import { settings, settingsPage } from "../bridge";
import { ui } from "../ui";
import Icon from "./Icon.vue";
import SettingRow from "./SettingRow.vue";
import BackgroundSettings from "./BackgroundSettings.vue";
import AppearanceSettings from "./AppearanceSettings.vue";

const props = defineProps<{ snap: OpenSnapshot }>();

const page = computed(() => settingsPage.value);
const tab = ref("");
const query = ref("");
const scope = ref<SettingScope>("global");
const changedOnly = ref(false);
const search = ref<HTMLInputElement | null>(null);
const body = ref<HTMLElement | null>(null);

const live = computed(() => props.snap.connection.state === "live");
/** The desktop app's own tabs, which work without an engine; `null` while an engine tab shows. */
const appTab = ref<"appearance" | "background" | null>(live.value ? null : "appearance");

onMounted(() => settings.load());
// The search box appears once the page has loaded; typing goes straight to it.
watch(search, (el) => el?.focus());
// A reconnected engine has a fresh copy.
watch(live, (now) => now && settings.load());
watch(
  page,
  (p) => {
    if (p && !p.tabs.includes(tab.value)) tab.value = p.tabs[0] ?? "";
  },
  { immediate: true },
);

const terms = computed(() => query.value.trim().toLowerCase().split(/\s+/).filter(Boolean));

function matches(s: Setting) {
  if (changedOnly.value && !s.modified) return false;
  if (!terms.value.length) return true;
  const hay = `${s.label} ${s.key} ${s.description} ${s.section} ${s.tab}`.toLowerCase();
  return terms.value.every((t) => hay.includes(t));
}

/** Searching or filtering looks across every tab. */
const across = computed(() => terms.value.length > 0 || changedOnly.value);

const visible = computed(() =>
  (page.value?.settings ?? []).filter((s) => (across.value || s.tab === tab.value) && matches(s)),
);

/** Visible settings grouped by section (by tab, then section, when searching). */
const groups = computed(() => {
  const out: { title: string; items: Setting[] }[] = [];
  for (const s of visible.value) {
    const title = across.value ? `${s.tab} · ${s.section || "General"}` : s.section || "General";
    let group = out.find((g) => g.title === title);
    if (!group) out.push((group = { title, items: [] }));
    group.items.push(s);
  }
  return out;
});

const changedIn = (t: string) => (page.value?.settings ?? []).filter((s) => s.tab === t && s.modified).length;
const changedTotal = computed(() => (page.value?.settings ?? []).filter((s) => s.modified).length);

function pick(t: string) {
  appTab.value = null;
  tab.value = t;
  query.value = "";
  changedOnly.value = false;
  nextTick(() => body.value?.scrollTo({ top: 0 }));
}

function pickApp(which: "appearance" | "background") {
  appTab.value = which;
  query.value = "";
  changedOnly.value = false;
  nextTick(() => body.value?.scrollTo({ top: 0 }));
}

function close() {
  ui.settingsOpen = false;
}
</script>

<template>
  <section class="settings">
    <header class="head" data-tauri-drag-region>
      <h1 data-tauri-drag-region>Settings</h1>
      <button class="close" title="Close settings (Esc)" aria-label="Close settings" @click="close">
        <Icon name="cross" :size="15" />
      </button>
    </header>

    <div class="layout">
      <nav class="tabs">
        <label v-if="page" class="find">
          <Icon name="search" :size="14" />
          <input ref="search" v-model="query" placeholder="Search settings" spellcheck="false" @keydown.esc.stop="query = ''" />
        </label>
        <div class="group-label">Desktop</div>
        <button class="tab" :class="{ active: appTab === 'appearance' && !across }" @click="pickApp('appearance')">
          <span class="truncate">Appearance</span>
        </button>
        <button class="tab" :class="{ active: appTab === 'background' && !across }" @click="pickApp('background')">
          <span class="truncate">Background</span>
        </button>
        <div class="group-label">Engine</div>
        <button
          v-for="t in page?.tabs ?? []"
          :key="t"
          class="tab"
          :class="{ active: !appTab && !across && t === tab }"
          @click="pick(t)"
        >
          <span class="truncate">{{ t }}</span>
          <span v-if="changedIn(t)" class="count" :title="`${changedIn(t)} changed`">{{ changedIn(t) }}</span>
        </button>
        <span v-if="!page" class="tab-hint">Loads with the engine</span>
      </nav>

      <div ref="body" class="body">
        <div v-if="appTab && !across" class="content">
          <AppearanceSettings v-if="appTab === 'appearance'" />
          <BackgroundSettings v-else />
        </div>

        <div v-else-if="!page" class="state">
          <template v-if="snap.connection.state === 'preview'">
            <Icon name="gear" :size="22" />
            <p>Engine settings come from the Dial engine.</p>
            <span>The design preview has no engine. Start the app normally to edit them.</span>
          </template>
          <template v-else-if="live">
            <div class="spinner" />
            <p>Loading settings…</p>
          </template>
          <template v-else>
            <Icon name="gear" :size="22" />
            <p>The engine is not running.</p>
            <span>Settings load once Dial is connected.</span>
          </template>
        </div>

        <div v-else class="content">
          <div class="toolbar">
            <h2>{{ across ? (terms.length ? `Results for “${query.trim()}”` : "Changed settings") : tab }}</h2>
            <div class="tools">
              <button
                class="chip"
                :class="{ on: changedOnly }"
                :disabled="!changedTotal"
                title="Show only settings changed from their defaults"
                @click="changedOnly = !changedOnly"
              >
                Changed <span class="mono">{{ changedTotal }}</span>
              </button>
              <div class="segmented" role="radiogroup" aria-label="Save changes to">
                <button
                  role="radio"
                  :aria-checked="scope === 'global'"
                  :class="{ on: scope === 'global' }"
                  title="Save for every project"
                  @click="scope = 'global'"
                >
                  Everywhere
                </button>
                <button
                  role="radio"
                  :aria-checked="scope === 'project'"
                  :class="{ on: scope === 'project' }"
                  :title="`Save for ${snap.workspace.name} only`"
                  @click="scope = 'project'"
                >
                  This project
                </button>
              </div>
            </div>
          </div>
          <p class="note">
            Changes save {{ scope === "global" ? "for every project" : `for ${snap.workspace.name} only` }} and apply
            to this session right away<template v-if="snap.turnRunning"> once the running turn ends</template>.
            Settings marked <span class="tag">Restart</span> take effect the next time Dial starts.
          </p>

          <p v-if="!groups.length" class="empty">No settings match.</p>
          <section v-for="g in groups" :key="g.title" class="group">
            <h3>{{ g.title }}</h3>
            <SettingRow
              v-for="s in g.items"
              :key="s.key"
              :setting="s"
              @save="(value) => settings.save(s.key, value, scope)"
              @reset="settings.reset(s.key, scope)"
            />
          </section>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.settings {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg);
}
.head {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: var(--topbar);
  padding: 0 14px 0 24px;
  border-bottom: 1px solid var(--border);
}
h1 {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
}
.close {
  display: grid;
  place-items: center;
  width: 30px;
  height: 30px;
  border-radius: var(--radius-sm);
  color: var(--text-muted);
}
.close:hover {
  background: var(--hover);
  color: var(--text);
}
.state {
  height: 100%;
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  color: var(--text-faint);
}
.state p {
  margin: 8px 0 0;
  color: var(--text);
}
.state span {
  font-size: var(--text-sm);
}
.layout {
  flex: 1;
  min-height: 0;
  display: flex;
}
.tabs {
  flex: none;
  width: 210px;
  display: flex;
  flex-direction: column;
  gap: 1px;
  padding: 14px 10px;
  overflow-y: auto;
  border-right: 1px solid var(--border);
}
.find {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 32px;
  margin-bottom: 10px;
  padding: 0 10px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: var(--surface);
  color: var(--text-faint);
  cursor: text;
}
.find:focus-within {
  border-color: var(--accent);
}
.find input {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: var(--text-sm);
}
.tab {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 32px;
  padding: 0 10px;
  border-radius: var(--radius-sm);
  font-size: 13.5px;
  color: var(--text-muted);
  text-align: left;
}
.tab span:first-child {
  flex: 1;
}
.tab:hover {
  background: var(--hover);
  color: var(--text);
}
.tab.active {
  background: var(--active);
  color: var(--text);
}
.group-label {
  margin: 12px 10px 4px;
  font-size: var(--text-xs);
  color: var(--text-faint);
}
.find + .group-label {
  margin-top: 2px;
}
.tab-hint {
  padding: 4px 10px;
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.count {
  min-width: 18px;
  padding: 0 5px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--accent) 18%, transparent);
  color: var(--accent);
  font-size: var(--text-xs);
  text-align: center;
}
.body {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
}
.content {
  max-width: 780px;
  margin: 0 auto;
  padding: 22px 32px 60px;
}
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  flex-wrap: wrap;
}
h2 {
  margin: 0;
  font-family: ui-serif, Georgia, "Times New Roman", serif;
  font-size: 24px;
  font-weight: 400;
}
.tools {
  display: flex;
  align-items: center;
  gap: 8px;
}
.chip {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  padding: 0 10px;
  border: 1px solid var(--border-strong);
  border-radius: 999px;
  font-size: var(--text-sm);
  color: var(--text-muted);
}
.chip:hover:not(:disabled) {
  color: var(--text);
}
.chip.on {
  border-color: var(--accent);
  color: var(--text);
}
.chip:disabled {
  opacity: 0.5;
}
.segmented {
  display: flex;
  padding: 2px;
  border-radius: var(--radius-sm);
  background: var(--sidebar);
  border: 1px solid var(--border);
}
.segmented button {
  height: 24px;
  padding: 0 10px;
  border-radius: 5px;
  font-size: var(--text-sm);
  color: var(--text-muted);
}
.segmented button.on {
  background: var(--raised);
  color: var(--text);
}
.note {
  margin: 10px 0 8px;
  font-size: var(--text-sm);
  line-height: 1.55;
  color: var(--text-faint);
}
.tag {
  padding: 0 6px;
  border-radius: 999px;
  border: 1px solid var(--border-strong);
  font-size: var(--text-xs);
}
.empty {
  margin-top: 40px;
  text-align: center;
  color: var(--text-faint);
}
.group h3 {
  margin: 26px 0 0;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border-strong);
  font-size: var(--text-xs);
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--text-faint);
}
</style>
