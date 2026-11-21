<script setup lang="ts">
import { computed } from "vue";
import { assetPath, fieldText, itemTitle, type Full, type Item } from "../../savedTypes";
import { dateOf } from "../../typeExtract";
import FieldValue from "./FieldValue.vue";
import TypeImage from "./TypeImage.vue";
import BookArt from "./BookArt.vue";
import Icon from "../Icon.vue";
import { folioOf, metaFields, type Block, type Book, type BookStyle, type Page } from "./bookPages";

/** One page of a book, drawn in the book's style: a cookbook, a journal or a guide. */
const props = defineProps<{ full: Full; book: Book; page: Page; side: "left" | "right"; bookStyle: BookStyle }>();
const emit = defineEmits<{ go: [spread: number]; edit: [item: Item] }>();

const def = computed(() => props.full.def);
const item = computed(() => ("item" in props.page ? props.page.item : null));
const number = computed(() => ("number" in props.page ? props.page.number : 0));
const title = computed(() => (item.value ? itemTitle(def.value, item.value) : ""));
const subtitle = computed(() => {
  const key = def.value.view.subtitleField;
  return key && item.value ? fieldText(item.value.fields[key]) : "";
});
const image = computed(() => {
  const key = def.value.view.imageField;
  if (!key || !item.value) return null;
  const value = item.value.fields[key];
  return assetPath(props.full, Array.isArray(value) ? value[0] : value);
});
const meta = computed(() => (item.value ? metaFields(def.value, item.value, props.bookStyle) : []));
const tags = computed(() => def.value.fields.filter((f) => f.kind === "tags" && item.value && fieldText(item.value.fields[f.key])));
/** The words a drawing is chosen from: what the item is made of. */
const words = computed(() => (item.value ? Object.values(item.value.fields).map(fieldText).join(" ").slice(0, 600) : ""));
const count = computed(() => props.full.items.length);
const noun = computed(() => ({ cookbook: ["recipe", "recipes"], journal: ["entry", "entries"], guide: ["chapter", "chapters"] })[props.bookStyle]);
const two = (n: number) => String(n).padStart(2, "0");

/** A journal entry's day: its date field, else the day it was written. */
const day = computed(() => {
  if (!item.value) return null;
  const key = def.value.view.timeField ?? def.value.fields.find((f) => f.kind === "date" || f.kind === "datetime")?.key;
  const at = (key && dateOf(item.value.fields[key])?.at) || new Date(item.value.created);
  return {
    day: at.getDate(),
    month: at.toLocaleDateString(undefined, { month: "long" }),
    weekday: at.toLocaleDateString(undefined, { weekday: "long" }),
    year: at.getFullYear(),
    long: at.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long", year: "numeric" }),
  };
});

/** "200 g pork mince" → amount "200 g", what "pork mince". */
const AMOUNT = /^((?:about |~)?[\d½¼¾⅓⅔⅛.,/\s–-]+(?:\s*(?:g|kg|mg|ml|l|cl|dl|tsp|tbsp|tablespoons?|teaspoons?|cups?|oz|lbs?|pounds?|pinch(?:es)?|cloves?|cans?|sticks?|slices?|bunch(?:es)?|handfuls?|sheets?|pcs?|pieces?)\.?)?)\s+(.+)$/i;
function ingredient(text: string): { amount: string; what: string } {
  const m = AMOUNT.exec(text.trim());
  return m && /\d|[½¼¾⅓⅔⅛]/.test(m[1]) ? { amount: m[1].trim(), what: m[2] } : { amount: "", what: text };
}
/** Short ingredients sit in two columns. */
function columns(block: Extract<Block, { kind: "items" }>): boolean {
  return !block.ordered && block.items.length >= 6 && block.items.every((s) => s.length <= 26);
}
const blocks = computed(() => ("blocks" in props.page ? props.page.blocks : []));
const contentsItems = computed(() =>
  props.page.kind === "contents" ? props.full.items.slice(props.page.from, props.page.to).map((entry, i) => ({ entry, index: props.page.kind === "contents" ? props.page.from + i : i })) : [],
);
function entryMeta(entry: Item): string {
  if (props.bookStyle === "journal") {
    const key = def.value.view.timeField ?? def.value.fields.find((f) => f.kind === "date" || f.kind === "datetime")?.key;
    const at = (key && dateOf(entry.fields[key])?.at) || new Date(entry.created);
    return at.toLocaleDateString(undefined, { day: "numeric", month: "short" });
  }
  const first = metaFields(def.value, entry, props.bookStyle)[0];
  return first ? `${first.label || first.key} ${fieldText(entry.fields[first.key])}` : "";
}
const running = computed(() => (props.side === "left" ? def.value.name : title.value));
</script>

