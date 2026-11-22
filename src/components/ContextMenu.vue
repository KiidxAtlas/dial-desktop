<script setup lang="ts">
// The one context menu: session actions from a right-click or a "…" button. Closes on a choice,
// a click elsewhere, Esc, scrolling or the window losing focus. Arrow keys move through it.
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import { ui, type MenuItem } from "../ui";
import Icon, { PATHS } from "./Icon.vue";

const root = ref<HTMLElement | null>(null);
const position = ref({ left: 0, top: 0 });

type Action = Exclude<MenuItem, { divider: true }>;
const isAction = (item: MenuItem): item is Action => !("divider" in item);
const items = computed(() => ui.menu?.items ?? []);
const icon = (name?: string) => (name && name in PATHS ? (name as keyof typeof PATHS) : null);
const prompt = ref<NonNullable<Action['prompt']> | null>(null);
const value = ref('');

function cancel() {
  const anchor = ui.menu?.anchor;
  close();
  anchor?.focus({ preventScroll: true });
}

function submit() {
  const field = prompt.value;
  const name = value.value.trim();
  if (!ui.menu || !field || !name) return;
  cancel();
  field.submit(name);
}

function close() {
  ui.menu = null;
}

function choose(item: Action) {
  if (item.disabled) return;
  if (item.prompt) {
    prompt.value = item.prompt;
    void place();
    return;
  }
  close();
  item.run();
}

/** Keep the menu inside the window. */
async function place() {
  await nextTick();
  const el = root.value;
  const menu = ui.menu;
  if (!el || !menu) return;
  const { width, height } = el.getBoundingClientRect();
  position.value = {
    left: Math.max(8, Math.min(menu.x, window.innerWidth - width - 8)),
    top: menu.y + height > window.innerHeight - 8 ? Math.max(8, menu.y - height) : menu.y,
  };
  el.querySelector<HTMLElement>(prompt.value ? 'input' : 'button:not(:disabled)')?.focus({ preventScroll: true });
}

function onKey(event: KeyboardEvent) {
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    if (!event.isComposing) cancel();
  } else if (!prompt.value && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
    event.preventDefault();
    const buttons = [...(root.value?.querySelectorAll<HTMLElement>("button:not(:disabled)") ?? [])];
    const at = buttons.indexOf(document.activeElement as HTMLElement);
    const next = event.key === "ArrowDown" ? at + 1 : at - 1;
    buttons[(next + buttons.length) % buttons.length]?.focus();
  }
}

function onOutside(event: MouseEvent) {
  if (root.value && !root.value.contains(event.target as Node)) close();
}

function listen(on: boolean) {
  if (on) {
    document.addEventListener("mousedown", onOutside, true);
    document.addEventListener("keydown", onKey, true);
    document.addEventListener("scroll", close, true);
    window.addEventListener("blur", close);
    window.addEventListener("resize", close);
  } else {
    document.removeEventListener("mousedown", onOutside, true);
    document.removeEventListener("keydown", onKey, true);
    document.removeEventListener("scroll", close, true);
    window.removeEventListener("blur", close);
    window.removeEventListener("resize", close);
  }
}

watch(
  () => ui.menu,
  (menu, before) => {
    prompt.value = null;
    value.value = '';
    if (menu && !before) listen(true);
    if (!menu && before) listen(false);
    if (menu) place();
  },
);
onBeforeUnmount(() => listen(false));
</script>

<template>
  <Transition name="pop">
    <div
      v-if="ui.menu"
      ref="root"
      class="context-menu"
      :role="prompt ? 'dialog' : 'menu'"
      :aria-label="ui.menu.title"
      :style="{ left: `${position.left}px`, top: `${position.top}px` }"
      @contextmenu.prevent
    >
      <div v-if="ui.menu.title" class="menu-title">{{ ui.menu.title }}</div>
      <form v-if="prompt" class="menu-form" @submit.prevent="submit">
        <label>{{ prompt.label }}<input v-model="value" :maxlength="prompt.maxLength" :placeholder="prompt.placeholder" required /></label>
        <div class="form-actions">
          <button class="btn btn-secondary" type="button" @click="cancel">Cancel</button>
          <button class="btn btn-primary" type="submit" :disabled="!value.trim()">{{ prompt.confirm }}</button>
        </div>
      </form>
      <template v-for="(item, index) in prompt ? [] : items" :key="index">
        <div v-if="!isAction(item)" class="rule" role="separator" />
        <button
          v-else
          role="menuitem"
          :class="{ danger: item.danger }"
          :disabled="item.disabled"
          @click="choose(item)"
        >
          <Icon v-if="icon(item.icon)" :name="icon(item.icon)!" :size="14" />
          <span class="label">{{ item.label }}</span>
          <kbd v-if="item.hint">{{ item.hint }}</kbd>
        </button>
      </template>
    </div>
  </Transition>
</template>

<style scoped>
.context-menu {
  position: fixed;
  /* Above every panel and the image viewer (75), below toasts (80). */
  z-index: 78;
  min-width: 200px;
  padding: 4px;
  border-radius: var(--radius-md);
  background: var(--raised);
  box-shadow: var(--shadow-pop);
}
.menu-title { padding: 7px 10px; font-size: var(--text-xs); color: var(--text-muted); font-weight: 600; }
.menu-form { display: grid; gap: 12px; padding: 4px 8px 8px; }
.menu-form label { display: grid; gap: 4px; font-size: var(--text-sm); color: var(--text-muted); }
.menu-form input { padding: 6px 8px; border: 1px solid var(--border-strong); border-radius: var(--radius-sm); background: var(--surface); color: var(--text); }
.form-actions { display: flex; justify-content: flex-end; gap: 6px; }
button[role="menuitem"] {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  height: 30px;
  padding: 0 10px;
  border-radius: var(--radius-sm);
  font-size: 13px;
  color: var(--text);
  text-align: left;
}
button svg {
  color: var(--text-faint);
}
button[role="menuitem"]:hover:not(:disabled),
button[role="menuitem"]:focus-visible {
  background: var(--hover);
  outline: none;
}
button.danger,
button.danger svg {
  color: var(--danger);
}
button:disabled {
  opacity: 0.4;
}
.label {
  flex: 1;
}
kbd {
  font-family: var(--font-ui);
  font-size: var(--text-xs);
  color: var(--text-faint);
}
.rule {
  height: 1px;
  margin: 4px 6px;
  background: var(--border);
}
</style>
