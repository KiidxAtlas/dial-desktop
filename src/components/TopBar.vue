<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import type { OpenSnapshot } from "../types";
import { openMenu, ui } from "../ui";
import { finishRename, openSession, sessionItems } from "../sessionActions";
import { conversationFindOpen } from "../commands";
import { closeType, types } from "../savedTypes";
import { showPanel, showSidebar, sidebarShown } from "../workspace";
import Icon from "./Icon.vue";
import GoalPill from "./GoalPill.vue";

const props = defineProps<{ snap: OpenSnapshot }>();

/** The session on screen, once it has a row in the list (a fresh one gets it with its first prompt). */
const session = computed(() => openSession(props.snap));
/** The type open in place of the transcript, if any. */
const type = computed(() => (ui.typeOpen ? types.list.find((t) => t.id === ui.typeOpen) ?? null : null));
const editing = computed(() => !!session.value && ui.renaming?.id === session.value.id && ui.renaming.place === "title");
/** The turn is paused on the user: an approval or a question is waiting. */
const needsYou = computed(() => props.snap.approvals.length > 0 || !!props.snap.question);
const planDone = computed(() => props.snap.plan.filter((t) => t.status === "completed").length);

const input = ref<HTMLInputElement | null>(null);
watch(editing, async (now) => {
  if (!now) return;
  await nextTick();
  input.value?.focus();
  input.value?.select();
});

function onKey(event: KeyboardEvent) {
  const s = session.value;
  if (!s) return;
  if (event.key === "Enter" && !event.isComposing) {
    event.preventDefault();
    finishRename(s.id, s.title, input.value?.value ?? "");
  } else if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    ui.renaming = null;
  }
}

function more(event: MouseEvent) {
  if (session.value) openMenu(event, sessionItems(props.snap, session.value, "title"), event.currentTarget as HTMLElement);
}
</script>

<template>
  <header class="topbar" data-tauri-drag-region>
    <button
      v-if="!sidebarShown()"
      class="icon-btn"
      title="Show sidebar (⌘B)"
      aria-label="Show sidebar"
      @click="showSidebar()"
    >
      <Icon name="panelLeft" :size="16" />
    </button>
    <h1 v-if="type" class="truncate type-name" data-tauri-drag-region><span class="type-icon">{{ type.icon }}</span>{{ type.name }}</h1>
    <input
      v-else-if="editing && session"
      ref="input"
      class="rename"
      :value="session.title"
      spellcheck="false"
      aria-label="Session name"
      @keydown="onKey"
      @blur="finishRename(session.id, session.title, ($event.target as HTMLInputElement).value)"
    />
    <button v-else-if="session" class="title" title="Rename, pin, export and more" @click="more">
      <span class="truncate" :title="snap.title">{{ snap.title }}</span>
      <Icon name="chevronDown" :size="13" />
    </button>
    <h1 v-else class="truncate" data-tauri-drag-region :title="snap.title">{{ snap.title }}</h1>
    <span v-if="snap.connection.state === 'preview'" class="pill preview" data-tauri-drag-region>Preview</span>
    <span
      v-if="snap.turnRunning"
      class="pill working"
      :class="{ waiting: needsYou }"
      role="status"
      :title="needsYou ? 'Dial is waiting for your answer' : 'Dial is working'"
      data-tauri-drag-region
    ><span class="live-dot" /><span class="pill-label">{{ needsYou ? "Needs you" : "Working" }}</span></span>
    <!-- Plan progress stays in sight with the side panel closed; a click opens the plan. -->
    <button
      v-if="snap.plan.length && !(ui.inspectorOpen && ui.inspectorTab === 'plan')"
      class="plan-chip"
      :title="`Plan: ${planDone} of ${snap.plan.length} done. Show the plan`"
      @click="showPanel('plan')"
    >
      <Icon name="list" :size="12" /> {{ planDone }}/{{ snap.plan.length }}
    </button>
    <GoalPill v-if="!type || ui.typeMode === 'chat'" :snap="snap" />
    <div v-if="type" class="switch" role="tablist" aria-label="Type or its conversation">
      <button role="tab" :aria-selected="ui.typeMode === 'type'" :class="{ on: ui.typeMode === 'type' }" @click="ui.typeMode = 'type'">
        <span class="switch-icon">{{ type.icon }}</span> {{ type.name }}
      </button>
      <button role="tab" :aria-selected="ui.typeMode === 'chat'" :class="{ on: ui.typeMode === 'chat' }" @click="ui.typeMode = 'chat'">
        <Icon name="chat" :size="13" /> Chat
      </button>
    </div>
    <div class="spacer" data-tauri-drag-region />
    <button
      v-if="snap.entries.length && (!type || ui.typeMode === 'chat')"
      class="icon-btn"
      title="Find in conversation (⌘F)"
      aria-label="Find in conversation"
      :aria-expanded="conversationFindOpen"
      :class="{ on: conversationFindOpen }"
      @click="conversationFindOpen = !conversationFindOpen"
    >
      <Icon name="search" :size="16" />
    </button>
    <button
      class="icon-btn"
      :class="{ on: ui.inspectorOpen }"
      :title="ui.inspectorOpen ? 'Hide plan and changes (⌥⌘I)' : 'Show plan and changes (⌥⌘I)'"
      aria-label="Plan and changes"
      :aria-pressed="ui.inspectorOpen"
      @click="ui.inspectorOpen = !ui.inspectorOpen"
    >
      <Icon name="sidebar" :size="16" />
    </button>
    <button v-if="type" class="icon-btn" title="Close the type" aria-label="Close the type" @click="closeType()">
      <Icon name="cross" :size="15" />
    </button>
  </header>