<template>
  <div class="page" :class="[side, `pg-${page.kind}`, `st-${bookStyle}`]">
    <!-- The cover -->
    <template v-if="page.kind === 'cover'">
      <div v-if="bookStyle === 'cookbook'" class="cover-cook">
        <span class="ribbon" />
        <div class="plate-label">
          <div class="label-art"><BookArt :title="full.items[0] ? itemTitle(def, full.items[0]) : def.name" :words="def.description" kind="cookbook" :seed="def.id" /></div>
          <span class="kicker">{{ def.view.tagline || "Recipes from the kitchen" }}</span>
          <h1>{{ def.name }}</h1>
          <span class="rule"><i /> ❦ <i /></span>
          <span class="count">{{ count }} {{ count === 1 ? noun[0] : noun[1] }}</span>
        </div>
      </div>
      <div v-else-if="bookStyle === 'journal'" class="cover-journal">
        <span class="band" />
        <div class="emboss">
          <span class="year">{{ new Date().getFullYear() }}</span>
          <h1>{{ def.name }}</h1>
          <p v-if="def.view.tagline || def.description">{{ def.view.tagline || def.description }}</p>
        </div>
      </div>
      <div v-else class="cover-guide">
        <div class="grid-art" />
        <div class="guide-title">
          <span class="kicker">{{ count }} {{ count === 1 ? noun[0] : noun[1] }}</span>
          <h1>{{ def.name }}</h1>
          <p v-if="def.view.tagline || def.description">{{ def.view.tagline || def.description }}</p>
        </div>
        <span class="guide-icon">{{ def.icon }}</span>
      </div>
    </template>

    <!-- Inside the cover -->
    <template v-else-if="page.kind === 'inside'">
      <div class="endpaper" :class="bookStyle">
        <div class="bookplate">
          <template v-if="bookStyle === 'cookbook'">
            <span class="hand">from the kitchen of</span>
            <h2>{{ def.name }}</h2>
            <p v-if="def.description">{{ def.description }}</p>
          </template>
          <template v-else-if="bookStyle === 'journal'">
            <span class="hand">if found, please return</span>
            <h2>{{ def.name }}</h2>
            <p v-if="def.description">{{ def.description }}</p>
          </template>
          <template v-else>
            <span class="kicker">About this guide</span>
            <h2>{{ def.name }}</h2>
            <p v-if="def.description">{{ def.description }}</p>
          </template>
        </div>
      </div>
    </template>

    <!-- Contents -->
    <template v-else-if="page.kind === 'contents'">
      <div class="contents">
        <h2>{{ bookStyle === "cookbook" ? "Recipes" : bookStyle === "journal" ? "Entries" : "Contents" }}</h2>
        <ol v-if="count">
          <li v-for="{ entry, index } in contentsItems" :key="entry.id">
            <button @click.stop="emit('go', book.starts[index])">
              <span class="no">{{ bookStyle === "guide" ? two(index + 1) : index + 1 }}</span>
              <span class="entry">
                <span class="truncate">{{ itemTitle(def, entry) }}</span>
                <small v-if="entryMeta(entry)">{{ entryMeta(entry) }}</small>
              </span>
              <span class="leader" />
              <span class="num">{{ folioOf(book, index) }}</span>
            </button>
          </li>
        </ol>
        <p v-else class="none">Nothing here yet. Save an answer from the conversation, or add a {{ noun[0] }}.</p>
      </div>
    </template>

    <!-- An item's picture -->
    <template v-else-if="page.kind === 'art'">
      <div v-if="image" class="photo-page">
        <TypeImage :path="image" :alt="title" />
        <span class="photo-no">{{ bookStyle === "guide" ? `Chapter ${two(number)}` : `No. ${number}` }}</span>
      </div>
      <div v-else-if="bookStyle === 'cookbook'" class="art-cook">
        <span class="big-no">{{ number }}</span>
        <div class="drawing"><BookArt :title="title" :words="words" kind="cookbook" :seed="item!.id" /></div>
        <span class="fig hand">fig. {{ number }} — {{ title }}</span>
      </div>
      <div v-else-if="bookStyle === 'journal'" class="art-journal">
        <div class="date-block">
          <span class="weekday">{{ day?.weekday }}</span>
          <span class="dnum">{{ day?.day }}</span>
          <span class="month">{{ day?.month }} {{ day?.year }}</span>
        </div>
        <div class="drawing"><BookArt :title="title" :words="words" kind="journal" :seed="item!.id" /></div>
      </div>
      <div v-else class="art-guide">
        <span class="chapter">Chapter</span>
        <span class="chapter-no">{{ two(number) }}</span>
        <div class="drawing"><BookArt :title="title" :words="words" kind="guide" :seed="item!.id" /></div>
      </div>
    </template>

    <!-- An item's words: its title page, then the pages it runs on to -->
    <template v-else-if="page.kind === 'head' || page.kind === 'flow'">
      <div class="words">
        <header class="running">
          <span class="truncate">{{ page.kind === "head" ? (bookStyle === "journal" ? day?.long : def.name) : running }}</span>
          <button class="ty-ghost edit" title="Edit this page" @click.stop="emit('edit', item!)"><Icon name="pencil" :size="12" /></button>
        </header>

        <div v-if="page.kind === 'head'" class="title-block">
          <span v-if="bookStyle === 'cookbook'" class="eyebrow">Recipe No. {{ number }}</span>
          <span v-else-if="bookStyle === 'guide'" class="eyebrow">Chapter {{ two(number) }}</span>
          <h2>{{ title }}</h2>
          <p v-if="subtitle" class="headnote">{{ subtitle }}</p>
          <div v-if="meta.length" class="meta">
            <div v-for="f in meta" :key="f.key" class="meta-cell">
              <small>{{ f.label }}</small>
              <FieldValue :field="f" :value="item!.fields[f.key]" />
            </div>
          </div>
          <div v-if="tags.length" class="tag-row">
            <FieldValue v-for="f in tags" :key="f.key" :field="f" :value="item!.fields[f.key]" />
          </div>
        </div>

        <template v-for="(block, i) in blocks" :key="i">
          <h3 v-if="block.kind === 'heading'" class="block-head">
            {{ block.text }}<span v-if="block.more" class="more"> continued</span>
          </h3>
          <ul v-else-if="block.kind === 'items' && !block.ordered" class="ingredients" :class="{ cols: columns(block) }">
            <li v-for="(text, j) in block.items" :key="j">
              <template v-if="ingredient(text).amount"><b>{{ ingredient(text).amount }}</b> {{ ingredient(text).what }}</template>
              <template v-else>{{ text }}</template>
            </li>
          </ul>
          <ol v-else-if="block.kind === 'items'" class="method">
            <li v-for="(text, j) in block.items" :key="j">
              <span class="step-no">{{ block.start + j + 1 }}</span>
              <span class="step">{{ text }}</span>
            </li>
          </ol>
          <aside v-else-if="block.kind === 'note'" class="note">
            <span class="tape" />
            <span class="note-label hand">{{ block.field.label || "Note" }}</span>
            <p>{{ block.text }}</p>
          </aside>
          <div v-else-if="block.kind === 'prose'" class="prose-block" :class="{ dropcap: block.first && page.kind === 'head' }">
            <FieldValue :field="block.field" :value="block.text" />
          </div>
          <div v-else class="value-block">
            <small>{{ block.field.label }}</small>
            <FieldValue :field="block.field" :value="block.value" />
          </div>
        </template>

        <p v-if="page.kind === 'head' && !blocks.length && !meta.length && !subtitle" class="none">Nothing written on this page yet.</p>
      </div>
      <footer v-if="page.kind === 'flow'" class="folio">{{ page.folio }}</footer>
    </template>

    <!-- The rest of an item's last leaf -->
    <template v-else-if="page.kind === 'tail'">
      <div class="tail">
        <span class="hand tail-head">{{ bookStyle === "cookbook" ? "Kitchen notes" : bookStyle === "journal" ? "Later…" : "Notes" }}</span>
        <div class="writing" />
        <span class="fin">{{ bookStyle === "guide" ? "■" : "❦" }}</span>
      </div>
    </template>
  </div>
