<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { connect, connecting, engine, snapshot, startupError } from "./bridge";
import { focusComposer, ui, type InspectorTab } from "./ui";
import { copyText, goToSession, sessionOrder, stepSession } from "./sessionActions";
import { followAgentApp, followAgentBrowser, showPanel, trackPreviews } from "./workspace";
import { zoomBy } from "./appearance";
import { startNewSession } from "./commands";
import { background, hasBackground, restoreImage } from "./backgrounds";
import { markTurnAborted, syncQueue } from "./queue";
import type { OpenSnapshot } from "./types";
import Sidebar from "./components/Sidebar.vue";
import TopBar from "./components/TopBar.vue";
import Timeline from "./components/Timeline.vue";
import Composer from "./components/Composer.vue";
import Inspector from "./components/Inspector.vue";
import Toasts from "./components/Toasts.vue";
import Logo from "./components/Logo.vue";
import Welcome from "./components/Welcome.vue";
import Settings from "./components/Settings.vue";
import Backdrop from "./components/Backdrop.vue";
import ContextMenu from "./components/ContextMenu.vue";
import Palette from "./components/Palette.vue";
import Shortcuts from "./components/Shortcuts.vue";
import ImageViewer from "./components/ImageViewer.vue";
import UpdateBanner from "./components/UpdateBanner.vue";
import { startUpdateChecks } from "./updates";
import TypeView from "./components/types/TypeView.vue";
import Icon from "./components/Icon.vue";
import { followTypeSession, typeScopedSnapshot } from "./savedTypes";

/** A type stays open while its own conversation is on screen; another session closes it. */
watch(
  () => [snapshot.value?.workspace?.root, snapshot.value?.sessionId ?? null] as const,
  ([root, id], [previousRoot, previousId]) => followTypeSession(root, id, previousRoot, previousId),
);
/** The type itself is in the middle, not the transcript. */
const showingType = computed(() => !!ui.typeOpen && ui.typeMode === "type");
/**
 * Over a type the composer is a round button until pressed; the transcript and the turn's output
 * stay on the Chat tab. An approval or a question still shows the full composer.
 */
const typeComposerOpen = ref(false);
const composerShown = computed(
  () => !showingType.value || typeComposerOpen.value || !!snapshot.value?.approvals?.length || !!snapshot.value?.question,
);
function openTypeComposer() {
  typeComposerOpen.value = true;
  void nextTick(focusComposer);
}
watch(() => [ui.typeOpen, ui.typeMode], () => (typeComposerOpen.value = false));

const snap = computed(() => snapshot.value);
/** The snapshot once a folder is open; until then the welcome screen shows. */
const opened = computed(() => (snap.value?.workspace ? (snap.value as OpenSnapshot) : null));
/**
 * What the conversation panes show: the open type's own conversation, never another chat's that
 * happens to be on screen until the type has one.
 */
const viewed = computed(() => (opened.value ? typeScopedSnapshot(opened.value) : null));
watch(() => viewed.value?.turnRunning, (running) => {
  if (running && showingType.value) typeComposerOpen.value = false;
});
const reviewing = computed(() => ui.inspectorOpen && ui.inspectorTab === "changes" && !!ui.diffPath && ui.diffExpanded && !ui.settingsOpen);
watch(() => [opened.value?.workspace.root, opened.value?.sessionId] as const, ([root, id], [previousRoot, previousId]) => {
  if (root !== previousRoot || id !== previousId) {
    ui.diffPath = null;
    ui.diffExpanded = false;
  }
});

/** No conversation yet: the background shows at full strength. */
const empty = computed(() => !!opened.value && !opened.value.historyLoading && !opened.value.entries.length && !opened.value.turnRunning);
const showBackdrop = computed(
  () => hasBackground.value && !showingType.value && (background.placement === "always" || empty.value),
);

// Dev servers Dial starts are offered in (or opened by) the browser panel.
watch(opened, (s) => {
  if (!s) return;
  trackPreviews(s);
  syncQueue(s);
});

