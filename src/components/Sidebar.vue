<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import type { OpenSnapshot, Session, TranscriptHit, TranscriptResults, Workspace } from "../types";
import { engine, folders } from "../bridge";
import { installEngine, updateEngine, updates } from "../updates";
import { focusComposer, openMenu, ui } from "../ui";
import { clickKind, pickAfter } from "../picking";
import { finishRename, goToSession, sessionGroups, sessionItems, startRename } from "../sessionActions";
import { isTypeSession, types } from "../savedTypes";
import { clearRecentProjects, forgetProject, loadRecentProjects, projectItems, recentProjects as recent } from "../projects";
import Icon from "./Icon.vue";
import Logo from "./Logo.vue";
import SidebarTypes from "./types/SidebarTypes.vue";

const props = defineProps<{ snap: OpenSnapshot }>();

const live = computed(() => props.snap.connection.state === "live");
const searchMessages = ref(false);
const results = ref<TranscriptResults | null>(null);
const searching = ref(false);
const visibleHits = computed(() => types.loaded ? results.value?.hits.filter(hit => !isTypeSession(hit.session_id)) ?? [] : []);
const searchError = ref("");
let searchVersion = 0;

watch([() => props.snap.workspace.root, () => ui.sessionFilter, searchMessages], () => {
  searchVersion++;
  results.value = null;
  searching.value = false;
  searchError.value = "";
});

async function searchTranscripts() {
  const query = ui.sessionFilter.trim();
  if (!searchMessages.value || !query || !live.value) return;
  const version = ++searchVersion;
  searching.value = true;
  searchError.value = "";
  try {
    const found = await engine.searchTranscripts(props.snap.workspace.root, query);
    if (version === searchVersion) results.value = found;
  } catch (error) {
    if (version === searchVersion) searchError.value = String(error);
  } finally {
    if (version === searchVersion) searching.value = false;
  }
}

async function openHit(hit: TranscriptHit) {
  const jump = { ...hit, root: props.snap.workspace.root };
  ui.settingsOpen = false;
  ui.transcriptJump = jump;
  const pending = ui.transcriptJump;
  if (hit.session_id !== props.snap.sessionId && !(await engine.resume(hit.session_id, jump.root))) {
    if (ui.transcriptJump === pending) ui.transcriptJump = null;
  }
}
/** Each session belongs to one group (Shift-click ranges follow the displayed order). */
const groups = computed(() => sessionGroups(props.snap, ui.sessionFilter));
const sessions = computed(() => groups.value.flatMap((g) => g.sessions));
/** ⌘1–9 numbers for the first nine rows, shown while ⌘ is held. */
const jumpNumber = computed(() => new Map(sessions.value.slice(0, 9).map((s, i) => [s.id, i + 1])));
const mod = navigator.platform.toLowerCase().includes("mac") ? "⌘" : "Ctrl+";

// ⇧⌘F: focus the session search; Esc in it clears the search, then leaves it.
const searchInput = ref<HTMLInputElement | null>(null);
watch(
  () => ui.focusSearch,
  async () => {
    await nextTick();
    searchInput.value?.focus();
    searchInput.value?.select();
  },
);
function onSearchKey(event: KeyboardEvent) {
  if (event.key !== "Escape" || event.isComposing) return;
  event.preventDefault();
  event.stopPropagation();
  if (ui.sessionFilter) ui.sessionFilter = "";
  else (event.target as HTMLInputElement).blur();
}

const status = computed(() => {
  const c = props.snap.connection;
  switch (c.state) {
    case "starting":
      return { tone: "warning", text: "Starting…", retry: false };
    case "live":
      return { tone: "success", text: c.engineVersion ? `dial ${c.engineVersion}` : "Connected", retry: false };
    case "preview":
      return { tone: "violet", text: "Design preview", retry: false };
    case "failed":
      return { tone: "danger", text: c.error, retry: true };
    case "ended":
      return { tone: "faint", text: "Engine stopped", retry: true };
  }
  return { tone: "faint", text: "", retry: false };
});

// The project menu: recent projects and "Open folder…".
const projectMenu = ref<HTMLElement | null>(null);
const menuOpen = ref(false);

async function toggleMenu() {
  menuOpen.value = !menuOpen.value;
  if (menuOpen.value) await loadRecentProjects();
}

function openFolder(root: string | null) {
  menuOpen.value = false;
  if (root) folders.open(root);
  else folders.choose();
}

