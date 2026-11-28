<script setup lang="ts">
// Settings: a sidebar of pages grouped by purpose, and the page. Desktop pages work without an
// engine; engine pages come from its settings reference and save through `dial config`.
import { computed, nextTick, onMounted, ref, watch } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import type { OpenSnapshot, Setting, SettingScope, Snapshot } from "../types";
import { engine, resetSettingRow, saveSettingRow, settings, settingsPage } from "../bridge";
import { appearance, type Theme } from "../appearance";
import { notifications, setNotifications } from "../notifications";
import { restartDone, settingFeedback } from "../settingsFeedback";
import { changedCounts, changedTitle, hasSources, resetScope, saveScope } from "../settingScope";
import { settingMatches } from "../settingSearch";
import { ui } from "../ui";
import { engineUpdateAvailable, updates } from "../updates";
import Icon, { PATHS } from "./Icon.vue";
import SettingRow from "./SettingRow.vue";
import AppearanceSettings from "./AppearanceSettings.vue";
import UpdatesSettings from "./UpdatesSettings.vue";
import HealthSettings from "./HealthSettings.vue";
import MaintenanceSettings from "./MaintenanceSettings.vue";
import NotificationSettings from "./NotificationSettings.vue";
import TypeSettings from "./TypeSettings.vue";
import ModelLibrary from "./ModelLibrary.vue";
import ProviderSettings from "./ProviderSettings.vue";
import RolesPresets from "./RolesPresets.vue";
import ImageSettings from "./ImageSettings.vue";
import LoraTraining from "./LoraTraining.vue";
import ProjectTools, { PROJECT_TABS, type ProjectTab } from "./ProjectTools.vue";
import SettingsTransfer from "./SettingsTransfer.vue";
import IntegrationsSettings from "./IntegrationsSettings.vue";
import ComputerUseSettings from "./ComputerUseSettings.vue";
import SPage from "./settings/SPage.vue";
import SGroup from "./settings/SGroup.vue";
import SRow from "./settings/SRow.vue";
import SDefaultThinking from "./settings/SDefaultThinking.vue";
import SSwitch from "./settings/SSwitch.vue";
import SSegmented from "./settings/SSegmented.vue";
import "./settings/settings.css";

type IconName = keyof typeof PATHS;
type DesktopPage = "overview" | "appearance" | "notifications" | "types" | "updates" | "health" | "maintenance" | "library" | "providers" | "roles" | "images" | "training" | "integrations" | ProjectTab;
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
  permissions: "shield",
};

