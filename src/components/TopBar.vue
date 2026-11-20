<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import type { OpenSnapshot } from "../types";
import { openMenu, ui } from "../ui";
import { finishRename, openSession, sessionItems } from "../sessionActions";
import { conversationFindOpen } from "../commands";
import Icon from "./Icon.vue";

const props = defineProps<{ snap: OpenSnapshot }>();

/** The session on screen, once it has a row in the list (a fresh one gets it with its first prompt). */
const session = computed(() => openSession(props.snap));
const editing = computed(() => !!session.value && ui.renaming?.id === session.value.id && ui.renaming.place === "title");

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
      v-if="!ui.sidebarOpen"
      class="icon-btn"
      title="Show sidebar (⌘B)"
      aria-label="Show sidebar"
      @click="ui.sidebarOpen = true"
    >
      <Icon name="panelLeft" :size="16" />
    </button>
    <input
      v-if="editing && session"
      ref="input"
      class="rename"
      :value="session.title"
      spellcheck="false"
      aria-label="Session name"
      @keydown="onKey"
      @blur="finishRename(session.id, session.title, ($event.target as HTMLInputElement).value)"
    />
    <button v-else-if="session" class="title" title="Rename, pin, export and more" @click="more">
      <span class="truncate">{{ snap.title }}</span>
      <Icon name="chevronDown" :size="13" />
    </button>
    <h1 v-else class="truncate" data-tauri-drag-region>{{ snap.title }}</h1>
    <span v-if="snap.connection.state === 'preview'" class="pill" data-tauri-drag-region>Preview</span>
    <span v-if="snap.turnRunning" class="pill working" role="status" data-tauri-drag-region><span class="live-dot" />Working</span>
    <div class="spacer" data-tauri-drag-region />
    <button
      v-if="snap.entries.length"
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
      @click="ui.inspectorOpen = !ui.inspectorOpen"
    >
      <Icon name="sidebar" :size="16" />
    </button>
  </header>
</template>

<style scoped>
.topbar {
  height: var(--topbar);
  flex: none;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 10px 0 14px;
}
h1,
.title {
  min-width: 0;
  margin: 0;
  font-size: 14px;
  font-weight: 500;
  color: var(--text-muted);
}
h1 {
  padding: 0 6px;
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
  opacity: 0;
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
.icon-btn {
  display: grid;
  place-items: center;
  width: 30px;
  height: 30px;
  flex: none;
  border-radius: var(--radius-sm);
  color: var(--text-faint);
  transition:
    background 120ms var(--ease),
    color 120ms var(--ease),
    transform 90ms var(--ease);
}
.icon-btn:hover,
.icon-btn.on {
  background: var(--hover);
  color: var(--text);
}
.icon-btn:active {
  transform: scale(0.94);
}
.working {
  gap: 6px;
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 12%, transparent);
}
.live-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: currentColor;
  animation: pulse 1.2s ease-in-out infinite;
}
</style>
