<script setup lang="ts">
// The Terminal panel: your own shell in the project folder (a real terminal), and a feed of the
// commands Dial ran with their output.
import { computed, nextTick, ref, watch } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import { showToast, type TerminalTask } from "../../bridge";
import PtyView from "./PtyView.vue";
import { addToPrompt } from "../../workspace";
import { terminalRequest } from "../../commandRunner";
import type { OpenSnapshot, ToolEntry } from "../../types";
import Icon from "../Icon.vue";

const props = defineProps<{ snap: OpenSnapshot; active: boolean }>();

type TaskSlot = TerminalTask["kind"];
const mode = ref<"shell" | TaskSlot | "agent">("shell");
const shellView = ref<InstanceType<typeof PtyView> | null>(null);
const shellKey = ref(0);
const shellVisited = ref(false);
const tasks = ref<Partial<Record<TaskSlot, { task: TerminalTask; root: string; key: number; ended: boolean }>>>({});
const taskSlots: TaskSlot[] = ["slash", "scheduler"];
const slotLabel = { slash: "Command", scheduler: "Scheduler" };
let nextKey = 0;
const feed = ref<HTMLElement | null>(null);

async function closeTask(slot: TaskSlot) {
  const active = tasks.value[slot];
  if (!active) return;
  if (!active.ended && !(await ask(`End the separate ${slotLabel[slot].toLowerCase()} session? Your shell will stay open.`, { title: `End ${slotLabel[slot].toLowerCase()}`, kind: "warning", okLabel: "End session" }))) return;
  if (tasks.value[slot]?.key !== active.key) return;
  delete tasks.value[slot];
  mode.value = "shell";
}
watch(terminalRequest, request => {
  if (!request) return;
  terminalRequest.value = null;
  const slot = request.task.kind;
  mode.value = slot;
  const active = tasks.value[slot];
  if (active && !active.ended) return showToast(slot === "scheduler" ? "The project scheduler is already open" : "End the current command session before starting another");
  tasks.value[slot] = { ...request, key: ++nextKey, ended: false };
}, { immediate: true });
/** Commands Dial ran, oldest first. */
const commands = computed(() =>
  props.snap.entries.filter((e): e is ToolEntry => e.kind === "tool" && e.tone === "shell").slice(-60),
);

function shareCommand(tool: ToolEntry) {
  const output = tool.output?.text?.trim().slice(-6000) ?? "";
  addToPrompt(`About the command \`${tool.target}\`${output ? `:\n\n\`\`\`\n${output}\n\`\`\`` : ""}`);
}

watch(
  () => [props.active, mode.value] as const,
  async ([active, now]) => {
    if (!active) return;
    if (now === "shell") shellVisited.value = true;
    if (now === "agent") { await nextTick(); feed.value?.scrollTo({ top: feed.value.scrollHeight }); }
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

</script>

<template>
  <div class="terminal">
    <div class="head">
      <div class="seg" role="tablist">
        <button role="tab" :class="{ on: mode === 'shell' }" @click="mode = 'shell'">Shell</button>
        <template v-for="slot in taskSlots" :key="slot">
          <button v-if="tasks[slot]" role="tab" :class="{ on: mode === slot }" @click="mode = slot">{{ slotLabel[slot] }}</button>
        </template>
        <button role="tab" :class="{ on: mode === 'agent' }" @click="mode = 'agent'">
          Dial's commands <span v-if="commands.length" class="count">{{ commands.length }}</span>
        </button>
      </div>
      <div class="spacer" />
      <template v-if="mode === 'shell'">
        <button class="icon-btn" title="Give the selection (or the screen) to Dial" @click="shellView?.shareShell()">
          <Icon name="send" :size="14" />
        </button>
        <button class="icon-btn" title="New shell" @click="shellKey++">
          <Icon name="plus" :size="14" />
        </button>
      </template>
      <button v-if="mode === 'slash' || mode === 'scheduler'" class="btn" @click="closeTask(mode)">{{ tasks[mode]?.ended ? "Close" : "End " + slotLabel[mode].toLowerCase() }}</button>
    </div>

    <PtyView v-if="shellVisited" v-show="mode === 'shell'" :key="'shell' + shellKey" ref="shellView" @restart="shellKey++" :root="snap.workspace.root" :active="active && mode === 'shell'" />
    <template v-for="slot in taskSlots" :key="slot">
      <PtyView v-if="tasks[slot]" v-show="mode === slot" :key="tasks[slot]!.key" :task="tasks[slot]!.task" :root="tasks[slot]!.root" :active="active && mode === slot" @ended="tasks[slot]!.ended = true" />
    </template>

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
