<script setup lang="ts">
// The Browser panel: an address bar over a native webview that the Rust side lays over `stage`.
// The webview follows the stage's box and hides while the panel is off screen or an overlay
// (menu, palette, settings) would sit under it, since a native view always draws on top.
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { browserApi, openExternal, showToast } from "../../bridge";
import { appearance } from "../../appearance";
import { openMenu, ui, type MenuItem } from "../../ui";
import { addToPrompt, workspace } from "../../workspace";
import { viewedImage } from "../../imageViewer";
import { paneErrorText } from "../../paneErrors";
import type { Bounds, BrowserState, PageInfo } from "../../types";
import {
  SEARCH_ENGINES,
  VIEWPORTS,
  browserOptionsApi,
  isSearchTemplate,
  loadSearch,
  loadViewport,
  pushHistory,
  saveSearch,
  saveViewport,
  searchLabel,
  searchTemplate,
  type HistoryEntry,
  type SearchChoice,
  type SearchEngineId,
  type ViewportId,
} from "../../browserOptions";
import Icon from "../Icon.vue";

const props = defineProps<{ active: boolean }>();

const stage = ref<HTMLElement | null>(null);
/** The box the native view fills: the whole stage, or a viewport preset's frame inside it. */
const view = ref<HTMLElement | null>(null);
const input = ref<HTMLInputElement | null>(null);
const address = ref("");
const editing = ref(false);
const state = ref<BrowserState>({ url: "", title: "", loading: false });
/** The webview exists (created on the first page opened). */
const created = ref(false);
const busy = ref(false);
const search = ref<SearchChoice>(loadSearch());
const viewport = ref<ViewportId>(loadViewport());
/** Pages shown in this panel session, newest first. */
const history = ref<HistoryEntry[]>([]);
const devtools = ref(false);

const preset = computed(() => VIEWPORTS.find((v) => v.id === viewport.value) ?? VIEWPORTS[0]);
const viewStyle = computed(() =>
  preset.value.width
    ? { width: `min(${preset.value.width}px, 100%)`, height: `min(${preset.value.height}px, 100%)` }
    : { width: "100%", height: "100%" },
);

const blank = computed(() => !state.value.url || state.value.url === "about:blank");
/** Overlays that would be hidden under the native view. */
const covered = computed(
  () => ui.settingsOpen || ui.paletteOpen || ui.shortcutsOpen || ui.modelPickerOpen || !!ui.menu || !!viewedImage.value,
);
const visible = computed(() => props.active && !covered.value && created.value && !blank.value);

function bounds(): Bounds | null {
  const el = view.value ?? stage.value;
  if (!el) return null;
  const box = el.getBoundingClientRect();
  if (box.width < 2 || box.height < 2) return null;
  // CSS pixels times the page zoom are the window's logical pixels.
  const z = appearance.zoom;
  return { x: box.left * z, y: box.top * z, width: box.width * z, height: box.height * z };
}

async function open(url: string) {
  const box = bounds();
  if (!box) return;
  busy.value = true;
  try {
    if (created.value) {
      await browserApi.navigate(url);
      await place();
    } else {
      state.value = await browserApi.show(box, url);
      created.value = true;
    }
    editing.value = false;
  } catch (error) {
    showToast(paneErrorText(error));
  } finally {
    busy.value = false;
  }
}

/** Put the webview where the stage is, or hide it. */
async function place() {
  try {
    const box = bounds();
    if (visible.value && box) await browserApi.show(box);
    else if (created.value) await browserApi.hide();
  } catch (error) {
    console.warn("browser placement failed", error);
  }
}

/** Run `fn` on the next frame, or soon when the window is in the background (WebKit pauses
 *  animation frames there, and the agent drives the panel while the user looks elsewhere). */
function soon(fn: () => void): () => void {
  let done = false;
  const once = () => {
    if (done) return;
    done = true;
    cancelAnimationFrame(id);
    clearTimeout(timer);
    fn();
  };
  const id = requestAnimationFrame(once);
  const timer = setTimeout(once, 80);
  return () => {
    done = true;
    cancelAnimationFrame(id);
    clearTimeout(timer);
  };
}

let cancelPlace: (() => void) | null = null;
function placeSoon() {
  cancelPlace?.();
  cancelPlace = soon(() => void place());
}

function submit() {
  const text = address.value.trim();
  if (text) void open(text);
}

function go(action: "back" | "forward" | "reload" | "stop") {
  browserApi.go(action).catch((error) => showToast(paneErrorText(error)));
}

