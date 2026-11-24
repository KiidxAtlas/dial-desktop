<script setup lang="ts">
// Each view owns exactly one PTY; hiding a view leaves its process and buffer intact.
import { onMounted, onBeforeUnmount, nextTick, ref, watch } from "vue";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import { openExternal, showToast, terminalApi, type TerminalTask } from "../../bridge";
import { appearance } from "../../appearance";
import { terminalPrefs } from "../../terminalPrefs";
import { addToPrompt, showFile } from "../../workspace";
import { paneErrorText } from "../../paneErrors";
import { relativeTo } from "../../paths";
import { findPathLinks } from "../../terminalLinks";
import type { TerminalEvent } from "../../types";
import Icon from "../Icon.vue";

const props = defineProps<{ active: boolean; root: string; task?: TerminalTask }>();
const emit = defineEmits<{ ended: []; restart: [] }>();
const host = ref<HTMLElement | null>(null);
let term: Terminal | null = null;
let fit: FitAddon;
let search: SearchAddon;
let id: number | null = null;
let disposed = false;
let ended = false;
/** Text pasted before the PTY opened, sent once it has. */
let pending = "";
let observer: ResizeObserver | undefined;
const isMac = navigator.platform.includes("Mac");
/** The find bar (⌘F on a Mac, Ctrl+Shift+F elsewhere, so Ctrl+F still reaches the shell). */
const finding = ref(false);
const needle = ref("");
const notFound = ref(false);
const findInput = ref<HTMLInputElement | null>(null);
function css(name: string) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

function theme() {
  return {
    background: css("--bg"),
    foreground: css("--text"),
    cursor: css("--accent"),
    selectionBackground: css("--border-strong"),
  };
}

function resize() {
  if (!term || !props.active) return;
  fit.fit();
  if (id !== null) void terminalApi.resize(id, term.cols, term.rows).catch(() => {});
}

onMounted(async () => {
  term = new Terminal({ fontFamily: css("--font-mono") || "monospace", fontSize: terminalPrefs.fontSize,
    cursorBlink: true, scrollback: terminalPrefs.scrollback, theme: theme() });
  fit = new FitAddon();
  search = new SearchAddon();
  term.loadAddon(fit);
  term.loadAddon(search);
  // Links open in the system browser, never in the app window.
  term.loadAddon(new WebLinksAddon((_event, uri) => openExternal(uri)));
  term.registerLinkProvider({
    provideLinks(y, callback) {
      const text = term?.buffer.active.getLine(y - 1)?.translateToString(true) ?? "";
      const links = findPathLinks(text, (path) => relativeTo(props.root, path)).map((link) => ({
        range: { start: { x: link.start + 1, y }, end: { x: link.end, y } },
        text: text.slice(link.start, link.end),
        activate: () => showFile(link.path, link.line ? { start: link.line, end: link.line } : null),
      }));
      callback(links.length ? links : undefined);
    },
  });
  term.attachCustomKeyEventHandler((event) => {
    const find = event.key.toLowerCase() === "f" && (isMac ? event.metaKey && !event.ctrlKey : event.ctrlKey && event.shiftKey);
    if (event.type === "keydown" && find) {
      // Keep the window's own find shortcuts (conversation, sessions) out of the terminal.
      event.preventDefault();
      event.stopPropagation();
      void openFind();
      return false;
    }
    return true;
  });
  term.open(host.value!);
  resize();
  term.focus();
  term.onData(data => {
    if (id !== null) void terminalApi.write(id, data).catch(e => showToast(paneErrorText(e)));
    else if (!props.task && ended && data === "\r") emit("restart");
  });
  observer = new ResizeObserver(resize);
  observer.observe(host.value!);
  const receive = (event: TerminalEvent) => {
    if (disposed) return;
    if (event.kind === "data") term?.write(event.data);
    else {
      ended = true;
      id = null;
      term?.writeln(props.task ? "\r\n[Process ended]" : "\r\n[Shell ended. Press Enter for a new one.]");
      emit("ended");
    }
  };
  try {
    const opened = await terminalApi.open(props.root, term.cols, term.rows, props.task, receive);
    if (disposed) return void terminalApi.close(opened).catch(() => {});
    // A fast child may have already sent its exit on this PTY's private channel.
    if (!ended) id = opened;
    if (id !== null && pending) term?.paste(pending);
    pending = "";
    resize();
  } catch (error) {
    if (!disposed) {
      ended = true;
      term?.writeln(`Could not start: ${paneErrorText(error)}`);
      emit("ended");
    }
  }
});