/** Right-click (or "…") on a project: open it elsewhere, reveal it, copy its path, forget it. */
function projectMenuFor(event: MouseEvent, w: Workspace, current = false, anchor?: HTMLElement | null) {
  openMenu(event, projectItems(w, current), anchor);
}

function onOutside(event: MouseEvent) {
  if (projectMenu.value && !projectMenu.value.contains(event.target as Node)) menuOpen.value = false;
}
watch(menuOpen, (isOpen) =>
  isOpen ? document.addEventListener("mousedown", onOutside) : document.removeEventListener("mousedown", onOutside),
);
onBeforeUnmount(() => document.removeEventListener("mousedown", onOutside));

function newSession() {
  ui.settingsOpen = false;
  engine.newSession();
  focusComposer();
}

// Picking several sessions (see `picking.ts`); a plain click opens the session.
const anchor = ref<string | null>(null);

function click(event: MouseEvent, session: Session) {
  const kind = clickKind(event);
  const ids = sessions.value.map((s) => s.id);
  const current = props.snap.sessions.find((s) => s.selected)?.id ?? null;
  const next = pickAfter({ picked: ui.picked, anchor: anchor.value }, ids, session.id, kind, current);
  ui.picked.clear();
  for (const pickedId of next.picked) ui.picked.add(pickedId);
  anchor.value = next.anchor;
  if (kind === "open") goToSession(session);
}

const pickedSessions = computed(() => props.snap.sessions.filter((s) => ui.picked.has(s.id)));

async function remove(list: { id: string; title: string }[]) {
  if (await engine.deleteSessions(list)) {
    for (const s of list) ui.picked.delete(s.id);
  }
}

function menuFor(event: MouseEvent, s: Session, anchor?: HTMLElement | null) {
  // A right-click on one of several picked sessions acts on all of them.
  if (ui.picked.size > 1 && ui.picked.has(s.id)) {
    const list = pickedSessions.value;
    const pinned = list.every((p) => p.pinned);
    openMenu(event, [
      {
        label: pinned ? `Unpin ${list.length} sessions` : `Pin ${list.length} sessions`,
        icon: pinned ? "unpin" : "pin",
        run: () => list.forEach((p) => engine.pinSession(p.id, !pinned)),
      },
      { divider: true },
      { label: `Delete ${list.length} sessions`, icon: "trash", danger: true, hint: "⌫", run: () => remove(list) },
    ]);
    return;
  }
  openMenu(event, sessionItems(props.snap, s, "sidebar"), anchor);
}

// Editing a session's name in its row.
const renameInput = ref<HTMLInputElement[]>([]);
watch(
  () => ui.renaming,
  async (now) => {
    if (now?.place !== "sidebar") return;
    await nextTick();
    const input = renameInput.value[0];
    input?.focus();
    input?.select();
  },
);
function onRenameKey(event: KeyboardEvent, s: Session) {
  if (event.key === "Enter" && !event.isComposing) {
    event.preventDefault();
    finishRename(s.id, s.title, (event.target as HTMLInputElement).value);
  } else if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    ui.renaming = null;
  }
}
const renamingHere = (id: string) => ui.renaming?.id === id && ui.renaming.place === "sidebar";

// Sessions that left the list (deleted, or from another folder) cannot stay picked.
watch(
  () => props.snap.sessions,
  (list) => {
    for (const id of [...ui.picked]) if (!list.some((s) => s.id === id)) ui.picked.delete(id);
  },
);