async function inspect(): Promise<PageInfo | null> {
  try {
    return await browserApi.inspect();
  } catch (error) {
    showToast(paneErrorText(error));
    return null;
  }
}

function quote(text: string, limit = 4000): string {
  const clipped = text.trim().slice(0, limit);
  return clipped
    .split("\n")
    .map((line) => `> ${line}`)
    .join("\n");
}

/** Hand the page to the agent: a menu of what to add to the prompt. */
async function share(event: MouseEvent) {
  const anchor = event.currentTarget as HTMLElement;
  const page = await inspect();
  if (!page) return;
  const name = page.title ? `${page.title} (${page.url})` : page.url;
  const items: MenuItem[] = [
    {
      label: "Ask Dial to check this page",
      icon: "sparkle",
      run: () =>
        addToPrompt(
          `Open ${page.url} in the Browser panel (browser_open) and check that it works: look for visual problems, broken interactions and console errors, then report what you find.`,
        ),
    },
    { label: "Add page address", icon: "globe", run: () => addToPrompt(`Page: ${name}`) },
    {
      label: "Add selected text",
      icon: "copy",
      disabled: !page.selection.trim(),
      run: () => addToPrompt(`From ${name}:\n\n${quote(page.selection)}`),
    },
    {
      label: "Add page text",
      icon: "file",
      disabled: !page.text.trim(),
      run: () => addToPrompt(`Text of ${name}:\n\n${quote(page.text, 8000)}`),
    },
    {
      label: page.errors.length ? `Add ${page.errors.length} console error${page.errors.length === 1 ? "" : "s"}` : "No console errors",
      icon: "terminal",
      disabled: !page.errors.length,
      run: () =>
        addToPrompt(`Console errors on ${name}:\n\n${page.errors.map((e) => `- ${e}`).join("\n")}\n\nFind and fix their cause.`),
    },
  ];
  openMenu(event, items, anchor);
}

function applySearch() {
  browserOptionsApi.setSearch(searchTemplate(search.value)).catch((error) => showToast(paneErrorText(error)));
}

function chooseSearch(engine: SearchEngineId, custom = search.value.custom) {
  search.value = { engine, custom };
  saveSearch(search.value);
  applySearch();
}

function chooseViewport(id: ViewportId) {
  viewport.value = id;
  saveViewport(id);
  placeSoon();
}

/** **Open file…**: the backend serves the file's folder on loopback and hands back its address. */
async function openFile() {
  try {
    const url = await browserOptionsApi.openFile();
    if (url) await open(url);
  } catch (error) {
    showToast(paneErrorText(error));
  }
}

function inspector() {
  browserOptionsApi.devtools().catch((error) => showToast(paneErrorText(error)));
}

/** The panel's menu: open a local file, the viewport and the search engine. */
function more(event: MouseEvent) {
  const mark = (on: boolean) => (on ? "✓" : undefined);
  const items: MenuItem[] = [
    { label: "Open file…", icon: "file", run: () => void openFile() },
    { divider: true },
    ...VIEWPORTS.map((v): MenuItem => ({ label: v.label, icon: "monitor", hint: mark(viewport.value === v.id), run: () => chooseViewport(v.id) })),
    { divider: true },
    ...SEARCH_ENGINES.map((e): MenuItem => ({
      label: `Search with ${e.label}`,
      icon: "search",
      hint: mark(search.value.engine === e.id),
      run: () => chooseSearch(e.id),
    })),
    {
      label: search.value.engine === "custom" ? `Custom search: ${search.value.custom}` : "Custom search…",
      icon: "pencil",
      hint: mark(search.value.engine === "custom"),
      prompt: {
        label: "Search address, with %s where the words go",
        placeholder: "https://example.com/search?q=%s",
        maxLength: 500,
        confirm: "Use",
        submit: (value: string) => {
          if (!isSearchTemplate(value)) {
            showToast("A search address must start with https:// and contain %s");
            return;
          }
          chooseSearch("custom", value.trim());
        },
      },
    },
  ];
  openMenu(event, items, event.currentTarget as HTMLElement, `Searches use ${searchLabel(search.value)}`);
}

/** Recent pages of this panel session, to jump back to. */
function showHistory(event: MouseEvent) {
  const items: MenuItem[] = history.value.length
    ? history.value.map((entry) => ({
        label: entry.title ? `${entry.title} — ${entry.url}` : entry.url,
        icon: "globe",
        hint: entry.url === state.value.url ? "✓" : undefined,
        run: () => void open(entry.url),
      }))
    : [{ label: "No pages yet", disabled: true, run: () => {} }];
  openMenu(event, items, event.currentTarget as HTMLElement, "Recent pages");
}

