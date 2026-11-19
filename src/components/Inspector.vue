<script setup lang="ts">
import { computed } from "vue";
import type { OpenSnapshot, ToolEntry } from "../types";
import { ui, type InspectorTab } from "../ui";
import Icon from "./Icon.vue";

const props = defineProps<{ snap: OpenSnapshot }>();

const TABS: { id: InspectorTab; label: string }[] = [
  { id: "plan", label: "Plan" },
  { id: "changes", label: "Changes" },
  { id: "activity", label: "Activity" },
];

const done = computed(() => props.snap.plan.filter((t) => t.status === "completed").length);
const progress = computed(() => (props.snap.plan.length ? (done.value / props.snap.plan.length) * 100 : 0));
const tools = computed(() =>
  props.snap.entries
    .filter((e): e is ToolEntry => e.kind === "tool")
    .reverse()
    .slice(0, 50),
);
const tabIndex = computed(() => TABS.findIndex((t) => t.id === ui.inspectorTab));

function taskClass(status: string) {
  if (["abandoned", "cancelled", "blocked"].includes(status)) return "dropped";
  return status;
}
</script>

<template>
  <aside class="inspector">
    <div class="drag" data-tauri-drag-region />
    <div class="tabs" role="tablist" :style="{ '--i': tabIndex }">
      <span class="thumb" />
      <button
        v-for="tab in TABS"
        :key="tab.id"
        role="tab"
        :aria-selected="ui.inspectorTab === tab.id"
        :class="{ active: ui.inspectorTab === tab.id }"
        @click="ui.inspectorTab = tab.id"
      >
        {{ tab.label }}
      </button>
    </div>

    <div class="body">
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
          </div>
          <div v-for="change in snap.changes" :key="change.path" class="file" :title="change.path">
            <Icon name="file" :size="14" />
            <div class="file-text">
              <div class="mono truncate">{{ change.name }}</div>
              <div class="faint truncate">{{ change.dir || "./" }}</div>
            </div>
          </div>
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
  width: var(--inspector-width);
  flex: none;
  display: flex;
  flex-direction: column;
  background: var(--sidebar);
  border-left: 1px solid var(--border);
  padding: 0 var(--space-lg) var(--space-md);
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
  gap: 4px;
  height: 32px;
  flex: none;
}
.thumb {
  display: none;
}
.tabs button {
  padding: 0 10px;
  border-radius: var(--radius-sm);
  font-size: 13px;
  color: var(--text-faint);
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
  padding: 6px 8px;
  margin: 0 -8px;
  border-radius: var(--radius-sm);
  color: var(--text-faint);
}
.file:hover {
  background: var(--surface);
}
.file-text {
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
