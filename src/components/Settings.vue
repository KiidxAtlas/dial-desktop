<script setup lang="ts">
// Settings: a sidebar of pages grouped by purpose, and the page. Desktop pages work without an
// engine; engine pages come from its settings reference and save through `dial config`.
import { computed, nextTick, onMounted, ref, watch } from "vue";
import type { OpenSnapshot, Setting, SettingScope } from "../types";
import { settings, settingsPage } from "../bridge";
import { appearance, type Theme } from "../appearance";
import { notifications, setNotifications } from "../notifications";
import { ui } from "../ui";
import Icon, { PATHS } from "./Icon.vue";
import SettingRow from "./SettingRow.vue";
import AppearanceSettings from "./AppearanceSettings.vue";
import UpdatesSettings from "./UpdatesSettings.vue";
import NotificationSettings from "./NotificationSettings.vue";
import ModelLibrary from "./ModelLibrary.vue";
import RolesPresets from "./RolesPresets.vue";
import ImageSettings from "./ImageSettings.vue";
import ProjectTools, { PROJECT_TABS, type ProjectTab } from "./ProjectTools.vue";
import SPage from "./settings/SPage.vue";
import SGroup from "./settings/SGroup.vue";
import SRow from "./settings/SRow.vue";
import SSwitch from "./settings/SSwitch.vue";
import SSegmented from "./settings/SSegmented.vue";
import "./settings/settings.css";

type IconName = keyof typeof PATHS;
type DesktopPage = "overview" | "appearance" | "notifications" | "updates" | "library" | "roles" | "images" | ProjectTab;
/** A desktop page, or `engine:<tab>` for one of the engine's tabs. */
type PageId = DesktopPage | `engine:${string}`;

interface PageInfo {
  id: PageId;
  label: string;
  icon: IconName;
  /** One sentence under the title; also searched. */
  about: string;
  /** Extra words search should find this page by. */
  words?: string;
}

const PROJECT_ICONS: Record<ProjectTab, IconName> = {
  usage: "activity",
  memory: "pin",
  skills: "sparkle",
  extensions: "chip",
  checks: "check",
  folders: "folder",
  schedules: "refresh",
};

const DESKTOP: { title: string; pages: PageInfo[] }[] = [
  {
    title: "",
    pages: [{ id: "overview", label: "Overview", icon: "gear", about: "The settings most people change, in one place." }],
  },
  {
    title: "App",
    pages: [
      { id: "appearance", label: "Appearance", icon: "sun", about: "Theme, zoom and the background behind sessions, on this computer.", words: "theme dark light zoom background image wallpaper dim" },
      { id: "notifications", label: "Notifications", icon: "volume", about: "Desktop alerts while Dial works in the background.", words: "alert notify" },
      { id: "updates", label: "Updates", icon: "download", about: "Versions of the app and the engine, and installing new ones.", words: "version upgrade install" },
    ],
  },
  {
    title: "Models",
    pages: [
      { id: "library", label: "Model library", icon: "brain", about: "Models you have, and downloading more.", words: "download hugging face local delete" },
      { id: "roles", label: "Roles & presets", icon: "list", about: "Which model handles which job, and saved setups to switch between.", words: "role preset profile embedding" },
      { id: "images", label: "Image generation", icon: "image", about: "Which model makes images, and how it renders them.", words: "image picture flux diffusion steps seed lora size quality mlx serve stable" },
    ],
  },
  {
    title: "This project",
    pages: PROJECT_TABS.map((t) => ({ id: t.id, label: t.label, icon: PROJECT_ICONS[t.id], about: "" })),
  },
];