watch(state, (now) => {
  if (!editing.value) address.value = now.url === "about:blank" ? "" : now.url;
  if (!now.loading) history.value = pushHistory(history.value, now.url, now.title);
});
watch(visible, placeSoon);
watch(() => appearance.zoom, placeSoon);

// Opening an empty browser puts the cursor in the address bar.
watch(
  () => props.active,
  async (active) => {
    if (!active || !blank.value || workspace.pendingUrl) return;
    await nextTick();
    input.value?.focus();
  },
  { immediate: true },
);

/** ⌘L (Ctrl+L) edits the address while the panel shows. */
function onKey(event: KeyboardEvent) {
  if (!props.active || !(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey) return;
  if (event.key.toLowerCase() !== "l") return;
  event.preventDefault();
  input.value?.focus();
}

// A preview Dial started (or a link) asks to open here.
watch(
  () => [workspace.pendingUrl, props.active] as const,
  async ([url, active]) => {
    if (!url || !active) return;
    workspace.pendingUrl = null;
    address.value = url;
    await nextTick();
    // The panel may still be sliding in; wait until it has a size.
    soon(() => void open(url));
  },
  { immediate: true },
);

let observer: ResizeObserver | null = null;
let unlisten: (() => void) | null = null;

onMounted(async () => {
  observer = new ResizeObserver(placeSoon);
  if (stage.value) observer.observe(stage.value);
  if (view.value) observer.observe(view.value);
  window.addEventListener("resize", placeSoon);
  window.addEventListener("keydown", onKey);
  try {
    void browserOptionsApi.setSearch(searchTemplate(search.value)).catch(() => {});
    browserOptionsApi
      .features()
      .then((features) => (devtools.value = features.devtools))
      .catch(() => {});
    unlisten = await browserApi.onState((now) => (state.value = now));
    const now = await browserApi.state();
    if (now.url) {
      state.value = now;
      created.value = true;
      placeSoon();
    }
  } catch {
    // Outside Tauri there is no browser.
  }
});

onBeforeUnmount(() => {
  observer?.disconnect();
  window.removeEventListener("resize", placeSoon);
  window.removeEventListener("keydown", onKey);
  cancelPlace?.();
  unlisten?.();
  if (created.value) void browserApi.hide().catch(() => {});
  void browserOptionsApi.stopLocal().catch(() => {});
});
</script>

<template>
  <div class="browser">
    <form class="bar" @submit.prevent="submit">
      <button type="button" class="icon-btn" title="Back" :disabled="blank" @click="go('back')">
        <Icon name="arrowLeft" :size="15" />
      </button>
      <button type="button" class="icon-btn" title="Forward" :disabled="blank" @click="go('forward')">
        <Icon name="arrowRight" :size="15" />
      </button>
      <button type="button" class="icon-btn" title="Recent pages" @click="showHistory">
        <Icon name="list" :size="15" />
      </button>
      <button
        type="button"
        class="icon-btn"
        :title="state.loading ? 'Stop' : 'Reload'"
        :disabled="blank"
        @click="go(state.loading ? 'stop' : 'reload')"
      >
        <Icon :name="state.loading ? 'cross' : 'refresh'" :size="15" />
      </button>
      <div class="address" :class="{ loading: state.loading || busy }">
        <Icon name="globe" :size="13" />
        <input
          ref="input"
          v-model="address"
          spellcheck="false"
          autocapitalize="off"
          placeholder="Address, localhost:3000 or search"
          aria-label="Address"
          @focus="editing = true; ($event.target as HTMLInputElement).select()"
          @blur="editing = false"
          @keydown.esc="address = state.url; ($event.target as HTMLInputElement).blur()"
        />
      </div>
      <button type="button" class="icon-btn" title="Give this page to Dial" :disabled="blank" @click="share">
        <Icon name="send" :size="15" />
      </button>
      <button
        type="button"
        class="icon-btn"
        title="Open in the system browser"
        :disabled="blank"
        @click="openExternal(state.url)"
      >
        <Icon name="external" :size="15" />
      </button>
      <button v-if="devtools" type="button" class="icon-btn" title="Inspect (Web Inspector)" :disabled="blank" @click="inspector">
        <Icon name="terminal" :size="15" />
      </button>
      <button type="button" class="icon-btn" title="Open file, viewport and search engine" @click="more">
        <Icon name="more" :size="15" />
      </button>
    </form>
    <div v-if="workspace.agentActivity" class="agent" role="status">
      <Icon name="sparkle" :size="12" />
      <span class="truncate">Dial: {{ workspace.agentActivity }}</span>
    </div>
    <div v-if="state.title && !blank" class="page-title truncate" :title="state.title">{{ state.title }}</div>

    <div ref="stage" class="stage" :class="{ framed: preset.width > 0 }">
      <div ref="view" class="view" :style="viewStyle">
        <div v-if="blank" class="start">
          <Icon name="globe" :size="28" />
          <strong>Browse without leaving Dial</strong>
          <span>Open your app's dev server or any page, then hand it to Dial with the send button.</span>
          <button type="button" class="preview open-file" @click="openFile">
            <Icon name="file" :size="13" />
            <span>Open a local HTML file…</span>
          </button>
          <div v-if="workspace.previews.length" class="previews">
            <span class="section-label">Started in this session</span>
            <button v-for="url in workspace.previews" :key="url" class="preview" @click="open(url)">
              <span class="dot" />
              <span class="mono truncate">{{ url }}</span>
            </button>
          </div>
          <label class="auto">
            <input v-model="workspace.autoOpenPreviews" type="checkbox" />
            Open dev servers Dial starts here automatically
          </label>
        </div>
        <div v-else-if="covered && active" class="covered">
          <Icon name="globe" :size="20" />
          <span>Page hidden while the menu is open</span>
          <span v-if="state.title" class="truncate covered-title">{{ state.title }}</span>
        </div>
      </div>
      <span v-if="preset.width > 0 && !blank" class="frame-label">{{ preset.label }}</span>
    </div>
  </div>
</template>

<style scoped>
.browser {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}
.bar {
  display: flex;
  align-items: center;
  gap: 2px;
  flex: none;
  padding-bottom: 8px;
}
.address {
  position: relative;
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  margin: 0 4px;
  padding: 0 10px;
  border-radius: var(--radius-sm);
  background: var(--surface);
  border: 1px solid var(--border);
  color: var(--text-faint);
  overflow: hidden;
}
.address:focus-within {
  border-color: var(--focus-border);
}
.address.loading::after {
  content: "";
  position: absolute;
  left: 0;
  bottom: 0;
  height: 2px;
  width: 40%;
  background: var(--accent);
  animation: loading 1.1s var(--ease) infinite;
}
@keyframes loading {
  from {
    transform: translateX(-100%);
  }
  to {
    transform: translateX(260%);
  }
}
.address input {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: var(--text-sm);
}
.page-title {
  flex: none;
  font-size: var(--text-xs);
  color: var(--text-faint);
  padding: 0 4px 6px;
}
.agent {
  flex: none;
  display: flex;
  align-items: center;
  gap: 6px;
  margin: 0 4px 6px;
  padding: 4px 8px;
  border-radius: var(--radius-sm);
  background: color-mix(in srgb, var(--accent) 14%, transparent);
  color: var(--text);
  font-size: var(--text-xs);
}
.stage {
  position: relative;
  flex: 1;
  min-height: 0;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--bg);
  overflow: hidden;
}
.stage.framed {
  display: grid;
  place-items: center;
  background: var(--surface);
}
.view {
  position: relative;
  min-width: 0;
  min-height: 0;
}
.stage.framed .view {
  background: var(--bg);
  outline: 1px solid var(--border);
  box-shadow: 0 0 0 6px color-mix(in srgb, var(--border) 50%, transparent);
}
.frame-label {
  position: absolute;
  right: 8px;
  bottom: 4px;
  font-size: var(--text-xs);
  color: var(--text-faint);
  pointer-events: none;
}
.open-file {
  margin-top: var(--space-lg);
}
.start {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: var(--space-xl);
  text-align: center;
  color: var(--text-faint);
}
.start strong {
  margin-top: 6px;
  font-weight: 500;
  color: var(--text-muted);
}
.start > span {
  font-size: var(--text-sm);
  max-width: 320px;
}
.previews {
  display: flex;
  flex-direction: column;
  gap: 4px;
  width: 100%;
  max-width: 340px;
  margin-top: var(--space-lg);
  text-align: left;
}
.preview {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 10px;
  border-radius: var(--radius-sm);
  background: var(--surface);
  color: var(--text);
  font-size: var(--text-sm);
}
.preview:hover {
  background: var(--raised);
}
.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--success);
  flex: none;
}
.auto {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: var(--space-lg);
  font-size: var(--text-sm);
}
.covered {
  height: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  font-size: var(--text-sm);
  color: var(--text-faint);
  padding: var(--space-lg);
}
.covered-title {
  max-width: 100%;
  font-size: var(--text-xs);
}
</style>