/** Show panel `tab`, or close the inspector when it already shows it. */
function togglePanel(tab: InspectorTab) {
  if (ui.inspectorOpen && ui.inspectorTab === tab && !ui.settingsOpen) ui.inspectorOpen = false;
  else showPanel(tab);
}

/** In full screen macOS hides the window buttons, so the room kept for them is given back. */
const fullscreen = ref(false);
let fullscreenTimers: ReturnType<typeof setTimeout>[] = [];
async function readFullscreen() {
  try {
    fullscreen.value = await getCurrentWindow().isFullscreen();
  } catch {
    // A window without the permission keeps the windowed layout.
  }
}
/** Track full screen: the state settles only once the system's transition ends, so look again after it. */
async function followFullscreen(): Promise<(() => void) | null> {
  if (!isTauri()) return null;
  await readFullscreen();
  try {
    return await getCurrentWindow().onResized(() => {
      fullscreenTimers.forEach(clearTimeout);
      fullscreenTimers = [0, 400, 1200].map((delay) => setTimeout(readFullscreen, delay));
    });
  } catch {
    return null;
  }
}

// Holding ⌘ (Ctrl elsewhere) alone for a moment shows the sessions' ⌘1–9 numbers.
const mac = navigator.platform.toLowerCase().includes("mac");
let hintTimer: ReturnType<typeof setTimeout> | undefined;
function hideHints() {
  clearTimeout(hintTimer);
  ui.jumpHints = false;
}
function onKeyUp(event: KeyboardEvent) {
  if (event.key === (mac ? "Meta" : "Control")) hideHints();
}

function onKey(event: KeyboardEvent) {
  const s = opened.value;
  if (!s) return;
  if (event.key === (mac ? "Meta" : "Control")) {
    if (!event.repeat) {
      clearTimeout(hintTimer);
      hintTimer = setTimeout(() => (ui.jumpHints = true), 450);
    }
    return;
  }
  hideHints();
  const command = event.metaKey || event.ctrlKey;
  const plain = command && !event.shiftKey && !event.altKey;
  const key = event.key.toLowerCase();
  if (plain && /^Digit[1-9]$/.test(event.code)) {
    const target = sessionOrder(s)[Number(event.code.slice(5)) - 1];
    if (target) {
      event.preventDefault();
      goToSession(target);
    }
    return;
  }
  if (
    (command && event.shiftKey && !event.altKey && (event.code === "BracketLeft" || event.code === "BracketRight")) ||
    (event.ctrlKey && !event.metaKey && !event.altKey && event.key === "Tab")
  ) {
    event.preventDefault();
    stepSession(s, event.code === "BracketLeft" || (event.key === "Tab" && event.shiftKey) ? -1 : 1);
    return;
  }
  if (command && event.shiftKey && !event.altKey && key === "f") {
    event.preventDefault();
    ui.sidebarOpen = true;
    ui.focusSearch++;
    return;
  }
  if (plain && key === "l") {
    event.preventDefault();
    ui.settingsOpen = false;
    focusComposer();
    return;
  }
  if (plain && key === "n") {
    event.preventDefault();
    startNewSession(s);
  } else if (plain && key === "k") {
    ui.filePalette = false;
    event.preventDefault();
    ui.menu = null;
    ui.paletteOpen = !ui.paletteOpen;
  } else if (plain && key === "p" && !event.defaultPrevented) {
    event.preventDefault();
    ui.filePalette = true;
    ui.paletteOpen = true;
  } else if (plain && key === "/") {
    event.preventDefault();
    ui.shortcutsOpen = !ui.shortcutsOpen;
  } else if (plain && key === "b") {
    event.preventDefault();
    ui.sidebarOpen = !ui.sidebarOpen;
  } else if (command && event.altKey && !event.shiftKey && event.code === "KeyI") {
    event.preventDefault();
    ui.inspectorOpen = !ui.inspectorOpen;
  } else if (command && event.shiftKey && !event.altKey && key === "b") {
    event.preventDefault();
    togglePanel("browser");
  } else if (command && event.shiftKey && !event.altKey && key === "e") {
    event.preventDefault();
    togglePanel("files");
  } else if (event.ctrlKey && !event.metaKey && !event.altKey && event.code === "Backquote") {
    event.preventDefault();
    togglePanel("terminal");
  } else if (command && !event.altKey && (key === "=" || key === "+")) {
    event.preventDefault();
    zoomBy(1);
  } else if (plain && key === "-") {
    event.preventDefault();
    zoomBy(-1);
  } else if (plain && key === "0") {
    event.preventDefault();
    zoomBy(0);
  } else if (plain && event.key === ",") {
    event.preventDefault();
    ui.settingsOpen = !ui.settingsOpen;
  } else if (command && event.shiftKey && key === "m") {
    event.preventDefault();
    ui.modelPickerOpen = !ui.modelPickerOpen;
  } else if (command && event.shiftKey && !event.altKey && key === "v") {
    // Voice: start it, or do what tapping the orb does (talk, stop recording, interrupt).
    event.preventDefault();
    void (s.voice.phase === "off" ? engine.voiceStart() : engine.voiceTap());
  } else if (event.key === "Escape" && !event.isComposing && ui.modelPickerOpen) {
    ui.modelPickerOpen = false;
  } else if (event.key === "Escape" && !event.isComposing && ui.picked.size) {
    event.preventDefault();
    ui.picked.clear();
  } else if (event.key === "Escape" && !event.isComposing && ui.settingsOpen) {
    event.preventDefault();
    ui.settingsOpen = false;
  } else if (event.key === "Escape" && !event.isComposing && showingType.value && typeComposerOpen.value) {
    event.preventDefault();
    typeComposerOpen.value = false;
  } else if (event.key === "Escape" && !event.isComposing && s.turnRunning) {
    event.preventDefault();
    markTurnAborted(s);
    engine.abort();
  }
}

