<script setup lang="ts">
// The keyboard shortcuts sheet (⌘/).
import { computed, ref, watch } from "vue";
import { ui } from "../ui";
import { SHORTCUTS } from "../commands";
import { shortcutLabel } from "../keysGuard";
import Icon from "./Icon.vue";

const mac = navigator.platform.toLowerCase().includes("mac");

/** Typing filters the list by what a shortcut does, its fine print or its keys. */
const query = ref("");
watch(() => ui.shortcutsOpen, () => (query.value = ""));
const groups = computed(() => {
  const words = query.value.trim().toLowerCase().split(/\s+/).filter(Boolean);
  return SHORTCUTS.map((g) => ({
    group: g.group,
    keys: g.keys.filter(([keys, what, detail]) => {
      const text = `${what} ${detail ?? ""} ${keys} ${shortcutLabel(keys, mac)}`.toLowerCase();
      return words.every((w) => text.includes(w));
    }),
  })).filter((g) => g.keys.length);
});

function onKey(event: KeyboardEvent) {
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    // The first Esc clears a filter; the next closes the sheet.
    if (query.value) query.value = "";
    else ui.shortcutsOpen = false;
  }
}
</script>

<template>
  <Transition name="fade">
    <div v-if="ui.shortcutsOpen" class="overlay" @mousedown.self="ui.shortcutsOpen = false">
      <div
        class="sheet"
        role="dialog"
        aria-label="Keyboard shortcuts"
        tabindex="-1"
        @keydown="onKey"
      >
        <header>
          <h2>Keyboard shortcuts</h2>
          <input
            v-model="query"
            v-focus
            class="filter"
            type="search"
            placeholder="Filter"
            aria-label="Filter shortcuts"
            spellcheck="false"
          />
          <button class="close" title="Close (Esc)" aria-label="Close" @click="ui.shortcutsOpen = false">
            <Icon name="cross" :size="13" />
          </button>
        </header>
        <div class="columns">
          <section v-for="group in groups" :key="group.group">
            <h3>{{ group.group }}</h3>
            <div v-for="[keys, what, detail] in group.keys" :key="what" class="row" :title="detail">
              <span>{{ what }}<span v-if="detail" class="more" aria-hidden="true"> ⓘ</span><span v-if="detail" class="sr">. {{ detail }}</span></span>
              <kbd>{{ shortcutLabel(keys, mac) }}</kbd>
            </div>
          </section>
        </div>
        <p v-if="!groups.length" class="none">No shortcut matches “{{ query.trim() }}”.</p>
      </div>
    </div>
  </Transition>
</template>

<script lang="ts">
/** Focus the filter when the sheet opens, so typing filters and Esc reaches the sheet. */
export const vFocus = { mounted: (el: HTMLElement) => el.focus() };
</script>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  z-index: 70;
  display: grid;
  /* Pinned near the top, so filtering shrinks the sheet without moving it. */
  justify-items: center;
  align-items: start;
  padding: 10vh 24px 24px;
  background: var(--scrim);
  -webkit-backdrop-filter: blur(2px);
  backdrop-filter: blur(2px);
}
.sheet {
  width: min(720px, 100%);
  max-height: 80vh;
  overflow-y: auto;
  padding: 18px 22px 22px;
  border-radius: var(--radius-lg);
  background: var(--raised);
  box-shadow: var(--shadow-pop);
  outline: none;
  animation: rise 160ms var(--ease) both;
}
header {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 8px;
}
h2 {
  flex: 1;
  margin: 0;
  font-size: 15px;
  font-weight: 600;
}
.filter {
  width: 180px;
  height: 28px;
  padding: 0 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  outline: none;
  background: var(--bg);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}
.filter:focus { border-color: var(--border-strong); }
.more { color: var(--text-faint); font-size: 11px; }
.sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
.none { margin: 18px 0 4px; color: var(--text-faint); font-size: 13px; }
.close {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  border-radius: var(--radius-sm);
  color: var(--text-faint);
}
.close:hover {
  background: var(--hover);
  color: var(--text);
}
.columns {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(230px, 1fr));
  gap: 8px 28px;
}
h3 {
  margin: 14px 0 6px;
  font-size: var(--text-xs);
  font-weight: 500;
  color: var(--text-faint);
}
.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  min-height: 30px;
  padding: 5px 0;
  line-height: 1.5;
  font-size: 13px;
  color: var(--text-muted);
}
kbd {
  flex: none;
  color: var(--text);
  white-space: pre;
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
