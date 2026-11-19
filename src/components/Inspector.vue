<script setup lang="ts">
import { computed, reactive, watch } from "vue";
import type { OpenSnapshot, ToolEntry } from "../types";
import { ui, type InspectorTab } from "../ui";
import { inspectorWidth, setInspectorWidth } from "../workspace";
import Icon from "./Icon.vue";
import BrowserPane from "./workspace/BrowserPane.vue";
import DiffPane from "./workspace/DiffPane.vue";
import FilesPane from "./workspace/FilesPane.vue";
import TerminalPane from "./workspace/TerminalPane.vue";

const props = defineProps<{ snap: OpenSnapshot; open: boolean }>();

const TABS: { id: InspectorTab; label: string; icon: "list" | "diff" | "activity" | "files" | "terminal" | "globe" }[] = [
  { id: "plan", label: "Plan", icon: "list" },
  { id: "changes", label: "Changes", icon: "diff" },
  { id: "activity", label: "Activity", icon: "activity" },
  { id: "files", label: "Files", icon: "files" },
  { id: "terminal", label: "Terminal", icon: "terminal" },
  { id: "browser", label: "Browser", icon: "globe" },
];

/** Heavy panels mount on first visit and stay mounted, so a shell or a page survives tab switches. */
const visited = reactive(new Set<InspectorTab>());
watch(
  () => [ui.inspectorTab, props.open] as const,
  ([tab, open]) => open && visited.add(tab),
  { immediate: true },
);
const shown = (tab: InspectorTab) => props.open && !ui.settingsOpen && ui.inspectorTab === tab;

const done = computed(() => props.snap.plan.filter((t) => t.status === "completed").length);
const progress = computed(() => (props.snap.plan.length ? (done.value / props.snap.plan.length) * 100 : 0));
const tools = computed(() =>
  props.snap.entries
    .filter((e): e is ToolEntry => e.kind === "tool")
    .reverse()
    .slice(0, 50),
);
const width = computed(() => inspectorWidth());

function taskClass(status: string) {
  if (["abandoned", "cancelled", "blocked"].includes(status)) return "dropped";
  return status;
}

/** Drag the left edge to resize. */
function startResize(event: PointerEvent) {
  event.preventDefault();
  const handle = event.currentTarget as HTMLElement;
  handle.setPointerCapture(event.pointerId);
  const move = (e: PointerEvent) => setInspectorWidth(window.innerWidth - e.clientX);
  const stop = () => {
    handle.removeEventListener("pointermove", move);
    handle.removeEventListener("pointerup", stop);
    document.body.classList.remove("resizing");
  };
  document.body.classList.add("resizing");
  handle.addEventListener("pointermove", move);
  handle.addEventListener("pointerup", stop);
}
</script>

<template>
  <aside class="inspector" :style="{ '--inspector-width': `${width}px` }">
    <div class="resize" title="Drag to resize" @pointerdown="startResize" />
    <div class="drag" data-tauri-drag-region />
    <div class="tabs" role="tablist">
      <button
        v-for="tab in TABS"
        :key="tab.id"
        role="tab"
        :title="tab.label"
        :aria-label="tab.label"
        :aria-selected="ui.inspectorTab === tab.id"
        :class="{ active: ui.inspectorTab === tab.id }"
        @click="ui.inspectorTab = tab.id"
      >
        <Icon :name="tab.icon" :size="14" />
        <span v-if="ui.inspectorTab === tab.id">{{ tab.label }}</span>
        <span v-else-if="tab.id === 'changes' && snap.changes.length" class="badge">{{ snap.changes.length }}</span>
        <span v-else-if="tab.id === 'activity' && snap.stats.running" class="live" />
      </button>
    </div>

    <div v-show="['files', 'terminal', 'browser'].includes(ui.inspectorTab)" class="panel">
      <FilesPane v-if="visited.has('files')" v-show="ui.inspectorTab === 'files'" :snap="snap" />
      <TerminalPane
        v-if="visited.has('terminal')"
        v-show="ui.inspectorTab === 'terminal'"
        :snap="snap"
        :active="shown('terminal')"
      />
      <BrowserPane v-if="visited.has('browser')" v-show="ui.inspectorTab === 'browser'" :active="shown('browser')" />
    </div>

    <div v-if="ui.inspectorTab === 'changes' && ui.diffPath" class="panel">
      <DiffPane :path="ui.diffPath" :root="snap.workspace.root" @back="ui.diffPath = null" />
    </div>

    <div v-else-if="['plan', 'changes', 'activity'].includes(ui.inspectorTab)" class="body">
      <!-- Plan -->
      <template v-if="ui.inspectorTab === 'plan'">
        <div v-if="!snap.plan.length" class="empty">
          <strong>No plan yet</strong>
          <span>Dial shares its task list here as it works.</span>
        </div>
        <template v-else>
          <div class="head">
            <span class="section-label">Plan</span>
            <span class="faint">{{ done }} of {{ snap.plan.length }} done</span>
          </div>
          <div class="bar"><span :style="{ width: `${progress}%` }" /></div>
          <ul class="tasks">
            <li v-for="(task, i) in snap.plan" :key="i" :class="taskClass(task.status)">
              <span class="mark">
                <Icon v-if="task.status === 'completed'" name="check" :size="11" />
                <span v-else-if="task.status === 'in_progress'" class="spinner" />
                <Icon v-else-if="taskClass(task.status) === 'dropped'" name="cross" :size="11" />
              </span>
              <span class="text">{{ task.text }}</span>
            </li>
          </ul>
        </template>
      </template>

      <!-- Changes -->
      <template v-else-if="ui.inspectorTab === 'changes'">
        <div v-if="!snap.changes.length" class="empty">
          <strong>No changes yet</strong>
          <span>Files Dial edits in this session appear here.</span>
        </div>
        <template v-else>
          <div class="head">
            <span class="section-label">{{ snap.changes.length }} files changed</span>
            <span class="faint">Click for the diff</span>
          </div>
          <button
            v-for="change in snap.changes"
            :key="change.path"
            class="file"
            :title="change.path"
            @click="ui.diffPath = change.path"
          >
            <Icon name="file" :size="14" />
            <div class="file-text">
              <div class="mono truncate">{{ change.name }}</div>
              <div class="faint truncate">{{ change.dir || "./" }}</div>
            </div>
            <Icon name="chevronRight" :size="13" />
          </button>
        </template>
      </template>

      <!-- Activity -->
      <template v-else>
        <div class="stats">
          <div class="stat"><span class="dot running" /><b>{{ snap.stats.running }}</b> running</div>
          <div class="stat"><span class="dot ok" /><b>{{ snap.stats.succeeded }}</b> done</div>
          <div class="stat"><span class="dot bad" /><b>{{ snap.stats.failed }}</b> failed</div>
        </div>
        <div v-if="!tools.length" class="empty">
          <strong>No tool calls yet</strong>
          <span>Every command, edit and search Dial runs is listed here.</span>
        </div>
        <template v-else>
          <div class="head"><span class="section-label">Recent</span></div>
          <div v-for="tool in tools" :key="tool.key" class="activity" :title="tool.target">
            <span
              class="dot"
              :class="{
                running: tool.state === 'running',
                ok: tool.state === 'succeeded',
                bad: tool.state === 'failed',
              }"
            />
            <span class="activity-label">{{ tool.label }}</span>
            <span class="mono faint truncate">{{ tool.target }}</span>
          </div>
        </template>
      </template>
    </div>
  </aside>