/** Friendlier names and a purpose for the engine's tabs; unknown tabs keep their own name. */
const ENGINE: Record<string, { label: string; icon: IconName; about: string }> = {
  Common: { label: "Essentials", icon: "sparkle", about: "The engine settings most people change." },
  Model: { label: "Model", icon: "brain", about: "Which model answers, how it reasons, and how much it reads and writes." },
  Safety: { label: "Safety", icon: "shield", about: "When Dial asks first, what the sandbox allows, and what stays secret." },
  Tools: { label: "Tools", icon: "terminal", about: "The tools Dial uses: shell, browser, search, language servers and more." },
  Memory: { label: "Memory", icon: "pin", about: "How Dial remembers facts across sessions." },
  Learning: { label: "Learning", icon: "activity", about: "How Dial improves from your sessions over time." },
  Connections: { label: "Connections", icon: "globe", about: "Providers, MCP servers, remote hosts and integrations." },
  Appearance: { label: "Terminal & voice", icon: "monitor", about: "How the terminal looks, read-aloud and dictation." },
  Keys: { label: "Keyboard & input", icon: "keyboard", about: "Shortcuts, the mouse, and what Enter does while Dial works." },
  Advanced: { label: "Advanced", icon: "gear", about: "Rarely changed engine settings." },
  Experimental: { label: "Experimental", icon: "chip", about: "Features still being tried out. They may change or go away." },
};
const ENGINE_ORDER = ["Common", "Model", "Safety", "Tools", "Memory", "Learning", "Connections", "Appearance", "Keys", "Advanced", "Experimental"];
const engineInfo = (tab: string) => ENGINE[tab] ?? { label: tab, icon: "gear" as IconName, about: "" };

const PROJECT_ABOUT: Record<ProjectTab, string> = {
  usage: "Plan limits, tokens and cost.",
  memory: "Facts Dial remembers for this project and for you.",
  skills: "Reusable procedures for bigger tasks.",
  extensions: "MCP servers and other add-ons.",
  checks: "Commands run after Dial edits files.",
  folders: "Folders outside the project Dial may use.",
  schedules: "Prompts that run on a timer or on file changes.",
};
for (const p of DESKTOP[3].pages) p.about = PROJECT_ABOUT[p.id as ProjectTab];

const props = defineProps<{ snap: OpenSnapshot }>();

const page = computed(() => settingsPage.value);
const live = computed(() => props.snap.connection.state === "live");
const query = ref("");
const scope = ref<SettingScope>("global");
const changedOnly = ref(false);
const search = ref<HTMLInputElement | null>(null);
const body = ref<HTMLElement | null>(null);

function fromUi(t: string): PageId {
  if (!t) return "overview";
  if (t === "background") return "appearance";
  return t as PageId;
}
const current = ref<PageId>(fromUi(ui.settingsTab));
// A slash command or palette entry can ask for a page while Settings is already open.
watch(() => ui.settingsTab, (t) => t && go(fromUi(t)));

onMounted(() => {
  settings.load();
  search.value?.focus();
});
// A reconnected engine has a fresh copy.
watch(live, (now) => now && settings.load());

const engineTabs = computed(() => {
  const tabs = page.value?.tabs ?? [];
  const rank = (t: string) => (ENGINE_ORDER.includes(t) ? ENGINE_ORDER.indexOf(t) : ENGINE_ORDER.length);
  return [...tabs].sort((a, b) => rank(a) - rank(b));
});
const engineTab = computed(() => (current.value.startsWith("engine:") ? current.value.slice(7) : null));
const desktopPage = computed(() => DESKTOP.flatMap((g) => g.pages).find((p) => p.id === current.value) ?? null);

const all = computed(() => page.value?.settings ?? []);
const changedIn = (t: string) => all.value.filter((s) => s.tab === t && s.modified).length;
const changedTotal = computed(() => all.value.filter((s) => s.modified).length);

/** A whole-table setting that other settings already show field by field. */
function isRawTable(s: Setting) {
  return s.control.type === "table" && all.value.some((o) => o.key !== s.key && o.key.startsWith(`${s.key}.`));
}

// ── Search ───────────────────────────────────────────────────────────────────

const terms = computed(() => query.value.trim().toLowerCase().split(/\s+/).filter(Boolean));
const searching = computed(() => terms.value.length > 0);
const has = (hay: string) => terms.value.every((t) => hay.toLowerCase().includes(t));