</template>

<style scoped>
.topbar {
  position: relative;
  height: var(--topbar);
  flex: none;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 10px 0 14px;
  container: topbar / inline-size;
}
/* In a narrow bar the title comes first, but "Needs you" keeps its words: the plan chip and the
 * Working label give way before it, and the title truncates before it shrinks. */
@container topbar (max-width: 640px) {
  .pill.preview { display: none; }
  .plan-chip { display: none; }
  .working:not(.waiting) { padding: 0 7px; }
  .working:not(.waiting) .pill-label {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
}
.working.waiting { flex: none; }
h1,
.title {
  min-width: 0;
  margin: 0;
  font-size: 14px;
  font-weight: 500;
  color: var(--text-muted);
}
h1 {
  padding: 0 8px;
}
.title {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 30px;
  padding: 0 8px;
  border-radius: var(--radius-sm);
  transition:
    background 120ms var(--ease),
    color 120ms var(--ease);
}
.title svg {
  color: var(--text-faint);
  opacity: 0.6;
  transition: opacity 120ms var(--ease);
}
.title:hover {
  background: var(--hover);
  color: var(--text);
}
.title:hover svg {
  opacity: 1;
}
.rename {
  width: min(420px, 50%);
  height: 30px;
  padding: 0 8px;
  border: 1px solid var(--accent);
  border-radius: var(--radius-sm);
  outline: none;
  background: var(--bg);
  color: var(--text);
  font: inherit;
  font-size: 14px;
  font-weight: 500;
}
.spacer {
  flex: 1;
  align-self: stretch;
}
.plan-chip {
  display: inline-flex;
  flex: none;
  align-items: center;
  gap: 4px;
  height: 22px;
  margin-left: 6px;
  padding: 0 8px;
  border-radius: 999px;
  font-size: var(--text-xs);
  font-variant-numeric: tabular-nums;
  color: var(--text-muted);
  box-shadow: inset 0 0 0 1px var(--border);
}
.plan-chip:hover { background: var(--hover); color: var(--text); }
/* Title-bar buttons match the 30px title; the shared look is `.icon-btn` in styles.css. */
.icon-btn {
  width: 30px;
  height: 30px;
}
/* Status colours: neutral while working, amber when Dial needs you; red is kept for failures. */
.working {
  gap: 6px;
  color: var(--text-muted);
  background: var(--hover);
  box-shadow: inset 0 0 0 1px var(--border);
}
.working .live-dot {
  background: var(--success);
}
.working.waiting {
  color: var(--warning);
  background: color-mix(in srgb, var(--warning) 12%, transparent);
  box-shadow: none;
}
.working.waiting .live-dot {
  background: currentColor;
}
.live-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  animation: pulse 1.2s ease-in-out infinite;
}
.type-name {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text);
}
.type-icon {
  font-size: 16px;
}
/* Centered on the bar, whatever sits left and right of it. */
.switch {
  position: absolute;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  gap: 2px;
  padding: 3px;
  border-radius: 999px;
  background: var(--hover);
  box-shadow: inset 0 0 0 1px var(--border);
}
.switch button {
  display: flex;
  align-items: center;
  gap: 6px;
  max-width: 200px;
  height: 26px;
  padding: 0 14px;
  border-radius: 999px;
  color: var(--text-muted);
  font-size: 12.5px;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  transition: background 120ms var(--ease), color 120ms var(--ease);
}
.switch button:hover {
  color: var(--text);
}
.switch button.on {
  background: var(--bg);
  color: var(--text);
  box-shadow: 0 1px 3px rgb(0 0 0 / 0.14);
}
.switch-icon {
  font-size: 13px;
}
</style>
