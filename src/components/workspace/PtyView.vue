<script setup lang="ts">
// Each view owns exactly one PTY; hiding a view leaves its process and buffer intact.
import { onMounted, onBeforeUnmount, nextTick, ref, watch } from "vue";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { showToast, terminalApi, type TerminalTask } from "../../bridge";
import { appearance } from "../../appearance";
import { addToPrompt } from "../../workspace";
import type { TerminalEvent } from "../../types";

const props = defineProps<{ active: boolean; root: string; task?: TerminalTask }>();
const emit = defineEmits<{ ended: []; restart: [] }>();
const host = ref<HTMLElement | null>(null);
let term: Terminal | null = null;
let fit: FitAddon;
let id: number | null = null;
let disposed = false;
let ended = false;
let observer: ResizeObserver | undefined;
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
  term = new Terminal({ fontFamily: css("--font-mono") || "monospace", fontSize: 12.5,
    cursorBlink: true, scrollback: 5000, theme: theme() });
  fit = new FitAddon();
  term.loadAddon(fit);
  term.open(host.value!);
  resize();
  term.focus();
  term.onData(data => {
    if (id !== null) void terminalApi.write(id, data).catch(e => showToast(String(e)));
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
    resize();
  } catch (error) {
    if (!disposed) {
      ended = true;
      term?.writeln(String(error));
      emit("ended");
    }
  }
});
watch(() => props.active, async active => { if (active) { await nextTick(); resize(); term?.focus(); } });
watch(() => appearance.theme, () => { if (term) term.options.theme = theme(); });
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

defineExpose({ shareShell });
</script>
<template><div ref="host" class="xterm-host" /></template>
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