</template>

<style scoped>
/* Every page sets its type in its own width, so a line holds the same text at any size (see
   bookPages.ts): 1em is 2.5% of the page. */
.page {
  position: absolute;
  inset: 0;
  overflow: hidden;
  container-type: size;
  background-color: var(--ty-paper);
  color: var(--ty-ink);
}
.page > * {
  font-size: 2.5cqw;
}
.page.pg-blank {
  background: none;
}
.left {
  border-radius: var(--ty-radius) 3px 3px var(--ty-radius);
}
.right {
  border-radius: 3px var(--ty-radius) var(--ty-radius) 3px;
}
/* The fold: pages darken toward the spine. */
.page:not(.pg-blank, .pg-cover)::after {
  content: "";
  position: absolute;
  top: 0;
  bottom: 0;
  width: 12%;
  pointer-events: none;
}
.left:not(.pg-blank, .pg-cover)::after {
  right: 0;
  background: linear-gradient(to left, rgb(0 0 0 / 17%), rgb(0 0 0 / 4%) 35%, transparent);
}
.right:not(.pg-blank, .pg-cover)::after {
  left: 0;
  background: linear-gradient(to right, rgb(0 0 0 / 19%), rgb(0 0 0 / 4%) 35%, transparent);
}
.hand {
  font-family: "Bradley Hand", "Segoe Print", "Chalkboard SE", cursive;
}
.kicker {
  font: 700 0.72em var(--font-ui);
  letter-spacing: 0.24em;
  text-transform: uppercase;
}
.truncate {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* ── Covers ─────────────────────────────────────────── */
.pg-cover {
  box-shadow: inset 5px 0 9px rgb(0 0 0 / 28%);
}
.cover-cook {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  /* Cloth: a weave of fine threads over the colour. */
  background:
    repeating-linear-gradient(0deg, rgb(255 255 255 / 4%) 0 1px, transparent 1px 3px),
    repeating-linear-gradient(90deg, rgb(0 0 0 / 5%) 0 1px, transparent 1px 3px),
    radial-gradient(120% 90% at 30% 15%, rgb(255 255 255 / 14%), transparent 60%),
    color-mix(in oklch, var(--ty-accent) 88%, #3a2414);
}
.ribbon {
  position: absolute;
  top: -2%;
  right: 7%;
  width: 4.5%;
  height: 30%;
  background: color-mix(in oklch, var(--ty-accent) 45%, #8c1c13);
  clip-path: polygon(0 0, 100% 0, 100% 100%, 50% 90%, 0 100%);
  box-shadow: 0 2px 4px rgb(0 0 0 / 30%);
}
.plate-label {
  display: flex;
  flex-direction: column;
  align-items: center;
  width: 64%;
  margin-right: 6%;
  padding: 6cqw 6cqw 7cqw;
  border-radius: 3cqw;
  background: #fbf3e2;
  color: #3b2a1c;
  text-align: center;
  box-shadow: inset 0 0 0 0.8cqw #fbf3e2, inset 0 0 0 1.1cqw color-mix(in oklch, var(--ty-accent) 70%, #3b2a1c), 0 1.4cqw 3cqw rgb(0 0 0 / 30%);
  --ty-paper: #fbf3e2;
  --ty-ink: #3b2a1c;
}
.label-art {
  width: 56%;
  aspect-ratio: 1;
  margin-bottom: 2cqw;
}
.plate-label .kicker {
  color: color-mix(in oklch, var(--ty-accent) 80%, #3b2a1c);
  font-size: 2.2cqw;
}
.plate-label h1 {
  margin: 1.6cqw 0 0;
  font-family: var(--ty-font);
  font-size: 9cqw;
  font-weight: 700;
  line-height: 1.02;
}
.rule {
  display: flex;
  align-items: center;
  gap: 2cqw;
  width: 70%;
  margin: 3cqw 0 2cqw;
  color: color-mix(in oklch, var(--ty-accent) 80%, #3b2a1c);
  font-size: 3.4cqw;
}
.rule i {
  flex: 1;
  height: 1px;
  background: currentColor;
  opacity: 0.5;
}
.plate-label .count {
  font: 600 2.2cqw var(--font-ui);
  letter-spacing: 0.2em;
  text-transform: uppercase;
  opacity: 0.7;
}

.cover-journal {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  /* Leather: a grain over a deep colour, the edges worn darker. */
  background:
    radial-gradient(circle at 20% 30%, rgb(255 255 255 / 5%) 0 1px, transparent 1.5px) 0 0 / 7px 7px,
    radial-gradient(130% 100% at 40% 30%, transparent 40%, rgb(0 0 0 / 35%)),
    color-mix(in oklch, var(--ty-accent) 55%, #1d1714);
  color: color-mix(in oklch, var(--ty-accent) 20%, #f2e3c8);
}
.band {
  position: absolute;
  top: 0;
  bottom: 0;
  right: 13%;
  width: 3.2%;
  background: linear-gradient(90deg, #111 0%, #2a2a2a 50%, #111 100%);
  box-shadow: -2px 0 4px rgb(0 0 0 / 40%);
}
.emboss {
  width: 70%;
  text-align: center;
}
.emboss .year {
  font: 600 2.4cqw var(--font-ui);
  letter-spacing: 0.5em;
  opacity: 0.7;
}
.emboss h1 {
  margin: 2cqw 0;
  font-family: var(--ty-font);
  font-size: 10cqw;
  font-weight: 600;
  line-height: 1.05;
  text-shadow: 0 -1px 0 rgb(0 0 0 / 50%), 0 1px 0 rgb(255 255 255 / 12%);
}
.emboss p {
  margin: 0;
  font-size: 3cqw;
  font-style: italic;
  opacity: 0.75;
}

.cover-guide {
  position: absolute;
  inset: 0;
  overflow: hidden;
  background: color-mix(in oklch, var(--ty-accent) 92%, black);
  color: #fff;
}
.grid-art {
  position: absolute;
  inset: 0;
  background:
    radial-gradient(circle at 78% 70%, rgb(255 255 255 / 22%) 0 22%, transparent 22.3%),
    radial-gradient(circle at 78% 70%, transparent 0 32%, rgb(255 255 255 / 14%) 32.2% 32.6%, transparent 32.8%),
    linear-gradient(rgb(255 255 255 / 7%) 1px, transparent 1px) 0 0 / 8cqw 8cqw,
    linear-gradient(90deg, rgb(255 255 255 / 7%) 1px, transparent 1px) 0 0 / 8cqw 8cqw;
}
.guide-title {
  position: absolute;
  top: 12%;
  left: 11%;
  right: 14%;
}
.guide-title .kicker {
  opacity: 0.8;
  font-size: 2.3cqw;
}
.guide-title h1 {
  margin: 3cqw 0 2cqw;
  font-family: var(--ty-font);
  font-size: 11cqw;
  font-weight: 800;
  line-height: 0.98;
  letter-spacing: -0.02em;
}
.guide-title p {
  max-width: 32ch;
  margin: 0;
  font-size: 3.2cqw;
  line-height: 1.45;
  opacity: 0.85;
}
.guide-icon {
  position: absolute;
  right: 16%;
  bottom: 19%;
  font-size: 13cqw;
}

/* ── Inside the cover ───────────────────────────────── */
.endpaper {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
}
/* A cookbook's endpapers are gingham; a journal's marbled; a guide's a fine grid. */
.endpaper.cookbook {
  background:
    linear-gradient(90deg, color-mix(in oklch, var(--ty-accent) 22%, transparent) 50%, transparent 50%) 0 0 / 5cqw 5cqw,
    linear-gradient(color-mix(in oklch, var(--ty-accent) 22%, transparent) 50%, transparent 50%) 0 0 / 5cqw 5cqw,
    var(--ty-paper);
}
.endpaper.journal {
  background:
    radial-gradient(60% 40% at 20% 30%, color-mix(in oklch, var(--ty-accent) 25%, transparent), transparent),
    radial-gradient(50% 50% at 80% 70%, color-mix(in oklch, var(--ty-accent) 18%, transparent), transparent),
    repeating-radial-gradient(circle at 40% 60%, transparent 0 2cqw, color-mix(in oklch, var(--ty-accent) 9%, transparent) 2cqw 2.3cqw),
    var(--ty-paper);
}
.endpaper.guide {
  background:
    radial-gradient(color-mix(in srgb, var(--ty-ink) 18%, transparent) 0.25cqw, transparent 0.3cqw) 0 0 / 4cqw 4cqw,
    var(--ty-paper);
}
.bookplate {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 1.6cqw;
  width: 62%;
  padding: 7cqw 5cqw;
  border: 0.4cqw double color-mix(in oklch, var(--ty-accent) 60%, var(--ty-ink));
  border-width: 1.2cqw;
  background: var(--ty-paper);
  text-align: center;
  box-shadow: 0 1cqw 2.4cqw rgb(0 0 0 / 10%);
}
.bookplate .hand {
  color: var(--ty-muted);
  font-size: 3.6cqw;
}
.bookplate h2 {
  margin: 0;
  font-family: var(--ty-font);
  font-size: 6.4cqw;
  line-height: 1.1;
}
.bookplate p {
  margin: 0;
  color: var(--ty-muted);
  font-size: 3cqw;
  line-height: 1.5;
}

/* ── Contents ───────────────────────────────────────── */
.contents {
  height: 100%;
  padding: 11cqw 11cqw 10cqw;
}
.contents h2 {
  margin: 0 0 1.6em;
  color: var(--ty-accent);
  font: 700 1.05em var(--font-ui);
  letter-spacing: 0.3em;
  text-transform: uppercase;
}
.st-cookbook .contents h2,
.st-journal .contents h2 {
  font-family: var(--ty-font);
  font-size: 2.4em;
  font-weight: 600;
  letter-spacing: 0;
  text-transform: none;
}
.contents ol {
  margin: 0;
  padding: 0;
  list-style: none;
}
.contents button {
  display: flex;
  align-items: baseline;
  gap: 0.8em;
  width: 100%;
  padding: 0.45em 0;
  color: inherit;
  font: inherit;
  font-size: 1.15em;
  text-align: left;
}
.contents .no {
  min-width: 1.6em;
  color: var(--ty-accent);
  font-family: var(--ty-font);
  font-weight: 700;
  font-variant-numeric: oldstyle-nums;
}
.contents .entry {
  display: flex;
  flex-direction: column;
  min-width: 0;
  max-width: 70%;
}
.contents .entry small {
  color: var(--ty-faint);
  font: 500 0.72em var(--font-ui);
  letter-spacing: 0.06em;
}
.contents button:hover .entry {
  color: var(--ty-accent);
}
.leader {
  flex: 1;
  min-width: 2em;
  border-bottom: 0.16em dotted var(--ty-line);
}
.num {
  color: var(--ty-muted);
  font-variant-numeric: tabular-nums;
}
.none {
  color: var(--ty-muted);
  line-height: 1.6;
}

/* ── An item's picture ──────────────────────────────── */
.photo-page {
  position: absolute;
  inset: 0;
}
.photo-page :deep(img),
.photo-page > :deep(div) {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.photo-no {
  position: absolute;
  left: 7cqw;
  bottom: 7cqw;
  padding: 1.2cqw 2.4cqw;
  border-radius: 99px;
  background: rgb(255 255 255 / 88%);
  color: #222;
  font: 700 2.2cqw var(--font-ui);
  letter-spacing: 0.18em;
  text-transform: uppercase;
}
.drawing {
  position: absolute;
}
.art-cook .big-no {
  position: absolute;
  top: 6cqw;
  left: 8cqw;
  color: transparent;
  font: 700 30cqw/1 var(--ty-font);
  -webkit-text-stroke: 0.35cqw color-mix(in oklch, var(--ty-accent) 55%, var(--ty-paper));
  font-variant-numeric: oldstyle-nums;
}
.art-cook .drawing {
  inset: 26cqw 6cqw 26cqw;
}
.art-cook .fig {
  position: absolute;
  left: 10cqw;
  right: 10cqw;
  bottom: 12cqw;
  color: var(--ty-muted);
  font-size: 3.6cqw;
  text-align: center;
}
.art-journal .date-block {
  position: absolute;
  top: 11cqw;
  left: 10cqw;
  display: flex;
  flex-direction: column;
}
.weekday,
.month {
  color: var(--ty-muted);
  font: 600 2.4cqw var(--font-ui);
  letter-spacing: 0.22em;
  text-transform: uppercase;
}
.dnum {
  color: var(--ty-accent);
  font: 600 26cqw/0.95 var(--ty-font);
}
.art-journal .drawing {
  inset: 58cqw 10cqw 12cqw;
}
.art-guide {
  position: absolute;
  inset: 0;
  background: color-mix(in oklch, var(--ty-accent) 8%, var(--ty-paper));
}
.art-guide .chapter {
  position: absolute;
  top: 12cqw;
  left: 11cqw;
  color: var(--ty-accent);
  font: 700 2.4cqw var(--font-ui);
  letter-spacing: 0.3em;
  text-transform: uppercase;
}
.art-guide .chapter-no {
  position: absolute;
  top: 16cqw;
  left: 9cqw;
  font: 800 36cqw/1 var(--ty-font);
  letter-spacing: -0.04em;
  color: color-mix(in oklch, var(--ty-accent) 85%, var(--ty-ink));
}
.art-guide .drawing {
  inset: 70cqw 12cqw 12cqw 40cqw;
}

/* ── An item's words ────────────────────────────────── */
.words {
  position: absolute;
  inset: 0;
  padding: 8cqw 10cqw 12cqw 12cqw;
  line-height: 1.5;
}
.left .words {
  padding: 8cqw 12cqw 12cqw 10cqw;
}
.running {
  display: flex;
  align-items: center;
  gap: 0.6em;
  height: 2em;
  margin-bottom: 1.4em;
  border-bottom: 1px solid var(--ty-line);
  color: var(--ty-faint);
  font: 600 0.72em var(--font-ui);
  letter-spacing: 0.2em;
  text-transform: uppercase;
}
.running .truncate {
  flex: 1;
}
.running .edit {
  opacity: 0;
  transition: opacity 0.15s;
}
.page:hover .running .edit {
  opacity: 1;
}
.title-block {
  margin-bottom: 1.4em;
}
.eyebrow {
  color: var(--ty-accent);
  font: 700 0.72em var(--font-ui);
  letter-spacing: 0.26em;
  text-transform: uppercase;
}
.title-block h2 {
  margin: 0.25em 0 0;
  font-family: var(--ty-font);
  font-size: 2.5em;
  font-weight: 700;
  line-height: 1.08;
  letter-spacing: -0.01em;
}
.headnote {
  margin: 0.6em 0 0;
  color: var(--ty-muted);
  font-style: italic;
  line-height: 1.5;
}
.meta {
  display: flex;
  flex-wrap: wrap;
  margin-top: 1.1em;
  border-block: 1px solid var(--ty-line);
}
.meta-cell {
  display: grid;
  gap: 0.1em;
  padding: 0.55em 1.2em 0.55em 0;
  margin-right: 1.2em;
  font-weight: 600;
}
.meta-cell + .meta-cell {
  padding-left: 1.2em;
  border-left: 1px solid var(--ty-line);
}
.meta small {
  color: var(--ty-muted);
  font: 600 0.66em var(--font-ui);
  letter-spacing: 0.16em;
  text-transform: uppercase;
}
.tag-row {
  display: flex;
  flex-wrap: wrap;
  gap: 0.4em;
  margin-top: 0.9em;
}
.block-head {
  margin: 1.1em 0 0.6em;
  color: var(--ty-accent);
  font: 700 0.8em var(--font-ui);
  letter-spacing: 0.24em;
  text-transform: uppercase;
}
.block-head:first-child,
.running + .block-head {
  margin-top: 0;
}
.more {
  color: var(--ty-faint);
  font-weight: 500;
  letter-spacing: 0.08em;
  text-transform: lowercase;
}
.ingredients {
  margin: 0;
  padding: 0;
  list-style: none;
}
.ingredients.cols {
  columns: 2;
  column-gap: 2em;
}
.ingredients li {
  position: relative;
  padding: 0.12em 0 0.12em 1.1em;
  break-inside: avoid;
}
.ingredients li::before {
  content: "";
  position: absolute;
  left: 0;
  top: 0.68em;
  width: 0.4em;
  height: 0.4em;
  border-radius: 50%;
  background: var(--ty-accent);
  opacity: 0.75;
}
.ingredients b {
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}
.method {
  margin: 0;
  padding: 0;
  list-style: none;
}
.method li {
  display: grid;
  grid-template-columns: 2.2em 1fr;
  gap: 0.4em;
  margin-bottom: 0.7em;
}
.step-no {
  color: var(--ty-accent);
  font: 700 1.5em/1 var(--ty-font);
  font-variant-numeric: oldstyle-nums;
}
.st-guide .step-no {
  display: grid;
  place-items: center;
  width: 1.6em;
  height: 1.6em;
  border-radius: 50%;
  background: var(--ty-accent);
  color: #fff;
  font: 700 0.8em var(--font-ui);
}
.note {
  position: relative;
  margin: 1.4em 0.4em 0.6em;
  padding: 1.4em 1.3em 1em;
  background: color-mix(in oklch, #fff3b8 75%, var(--ty-paper));
  box-shadow: 0 0.3em 0.8em rgb(0 0 0 / 10%);
  transform: rotate(-0.8deg);
}
.tape {
  position: absolute;
  top: -0.7em;
  left: 50%;
  width: 5em;
  height: 1.4em;
  background: rgb(255 255 255 / 55%);
  box-shadow: 0 0 0 1px rgb(0 0 0 / 4%);
  transform: translateX(-50%) rotate(2deg);
}
.note-label {
  color: color-mix(in oklch, var(--ty-accent) 70%, #5b3b10);
  font-size: 1.25em;
}
.note p {
  margin: 0.3em 0 0;
  white-space: pre-line;
  font-family: "Bradley Hand", "Segoe Print", cursive;
  font-size: 1.05em;
  line-height: 1.45;
}
.prose-block {
  margin-bottom: 0.8em;
}
.prose-block :deep(p) {
  margin: 0 0 0.6em;
}
.prose-block :deep(*) {
  font-size: inherit;
}
/* The letter stays in its own paragraph, however short: the next one starts at the margin. */
.dropcap :deep(p:first-child) {
  display: flow-root;
}
.dropcap :deep(p:first-child::first-letter),
.dropcap :deep(.long::first-letter) {
  float: left;
  margin: 0.06em 0.12em 0 0;
  color: var(--ty-accent);
  font: 700 3.2em/0.85 var(--ty-font);
}
.value-block {
  display: grid;
  gap: 0.2em;
  margin-bottom: 0.9em;
}
.value-block small {
  color: var(--ty-muted);
  font: 600 0.66em var(--font-ui);
  letter-spacing: 0.16em;
  text-transform: uppercase;
}
/* A journal page has a notebook's margin line; its title is in a hand. */
.st-journal .words::before {
  content: "";
  position: absolute;
  top: 0;
  bottom: 0;
  left: 7.5cqw;
  border-left: 0.25cqw double color-mix(in oklch, #d0574b 55%, transparent);
  width: 0.8cqw;
}
.st-journal .title-block h2 {
  font-family: "Bradley Hand", "Segoe Print", cursive;
  font-weight: 700;
}
/* A guide has a margin rule. */
.st-guide .words::before {
  content: "";
  position: absolute;
  top: 8cqw;
  bottom: 12cqw;
  left: 7cqw;
  width: 0.5cqw;
  background: color-mix(in oklch, var(--ty-accent) 35%, transparent);
}
.right.st-guide .words::before {
  left: auto;
  right: 7cqw;
}
.folio {
  position: absolute;
  bottom: 5cqw;
  color: var(--ty-faint);
  font: 500 2.2cqw var(--font-ui);
  font-variant-numeric: tabular-nums;
}
.left .folio {
  left: 9cqw;
}
.right .folio {
  right: 9cqw;
}

/* ── The rest of a leaf ─────────────────────────────── */
.tail {
  position: absolute;
  inset: 0;
  padding: 14cqw 12cqw;
}
.tail-head {
  color: var(--ty-muted);
  font-size: 4.2cqw;
}
.writing {
  position: absolute;
  inset: 24cqw 12cqw 26cqw;
  background: linear-gradient(transparent calc(7cqw - 1px), var(--ty-line) calc(7cqw - 1px), transparent 7cqw) 0 0 / 100% 7cqw;
}
.st-guide .writing {
  background: radial-gradient(var(--ty-line) 0.3cqw, transparent 0.35cqw) 0 0 / 5cqw 5cqw;
}
.fin {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 12cqw;
  color: color-mix(in oklch, var(--ty-accent) 60%, var(--ty-paper));
  font-size: 5cqw;
  text-align: center;
}
</style>