async function openFind() {
  finding.value = true;
  await nextTick();
  findInput.value?.focus();
  findInput.value?.select();
}
function closeFind() {
  finding.value = false;
  notFound.value = false;
  search.clearDecorations();
  term?.focus();
}
function find(backwards = false) {
  if (!needle.value) return void (notFound.value = false);
  const found = backwards ? search.findPrevious(needle.value) : search.findNext(needle.value, { incremental: !backwards });
  notFound.value = !found;
}
watch(() => props.active, async active => { if (active) { await nextTick(); resize(); term?.focus(); } });
watch(() => appearance.theme, () => { if (term) term.options.theme = theme(); });
// Settings › Appearance › Terminal applies to open terminals at once.
watch(() => terminalPrefs.fontSize, (size) => { if (term) { term.options.fontSize = size; resize(); } });
watch(() => terminalPrefs.scrollback, (lines) => { if (term) term.options.scrollback = lines; });
onBeforeUnmount(() => {
  disposed = true;
  observer?.disconnect();
  if (id !== null) void terminalApi.close(id).catch(() => {});
  term?.dispose();
  term = null;
});
/** Put the terminal's selection (or its visible screen) into the prompt. */
function shareShell() {
  if (!term) return;
  let text = term.getSelection();
  if (!text.trim()) {
    const buffer = term.buffer.active;
    const lines: string[] = [];
    for (let i = Math.max(0, buffer.length - term.rows); i < buffer.length; i++) {
      lines.push(buffer.getLine(i)?.translateToString(true) ?? "");
    }
    text = lines.join("\n");
  }
  text = text.replace(/\s+$/, "");
  if (!text) return;
  addToPrompt(`From my terminal:\n\n\`\`\`\n${text}\n\`\`\``);
}

/** Type `text` at the prompt without running it (no Enter). `false` when the shell has ended, or
 *  when multi-line text would run line by line because the shell does not take bracketed pastes. */
function paste(text: string): boolean {
  if (!term || ended) return false;
  const clean = text.replace(/\s+$/, "");
  if (clean.includes("\n") && (id === null || !term.modes.bracketedPasteMode)) return false;
  if (id === null) {
    pending += clean;
    return true;
  }
  term.paste(clean);
  term.focus();
  return true;
}

defineExpose({ shareShell, openFind, paste });
</script>
<template>
  <div class="pty">
    <div ref="host" class="xterm-host" />
    <div v-if="finding" class="find" role="search">
      <input
        ref="findInput"
        v-model="needle"
        class="find-input"
        :class="{ miss: notFound }"
        type="search"
        placeholder="Find in terminal"
        aria-label="Find in terminal"
        spellcheck="false"
        @input="find()"
        @keydown.enter.exact.prevent="find()"
        @keydown.shift.enter.prevent="find(true)"
        @keydown.esc.prevent="closeFind"
      />
      <button class="icon-btn" title="Previous match (Shift+Enter)" @click="find(true)"><Icon name="arrowUp" :size="13" /></button>
      <button class="icon-btn" title="Next match (Enter)" @click="find()"><Icon name="arrowDown" :size="13" /></button>
      <button class="icon-btn" title="Close (Esc)" @click="closeFind"><Icon name="cross" :size="13" /></button>
    </div>
  </div>
</template>
<style scoped>
.pty {
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.find {
  position: absolute;
  top: 6px;
  right: 10px;
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 3px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--surface);
  box-shadow: 0 4px 14px rgb(0 0 0 / 0.18);
  z-index: 2;
}
.find-input {
  width: 170px;
  height: 24px;
  padding: 0 6px;
  border-radius: 5px;
  border: 1px solid var(--border);
  background: var(--bg);
  color: var(--text);
  font-size: var(--text-sm);
  outline: none;
}
.find-input.miss {
  border-color: var(--danger);
}
</style>
<style scoped>
.xterm-host {
  flex: 1;
  min-height: 0;
  padding: 6px 0 0 8px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--bg);
  overflow: hidden;
}
.xterm-host :deep(.xterm) {
  height: 100%;
}
</style>
