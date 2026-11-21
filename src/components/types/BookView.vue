<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { itemTitle, type Full, type Item } from "../../savedTypes";
import BookPage from "./BookPage.vue";
import Icon from "../Icon.vue";
import { pageArrow } from "./pageKeys";
import { bookOf, bookStyle, folioOf, type Page } from "./bookPages";

/**
 * A book: a cover, the contents, then each item over as many pages as it needs, laid out for the
 * book's style (a cookbook, a journal, a guide). Pages turn, never scroll: turning flips a real
 * leaf, its front the page being turned and its back the one it lands on, while the book
 * underneath already shows what lies beyond.
 */
const props = defineProps<{ full: Full; texture: string }>();
const emit = defineEmits<{ edit: [item: Item] }>();

const FLIP_MS = 760;
const style = computed(() => bookStyle(props.full.def));
const book = computed(() => bookOf(props.full.def, props.full.items, style.value));
/** The spread open: 0 is the closed cover. */
const at = ref(0);
const flipping = ref<{ from: number; to: number; dir: "fwd" | "back" } | null>(null);
let timer = 0;

const last = computed(() => book.value.spreads.length - 1);
watch(last, (n) => {
  if (at.value > n) at.value = n;
});

const BLANK: Page = { kind: "blank" };
function spread(i: number): { left: Page; right: Page } {
  return book.value.spreads[i] ?? { left: BLANK, right: BLANK };
}

const base = computed(() => {
  const f = flipping.value;
  if (!f) return spread(at.value);
  return f.dir === "fwd"
    ? { left: spread(f.from).left, right: spread(f.to).right }
    : { left: spread(f.to).left, right: spread(f.from).right };
});
const leaf = computed(() => {
  const f = flipping.value;
  if (!f) return null;
  return f.dir === "fwd"
    ? { front: spread(f.from).right, back: spread(f.to).left }
    : { front: spread(f.from).left, back: spread(f.to).right };
});
const closed = computed(() => (flipping.value ? flipping.value.to === 0 : at.value === 0));

function go(to: number) {
  to = Math.max(0, Math.min(last.value, to));
  if (flipping.value || to === at.value) return;
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
    at.value = to;
    return;
  }
  flipping.value = { from: at.value, to, dir: to > at.value ? "fwd" : "back" };
  window.clearTimeout(timer);
  timer = window.setTimeout(() => {
    at.value = to;
    flipping.value = null;
  }, FLIP_MS);
}

function onKey(event: KeyboardEvent) {
  const step = pageArrow(event);
  if (!step) return;
  event.preventDefault();
  go(at.value + step);
}
onMounted(() => window.addEventListener("keydown", onKey));
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKey);
  window.clearTimeout(timer);
});

/** The item open, by index, for the page turner's label. */
const open = computed(() => {
  const starts = book.value.starts;
  let found = -1;
  starts.forEach((s, i) => {
    if (s <= at.value) found = i;
  });
  return found;
});
const label = computed(() => {
  if (at.value === 0) return "Cover";
  if (open.value < 0) return style.value === "cookbook" ? "Recipes" : "Contents";
  const item = props.full.items[open.value];
  const page = folioOf(book.value, open.value) + (at.value - book.value.starts[open.value]) * 2;
  return `${item ? itemTitle(props.full.def, item) : ""} · p. ${page}–${page + 1}`;
});

/** A book has its own paper and ink unless the type chose them: cream for a cookbook, ivory for a journal. */
const PAPER = { cookbook: ["#fbf5e9", "#2f251c"], journal: ["#f6f0e3", "#2a2830"], guide: ["#fdfdfb", "#1e2228"] } as const;
const paper = computed(() => {
  const t = props.full.def.theme;
  const [p, i] = PAPER[style.value];
  return { "--ty-paper": t.paper || p, "--ty-ink": t.ink || i };
});
</script>

<template>
  <div class="stage">
    <div class="book" :class="[{ closed }, `st-${style}`]" :style="{ '--flip': `${FLIP_MS}ms`, ...paper }">
      <div class="half left" :class="{ empty: base.left.kind === 'blank' }" @click="go(at - 1)">
        <BookPage :class="texture" :full="full" :book="book" :book-style="style" :page="base.left" side="left" @go="go" @edit="emit('edit', $event)" />
      </div>
      <div class="half right" :class="{ empty: base.right.kind === 'blank' }" @click="go(at + 1)">
        <BookPage :class="texture" :full="full" :book="book" :book-style="style" :page="base.right" side="right" @go="go" @edit="emit('edit', $event)" />
      </div>
      <div v-if="leaf && flipping" class="leaf" :class="flipping.dir">
        <div class="face front"><BookPage :class="texture" :full="full" :book="book" :book-style="style" :page="leaf.front" :side="flipping.dir === 'fwd' ? 'right' : 'left'" /></div>
        <div class="face back"><BookPage :class="texture" :full="full" :book="book" :book-style="style" :page="leaf.back" :side="flipping.dir === 'fwd' ? 'left' : 'right'" /></div>
      </div>
    </div>

    <nav class="controls">
      <button class="ty-ghost" title="Back to the cover" :disabled="at <= 0" @click="go(0)"><Icon name="undo" :size="14" /></button>
      <button class="ty-ghost" title="Previous page (←)" :disabled="at <= 0" @click="go(at - 1)"><Icon name="arrowLeft" :size="15" /></button>
      <button class="where truncate" title="Contents" @click="go(book.contents)">{{ label }}</button>
      <button class="ty-ghost" title="Next page (→)" :disabled="at >= last" @click="go(at + 1)"><Icon name="arrowRight" :size="15" /></button>
      <span class="hint">← → to turn pages</span>
    </nav>
  </div>
