<script setup lang="ts">
// The Terminal panel: your own shell in the project folder (a real terminal), and a feed of the
// commands Dial ran with their output.
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { showToast, terminalApi } from "../../bridge";
import { appearance } from "../../appearance";
import { addToPrompt } from "../../workspace";
import type { OpenSnapshot, ToolEntry } from "../../types";
import Icon from "../Icon.vue";

const props = defineProps<{ snap: OpenSnapshot; active: boolean }>();

const mode = ref<"shell" | "agent">("shell");
const host = ref<HTMLElement | null>(null);
const feed = ref<HTMLElement | null>(null);
const exited = ref(false);

let term: Terminal | null = null;
let fit: FitAddon | null = null;
let shell: number | null = null;
let unlisten: (() => void) | null = null;
let observer: ResizeObserver | null = null;

/** Commands Dial ran, oldest first. */
const commands = computed(() =>
  props.snap.entries.filter((e): e is ToolEntry => e.kind === "tool" && e.tone === "shell").slice(-60),
);

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

/** End the current shell, if any, and start a fresh one. */
async function restart() {
  if (shell !== null) await terminalApi.close(shell).catch(() => {});
  shell = null;
  await start();
}

async function start() {
  if (!term || !fit) return;
  exited.value = false;
  term.reset();
  try {
    fit.fit();
    shell = await terminalApi.open(term.cols, term.rows);
  } catch (error) {
    shell = null;
    exited.value = true;
    term.writeln(`\x1b[31m${String(error)}\x1b[0m`);
  }
}

function resize() {
  if (!term || !fit || !props.active || mode.value !== "shell") return;
  try {
    fit.fit();
  } catch {
    return;
  }
  if (shell !== null) terminalApi.resize(shell, term.cols, term.rows).catch(() => {});
}

async function create() {
  if (term || !host.value) return;
  term = new Terminal({
    fontFamily: css("--font-mono") || "monospace",
    fontSize: 12.5,
    cursorBlink: true,
    scrollback: 5000,
    allowProposedApi: false,
    theme: theme(),
  });
  fit = new FitAddon();
  term.loadAddon(fit);
  term.open(host.value);
  term.onData((data) => {
    if (shell !== null) terminalApi.write(shell, data).catch((error) => showToast(String(error)));
    else if (exited.value && data === "\r") void start();
  });
  unlisten = await terminalApi.onEvent((event) => {
    if (event.id !== shell) return;
    if (event.kind === "data") term?.write(event.data);
    else {
      shell = null;
      exited.value = true;
      term?.writeln("\r\n\x1b[2m[The shell ended. Press Enter for a new one.]\x1b[0m");
    }
  });
  observer = new ResizeObserver(() => requestAnimationFrame(resize));
  observer.observe(host.value);
  await start();
}

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

function shareCommand(tool: ToolEntry) {
  const output = tool.output?.text?.trim().slice(-6000) ?? "";
  addToPrompt(`About the command \`${tool.target}\`${output ? `:\n\n\`\`\`\n${output}\n\`\`\`` : ""}`);
}

watch(
  () => [props.active, mode.value] as const,
  async ([active, now]) => {
    if (!active) return;
    await nextTick();
    if (now === "shell") {
      if (!term) await create();
      else requestAnimationFrame(resize);
      term?.focus();
    } else {
      feed.value?.scrollTo({ top: feed.value.scrollHeight });
    }
  },
  { immediate: true },
);

watch(
  () => commands.value.at(-1)?.output?.text,
  async () => {
    if (mode.value !== "agent" || !feed.value) return;
    const el = feed.value;
    const atEnd = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    await nextTick();
    if (atEnd) el.scrollTo({ top: el.scrollHeight });
  },
);

watch(
  () => appearance.theme,
  () => requestAnimationFrame(() => term && (term.options.theme = theme())),
);

onMounted(() => {
  if (props.active && mode.value === "shell") void create();
});

onBeforeUnmount(() => {
  observer?.disconnect();
  unlisten?.();
  if (shell !== null) void terminalApi.close(shell).catch(() => {});
  term?.dispose();
});
</script>

<template>
  <div class="terminal">
    <div class="head">
      <div class="seg" role="tablist">
        <button role="tab" :class="{ on: mode === 'shell' }" @click="mode = 'shell'">Shell</button>
        <button role="tab" :class="{ on: mode === 'agent' }" @click="mode = 'agent'">
          Dial's commands <span v-if="commands.length" class="count">{{ commands.length }}</span>
        </button>
      </div>
      <div class="spacer" />
      <template v-if="mode === 'shell'">
        <button class="icon-btn" title="Give the selection (or the screen) to Dial" @click="shareShell">
          <Icon name="send" :size="14" />
        </button>
        <button class="icon-btn" title="New shell" @click="restart">
          <Icon name="plus" :size="14" />
        </button>
      </template>
    </div>

    <div v-show="mode === 'shell'" ref="host" class="xterm-host" />

    <div v-show="mode === 'agent'" ref="feed" class="feed">
      <div v-if="!commands.length" class="empty">
        <strong>No commands yet</strong>
        <span>Commands Dial runs show here with their output.</span>
      </div>
      <div v-for="tool in commands" :key="tool.key" class="cmd">
        <div class="cmd-head">
          <span
            class="dot"
            :class="{ running: tool.state === 'running', ok: tool.state === 'succeeded', bad: tool.state === 'failed' }"
          />
          <span class="mono truncate prompt" :title="tool.target">$ {{ tool.target }}</span>
          <button class="icon-btn" title="Ask Dial about this command" @click="shareCommand(tool)">
            <Icon name="chat" :size="13" />
          </button>
        </div>
        <pre v-if="tool.output?.text || tool.progress" class="out">{{ (tool.output?.text ?? tool.progress ?? "").slice(-20000) }}</pre>
        <div v-for="fact in tool.facts" :key="fact" class="fact">{{ fact }}</div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.terminal {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}
.head {
  display: flex;
  align-items: center;
  gap: 4px;
  padding-bottom: 8px;
}
.spacer {
  flex: 1;
}
.seg {
  display: flex;
  gap: 2px;
  padding: 2px;
  border-radius: var(--radius-sm);
  background: var(--hover);
}
.seg button {
  padding: 3px 10px;
  border-radius: 5px;
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.seg button.on {
  background: var(--active);
  color: var(--text);
}
.count {
  margin-left: 4px;
  font-size: var(--text-xs);
  color: var(--text-faint);
}
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
.feed {
  flex: 1;
  min-height: 0;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.cmd {
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--bg);
}
.cmd-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 4px 4px 10px;
  border-bottom: 1px solid var(--border);
  font-size: 12px;
}
.prompt {
  flex: 1;
  min-width: 0;
}
.out {
  margin: 0;
  padding: 8px 10px;
  max-height: 280px;
  overflow: auto;
  font-family: var(--font-mono);
  font-size: 11.5px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-word;
  color: var(--text-muted);
  user-select: text;
  -webkit-user-select: text;
}
.fact {
  padding: 0 10px 6px;
  font-size: var(--text-xs);
  color: var(--warning);
}
.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--text-faint);
  flex: none;
}
.dot.running {
  background: var(--accent);
  animation: pulse 1.2s ease-in-out infinite;
}
.dot.ok {
  background: var(--success);
}
.dot.bad {
  background: var(--danger);
}
.empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  margin-top: var(--space-xl);
  text-align: center;
}
.empty strong {
  font-weight: 500;
  color: var(--text-muted);
}
.empty span {
  font-size: var(--text-sm);
  color: var(--text-faint);
}
</style>