function scanOnReturn() {
  if (opened.value?.connection.state === "live") void engine.discoverModels();
}

let stopFollowing: (() => void) | null = null;
let stopFollowingApp: (() => void) | null = null;
let stopFullscreen: (() => void) | null = null;
onMounted(async () => {
  window.addEventListener("keydown", onKey);
  window.addEventListener("keyup", onKeyUp);
  window.addEventListener("blur", hideHints);
  window.addEventListener("focus", scanOnReturn);
  restoreImage();
  startUpdateChecks();
  void followFullscreen().then((stop) => (stopFullscreen = stop));
  stopFollowing = await followAgentBrowser();
  stopFollowingApp = await followAgentApp();
});
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKey);
  window.removeEventListener("keyup", onKeyUp);
  window.removeEventListener("blur", hideHints);
  window.removeEventListener("focus", scanOnReturn);
  fullscreenTimers.forEach(clearTimeout);
  stopFullscreen?.();
  stopFollowing?.();
  stopFollowingApp?.();
});
</script>

<template>
  <div
    v-if="opened && viewed"
    class="shell"
    :class="{ mac: opened.platform === 'macos', 'no-sidebar': !ui.sidebarOpen, fullscreen }"
  >
    <Transition name="drawer">
      <Sidebar v-if="ui.sidebarOpen && !reviewing" :snap="opened" />
    </Transition>
    <!-- Kept mounted under the settings page, so the conversation keeps its scroll and draft. -->
    <main v-show="!ui.settingsOpen" class="main" :class="{ 'has-backdrop': showBackdrop, orb: !composerShown }">
      <Backdrop v-if="showBackdrop" :dimmed="!empty" />
      <TopBar :snap="viewed" />
      <UpdateBanner />
      <!-- Kept mounted behind an open type, so the transcript keeps its scroll. Timeline has several
           root nodes, so it is hidden through this wrapper. -->
      <div v-show="!showingType" class="transcript">
        <Timeline :snap="viewed" />
      </div>
      <TypeView v-if="showingType && ui.typeOpen" :id="ui.typeOpen" :key="ui.typeOpen" :snap="viewed" />
      <Composer v-if="composerShown" :snap="viewed" />
      <div v-else class="type-compose">
        <button
          class="compose-orb"
          :class="{ working: viewed.turnRunning }"
          :title="viewed.turnRunning ? 'Dial is working on it' : 'Ask Dial to change this'"
          :aria-label="viewed.turnRunning ? 'Dial is working on it' : 'Ask Dial to change this'"
          @click="openTypeComposer"
        >
          <Icon name="arrowUp" :size="19" />
        </button>
      </div>
    </main>
    <Settings v-if="ui.settingsOpen" :snap="opened" />
    <Transition name="slide">
      <Inspector v-show="ui.inspectorOpen && !ui.settingsOpen" :open="ui.inspectorOpen" :snap="opened" />
    </Transition>
  </div>
  <Welcome v-else-if="snap" />
  <div v-else-if="startupError" class="boot">
    <section class="boot-error" aria-labelledby="boot-title">
      <Logo :size="36" />
      <h2 id="boot-title">Couldn’t connect to Dial</h2>
      <p>Reconnect to load the current session. No prompt will be resent.</p>
      <pre>{{ startupError }}</pre>
      <button class="btn btn-primary" :disabled="connecting" @click="connect">{{ connecting ? 'Reconnecting…' : 'Reconnect' }}</button>
      <button class="btn btn-secondary" @click="copyText(startupError, 'Error copied')">Copy error</button>
    </section>
  </div>
  <div v-else class="boot" data-tauri-drag-region>
    <Logo :size="40" />
  </div>
  <Toasts />
  <ContextMenu />
  <template v-if="opened">
    <Palette :snap="opened" />
    <Shortcuts />
    <ImageViewer />
  </template>