</template>

<style scoped>
.inspector {
  position: relative;
  width: var(--inspector-width);
  flex: none;
  display: flex;
  flex-direction: column;
  background: var(--sidebar);
  border-left: 1px solid var(--border);
  padding: 0 var(--space-lg) var(--space-md);
  transition: width 160ms var(--ease);
}
:global(body.resizing) .inspector {
  transition: none;
}
:global(body.resizing) {
  cursor: col-resize;
  user-select: none;
}
.resize {
  position: absolute;
  left: -3px;
  top: 0;
  bottom: 0;
  width: 6px;
  cursor: col-resize;
  z-index: 2;
}
.resize:hover {
  background: var(--border-strong);
}
.panel {
  flex: 1;
  min-height: 0;
  padding-top: var(--space-md);
}
.panel > * {
  height: 100%;
}
.drag {
  height: 10px;
  flex: none;
}
.mac .drag {
  height: 14px;
}
.tabs {
  display: flex;
  gap: 2px;
  height: 32px;
  flex: none;
}
.tabs button {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 8px;
  border-radius: var(--radius-sm);
  font-size: 13px;
  color: var(--text-faint);
  white-space: nowrap;
  transition:
    background 120ms var(--ease),
    color 120ms var(--ease);
}
.tabs button:hover {
  color: var(--text);
}
.tabs button.active {
  background: var(--active);
  color: var(--text);
}
.badge {
  font-size: var(--text-xs);
  color: var(--text-muted);
}
.live {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--accent);
  animation: pulse 1.2s ease-in-out infinite;
}
.body {
  flex: 1;
  overflow-y: auto;
  padding-top: var(--space-lg);
  margin-right: -8px;
  padding-right: 8px;
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
.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
}
.faint {
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.bar {
  height: 4px;
  border-radius: 2px;
  background: var(--raised);
  overflow: hidden;
  margin-bottom: var(--space-md);
}
.bar span {
  display: block;
  height: 100%;
  border-radius: 2px;
  background: var(--text-muted);
  transition: width 300ms var(--ease);
}
.tasks {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.tasks li {
  display: flex;
  gap: 10px;
  font-size: 13.5px;
  line-height: 1.45;
  color: var(--text-muted);
}
.mark {
  display: grid;
  place-items: center;
  width: 18px;
  height: 18px;
  margin-top: 1px;
  border-radius: 50%;
  flex: none;
  border: 1.4px solid var(--border-strong);
}
.completed .mark {
  border: none;
  color: var(--success);
}
.in_progress .mark {
  border: none;
  color: var(--accent);
}
.in_progress .text {
  color: var(--text);
}
.dropped .mark {
  border: none;
  color: var(--text-faint);
}
.dropped .text {
  color: var(--text-faint);
  text-decoration: line-through;
}
.file {
  display: flex;
  align-items: center;
  gap: 10px;
  width: calc(100% + 16px);
  padding: 6px 8px;
  margin: 0 -8px;
  border-radius: var(--radius-sm);
  color: var(--text-faint);
  text-align: left;
}
.file:hover {
  background: var(--surface);
}
.file-text {
  flex: 1;
  min-width: 0;
  color: var(--text);
  font-size: 13px;
  line-height: 1.4;
}
.stats {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 6px;
  margin-bottom: var(--space-lg);
}
.stat {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  background: var(--hover);
  font-size: var(--text-sm);
  color: var(--text-faint);
  white-space: nowrap;
}
.stat b {
  color: var(--text);
  font-weight: 600;
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
.activity {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 26px;
  font-size: var(--text-sm);
}
.activity-label {
  white-space: nowrap;
}
</style>
