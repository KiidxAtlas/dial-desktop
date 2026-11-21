<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { focusComposer } from "../../ui";
import { typeApi, types, type Full, type Item } from "../../savedTypes";
import type { OpenSnapshot } from "../../types";
import BookView from "./BookView.vue";
import ChecklistView from "./ChecklistView.vue";
import BoardView from "./BoardView.vue";
import WheelView from "./WheelView.vue";
import CardsView from "./CardsView.vue";
import CustomView from "./CustomView.vue";
import CalendarView from "./CalendarView.vue";
import ItemEditor from "./ItemEditor.vue";
import Icon from "../Icon.vue";
import "./types.css";

/**
 * One type, in the middle of the window where the transcript would be: its theme on the canvas
 * and the layout its blueprint names. The composer below talks to the type's own conversation.
 * It reloads whenever the type changes (here, or by the agent).
 */
const props = defineProps<{ id: string; snap: OpenSnapshot }>();

const full = ref<Full | null>(null);
const error = ref("");
const editing = ref<{ item: Item | null } | null>(null);
const board = ref<InstanceType<typeof BoardView> | null>(null);

async function load() {
  try {
    full.value = await typeApi.get(props.id);
    error.value = "";
  } catch (failure) {
    error.value = String(failure);
  }
}
watch(() => [props.id, types.revision[props.id]], load, { immediate: true });

const def = computed(() => full.value?.def ?? null);

const FONTS: Record<string, string> = {
  sans: "var(--font-ui)",
  serif: '"Iowan Old Style", "Palatino Linotype", Palatino, "Book Antiqua", Georgia, serif',
  mono: "var(--font-mono)",
  hand: '"Bradley Hand", "Segoe Print", "Comic Sans MS", "Chalkboard SE", cursive',
  display: '"Avenir Next", Futura, "Trebuchet MS", "Segoe UI", sans-serif',
  rounded: 'ui-rounded, "SF Pro Rounded", "Arial Rounded MT Bold", Nunito, var(--font-ui)',
};

const canvasStyle = computed(() => {
  const t = def.value?.theme;
  if (!t) return {};
  return {
    "--ty-accent": t.accent || "var(--accent)",
    "--ty-paper": t.paper || "var(--surface)",
    "--ty-ink": t.ink || "var(--text)",
    "--ty-font": FONTS[t.font] ?? FONTS.sans,
    "--ty-radius": `${t.radius || 14}px`,
  };
});
const texture = computed(() => (def.value && def.value.theme.texture !== "none" ? `ty-texture-${def.value.theme.texture}` : ""));
const layout = computed(() => def.value?.view.layout ?? "cards");
/** A book lies on a desk; the other layouts are drawn on the type's own page colour. */
const desk = computed(() => layout.value === "book");
const count = computed(() => full.value?.items.length ?? 0);
/** Just named, nothing in it: Dial designs it once told what it is for, or on the first save. */
const fresh = computed(() => !!def.value && !def.value.designed && !count.value);

function add() {
  if (layout.value === "board") return void board.value?.addImages();
  editing.value = { item: null };
}
const edit = (item: Item) => (editing.value = { item });
const addLabel = computed(() => ({ book: "Add page", board: "Add images", wheel: "Add option", checklist: "Add item", calendar: "Add event" } as Record<string, string>)[layout.value] ?? "Add");
/** Layouts drawn under a heading of the type's own (its name, tagline and count, and Add). */
const headed = computed(() => ["cards", "table", "board", "wheel"].includes(layout.value));
const countLabel = computed(() => `${count.value} ${count.value === 1 ? "item" : "items"}`);
</script>