</template>

<style scoped>
.shell {
  display: flex;
  height: 100%;
}
.main {
  position: relative;
  /* The backdrop sits under the content but above this pane's own colour. */
  isolation: isolate;
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg);
}
.transcript {
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
/* Room under the type for the round composer button. */
.main.orb :deep(.type-view) {
  padding-bottom: 80px;
}
/* Over a type: the composer as one round button, floating at the bottom centre. */
.type-compose {
  position: absolute;
  left: 50%;
  bottom: 22px;
  z-index: 6;
  transform: translateX(-50%);
}
.compose-orb {
  position: relative;
  display: grid;
  place-items: center;
  width: 46px;
  height: 46px;
  border-radius: 50%;
  background: var(--accent);
  color: #fff;
  box-shadow: 0 6px 22px rgb(0 0 0 / 0.28), 0 0 0 1px rgb(255 255 255 / 0.08) inset;
  transition: transform 120ms var(--ease), background 120ms var(--ease);
}
.compose-orb:hover {
  background: var(--accent-hover);
  transform: translateY(-1px) scale(1.04);
}
.compose-orb:active {
  transform: scale(0.94);
}
/* Dial is working on the type: a ring runs round the button. */
.compose-orb.working::after {
  content: "";
  position: absolute;
  inset: -5px;
  border-radius: 50%;
  border: 2px solid transparent;
  border-top-color: var(--accent);
  animation: orb-spin 0.9s linear infinite;
}
@keyframes orb-spin {
  to {
    transform: rotate(360deg);
  }
}
.boot {
  height: 100%;
  display: grid;
  place-items: center;
}
.boot-error { max-width: min(38rem, 90vw); padding: 24px; }
.boot-error pre { max-height: 30vh; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; user-select: text; }
.boot-error button + button { margin-left: 8px; }
.boot > :only-child:not(.boot-error) {
  animation: pulse 1.6s ease-in-out infinite;
}
.slide-enter-active,
.slide-leave-active {
  transition:
    margin-right 180ms var(--ease),
    opacity 180ms var(--ease);
}
.slide-enter-from,
.slide-leave-to {
  margin-right: calc(-1 * var(--inspector-width));
  opacity: 0;
}
.drawer-enter-active,
.drawer-leave-active {
  transition:
    margin-left 180ms var(--ease),
    opacity 180ms var(--ease);
}
.drawer-enter-from,
.drawer-leave-to {
  margin-left: calc(-1 * var(--sidebar-width));
  opacity: 0;
}
</style>