function onKey(event: KeyboardEvent) {
  const target = event.target as HTMLElement | null;
  const typing = target?.closest("input, textarea, select, [contenteditable]");
  if (typing || !ui.picked.size) return;
  if (event.key === "Delete" || event.key === "Backspace") {
    event.preventDefault();
    remove(pickedSessions.value);
  } else if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "a") {
    event.preventDefault();
    for (const s of sessions.value) ui.picked.add(s.id);
  }
}
onMounted(() => window.addEventListener("keydown", onKey));
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <aside class="sidebar">
    <div class="brand" data-tauri-drag-region>
      <Logo :size="18" />
      <span class="word" data-tauri-drag-region>Dial</span>
      <button class="collapse" title="Hide sidebar (⌘B)" aria-label="Hide sidebar" @click="ui.sidebarOpen = false">
        <Icon name="panelLeft" :size="16" />
      </button>
    </div>

    <nav class="actions">
      <button class="item new" :disabled="!live" title="Start a fresh session (⌘N)" @click="newSession">
        <span class="plus"><Icon name="plus" :size="13" /></span>
        New session
      </button>
      <form class="item search" @submit.prevent="searchTranscripts">
        <Icon name="search" :size="16" />
        <input ref="searchInput" v-model="ui.sessionFilter" :placeholder="searchMessages ? 'Search messages' : 'Search titles'" :aria-label="searchMessages ? 'Search messages' : 'Search titles'" spellcheck="false" @keydown="onSearchKey" />
        <button
          type="button" class="scope" :aria-pressed="searchMessages"
          :title="searchMessages ? 'Searching every message in this project; click to search titles' : 'Searching titles; click to search every message in this project'"
          @click="searchMessages = !searchMessages"
        >{{ searchMessages ? 'Messages' : 'Titles' }}</button>
      </form>
    </nav>

    <SidebarTypes />

    <nav v-if="searchMessages" class="list" aria-label="Transcript search results">
      <button class="item" :disabled="!live || searching || !ui.sessionFilter.trim()" @click="searchTranscripts">Search messages</button>
      <p v-if="searching" class="empty" role="status">Searching messages…</p>
      <p v-else-if="searchError" class="empty" role="alert">{{ searchError }}</p>
      <p v-else-if="results && !visibleHits.length" class="empty">No matching messages.</p>
      <template v-else-if="results">
        <button v-for="(hit, index) in visibleHits" :key="`${hit.session_id}:${hit.entry_id}:${index}`" class="hit" @click="openHit(hit)">
          <strong class="truncate">{{ hit.title || 'Untitled session' }}</strong>
          <span>{{ hit.snippet }}</span>
        </button>
        <p v-if="results.has_more" class="empty">Showing the first 40 matches. Refine your search for more specific results.</p>
      </template>
    </nav>
    <nav v-else class="list">
      <p v-if="!sessions.length" class="empty">
        {{ snap.sessions.length ? "No sessions match." : "Your sessions appear here." }}
      </p>
      <template v-for="group in groups" :key="group.name">
        <div class="label">{{ group.name }}</div>
        <div
          v-for="s in group.sessions"
          :key="s.id"
          class="session"
          :class="{
            selected: s.selected && !ui.settingsOpen && !ui.typeOpen,
            picked: ui.picked.has(s.id),
            editing: renamingHere(s.id),
          }"
          role="button"
          tabindex="0"
          :aria-pressed="ui.picked.has(s.id)"
          :title="`${s.title}\n${s.waiting ? 'Waiting for you' : s.running ? 'Working' : s.meta}`"
          @click="renamingHere(s.id) || click($event, s)"
          @dblclick="startRename(s.id, 'sidebar')"
          @contextmenu="menuFor($event, s)"
          @keydown.self.enter="goToSession(s)"
          @keydown.self.f2.prevent="startRename(s.id, 'sidebar')"
          @keydown.self.space.prevent="ui.picked.has(s.id) ? ui.picked.delete(s.id) : ui.picked.add(s.id)"
        >
          <input
            v-if="renamingHere(s.id)"
            ref="renameInput"
            class="rename"
            :value="s.title"
            spellcheck="false"
            aria-label="Session name"
            @keydown="onRenameKey($event, s)"
            @blur="finishRename(s.id, s.title, ($event.target as HTMLInputElement).value)"
          />
          <template v-else>
            <span class="truncate">{{ s.title }}</span>
            <kbd v-if="ui.jumpHints && jumpNumber.has(s.id)" class="jump-hint">{{ mod }}{{ jumpNumber.get(s.id) }}</kbd>
            <i v-if="s.waiting" class="activity waiting" />
            <i v-else-if="s.running" class="activity running" />
            <button
              class="row-btn"
              title="More"
              aria-label="More actions"
              @click.stop="menuFor($event, s, $event.currentTarget as HTMLElement)"
            >
              <Icon name="more" :size="14" />
            </button>
            <button class="row-btn" :title="`Delete “${s.title}”`" @click.stop="remove([s])">
              <Icon name="cross" :size="12" />
            </button>
          </template>
        </div>
      </template>
    </nav>

    <Transition name="pop">
      <div v-if="ui.picked.size" class="picking">
        <span>{{ ui.picked.size }} selected</span>
        <button class="pick-action danger" title="Delete the selected sessions (⌫)" @click="remove(pickedSessions)">
          <Icon name="trash" :size="13" /> Delete
        </button>
        <button class="pick-action" title="Clear the selection (Esc)" @click="ui.picked.clear()">Cancel</button>
      </div>
    </Transition>

    <button
      class="item settings-btn"
      :class="{ on: ui.settingsOpen }"
      title="Settings (⌘,)"
      @click="ui.settingsOpen = !ui.settingsOpen"
    >
      <Icon name="gear" :size="16" />
      Settings
    </button>

    <div ref="projectMenu" class="project">
      <Transition name="pop">
        <div v-if="menuOpen" class="menu">
          <div class="menu-head">
            <span class="menu-path mono truncate" :title="snap.workspace.root">{{ snap.workspace.display }}</span>
            <button class="head-btn" title="Project actions" aria-label="Project actions" @click.stop="projectMenuFor($event, snap.workspace, true, $event.currentTarget as HTMLElement)">
              <Icon name="more" :size="13" />
            </button>
          </div>
          <template v-if="recent.length">
            <div class="menu-label">
              <span>Recent</span>
              <button class="clear" title="Remove every project from this list (folders stay on disk)" @click.stop="clearRecentProjects()">Clear</button>
            </div>
            <div v-for="w in recent" :key="w.root" class="menu-item" @contextmenu="projectMenuFor($event, w)">
              <button class="menu-row" :title="`${w.root}\nRight-click for more`" @click="openFolder(w.root)">
                <Icon name="folder" :size="14" />
                <span class="truncate">{{ w.name }}</span>
              </button>
              <button class="forget" :aria-label="`Remove ${w.name} from recent projects`" :title="`Remove from this list (the folder stays on disk)`" @click.stop="forgetProject(w.root)">
                <Icon name="cross" :size="12" />
              </button>
            </div>
          </template>
          <div class="menu-divider" />
          <button class="menu-row" @click="openFolder(null)">
            <Icon name="plus" :size="14" /> <span>Open folder…</span>
          </button>
          <button class="menu-row" @click="menuOpen = false; folders.choose(true)">
            <Icon name="external" :size="14" /> <span>Open folder in new window…</span>
          </button>
        </div>
      </Transition>
      <button class="project-btn" :title="`${snap.workspace.root} · switch project`" @click="toggleMenu">
        <span class="avatar">{{ snap.workspace.name.slice(0, 1).toUpperCase() }}</span>
        <span class="project-text">
          <span class="truncate name">{{ snap.workspace.name }}</span>
          <span class="truncate state" :class="status.tone" :title="status.text"><i />{{ status.text }}</span>
        </span>
        <Icon name="chevronDown" :size="13" />
      </button>
      <button
        v-if="snap.connection.state === 'failed' && snap.connection.problem === 'missing'"
        class="btn btn-primary retry"
        :disabled="!!updates.engineBusy"
        @click="installEngine()"
      >
        <Icon name="download" :size="13" /> {{ updates.engineBusy ? "Installing…" : "Install Dial engine" }}
      </button>
      <button
        v-else-if="snap.connection.state === 'failed' && snap.connection.problem === 'incompatible'"
        class="btn btn-primary retry"
        :disabled="!!updates.engineBusy"
        @click="updateEngine()"
      >
        <Icon name="download" :size="13" /> {{ updates.engineBusy ? "Updating…" : "Update Dial engine" }}
      </button>
      <button
        v-if="(snap.connection.state === 'failed' && snap.connection.problem === 'other') || (snap.connection.state === 'live' && !(snap.model?.modelId || snap.connection.model))"
        class="btn btn-secondary retry"
        @click="ui.settingsTab = 'providers'; ui.settingsOpen = true"
      >Set up a provider</button>
      <button v-if="status.retry" class="btn btn-secondary retry" @click="engine.reconnect()">
        <Icon name="refresh" :size="13" /> Reconnect
      </button>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
  width: var(--sidebar-width);
  flex: none;
  display: flex;
  flex-direction: column;
  padding: 8px 8px 10px;
  background: var(--sidebar);
  border-right: 1px solid var(--border);
}
.brand {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 36px;
  padding: 0 10px;
}
.mac:not(.fullscreen) .brand {
  /* Room for the window buttons, which sit over the content; full screen has none. */
  margin-top: 26px;
}
.collapse {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  margin: 0 -4px 0 auto;
  border-radius: var(--radius-sm);
  color: var(--text-faint);
  opacity: 0;
  transition:
    opacity 120ms var(--ease),
    background 120ms var(--ease);
}
.sidebar:hover .collapse,
.collapse:focus-visible {
  opacity: 1;
}
.collapse:hover {
  background: var(--hover);
  color: var(--text);
}
.word {
  font-size: 15px;
  font-weight: 600;
  letter-spacing: -0.01em;
}
.actions {
  display: flex;
  flex-direction: column;
  gap: 1px;
  margin: 10px 0 14px;
}
.item {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 34px;
  padding: 0 10px;
  border-radius: var(--radius-sm);
  font-size: 13.5px;
  color: var(--text-muted);
  text-align: left;
  transition:
    background 120ms var(--ease),
    color 120ms var(--ease);
}
.item:hover:not(:disabled) {
  background: var(--hover);
  color: var(--text);
}
.item:disabled {
  opacity: 0.45;
}
.plus {
  display: grid;
  place-items: center;
  width: 20px;
  height: 20px;
  /* Optically centred on the 16px row icons, so every label starts on one line. */
  margin: 0 -2px;
  border-radius: 50%;
  background: var(--accent);
  color: var(--on-accent);
  transition: transform 160ms var(--ease), background 120ms var(--ease);
}
.new {
  color: var(--text);
}
.new:hover:not(:disabled) .plus {
  background: var(--accent-hover);
  transform: rotate(90deg);
}
.search {
  margin: 4px 0;
  padding-right: 5px;
  background: var(--hover);
  box-shadow: inset 0 0 0 1px var(--border);
  cursor: text;
}
.item.search:hover {
  background: var(--hover);
}
.search:focus-within {
  box-shadow: inset 0 0 0 1px var(--focus-border);
  color: var(--text);
}
.search input {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: none;
  color: var(--text);
  font: inherit;
}
.search input::placeholder {
  color: var(--text-muted);
}
.search:focus-within input::placeholder {
  color: var(--text-faint);
}
.scope {
  flex: none;
  padding: 2px 7px;
  border-radius: 999px;
  font-size: var(--text-xs);
  color: var(--text-faint);
  box-shadow: inset 0 0 0 1px var(--border);
  transition: color 120ms var(--ease), background 120ms var(--ease);
}
.scope:hover {
  color: var(--text);
  background: var(--hover);
}
.scope[aria-pressed="true"] {
  color: var(--accent);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 45%, transparent);
}
.label {
  padding: 0 10px 6px;
  font-size: var(--text-xs);
  color: var(--text-faint);
}
.list .label {
  padding-top: 4px;
}
.list .label ~ .label {
  margin-top: 12px;
}
.rename {
  flex: 1;
  min-width: 0;
  height: 24px;
  margin: 0 -6px;
  padding: 0 6px;
  border: 1px solid var(--accent);
  border-radius: 6px;
  outline: none;
  background: var(--bg);
  color: var(--text);
  font: inherit;
}
.list {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.empty {
  margin: 4px 10px;
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.session {
  position: relative;
  display: flex;
  align-items: center;
  gap: 6px;
  flex: none;
  height: 32px;
  padding: 0 10px;
  border-radius: var(--radius-sm);
  font-size: 13.5px;
  color: var(--text-muted);
  cursor: default;
  outline: none;
  transition:
    background 120ms var(--ease),
    color 120ms var(--ease);
}
.session span {
  flex: 1;
}
.session:hover,
.session:focus-visible {
  background: var(--hover);
  color: var(--text);
}
.session.selected {
  background: var(--active);
  color: var(--text);
}
.session.picked {
  background: color-mix(in srgb, var(--accent) 14%, transparent);
  color: var(--text);
  box-shadow: inset 2px 0 0 var(--accent);
}
.picking {
  display: flex;
  align-items: center;
  gap: 4px;
  margin: 6px 0 0;
  padding: 6px 6px 6px 10px;
  border-radius: var(--radius-md);
  background: var(--surface);
  font-size: var(--text-xs);
  color: var(--text-muted);
}
.picking span {
  flex: 1;
}
.pick-action {
  display: flex;
  align-items: center;
  gap: 4px;
  height: 22px;
  padding: 0 8px;
  border-radius: 6px;
  font-size: var(--text-xs);
  color: var(--text-muted);
}
.pick-action:hover {
  background: var(--hover);
  color: var(--text);
}
.pick-action.danger {
  color: var(--danger);
}
.pick-action.danger:hover {
  background: color-mix(in srgb, var(--danger) 12%, transparent);
  color: var(--danger);
}
.settings-btn {
  flex: none;
  margin: 6px 0 2px;
}
.settings-btn.on {
  background: var(--active);
  color: var(--text);
}
.jump-hint {
  flex: none;
  margin-left: auto;
  padding: 0 5px;
  border: 1px solid var(--border-strong);
  border-radius: 4px;
  font: 500 10px/16px var(--font-mono);
  color: var(--text-muted);
  background: var(--raised);
  animation: hint-in 120ms var(--ease);
}
@keyframes hint-in {
  from { opacity: 0; transform: scale(0.9); }
}
.activity {
  flex: none;
  width: 7px;
  height: 7px;
  border-radius: 50%;
}
.activity.running {
  background: var(--accent);
  animation: pulse 1.2s ease-in-out infinite;
}
.activity.waiting {
  background: var(--warning);
}
.row-btn {
  display: none;
  place-items: center;
  width: 22px;
  height: 22px;
  flex: none;
  border-radius: var(--radius-sm);
  color: var(--text-faint);
}
.row-btn:last-child {
  margin-right: -6px;
}
.session:hover .row-btn,
.session:focus-within .row-btn {
  display: grid;
}
.row-btn:hover {
  background: var(--hover);
  color: var(--text);
}
.project {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding-top: 8px;
  border-top: 1px solid var(--border);
}
.project-btn {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 6px 8px;
  border-radius: var(--radius-md);
  text-align: left;
  color: var(--text-faint);
  transition: background 120ms var(--ease);
}
.project-btn:hover {
  background: var(--hover);
}
.avatar {
  display: grid;
  place-items: center;
  width: 30px;
  height: 30px;
  flex: none;
  border-radius: 50%;
  background: var(--raised);
  color: var(--text);
  font-size: 13px;
  font-weight: 600;
}
.project-text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  line-height: 1.3;
}
.name {
  font-size: 13.5px;
  font-weight: 500;
  color: var(--text);
}
.state {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: var(--text-xs);
  --tone: var(--text-faint);
}
.state i {
  width: 6px;
  height: 6px;
  flex: none;
  border-radius: 50%;
  background: var(--tone);
}
.state.success {
  --tone: var(--success);
}
.state.warning {
  --tone: var(--warning);
}
.state.warning i {
  animation: pulse 1.4s ease-in-out infinite;
}
.state.danger {
  --tone: var(--danger);
  color: var(--danger);
}
.state.violet {
  --tone: var(--violet);
}
.retry {
  width: 100%;
}
.menu {
  position: absolute;
  left: 0;
  right: 0;
  bottom: calc(100% + var(--pop-gap));
  z-index: 30;
  padding: 6px;
  border-radius: var(--radius-md);
  background: var(--raised);
  box-shadow: var(--shadow-pop);
}
.menu-head {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 2px 3px 6px 8px;
}
.menu-path {
  flex: 1;
  min-width: 0;
  font-size: var(--text-xs);
  color: var(--text-faint);
}
.head-btn {
  display: grid;
  place-items: center;
  width: 22px;
  height: 22px;
  border-radius: var(--radius-sm);
  color: var(--text-faint);
}
.head-btn:hover {
  background: var(--hover);
  color: var(--text);
}
.menu-label {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 8px 4px;
  font-size: var(--text-xs);
  color: var(--text-faint);
}
.menu-label .clear {
  padding: 1px 6px;
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  color: var(--text-faint);
}
.menu-label .clear:hover {
  background: var(--hover);
  color: var(--text);
}
.menu-divider {
  height: 1px;
  margin: 4px 6px;
  background: var(--border);
}
.menu-row {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  height: 32px;
  padding: 0 8px;
  border-radius: var(--radius-sm);
  text-align: left;
  font-size: 13px;
  color: var(--text-muted);
}
.menu-row:hover {
  background: var(--hover);
  color: var(--text);
}
.menu-item {
  position: relative;
}
.menu-item .menu-row {
  padding-right: 32px;
}
.forget {
  position: absolute;
  top: 50%;
  right: 5px;
  display: grid;
  place-items: center;
  width: 22px;
  height: 22px;
  border-radius: var(--radius-sm);
  color: var(--text-faint);
  opacity: 0;
  transform: translateY(-50%);
  transition: opacity 120ms var(--ease), background 120ms var(--ease), color 120ms var(--ease);
}
.menu-item:hover .forget,
.forget:focus-visible {
  opacity: 1;
}
.forget:hover {
  background: color-mix(in srgb, var(--danger) 14%, transparent);
  color: var(--danger);
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
.hit { display: grid; gap: 4px; width: 100%; padding: 8px; text-align: left; border-radius: var(--radius-sm); }
.hit:hover { background: var(--hover); }
.hit span { color: var(--text-faint); overflow-wrap: anywhere; }
</style>