<template>
  <section class="type-view">
    <div v-if="error" class="failed">
      <p>{{ error }}</p>
    </div>
    <div v-else-if="fresh && def" class="fresh">
      <span class="glyph">{{ def.icon }}</span>
      <h2>{{ def.name }}</h2>
      <p>Tell Dial below what {{ def.name }} should be, and it builds it here. Or save an answer to it with the bookmark under any reply, and Dial designs it around what you save.</p>
      <button class="btn btn-secondary" @click="focusComposer()"><Icon name="sparkle" :size="13" /> Describe it to Dial</button>
    </div>
    <div v-else-if="full && def" class="ty-canvas canvas" :class="[desk ? 'desk' : texture, `layout-${layout}`]" :style="canvasStyle">
      <button v-if="layout === 'book'" class="add btn btn-secondary" :title="addLabel" @click="add"><Icon name="plus" :size="13" /> {{ addLabel }}</button>
      <BookView v-if="layout === 'book'" :full="full" :texture="texture" @edit="edit" />
      <CustomView v-else-if="layout === 'custom'" :full="full" :platform="snap.platform" />
      <div v-else class="scroll">
        <header v-if="headed" class="head">
          <div class="titles">
            <h1>{{ def.name }}</h1>
            <p>
              <span v-if="def.view.tagline">{{ def.view.tagline }}</span>
              <span v-if="def.view.tagline" aria-hidden="true"> · </span>
              <span>{{ countLabel }}</span>
            </p>
          </div>
          <button class="head-add" :title="addLabel" @click="add"><Icon name="plus" :size="14" /> {{ addLabel }}</button>
        </header>
        <div v-if="!count && (layout === 'cards' || layout === 'table')" class="ty-empty">
          <span class="glyph">{{ def.icon }}</span>
          <strong>Nothing in {{ def.name }} yet</strong>
          <span>Save an answer here with the bookmark under any reply, or ask Dial below.</span>
        </div>
        <ChecklistView v-else-if="layout === 'checklist'" :full="full" @edit="edit" />
        <CalendarView v-else-if="layout === 'calendar'" :full="full" @edit="edit" @add="add" />
        <BoardView v-else-if="layout === 'board'" ref="board" :full="full" @edit="edit" />
        <WheelView v-else-if="layout === 'wheel'" :full="full" @edit="edit" />
        <CardsView v-else :full="full" :table="layout === 'table'" @edit="edit" />
      </div>
    </div>

    <ItemEditor v-if="editing && full" :full="full" :item="editing.item" @close="editing = null" />
  </section>
</template>

<style scoped>
.type-view {
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.canvas {
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  margin: 0 12px;
  border-radius: 14px;
  overflow: hidden;
  background-color: var(--ty-paper);
}
.canvas.desk {
  background:
    radial-gradient(ellipse at 50% 40%, color-mix(in srgb, var(--ty-accent) 10%, transparent), transparent 70%),
    var(--bg);
}
.add {
  position: absolute;
  top: 12px;
  right: 14px;
  z-index: 3;
  gap: 6px;
  box-shadow: 0 2px 10px rgb(0 0 0 / 0.12);
}
/* A book's own controls sit under it; adding a page joins them, clear of the pages. */
.layout-book .add {
  top: auto;
  bottom: 14px;
}
.scroll {
  flex: 1;
  min-height: 0;
  overflow: auto;
  container-type: inline-size;
}
.head {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 16px;
  padding: 30px 28px 0;
  color: var(--ty-ink);
}
.titles {
  min-width: 0;
}
.head h1 {
  margin: 0;
  overflow: hidden;
  font-family: var(--ty-font);
  font-size: 30px;
  font-weight: 700;
  letter-spacing: -0.015em;
  line-height: 1.15;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.head p {
  margin: 5px 0 0;
  color: var(--ty-muted);
  font: 13.5px var(--font-ui);
}
.head-add {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex: none;
  height: 34px;
  padding: 0 14px 0 12px;
  border-radius: 999px;
  background: var(--ty-accent);
  color: #fff;
  font: 600 13px var(--font-ui);
  box-shadow: 0 6px 16px color-mix(in srgb, var(--ty-accent) 30%, transparent);
  transition: transform 120ms var(--ease), filter 120ms var(--ease);
}
.head-add:hover {
  filter: brightness(1.06);
  transform: translateY(-1px);
}
.head-add:active {
  transform: scale(0.97);
}
.fresh {
  margin: auto;
  max-width: 420px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding: 24px;
  text-align: center;
  color: var(--text-muted);
}
.fresh .glyph {
  font-size: 44px;
  line-height: 1;
}
.fresh h2 {
  margin: 4px 0 0;
  color: var(--text);
  font-size: 22px;
  font-weight: 600;
}
.fresh p {
  margin: 0 0 6px;
  font-size: 13.5px;
  line-height: 1.55;
}
.fresh .btn {
  gap: 6px;
}
.failed {
  display: grid;
  place-items: center;
  gap: 12px;
  margin: auto;
  color: var(--text-muted);
  font-size: 13px;
}
</style>