const pageHits = computed(() => {
  if (!searching.value) return [];
  const pages: PageInfo[] = [
    ...DESKTOP.flatMap((g) => g.pages),
    ...engineTabs.value.map((t) => ({ id: `engine:${t}` as PageId, ...engineInfo(t), words: t })),
  ];
  return pages.filter((p) => has(`${p.label} ${p.about} ${p.words ?? ""}`));
});

/** The engine files leftovers under "Configuration"; on a page that reads as "General". */
const sectionName = (s: Setting) => (!s.section || s.section === "Configuration" || s.section === s.tab ? "General" : s.section);

/** Settings shown on the current engine page, or found by search, grouped by section. */
const groups = computed(() => {
  const out: { id: string; title: string; items: Setting[] }[] = [];
  for (const s of all.value) {
    if (changedOnly.value && !s.modified) continue;
    if (searching.value) {
      if (!has(`${s.label} ${s.key} ${s.description} ${s.section} ${s.tab} ${engineInfo(s.tab).label}`)) continue;
    } else if (!changedOnly.value && (s.tab !== engineTab.value || isRawTable(s))) continue;
    const title = searching.value || changedOnly.value ? [engineInfo(s.tab).label, sectionName(s)].filter((x) => x !== "General").join(" · ") || "General" : sectionName(s);
    let g = out.find((x) => x.title === title);
    if (!g) out.push((g = { id: `sec-${out.length}`, title, items: [] }));
    g.items.push(s);
  }
  return out;
});
/** Whole tables on this page that other rows already cover; kept reachable but out of the way. */
const rawTables = computed(() => (engineTab.value ? all.value.filter((s) => s.tab === engineTab.value && isRawTable(s)) : []));

const overlay = computed(() => searching.value || changedOnly.value);

// ── Navigation ───────────────────────────────────────────────────────────────

function go(id: PageId) {
  current.value = id;
  query.value = "";
  changedOnly.value = false;
  nextTick(() => body.value?.scrollTo({ top: 0 }));
}
function jump(id: string) {
  body.value?.querySelector(`#${id}`)?.scrollIntoView({ behavior: "smooth", block: "start" });
}
function close() {
  ui.settingsOpen = false;
  ui.settingsTab = "";
}

// ── Overview ─────────────────────────────────────────────────────────────────

/** The engine settings on the overview, with plainer words than the reference uses. */
const QUICK: Record<string, { label: string; description: string }> = {
  model: { label: "Model", description: "The model that answers in new sessions." },
  approval: { label: "Ask before acting", description: "When Dial stops to ask before it edits files or runs commands." },
  "sandbox.mode": { label: "Sandbox", description: "Off, project writes only (Dev), or project only with no network (Strict)." },
  "ui.show_thinking": { label: "Show reasoning", description: "Show the model's reasoning, or fold it to one line." },
};
const quick = computed(() =>
  Object.entries(QUICK).flatMap(([key, words]) => {
    const s = all.value.find((x) => x.key === key);
    return s ? [{ ...s, ...words }] : [];
  }),
);
const THEMES: { value: Theme; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];
const tiles = computed<PageInfo[]>(() => [
  ...DESKTOP.slice(1, 3).flatMap((g) => g.pages),
  ...engineTabs.value.filter((t) => t !== "Common").map((t) => ({ id: `engine:${t}` as PageId, ...engineInfo(t) })),
]);

