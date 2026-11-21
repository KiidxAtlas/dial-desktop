<script setup lang="ts">
// The answer to a typed slash command (`/goal`, `/policy`, `/search …`), shown as a sheet.
import { ref, watch } from "vue";
import { commandResult } from "../commandRunner";
import { copyText } from "../sessionActions";
import Icon from "./Icon.vue";
import { vFocus } from "./Shortcuts.vue";

const raw = ref(false);
watch(commandResult, () => (raw.value = false));

function close() {
  commandResult.value = null;
}

function onKey(event: KeyboardEvent) {
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    close();
  }
}

function copy() {
  const r = commandResult.value;
  if (!r) return;
  const text = raw.value
    ? JSON.stringify(r.data, null, 2)
    : [r.error, r.text, ...(r.rows ?? []).map((row) => [row.title, row.detail].filter(Boolean).join(" — "))].filter(Boolean).join("\n");
  void copyText(text, "Copied");
}
</script>

<template>
  <Teleport to="body">
    <Transition name="fade">
      <div v-if="commandResult" class="overlay" @mousedown.self="close">
        <div v-focus class="sheet" role="dialog" :aria-label="commandResult.title" tabindex="-1" @keydown="onKey">
          <header>
            <h2 class="truncate">{{ commandResult.title }}</h2>
            <span v-if="commandResult.busy" class="spinner" aria-label="Working" />
            <button v-if="commandResult.data !== undefined" class="tool" :class="{ on: raw }" title="Show the engine's raw reply" @click="raw = !raw">Raw</button>
            <button v-if="!commandResult.busy || commandResult.text" class="close" title="Copy" aria-label="Copy" @click="copy">
              <Icon name="copy" :size="13" />
            </button>
            <button class="close" title="Close (Esc)" aria-label="Close" @click="close">
              <Icon name="cross" :size="13" />
            </button>
          </header>
          <p v-if="commandResult.error" class="error">{{ commandResult.error }}</p>
          <pre v-else-if="raw" class="text">{{ JSON.stringify(commandResult.data, null, 2) }}</pre>
          <template v-else>
            <p v-if="commandResult.busy && !commandResult.text" class="faint">Working…</p>
            <pre v-if="commandResult.text" class="text">{{ commandResult.text }}</pre>
            <ul v-if="commandResult.rows?.length" class="rows">
              <li v-for="(row, i) in commandResult.rows" :key="i">
                <div class="row-text">
                  <span class="row-title truncate">{{ row.title }}</span>
                  <span v-if="row.detail" class="row-detail">{{ row.detail }}</span>
                </div>
                <button
                  v-for="a in row.actions ?? []"
                  :key="a.label"
                  class="btn"
                  :class="a.danger ? 'btn-danger' : 'btn-secondary'"
                  @click="a.run()"
                >
                  {{ a.label }}
                </button>
              </li>
            </ul>
          </template>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  z-index: 70;
  display: grid;
  place-items: center;
  padding: 24px;
  background: var(--scrim);
  -webkit-backdrop-filter: blur(2px);
  backdrop-filter: blur(2px);
}
.sheet {
  display: flex;
  flex-direction: column;
  width: min(760px, 100%);
  max-height: 80vh;
  padding: 16px 18px 18px;
  border-radius: var(--radius-lg);
  background: var(--raised);
  box-shadow: var(--shadow-pop);
  outline: none;
  animation: rise 160ms var(--ease) both;
}
header {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-bottom: 10px;
}
h2 {
  flex: 1;
  min-width: 0;
  margin: 0;
  font-size: 15px;
  font-weight: 600;
}
.close {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  border-radius: var(--radius-sm);
  color: var(--text-faint);
}
.close:hover,
.tool:hover {
  background: var(--hover);
  color: var(--text);
}
.tool {
  padding: 3px 8px;
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  color: var(--text-faint);
}
.tool.on {
  background: var(--active);
  color: var(--text);
}
.spinner {
  width: 13px;
  height: 13px;
  margin-right: 6px;
  border: 2px solid var(--border-strong);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}
@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
.text {
  flex: 1 1 auto;
  min-height: 0;
  margin: 0;
  padding: 10px 12px;
  overflow: auto;
  border-radius: var(--radius-sm);
  background: var(--bg);
  border: 1px solid var(--border);
  font-family: var(--font-mono);
  font-size: 12px;
  line-height: 1.55;
  white-space: pre-wrap;
  word-break: break-word;
  user-select: text;
  -webkit-user-select: text;
}
.error {
  margin: 0;
  padding: 10px 12px;
  border-radius: var(--radius-sm);
  background: color-mix(in srgb, var(--danger) 10%, transparent);
  color: var(--danger);
  font-size: 13px;
  user-select: text;
  -webkit-user-select: text;
}
.faint {
  margin: 0;
  color: var(--text-faint);
  font-size: 13px;
}
.rows {
  flex: 1 1 auto;
  min-height: 0;
  margin: 8px 0 0;
  padding: 0;
  overflow: auto;
  list-style: none;
}
.rows li {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 4px;
  border-top: 1px solid var(--border);
}
.rows li:first-child {
  border-top: none;
}
.row-text {
  display: flex;
  flex: 1;
  min-width: 0;
  flex-direction: column;
  gap: 2px;
}
.row-title {
  font-size: 13px;
  color: var(--text);
}
.row-detail {
  font-size: var(--text-xs);
  color: var(--text-faint);
  overflow: hidden;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
}
.fade-enter-active,
.fade-leave-active {
  transition: opacity 120ms var(--ease);
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
