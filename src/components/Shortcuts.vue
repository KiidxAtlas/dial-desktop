<script setup lang="ts">
// The keyboard shortcuts sheet (⌘/).
import { ui } from "../ui";
import { SHORTCUTS } from "../commands";
import { shortcutLabel } from "../keysGuard";
import Icon from "./Icon.vue";

const mac = navigator.platform.toLowerCase().includes("mac");

function onKey(event: KeyboardEvent) {
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    ui.shortcutsOpen = false;
  }
}
</script>

<template>
  <Transition name="fade">
    <div v-if="ui.shortcutsOpen" class="overlay" @mousedown.self="ui.shortcutsOpen = false">
      <div
        v-focus
        class="sheet"
        role="dialog"
        aria-label="Keyboard shortcuts"
        tabindex="-1"
        @keydown="onKey"
      >
        <header>
          <h2>Keyboard shortcuts</h2>
          <button class="close" title="Close (Esc)" aria-label="Close" @click="ui.shortcutsOpen = false">
            <Icon name="cross" :size="13" />
          </button>
        </header>
        <div class="columns">
          <section v-for="group in SHORTCUTS" :key="group.group">
            <h3>{{ group.group }}</h3>
            <div v-for="[keys, what] in group.keys" :key="what" class="row">
              <span>{{ what }}</span>
              <kbd>{{ shortcutLabel(keys, mac) }}</kbd>
            </div>
          </section>
        </div>
      </div>
    </div>
  </Transition>
</template>

<script lang="ts">
/** Focus the sheet when it opens, so Esc reaches it. */
export const vFocus = { mounted: (el: HTMLElement) => el.focus() };
</script>

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
  margin-bottom: 8px;
}
h2 {
  flex: 1;
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