/** Save one setting, and whatever its chosen option brings along (an image model's endpoint). */
function save(s: Setting, value: unknown, where: SettingScope = scope.value, linked?: Record<string, unknown>) {
  settings.save(s.key, value, where);
  for (const [key, also] of Object.entries(linked ?? {})) settings.save(key, also, where);
}
const reset = (s: Setting, where: SettingScope = scope.value) => settings.reset(s.key, where);
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
      <nav class="nav" aria-label="Settings pages">
        <label class="find">
          <Icon name="search" :size="13" />
          <input ref="search" v-model="query" placeholder="Search settings" spellcheck="false" @keydown.esc.stop="query = ''" />
          <button v-if="query" class="clear" aria-label="Clear search" @click="query = ''"><Icon name="cross" :size="11" /></button>
        </label>

        <template v-for="g in DESKTOP" :key="g.title">
          <div v-if="g.title" class="nav-label">{{ g.title }}</div>
          <button
            v-for="p in g.pages"
            :key="p.id"
            class="item"
            :class="{ active: current === p.id && !overlay }"
            :aria-current="current === p.id && !overlay ? 'page' : undefined"
            @click="go(p.id)"
          >
            <Icon :name="p.icon" :size="15" />
            <span class="truncate">{{ p.label }}</span>
          </button>
        </template>

        <div class="nav-label">
          Engine
          <button
            v-if="changedTotal"
            class="changed"
            :class="{ on: changedOnly }"
            :title="changedOnly ? 'Show every page' : 'Show every setting changed from its default'"
            @click="changedOnly = !changedOnly; query = ''"
          >
            {{ changedTotal }} changed
          </button>
        </div>
        <button
          v-for="t in engineTabs"
          :key="t"
          class="item"
          :class="{ active: engineTab === t && !overlay }"
          @click="go(`engine:${t}`)"
        >
          <Icon :name="engineInfo(t).icon" :size="15" />
          <span class="truncate">{{ engineInfo(t).label }}</span>
          <i v-if="changedIn(t)" class="mark" :title="`${changedIn(t)} changed from the default`" />
        </button>
        <span v-if="!page" class="nav-hint">{{ snap.connection.state === "preview" ? "Needs the engine" : live ? "Loading…" : "Loads with the engine" }}</span>
      </nav>

      <div ref="body" class="body">
        <!-- Search results and the changed-settings list, across every page. -->
        <SPage
          v-if="overlay"
          :title="searching ? `“${query.trim()}”` : 'Changed settings'"
          :description="searching ? undefined : 'Everything changed from its default. Reset one with the arrow beside its name.'"
        >
          <template #actions>
            <button class="btn btn-secondary" @click="query = ''; changedOnly = false">Done</button>
          </template>
          <SGroup v-if="pageHits.length" title="Pages">
            <button v-for="p in pageHits" :key="p.id" class="s-row link-row" @click="go(p.id)">
              <Icon :name="p.icon" :size="15" />
              <div class="s-row-text">
                <div class="s-label">{{ p.label }}</div>
                <p v-if="p.about" class="s-desc">{{ p.about }}</p>
              </div>
              <Icon name="external" :size="12" class="go" />
            </button>
          </SGroup>
          <SGroup v-for="g in groups" :key="g.id" :title="g.title">
            <SettingRow
              v-for="s in g.items"
              :key="`${snap.workspace.root}:${scope}:${s.key}`"
              :setting="s"
              @save="(value, linked) => save(s, value, scope, linked)"
              @reset="reset(s)"
            />
          </SGroup>
          <p v-if="!pageHits.length && !groups.length" class="nothing">Nothing matches. Try a shorter word.</p>
          <p v-if="searching && !page" class="s-note">Engine settings are searchable once the engine is connected.</p>
        </SPage>

        <!-- Overview -->
        <SPage v-else-if="current === 'overview'" title="Settings" description="The settings most people change. Everything else is a page away.">
          <SGroup title="Quick settings">
            <SRow label="App theme" description="Light, dark, or follow the system.">
              <SSegmented :model-value="appearance.theme" :options="THEMES" label="App theme" @update:model-value="appearance.theme = $event as Theme" />
            </SRow>
            <SettingRow
              v-for="s in quick"
              :key="`${snap.workspace.root}:global:${s.key}`"
              :setting="s"
              @save="(value, linked) => save(s, value, 'global', linked)"
              @reset="reset(s, 'global')"
            />
            <SRow label="Desktop notifications" description="An alert when Dial needs you or finishes in the background.">
              <SSwitch
                :model-value="notifications.enabled"
                :disabled="notifications.busy"
                label="Desktop notifications"
                @update:model-value="setNotifications"
              />
            </SRow>
          </SGroup>
          <SGroup title="More settings" plain>
            <div class="tiles">
              <button v-for="p in tiles" :key="p.id" class="tile" @click="go(p.id)">
                <span class="tile-icon"><Icon :name="p.icon" :size="16" /></span>
                <span class="tile-text">
                  <span class="tile-title">{{ p.label }}</span>
                  <span class="tile-about">{{ p.about }}</span>
                </span>
              </button>
            </div>
          </SGroup>
        </SPage>

        <!-- Pages that draw their own header. -->
        <div v-else-if="current === 'library' || current === 'roles' || current === 'images' || PROJECT_TABS.some((t) => t.id === current)" class="s-page own">
          <ModelLibrary v-if="current === 'library'" :preview="snap.connection.state === 'preview'" />
          <RolesPresets v-else-if="current === 'roles'" :snap="snap" />
          <ImageSettings v-else-if="current === 'images'" :snap="snap" />
          <ProjectTools v-else :key="current" :which="current as ProjectTab" :snap="snap" />
        </div>

        <!-- Desktop pages. -->
        <SPage v-else-if="desktopPage" :title="desktopPage.label" :description="desktopPage.about">
          <AppearanceSettings v-if="current === 'appearance'" />
          <NotificationSettings v-else-if="current === 'notifications'" />
          <UpdatesSettings v-else />
        </SPage>

        <!-- Engine pages. -->
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

        <SPage v-else :title="engineInfo(engineTab ?? '').label" :description="engineInfo(engineTab ?? '').about">
          <template #actions>
            <SSegmented
              v-model="scope"
              :options="[{ value: 'global', label: 'Everywhere' }, { value: 'project', label: 'This project' }]"
              label="Save changes for"
            />
          </template>
          <p class="scope-note">
            Saving {{ scope === "global" ? "for every project" : `for ${snap.workspace.name} only` }}. Changes apply right
            away<template v-if="snap.turnRunning">, once the running turn ends</template>; those marked
            <span class="s-tag">Restart</span> on the next start.
          </p>
          <div v-if="groups.length > 2" class="jump" aria-label="Sections">
            <button v-for="g in groups" :key="g.id" @click="jump(g.id)">{{ g.title }}</button>
          </div>
          <SGroup v-for="g in groups" :id="g.id" :key="g.id" :title="g.title">
            <SettingRow
              v-for="s in g.items"
              :key="`${snap.workspace.root}:${scope}:${s.key}`"
              :setting="s"
              @save="(value, linked) => save(s, value, scope, linked)"
              @reset="reset(s)"
            />
          </SGroup>
          <p v-if="!groups.length && !rawTables.length" class="nothing">This page has no settings.</p>
          <details v-if="rawTables.length" class="raw">
            <summary>Full configuration tables ({{ rawTables.length }})</summary>
            <p class="s-note">The same settings as above, as whole tables, for entries the rows above do not cover.</p>
            <div class="s-card">
              <SettingRow
                v-for="s in rawTables"
                :key="`${snap.workspace.root}:${scope}:${s.key}`"
                :setting="s"
                @save="(value, linked) => save(s, value, scope, linked)"
                @reset="reset(s)"
              />
            </div>
          </details>
        </SPage>
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
  padding: 0 14px 0 22px;
  border-bottom: 1px solid var(--border);
}
h1 {
  margin: 0;
  font-size: 14px;
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
.layout {
  flex: 1;
  min-height: 0;
  display: flex;
}

/* Sidebar */
.nav {
  flex: none;
  width: 220px;
  display: flex;
  flex-direction: column;
  gap: 1px;
  padding: 12px 10px 24px;
  overflow-y: auto;
  border-right: 1px solid var(--border);
}
.find {
  display: flex;
  align-items: center;
  gap: 7px;
  height: 30px;
  margin-bottom: 8px;
  padding: 0 8px 0 10px;
  border-radius: var(--radius-sm);
  background: var(--active);
  color: var(--text-faint);
  cursor: text;
}
.find:focus-within {
  box-shadow: 0 0 0 1px var(--accent);
}
.find input {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
}
.find input::placeholder {
  color: var(--text-faint);
}
.find .clear {
  display: grid;
  place-items: center;
  width: 18px;
  height: 18px;
  border-radius: 4px;
  color: var(--text-faint);
}
.nav-label {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin: 14px 10px 4px;
  font-size: 11px;
  font-weight: 600;
  color: var(--text-faint);
}
.changed {
  padding: 0 7px;
  border-radius: 999px;
  font-size: 10.5px;
  font-weight: 500;
  line-height: 18px;
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 12%, transparent);
}
.changed.on,
.changed:hover {
  background: var(--accent);
  color: var(--on-accent);
}
.item {
  display: flex;
  align-items: center;
  gap: 9px;
  height: 30px;
  padding: 0 10px;
  border-radius: var(--radius-sm);
  font-size: 13px;
  color: var(--text-muted);
  text-align: left;
  transition: background 120ms var(--ease), color 120ms var(--ease);
}
.item :deep(svg) {
  flex: none;
  color: var(--text-faint);
}
.item span {
  flex: 1;
}
.item:hover {
  background: var(--hover);
  color: var(--text);
}
.item.active {
  background: var(--active);
  color: var(--text);
  font-weight: 500;
}
.item.active :deep(svg) {
  color: var(--accent);
}
.mark {
  flex: none;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--accent);
}
.nav-hint {
  padding: 4px 10px;
  font-size: 12px;
  color: var(--text-faint);
}

