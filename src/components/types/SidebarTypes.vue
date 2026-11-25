<script setup lang="ts">
import { nextTick, ref } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import { showToast } from "../../bridge";
import { openMenu, ui } from "../../ui";
import { createType, deleteType, openType, refreshTypes, typeApi, types, type Summary } from "../../savedTypes";
import Icon from "../Icon.vue";

/** The sidebar's Types: each one opens as its own small app in place of the transcript. */
const open = ref(localStorage.getItem("dial.sidebar-types") !== "closed");
function toggle() {
  open.value = !open.value;
  localStorage.setItem("dial.sidebar-types", open.value ? "open" : "closed");
}

/** The name field: a new type (`null` id) or a rename. */
const naming = ref<{ id: string | null; before: string } | null>(null);
const field = ref<HTMLInputElement | null>(null);
const busy = ref(false);

async function startNaming(id: string | null = null, before = "") {
  open.value = true;
  naming.value = { id, before };
  await nextTick();
  field.value?.focus();
  field.value?.select();
}

async function finishNaming() {
  const target = naming.value;
  const name = field.value?.value.trim() ?? "";
  if (!target || busy.value) return;
  if (!name || name === target.before) return cancelNaming();
  busy.value = true;
  try {
    if (target.id) {
      await typeApi.update(target.id, { name });
      await refreshTypes();
    } else {
      await createType(name);
    }
  } catch (error) {
    showToast(String(error));
  } finally {
    busy.value = false;
    naming.value = null;
  }
}
function cancelNaming() {
  naming.value = null;
}
function onKey(event: KeyboardEvent) {
  if (event.key === "Enter" && !event.isComposing) {
    event.preventDefault();
    void finishNaming();
  } else if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    cancelNaming();
  }
}

async function remove(t: Summary) {
  const sure = await ask(`“${t.name}” and everything saved in it will be removed. This cannot be undone.`, {
    title: "Delete type?", kind: "warning", okLabel: "Delete", cancelLabel: "Cancel",
  }).catch(() => false);
  if (!sure) return;
  try {
    await deleteType(t.id);
  } catch (error) {
    showToast(String(error));
  }
}

function menu(event: MouseEvent, t: Summary) {
  openMenu(event, [
    { label: "Open", icon: "book", run: () => void openType(t.id) },
    { label: "Rename", icon: "pencil", run: () => void startNaming(t.id, t.name) },
    { divider: true },
    { label: "Delete", icon: "trash", danger: true, run: () => void remove(t) },
  ]);
}
</script>

<template>
  <section class="types" aria-label="Types">
    <div class="head">
      <button class="fold" :aria-expanded="open" @click="toggle">
        Types
        <Icon :name="open ? 'chevronDown' : 'chevronRight'" :size="11" />
      </button>
      <button class="add" title="New type" aria-label="New type" @click="startNaming()">
        <Icon name="plus" :size="13" />
      </button>
    </div>
    <div v-if="open" class="rows">
      <button v-if="types.listError" class="row" @click="refreshTypes">Couldn’t load Types · Retry</button>
      <div v-if="naming && !naming.id" class="row naming">
        <span class="glyph">✦</span>
        <input ref="field" class="name-field" placeholder="Name it — Cookbook, Routine…" aria-label="Type name" maxlength="60" :disabled="busy" @keydown="onKey" @blur="finishNaming" />
      </div>
      <button v-else-if="types.loaded && !types.list.length" class="row hint" title="Keep things in a place of their own: a cookbook, a routine, a reading list…" @click="startNaming()">
        <span class="glyph">✦</span>
        <span class="truncate">New type…</span>
      </button>
      <template v-for="t in types.list" :key="t.id">
        <div v-if="naming?.id === t.id" class="row naming">
          <span class="glyph">{{ t.icon }}</span>
          <input ref="field" class="name-field" :value="t.name" aria-label="Type name" maxlength="60" :disabled="busy" @keydown="onKey" @blur="finishNaming" />
        </div>
        <button
          v-else
          class="row"
          :class="{ on: ui.typeOpen === t.id && !ui.settingsOpen }"
          :title="t.description || t.name"
          :style="{ '--type-accent': t.accent || 'var(--accent)' }"
          @click="openType(t.id)"
          @contextmenu.prevent="menu($event, t)"
        >
          <span class="glyph">{{ t.icon }}</span>
          <span class="truncate name">{{ t.name }}</span>
          <span class="count">{{ t.count }}</span>
        </button>
      </template>
    </div>
  </section>
</template>

<style scoped>
.types {
  flex: none;
  margin: -6px 0 12px;
}
.head {
  display: flex;
  align-items: center;
  height: 24px;
  /* The heading text lines up with the Pinned and Recents labels (10px in); the fold chevron
     follows the word instead of pushing it right. */
  padding: 0 4px 0 10px;
}
.fold {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 4px;
  height: 22px;
  color: var(--text-faint);
  font-size: var(--text-xs);
  text-align: left;
}
.fold:hover,
.add:hover {
  color: var(--text);
}
.add {
  display: grid;
  place-items: center;
  width: 22px;
  height: 22px;
  border-radius: 6px;
  color: var(--text-faint);
}
.add:hover {
  background: var(--hover);
}
.rows {
  display: flex;
  flex-direction: column;
  gap: 1px;
  max-height: 30vh;
  overflow-y: auto;
}
.row {
  display: flex;
  align-items: center;
  gap: 9px;
  height: 30px;
  flex: none;
  padding: 0 10px;
  border-radius: var(--radius-sm);
  color: var(--text-muted);
  text-align: left;
  transition: background 120ms var(--ease), color 120ms var(--ease);
}
.row:hover {
  background: var(--hover);
  color: var(--text);
}
.row.on {
  background: color-mix(in srgb, var(--type-accent) 16%, var(--active));
  color: var(--text);
}
.glyph {
  width: 18px;
  flex: none;
  font-size: 14px;
  text-align: center;
}
.name {
  flex: 1;
}
.count {
  color: var(--text-faint);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
}
.hint {
  color: var(--text-faint);
  font-size: var(--text-sm);
}
.naming {
  background: var(--hover);
}
.name-field {
  flex: 1;
  min-width: 0;
  height: 24px;
  padding: 0;
  border: 0;
  outline: none;
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: var(--text-sm);
}
</style>