const DESKTOP: { title: string; pages: PageInfo[] }[] = [
  {
    title: "",
    pages: [{ id: "overview", label: "Overview", icon: "gear", about: "The settings most people change, in one place." }],
  },
  {
    title: "App",
    pages: [
      { id: "appearance", label: "Appearance", icon: "sun", about: "Theme, zoom, what answers draw, and the background behind sessions, on this computer.", words: "theme dark light system zoom in out size background image wallpaper dim interactive cards diagrams charts mermaid calculator answers" },
      { id: "notifications", label: "Notifications", icon: "volume", about: "Desktop alerts while Dial works in the background.", words: "alert notify desktop notifications sound finished needs you" },
      { id: "types", label: "Types", icon: "folder", about: "Where new Types are saved: in the app for every project, or in this project.", words: "type collection save project folder global location storage where new types" },
      { id: "updates", label: "Updates", icon: "download", about: "Versions of the app and the engine, which engine Desktop runs, and startup problems.", words: "version upgrade install check engine path binary dial_bin diagnostics warnings shell environment browser tools reconnect desktop copy" },
      { id: "health", label: "Health & setup", icon: "shield", about: "What Dial can use on this computer, what is missing, what it keeps and runs, and safe cleanups.", words: "doctor setup install capability language server lsp voice whisper piper ollama llama mlx runtime path terminal command cli fix missing check storage disk space gc garbage clean cleanup cache images purge archive processes ps running stop kill memory local model server rules stream" },
    ],
  },
  {
    title: "",
    // One "Models" entry: providers, the library, roles and image generation are tabs on it.
    pages: [
      { id: "providers", label: "Providers", icon: "globe", about: "Sign in to a model provider or configure an API key.", words: "models login sign in out account authentication credentials api key token subscription hosted setup anthropic openai" },
      { id: "library", label: "Model library", icon: "brain", about: "Models you have, and downloading more.", words: "models download hugging face local delete mlx gguf cache" },
      { id: "roles", label: "Roles & presets", icon: "list", about: "Which model handles which job, and saved setups to switch between.", words: "models role preset profile embedding thinking helper model" },
      { id: "images", label: "Image generation", icon: "image", about: "Which model makes images, and how it renders them.", words: "models image picture flux diffusion steps seed lora strength size quality moderation edit background mlx serve stable" },
      { id: "training", label: "LoRA training", icon: "activity", about: "Train a local adapter from approved examples, then test it against the base model.", words: "models fine tune finetune dataset learn lora mlx gguf adapter training" },
    ],
  },
  {
    title: "This project",
    // Extra folders are shown on the Permissions page: both say what Dial may reach.
    pages: PROJECT_TABS.filter((t) => t.id !== "folders").map((t) => ({ id: t.id, label: t.label, icon: PROJECT_ICONS[t.id], about: "" })),
  },
  {
    title: "Connections",
    pages: [
      { id: "integrations", label: "Integrations", icon: "terminal", about: "Hooks, SSH hosts, model profiles, computer use and the collaboration relay.", words: "hooks hook lifecycle pre tool use event script ssh remote host server machine key user port profile model profiles computer use screen mouse keyboard relay collaboration collab room" },
    ],
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
  permissions: "What Dial may do without asking, what it must never do, and folders outside the project it may use.",
};
for (const p of DESKTOP[3].pages) p.about = PROJECT_ABOUT[p.id as ProjectTab];

/** The pages under the one "Models" entry, shown as tabs. */
const MODEL_PAGES = DESKTOP[2].pages;
const isModelPage = (id: PageId) => MODEL_PAGES.some((p) => p.id === id);

// Without a project (the welcome screen) global pages work directly, including providers and the
// local model library; engine settings and per-project tools open once a project is chosen.
const props = defineProps<{ snap: Snapshot }>();
const project = computed(() => (props.snap.workspace ? (props.snap as OpenSnapshot) : null));
/** Pages that work without a project. */
const NO_PROJECT = new Set<PageId>(["overview", "appearance", "notifications", "updates", "health", "providers", "library"]);
const groupsShown = computed(() =>
  project.value ? DESKTOP : DESKTOP.map((g) => ({ ...g, pages: g.pages.filter((p) => NO_PROJECT.has(p.id)) })).filter((g) => g.pages.length),
);
/** The menu: the model pages collapse into one "Models" entry that opens the last one used. */
const lastModelPage = ref<PageId>("providers");
const navGroups = computed(() =>
  groupsShown.value.map((g) => (g.pages.some((p) => isModelPage(p.id))
    ? { title: "", pages: [{ id: lastModelPage.value, label: "Models", icon: "brain" as IconName, about: "" }] }
    : g)),
);
const modelTabs = computed(() => groupsShown.value.flatMap((g) => g.pages).filter((p) => isModelPage(p.id)));

const page = computed(() => (project.value ? settingsPage.value : null));
const live = computed(() => props.snap.connection.state === "live");
const query = ref("");
const scope = ref<SettingScope>("global");
const scopeOptions = [{ value: 'global', label: 'Everywhere' }, { value: 'project', label: 'This project' }];
const scopeDestination = computed(() => scope.value === 'global' ? 'every project' : `${props.snap.workspace?.name ?? 'this project'} only`);
/** The cross-page list: settings changed from the default, or saved for this project. */
const listing = ref<"" | "changed" | "project">("");
const changedOnly = computed(() => listing.value !== "");
function toggleListing(which: "changed" | "project") {
  listing.value = listing.value === which ? "" : which;
  query.value = "";
}
const search = ref<HTMLInputElement | null>(null);
const body = ref<HTMLElement | null>(null);

function fromUi(t: string): PageId {
  if (!t) return "overview";
  if (t === "background") return "appearance";
  return t as PageId;
}
/** A page this window can show: without a project, one of the app's own. Pages folded into
 *  another (storage into Health, extra folders into Permissions) open that page. */
const MERGED: Partial<Record<PageId, PageId>> = { maintenance: "health", folders: "permissions" };
const allowed = (asked: PageId): PageId => {
  const id = MERGED[asked] ?? asked;
  return project.value || NO_PROJECT.has(id) ? id : "overview";
};
const current = ref<PageId>(allowed(fromUi(ui.settingsTab)));
if (isModelPage(current.value)) lastModelPage.value = current.value;
// A slash command or palette entry can ask for a page while Settings is already open. The request
// is consumed, so asking for the same page again (a row's "Manage in Providers") still moves.
watch(() => ui.settingsTab, (t) => {
  if (!t) return;
  go(fromUi(t));
  ui.settingsTab = "";
});

onMounted(() => {
  if (project.value) settings.load();
  // The page gets focus, not the search box (no ring on open); typing anywhere goes to search.
  sheet.value?.focus();
  ui.settingsTab = "";
});
const sheet = ref<HTMLElement | null>(null);
/** A printable key pressed outside a field starts a settings search. */
function typeToSearch(event: KeyboardEvent) {
  if (event.metaKey || event.ctrlKey || event.altKey || event.isComposing || event.key.length !== 1 || event.key === " ") return;
  const t = event.target as HTMLElement | null;
  if (t?.closest("input, textarea, select, [contenteditable='true'], [role='textbox']")) return;
  search.value?.focus();
}
// A reconnected engine has a fresh copy.
watch(live, (now) => now && project.value && settings.load());

const engineTabs = computed(() => {
  const tabs = page.value?.tabs ?? [];
  const rank = (t: string) => (ENGINE_ORDER.includes(t) ? ENGINE_ORDER.indexOf(t) : ENGINE_ORDER.length);
  return [...tabs].sort((a, b) => rank(a) - rank(b));
});
const engineTab = computed(() => (current.value.startsWith("engine:") ? current.value.slice(7) : null));
const desktopPage = computed(() => DESKTOP.flatMap((g) => g.pages).find((p) => p.id === current.value) ?? null);

const all = computed(() => page.value?.settings ?? []);
const changedIn = (t: string) => all.value.filter((s) => s.tab === t && s.modified).length;
/** The engine reports which layer sets each value (older engines do not). */
const sourced = computed(() => hasSources(all.value));
const counts = computed(() => changedCounts(all.value));
const changedTotal = computed(() => counts.value.total);
const projectTotal = computed(() => all.value.filter((s) => s.source === "project").length);

/** A whole-table setting that other settings already show field by field. */
function isRawTable(s: Setting) {
  return s.control.type === "table" && all.value.some((o) => o.key !== s.key && o.key.startsWith(`${s.key}.`));
}

// ── Search ───────────────────────────────────────────────────────────────────

const terms = computed(() => query.value.trim().toLowerCase().split(/\s+/).filter(Boolean));
const searching = computed(() => terms.value.length > 0);
const has = (hay: string) => terms.value.every((t) => hay.toLowerCase().includes(t));
const computerHit = computed(() => searching.value && live.value && !!project.value && has("computer use desktop control mouse keyboard screenshots"));

const pageHits = computed(() => {
  if (!searching.value) return [];
  const pages: PageInfo[] = [
    ...groupsShown.value.flatMap((g) => g.pages),
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
    if (listing.value === "changed" && !s.modified) continue;
    if (listing.value === "project" && s.source !== "project") continue;
    if (searching.value) {
      if (!settingMatches(terms.value, s, engineInfo(s.tab).label)) continue;
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
  current.value = allowed(id);
  if (isModelPage(current.value)) lastModelPage.value = current.value;
  query.value = "";
  listing.value = "";
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
  model: { label: "Default model", description: "The model new sessions start with. The model picker in the composer (⇧⌘M) switches only the session you are in." },
  local_context_window: { label: "Local model context window", description: "Default token limit for local models. Auto follows the model or server capacity. A custom current-model limit takes precedence." },
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
/** What needs a look, for the overview: the menu on the left already lists every page. */
const attention = computed(() => {
  const out: { id: PageId; icon: IconName; title: string; detail: string }[] = [];
  const c = props.snap.connection;
  if (updates.app) out.push({ id: "updates", icon: "download", title: `Dial ${updates.app.version} is available`, detail: "Install the new version of the app." });
  if (engineUpdateAvailable()) out.push({ id: "updates", icon: "download", title: "A newer Dial engine is available", detail: "Update the engine this app runs." });
  if (c.state === "failed" || c.state === "ended") out.push({ id: "updates", icon: "refresh", title: "The Dial engine isn't running", detail: "See which engine Desktop runs and what went wrong at startup." });
  if (project.value && c.state === "live" && !(project.value.model?.modelId || c.model)) out.push({ id: "providers", icon: "globe", title: "No model is set up", detail: "Sign in to a provider or add an API key." });
  return out;
});

/** Save one setting, and whatever its chosen option brings along (an image model's endpoint). */
async function save(s: Setting, value: unknown, where: SettingScope = scope.value, linked?: Record<string, unknown>) {
  return saveSettingRow(s, value, where, props.snap.turnRunning, linked);
}
/** Reset the layer that set the value when the engine says which; else the picked scope. */
async function reset(s: Setting, where: SettingScope = resetScope(s.source, scope.value)) {
  return resetSettingRow(s, where, props.snap.turnRunning);
}

/** The changed list: put every row shown back to its default, after asking. */
const shownChanged = computed(() => groups.value.flatMap((g) => g.items).filter((s) => (listing.value === "project" ? s.source === "project" : s.modified) && !s.redacted));
async function resetShown() {
  const list = shownChanged.value;
  if (!list.length) return;
  const names = list.slice(0, 8).map((s) => `• ${s.label}`).join("\n") + (list.length > 8 ? `\n…and ${list.length - 8} more` : "");
  const where = sourced.value
    ? `Each is reset where it was saved (this project or everywhere).`
    : `Resets apply to ${scopeDestination.value}. A value set for the other scope (${scope.value === "global" ? "this project" : "everywhere"}) stays and may still apply.`;
  const ok = await ask(
    `Reset ${list.length} setting${list.length === 1 ? "" : "s"}?\n\n${names}\n\n${where}`,
    { title: "Reset settings?", kind: "warning", okLabel: "Reset all", cancelLabel: "Cancel" },
  ).catch(() => false);
  if (ok) for (const s of list) await reset(s);
}

// ── Restart ──────────────────────────────────────────────────────────────────

const restarting = ref(false);
/** Restart-tagged settings changed since the engine started apply once it restarts. */
async function restartEngine() {
  restarting.value = true;
  try {
    if (await engine.reconnect()) restartDone();
  } finally {
    restarting.value = false;
  }
}
</script>

<template>
  <section ref="sheet" class="settings" tabindex="-1" @keydown="typeToSearch">
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

        <template v-for="(g, gi) in navGroups" :key="g.title || `g${gi}`">
          <div v-if="g.title" class="nav-label">{{ g.title }}</div>
          <div v-else-if="gi" class="nav-gap" />
          <button
            v-for="p in g.pages"
            :key="p.id"
            class="item"
            :class="{ active: (current === p.id || (isModelPage(p.id) && isModelPage(current))) && !overlay }"
            :aria-current="(current === p.id || (isModelPage(p.id) && isModelPage(current))) && !overlay ? 'page' : undefined"
            @click="go(p.id)"
          >
            <Icon :name="p.icon" :size="15" />
            <span class="truncate" :title="p.label">{{ p.label }}</span>
          </button>
        </template>

        <p v-if="!project" class="nav-hint">Open a project for engine settings and models. Providers and Health &amp; setup work without one.</p>
        <div v-if="project" class="nav-label engine-heading">
          Engine
          <span v-if="changedTotal || (sourced && projectTotal)" class="filters">
            <button
              v-if="changedTotal"
              class="changed"
              :class="{ on: listing === 'changed' }"
              :title="listing === 'changed' ? 'Show every page' : changedTitle(counts, sourced)"
              @click="toggleListing('changed')"
            >
              {{ changedTotal }} changed
            </button>
            <button
              v-if="sourced && projectTotal"
              class="changed"
              :class="{ on: listing === 'project' }"
              :title="listing === 'project' ? 'Show every page' : `Show the ${projectTotal} setting${projectTotal === 1 ? '' : 's'} saved for this project only`"
              @click="toggleListing('project')"
            >
              {{ projectTotal }} this project
            </button>
          </span>
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
        <span v-if="!page && project" class="nav-hint">{{ snap.connection.state === "preview" ? "Needs the engine" : live ? "Loading…" : "Loads with the engine" }}</span>
      </nav>

      <div ref="body" class="body">
        <!-- Restart-tagged settings changed: they apply once the engine restarts. -->
        <div v-if="settingFeedback.restart.length && live" class="restart-wrap">
          <div class="s-banner" role="status">
            <span>
              {{ settingFeedback.restart.length === 1 ? `“${settingFeedback.restart[0]}” applies` : `${settingFeedback.restart.length} changed settings apply` }}
              after the engine restarts.<template v-if="snap.turnRunning"> Wait for the running turn to finish.</template>
            </span>
            <button class="btn btn-primary" :disabled="snap.turnRunning || restarting" @click="restartEngine">
              {{ restarting ? "Restarting…" : "Restart engine now" }}
            </button>
          </div>
        </div>
        <!-- Sign-in may be awaiting a browser or code. Cache only this page, and only for
             this project's lifetime in Settings; unmounting still closes its auth process. -->
        <!-- The model pages share one menu entry; tabs move between them. -->
        <div v-if="!overlay && isModelPage(current) && modelTabs.length > 1" class="model-tabs" role="tablist" aria-label="Models">
          <button
            v-for="p in modelTabs"
            :key="p.id"
            role="tab"
            :aria-selected="current === p.id"
            :class="{ on: current === p.id }"
            @click="go(p.id)"
          >{{ p.label }}</button>
        </div>
        <KeepAlive :key="project?.workspace.root ?? ''">
          <SPage v-if="current === 'providers' && !overlay && desktopPage" :title="desktopPage.label" :description="desktopPage.about">
            <ProviderSettings :snap="snap" />
          </SPage>
        </KeepAlive>
        <!-- Search results and the changed-settings list, across every page. -->
        <SPage
          v-if="overlay"
          :title="searching ? `“${query.trim()}”` : listing === 'project' ? 'Changed in this project' : 'Changed settings'"
          :description="searching ? undefined : listing === 'project' ? 'Values saved for this project only; they override the everywhere value. Reset one to use the everywhere value again.' : 'Everything changed from its default. Reset one with the arrow beside its name.'"
        >
          <template #actions>
            <button v-if="!searching && shownChanged.length" class="btn btn-secondary" @click="resetShown">Reset all shown</button>
            <button class="btn btn-secondary" @click="query = ''; listing = ''">Done</button>
          </template>
          <ComputerUseSettings v-if="computerHit && project" :key="project.workspace.root" :root="project.workspace.root" />
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
          <div v-if="groups.length" class="result-scope">
            <SSegmented v-model="scope" :options="scopeOptions" label="Save changes for" />
            <span class="s-note">
              Edits apply to {{ scopeDestination }}. A value saved for this project overrides the everywhere value.
              <template v-if="sourced">Resets apply where each value was saved.</template>
              <template v-else-if="listing">This engine does not report where each value was saved, so resets also apply to {{ scopeDestination }}.</template>
            </span>
          </div>
          <SGroup v-for="g in groups" :key="g.id" :title="g.title">
            <SettingRow
              v-for="s in g.items"
              :key="`${snap.workspace?.root}:${scope}:${s.key}`"
              :setting="s"
              :page-scope="scope"
              @save="(value, linked) => save(s, value, scope, linked)"
              @reset="reset(s)"
            />
          </SGroup>
          <p v-if="!computerHit && !pageHits.length && !groups.length" class="nothing">Nothing matches. Try a shorter word.</p>
          <p v-if="searching && project && !page" class="s-note">Engine settings are searchable once the engine is connected.</p>
          <p v-else-if="searching && !project" class="s-note">Open a project to search engine settings.</p>
        </SPage>

        <!-- Overview -->
        <SPage v-else-if="current === 'overview'" title="">
          <div v-if="quick.length" class="result-scope">
            <SSegmented v-model="scope" :options="scopeOptions" label="Save changes for" />
            <span class="s-note" :title="`A value saved for this project overrides the everywhere value.${sourced ? ' Values already saved for this project stay project values.' : ''}`">Changes apply to {{ scopeDestination }}.</span>
          </div>
          <SGroup title="Quick settings">
            <SRow label="App theme" description="Light, dark, or follow the system. Applies to this computer.">
              <SSegmented :model-value="appearance.theme" :options="THEMES" label="App theme" @update:model-value="appearance.theme = $event as Theme" />
            </SRow>
            <template v-for="s in quick" :key="`${snap.workspace?.root}:${scope}:${s.key}`">
              <SettingRow
                :setting="s"
                :page-scope="saveScope(s.source, scope)"
                @save="(value, linked) => save(s, value, saveScope(s.source, scope), linked)"
                @reset="reset(s)"
              />
              <SDefaultThinking v-if="s.key === 'model' && live && project"
                :key="`${s.value}:${all.find(row => row.key === 'provider')?.value}:${s.source}`"
                :root="project.workspace.root" :scope="saveScope(s.source, scope)" />
            </template>
            <SRow label="Desktop notifications" description="An alert when Dial needs you or finishes in the background.">
              <SSwitch
                :model-value="notifications.enabled"
                :disabled="notifications.busy"
                label="Desktop notifications"
                @update:model-value="setNotifications"
              />
            </SRow>
            <template v-if="project && quick.some((q) => q.key === 'model')" #after>
              <p class="s-note model-link">
                The default model applies to new sessions.
                <button class="link" @click="close(); ui.modelPickerOpen = true">Switch this session’s model…</button>
              </p>
            </template>
          </SGroup>
          <SettingsTransfer v-if="project && page" :snap="project" :rows="all" />
          <p v-if="!project" class="s-note">Engine settings live with a project’s engine. Sign in to a provider, download local models, and check your setup here; open a project to change the rest.</p>
          <SGroup title="Needs a look" plain>
            <div v-if="attention.length" class="tiles">
              <button v-for="a in attention" :key="a.title" class="tile" @click="go(a.id)">
                <span class="tile-icon"><Icon :name="a.icon" :size="16" /></span>
                <span class="tile-text">
                  <span class="tile-title">{{ a.title }}</span>
                  <span class="tile-about">{{ a.detail }}</span>
                </span>
              </button>
            </div>
            <p v-else class="s-note">Nothing needs your attention. Every other setting is in the list on the left.</p>
          </SGroup>
        </SPage>

        <!-- Project pages drawn on SPage themselves. -->
        <RolesPresets v-else-if="project && current === 'roles'" :snap="project" />
        <LoraTraining v-else-if="project && current === 'training'" :key="JSON.stringify([project.workspace.root, project.sessionId, project.connection.state])" :snap="project" @library="go('library')" @settings="go('engine:Learning')" />
        <template v-else-if="project && PROJECT_TABS.some((t) => t.id === current)">
          <ProjectTools :key="current" :which="current as ProjectTab" :snap="project" />
          <ProjectTools v-if="current === 'permissions'" key="folders" which="folders" :snap="project" embedded />
        </template>

        <!-- Pages that draw their own header. -->
        <div v-else-if="current === 'library' || (project && current === 'images')" class="s-page own">
          <ModelLibrary v-if="current === 'library'" :preview="snap.connection.state === 'preview'" />
          <ImageSettings v-else-if="project" :snap="project" />
        </div>

        <!-- Desktop pages. -->
        <SPage v-else-if="desktopPage && current !== 'providers'" :title="desktopPage.label" :description="desktopPage.about">
          <AppearanceSettings v-if="current === 'appearance'" />
          <NotificationSettings v-else-if="current === 'notifications'" />
          <TypeSettings v-else-if="current === 'types'" />
          <template v-else-if="current === 'health'">
            <HealthSettings />
            <details class="advanced">
              <summary>Advanced: storage, running processes and stream rules</summary>
              <MaintenanceSettings />
            </details>
          </template>
          <IntegrationsSettings v-else-if="current === 'integrations'" />
          <UpdatesSettings v-else />
        </SPage>

        <!-- Engine pages. -->
        <div v-else-if="engineTab && !page" class="state">
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

        <SPage v-else-if="engineTab" :title="engineInfo(engineTab).label" :description="engineInfo(engineTab).about">
          <template #actions>
            <SSegmented
              v-model="scope"
              :options="scopeOptions"
              label="Save changes for"
            />
          </template>
          <p class="scope-note">
            Saving for {{ scopeDestination }}; a value saved for this project overrides the everywhere value.<template v-if="sourced"> Resets apply where each value was saved.</template> Changes apply right
            away<template v-if="snap.turnRunning">, once the running turn ends</template>; those marked
            <span class="s-tag">Restart</span> on the next start.
          </p>
          <div v-if="groups.length > 2" class="jump" aria-label="Sections">
            <button v-for="g in groups" :key="g.id" @click="jump(g.id)">{{ g.title }}</button>
          </div>
          <SGroup v-for="g in groups" :id="g.id" :key="g.id" :title="g.title">
            <SettingRow
              v-for="s in g.items"
              :key="`${snap.workspace?.root}:${scope}:${s.key}`"
              :setting="s"
              :page-scope="scope"
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
                :key="`${snap.workspace?.root}:${scope}:${s.key}`"
                :setting="s"
                :page-scope="scope"
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
.nav > * {
  flex-shrink: 0;
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
.engine-heading {
  flex-direction: column;
  align-items: flex-start;
  gap: 4px;
}
.filters {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}
.changed {
  white-space: nowrap;
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
.model-link {
  margin: 8px 4px 0;
}
.model-link .link {
  color: var(--accent);
  font: inherit;
}
.model-link .link:hover {
  text-decoration: underline;
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
.restart-wrap {
  max-width: 760px;
  margin: 0 auto;
  padding: 20px 40px 0;
}
.restart-wrap .s-banner {
  margin: 0;
}
.result-scope { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 12px; margin-bottom: 12px; }
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
  border-radius: var(--radius-md);
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
  border-radius: var(--radius-sm);
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
.nav-gap { height: 12px; }
/* Tabs across the model pages, which share one menu entry. */
.model-tabs {
  display: flex;
  gap: 2px;
  width: min(680px, 100%);
  margin: 20px auto 0;
  padding: 2px;
  border-radius: var(--radius-sm);
  background: var(--hover);
}
.model-tabs button {
  flex: 1;
  height: 28px;
  border-radius: calc(var(--radius-sm) - 2px);
  font-size: var(--text-sm);
  color: var(--text-muted);
}
.model-tabs button:hover { color: var(--text); }
.model-tabs button.on { background: var(--raised); color: var(--text); box-shadow: 0 1px 2px rgb(0 0 0 / 0.15); }
/* Storage, processes and stream rules: kept on Health & setup, folded away. */
.advanced { margin-top: var(--space-lg); }
.advanced > summary {
  cursor: pointer;
  padding: 6px 0;
  font-size: var(--text-sm);
  color: var(--text-muted);
}
.advanced > summary:hover { color: var(--text); }
.advanced[open] > summary { margin-bottom: var(--space-md); }
.settings:focus { outline: none; }
</style>