/* Page */
.body {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
}
.own {
  padding-top: 26px;
}
.state {
  height: 100%;
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
.scope-note {
  margin: -10px 0 18px;
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-faint);
}
.jump {
  position: sticky;
  top: 0;
  z-index: 5;
  display: flex;
  gap: 6px;
  margin: 0 -40px 18px;
  padding: 10px 40px;
  overflow-x: auto;
  background: var(--bg);
  scrollbar-width: none;
}
.jump button {
  flex: none;
  height: 26px;
  padding: 0 11px;
  border: 1px solid var(--border-strong);
  border-radius: 999px;
  font-size: 12px;
  color: var(--text-muted);
}
.jump button:hover {
  border-color: var(--focus-border);
  color: var(--text);
}
.nothing {
  margin: 48px 0;
  text-align: center;
  font-size: 13px;
  color: var(--text-faint);
}
.raw {
  margin-top: 30px;
}
.raw summary {
  margin: 0 4px 8px;
  font-size: 12.5px;
  color: var(--text-faint);
  cursor: pointer;
}
.raw summary:hover {
  color: var(--text);
}
.link-row {
  width: 100%;
  text-align: left;
}
.link-row:hover {
  background: var(--hover);
}
.link-row > :deep(svg:first-child) {
  color: var(--text-faint);
}
.go {
  color: var(--text-faint);
}

/* Overview tiles */
.tiles {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 10px;
}
.tile {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 13px 14px;
  border: 1px solid var(--border);
  border-radius: 12px;
  background: var(--surface);
  text-align: left;
  transition: border-color 120ms var(--ease), background 120ms var(--ease);
}
.tile:hover {
  border-color: var(--border-strong);
  background: var(--raised);
}
.tile-icon {
  display: grid;
  flex: none;
  place-items: center;
  width: 30px;
  height: 30px;
  border-radius: 8px;
  background: color-mix(in srgb, var(--accent) 12%, transparent);
  color: var(--accent);
}
.tile-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.tile-title {
  font-size: 13.5px;
  color: var(--text);
}
.tile-about {
  font-size: 12px;
  line-height: 1.45;
  color: var(--text-faint);
}
</style>
