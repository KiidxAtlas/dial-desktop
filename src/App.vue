<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, watch } from "vue";
import { engine, snapshot } from "./bridge";
import { ui, type InspectorTab } from "./ui";
import { followAgentBrowser, showPanel, trackPreviews } from "./workspace";
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
import UpdateBanner from "./components/UpdateBanner.vue";
import { startUpdateChecks } from "./updates";

const snap = computed(() => snapshot.value);
/** The snapshot once a folder is open; until then the welcome screen shows. */
const opened = computed(() => (snap.value?.workspace ? (snap.value as OpenSnapshot) : null));
const reviewing = computed(() => ui.inspectorOpen && ui.inspectorTab === "changes" && !!ui.diffPath && ui.diffExpanded && !ui.settingsOpen);
watch(() => [opened.value?.workspace.root, opened.value?.sessionId] as const, ([root, id], [previousRoot, previousId]) => {
  if (root !== previousRoot || id !== previousId) {
    ui.diffPath = null;
    ui.diffExpanded = false;
  }
});

/** No conversation yet: the background shows at full strength. */
const empty = computed(() => !!opened.value && opened.value.entries.length === 0 && !opened.value.turnRunning);
const showBackdrop = computed(
  () => hasBackground.value && (background.placement === "always" || empty.value),
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

function onKey(event: KeyboardEvent) {
  const s = opened.value;
  if (!s) return;
  const command = event.metaKey || event.ctrlKey;
  const plain = command && !event.shiftKey && !event.altKey;
  const key = event.key.toLowerCase();
  if (plain && key === "n") {
    event.preventDefault();
    startNewSession(s);
  } else if (plain && key === "k") {
    event.preventDefault();
    ui.menu = null;
    ui.paletteOpen = !ui.paletteOpen;
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
  } else if (event.key === "Escape" && !event.isComposing && s.turnRunning) {
    event.preventDefault();
    markTurnAborted(s);
    engine.abort();
  }
}

let stopFollowing: (() => void) | null = null;
onMounted(async () => {
  window.addEventListener("keydown", onKey);
  restoreImage();
  startUpdateChecks();
  stopFollowing = await followAgentBrowser();
});
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKey);
  stopFollowing?.();
});
</script>

<template>
  <div
    v-if="opened"
    class="shell"
    :class="{ mac: opened.platform === 'macos', 'no-sidebar': !ui.sidebarOpen }"
  >
    <Transition name="drawer">
      <Sidebar v-if="ui.sidebarOpen && !reviewing" :snap="opened" />
    </Transition>
    <!-- Kept mounted under the settings page, so the conversation keeps its scroll and draft. -->
    <main v-show="!ui.settingsOpen" class="main" :class="{ 'has-backdrop': showBackdrop }">
      <Backdrop v-if="showBackdrop" :dimmed="!empty" />
      <TopBar :snap="opened" />
      <UpdateBanner />
      <Timeline :snap="opened" />
      <Composer :snap="opened" />
    </main>
    <Settings v-if="ui.settingsOpen" :snap="opened" />
    <Transition name="slide">
      <Inspector v-show="ui.inspectorOpen && !ui.settingsOpen" :open="ui.inspectorOpen" :snap="opened" />
    </Transition>
  </div>
  <Welcome v-else-if="snap" />
  <div v-else class="boot" data-tauri-drag-region>
    <Logo :size="40" />
  </div>
  <Toasts />
  <ContextMenu />
  <template v-if="opened">
    <Palette :snap="opened" />
    <Shortcuts />
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
.boot {
  height: 100%;
  display: grid;
  place-items: center;
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