</template>

<style scoped>
.stage {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 18px;
  padding: 26px 36px 18px;
  /* The book is sized from the room the stage really has, whatever is above or below it. */
  container-type: size;
}
.book {
  position: relative;
  display: grid;
  grid-template-columns: 1fr 1fr;
  /* Room for the page turner (about 34px) and the gap under the book. */
  width: min(1120px, 100cqw, calc((100cqh - 56px) * 1.42));
  aspect-ratio: 1.42;
  perspective: 4200px;
  transition: transform var(--flip) cubic-bezier(0.45, 0.05, 0.25, 1);
}
.book {
  --ty-muted: color-mix(in srgb, var(--ty-ink) 62%, var(--ty-paper));
  --ty-faint: color-mix(in srgb, var(--ty-ink) 40%, var(--ty-paper));
  --ty-line: color-mix(in srgb, var(--ty-ink) 13%, transparent);
  --ty-soft: color-mix(in srgb, var(--ty-accent) 14%, var(--ty-paper));
}
.book.closed {
  transform: translateX(-25%);
}
.half {
  position: relative;
  cursor: pointer;
}
.book:not(.closed) .half {
  filter: drop-shadow(0 18px 28px rgb(0 0 0 / 28%));
}
.book.closed .half.right {
  filter: drop-shadow(0 20px 34px rgb(0 0 0 / 35%));
}
/* The block of pages under the turning one. */
.book:not(.closed) .half.left::before,
.book:not(.closed) .half.right::before {
  content: "";
  position: absolute;
  inset: 3px -3px -4px;
  border-radius: inherit;
  background: color-mix(in srgb, var(--ty-paper) 80%, black);
}
.half.left::before {
  right: 0 !important;
  left: -4px !important;
  border-radius: var(--ty-radius) 0 0 var(--ty-radius);
}
.half.right::before {
  left: 0 !important;
  border-radius: 0 var(--ty-radius) var(--ty-radius) 0;
}
/* Nothing lies under the cover while it opens or closes: no page block, no shadow. */
.book .half.empty {
  filter: none !important;
  cursor: default;
}
.book .half.empty::before {
  display: none !important;
}
.leaf {
  position: absolute;
  top: 0;
  bottom: 0;
  width: 50%;
  z-index: 5;
  transform-style: preserve-3d;
  pointer-events: none;
}
.leaf.fwd {
  left: 50%;
  transform-origin: left center;
  animation: turn-fwd var(--flip) cubic-bezier(0.45, 0.05, 0.25, 1) forwards;
}
.leaf.back {
  left: 0;
  transform-origin: right center;
  animation: turn-back var(--flip) cubic-bezier(0.45, 0.05, 0.25, 1) forwards;
}
.face {
  position: absolute;
  inset: 0;
  backface-visibility: hidden;
  -webkit-backface-visibility: hidden;
}
.face.back {
  transform: rotateY(180deg);
}
/* Light falls across the turning page. */
.face::after {
  content: "";
  position: absolute;
  inset: 0;
  border-radius: inherit;
  pointer-events: none;
  animation: shade var(--flip) ease-in-out forwards;
}
.face.front::after {
  background: linear-gradient(to left, rgb(0 0 0 / 30%), transparent 70%);
}
.face.back::after {
  background: linear-gradient(to right, rgb(0 0 0 / 30%), transparent 70%);
}
@keyframes turn-fwd {
  0% { transform: rotateY(0deg); }
  100% { transform: rotateY(-180deg); }
}
@keyframes turn-back {
  0% { transform: rotateY(0deg); }
  100% { transform: rotateY(180deg); }
}
@keyframes shade {
  0%, 100% { opacity: 0; }
  50% { opacity: 1; }
}
.controls {
  display: flex;
  align-items: center;
  gap: 6px;
  font-family: var(--font-ui);
}
.controls .ty-ghost {
  color: var(--text-muted);
}
.controls .ty-ghost:disabled {
  opacity: 0.35;
}
.where {
  min-width: 96px;
  max-width: 340px;
  height: 28px;
  padding: 0 12px;
  border-radius: 999px;
  background: var(--hover);
  color: var(--text-muted);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}
.hint {
  margin-left: 10px;
  color: var(--text-faint);
  font-size: 11px;
}
</style>
