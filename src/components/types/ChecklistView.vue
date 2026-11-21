<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { showToast } from "../../bridge";
import { itemTitle, timeLabel, typeApi, type Full, type Item } from "../../savedTypes";
import { findTime, isDone, occurrence, today, withoutTime } from "../../typeExtract";
import Icon from "../Icon.vue";

/**
 * A checklist: today's date, how much is done, what comes next, and each item with its time.
 * With reminders on, a bell on each timed item asks for a notification before it.
 */
const props = defineProps<{ full: Full }>();
const emit = defineEmits<{ edit: [item: Item] }>();

const def = computed(() => props.full.def);
const timeKey = computed(() => def.value.view.timeField);
const notesKey = computed(() => def.value.fields.find((f) => (f.kind === "longtext" || f.kind === "markdown") && f.key !== def.value.view.titleField)?.key ?? null);

const now = ref(new Date());
let clock = 0;
onMounted(() => (clock = window.setInterval(() => (now.value = new Date()), 30_000)));
onBeforeUnmount(() => window.clearInterval(clock));

const done = (item: Item) => isDone(item, def.value.view.dailyReset, now.value);
const at = (item: Item) => (timeKey.value ? occurrence(item.fields[timeKey.value], now.value) : null);

/** Timed items in time order, then the rest as saved. */
const rows = computed(() => {
  const list = props.full.items.map((item, index) => ({ item, index, at: at(item) }));
  return list.sort((a, b) => {
    if (a.at && b.at) return a.at.getTime() - b.at.getTime();
    if (a.at || b.at) return a.at ? -1 : 1;
    return a.index - b.index;
  });
});
const doneCount = computed(() => props.full.items.filter(done).length);
const total = computed(() => props.full.items.length);
const progress = computed(() => (total.value ? doneCount.value / total.value : 0));
const next = computed(() => rows.value.find((r) => r.at && r.at > now.value && !done(r.item)) ?? null);
const nextIn = computed(() => {
  if (!next.value?.at) return "";
  const minutes = Math.round((next.value.at.getTime() - now.value.getTime()) / 60_000);
  if (minutes < 1) return "now";
  if (minutes < 60) return `in ${minutes} min`;
  const h = Math.floor(minutes / 60), m = minutes % 60;
  return `in ${h} h${m ? ` ${m} min` : ""}`;
});
const dateLabel = computed(() => now.value.toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" }));
const past = (row: { at: Date | null; item: Item }) => !!row.at && row.at < now.value && !done(row.item);

async function toggle(item: Item) {
  const value = !done(item);
  try {
    await typeApi.updateItem(def.value.id, item.id, { done: value, doneOn: today() });
  } catch (error) {
    showToast(String(error));
  }
}

async function bell(item: Item) {
  try {
    if (!item.remind) await typeApi.allowReminders();
    await typeApi.updateItem(def.value.id, item.id, { remind: !item.remind });
  } catch (error) {
    showToast(String(error));
  }
}

const busy = ref(false);
async function setReminders(on: boolean) {
  busy.value = true;
  try {
    if (on) await typeApi.allowReminders();
    await typeApi.update(def.value.id, { view: { reminders: on } });
    if (on) {
      for (const item of props.full.items) {
        if (!item.remind && at(item)) await typeApi.updateItem(def.value.id, item.id, { remind: true });
      }
      showToast("Dial will remind you while it is open");
    }
  } catch (error) {
    showToast(String(error));
  } finally {
    busy.value = false;
  }
}

const draft = ref("");
async function add() {
  const text = draft.value.trim();
  const titleKey = def.value.view.titleField;
  if (!text || !titleKey) return;
  const time = timeKey.value ? findTime(text) : null;
  const fields: Record<string, unknown> = { [titleKey]: time ? withoutTime(text) || text : text };
  if (time && timeKey.value) fields[timeKey.value] = time;
  try {
    await typeApi.addItem(def.value.id, fields);
    draft.value = "";
  } catch (error) {
    showToast(String(error));
  }
}

async function remove(item: Item) {
  try {
    await typeApi.removeItem(def.value.id, item.id);
  } catch (error) {
    showToast(String(error));
  }
}

const RING = 2 * Math.PI * 26;
</script>

<template>
  <div class="checklist">
    <header class="today">
      <div class="ring" :aria-label="`${doneCount} of ${total} done`">
        <svg viewBox="0 0 64 64">
          <circle cx="32" cy="32" r="26" class="track" />
          <circle cx="32" cy="32" r="26" class="fill" :style="{ strokeDasharray: RING, strokeDashoffset: RING * (1 - progress) }" />
        </svg>
        <span>{{ Math.round(progress * 100) }}<small>%</small></span>
      </div>
      <div class="when">
        <span class="eyebrow">{{ def.view.dailyReset ? "Today" : def.name }}</span>
        <h2>{{ def.view.dailyReset ? dateLabel : def.view.tagline || dateLabel }}</h2>
        <p>
          {{ doneCount }} of {{ total }} done<template v-if="next"> · Next: <strong>{{ itemTitle(def, next.item) }}</strong> {{ nextIn }}</template>
          <template v-else-if="total && doneCount === total"> · All done 🎉</template>
        </p>
      </div>
      <label v-if="timeKey" class="switch" :class="{ on: def.view.reminders }" :title="def.view.reminders ? 'Reminders are on while Dial is open' : 'Get a notification before each timed item'">
        <Icon name="bell" :size="14" />
        <span>Remind me</span>
        <input type="checkbox" :checked="def.view.reminders" :disabled="busy" @change="setReminders(($event.target as HTMLInputElement).checked)" />
        <i />
      </label>
    </header>

    <ul class="rows">
      <TransitionGroup name="row">
        <li v-for="row in rows" :key="row.item.id" class="row" :class="{ done: done(row.item), past: past(row), next: next?.item.id === row.item.id }">
          <button class="box" role="checkbox" :aria-checked="done(row.item)" :title="done(row.item) ? 'Mark not done' : 'Mark done'" @click="toggle(row.item)">
            <svg viewBox="0 0 24 24"><path d="M5 12.5l4.5 4.5L19 7.5" /></svg>
          </button>
          <span v-if="timeKey" class="time" :class="{ empty: !row.at }">{{ row.at ? timeLabel(row.item.fields[timeKey]) : "—" }}</span>
          <button class="title" @click="emit('edit', row.item)">
            <span class="truncate">{{ itemTitle(def, row.item) }}</span>
            <small v-if="notesKey && row.item.fields[notesKey]" class="truncate">{{ row.item.fields[notesKey] }}</small>
          </button>
          <button
            v-if="def.view.reminders && row.at"
            class="ty-ghost bell"
            :class="{ on: row.item.remind }"
            :title="row.item.remind ? `Reminding ${def.view.leadMinutes.map((m) => (m ? `${m} min before` : 'at the time')).join(' and ')}` : 'Remind me'"
            @click="bell(row.item)"
          >
            <Icon :name="row.item.remind ? 'bell' : 'bellOff'" :size="14" />
          </button>
          <button class="ty-ghost remove" title="Remove" @click="remove(row.item)"><Icon name="cross" :size="12" /></button>
        </li>
      </TransitionGroup>
    </ul>

    <form class="add" @submit.prevent="add">
      <Icon name="plus" :size="14" />
      <input v-model="draft" class="bare" :placeholder="timeKey ? 'Add an item — “7:30 Stretch”' : 'Add an item'" />
    </form>
    <p v-if="def.view.dailyReset" class="foot">Ticks clear every morning, so the list is ready for the next day.</p>
  </div>
</template>

<style scoped>
.checklist {
  width: min(720px, 100%);
  margin: 0 auto;
  padding: 34px 28px 60px;
}
.today {
  display: flex;
  align-items: center;
  gap: 18px;
  margin-bottom: 22px;
  padding: 18px 20px;
  border-radius: var(--ty-radius);
  background: var(--ty-raised);
  box-shadow: 0 1px 0 var(--ty-line), 0 12px 30px rgb(0 0 0 / 10%);
}
.ring {
  position: relative;
  width: 64px;
  height: 64px;
  flex: none;
}
.ring svg {
  width: 100%;
  height: 100%;
  transform: rotate(-90deg);
}
.ring circle {
  fill: none;
  stroke-width: 6;
}
.track {
  stroke: var(--ty-line);
}
.fill {
  stroke: var(--ty-accent);
  stroke-linecap: round;
  transition: stroke-dashoffset 500ms var(--ease);
}
.ring span {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  font: 700 15px var(--font-ui);
}
.ring small {
  font-size: 10px;
}
.when {
  flex: 1;
  min-width: 0;
}
.eyebrow {
  color: var(--ty-accent);
  font: 700 10.5px var(--font-ui);
  letter-spacing: 0.2em;
  text-transform: uppercase;
}
.when h2 {
  margin: 3px 0 4px;
  font-size: 22px;
  font-weight: 700;
}
.when p {
  margin: 0;
  color: var(--ty-muted);
  font: 13px var(--font-ui);
}
.when strong {
  color: var(--ty-ink);
}
.switch {
  position: relative;
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 7px 10px 7px 12px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--ty-ink) 6%, transparent);
  color: var(--ty-muted);
  font: 600 12.5px var(--font-ui);
  cursor: pointer;
  user-select: none;
}
.switch.on {
  background: var(--ty-soft);
  color: color-mix(in srgb, var(--ty-accent) 75%, var(--ty-ink));
}
.switch input {
  position: absolute;
  opacity: 0;
  pointer-events: none;
}
.switch i {
  position: relative;
  width: 30px;
  height: 18px;
  border-radius: 999px;
  background: var(--ty-line);
  transition: background 150ms var(--ease);
}
.switch i::after {
  content: "";
  position: absolute;
  top: 2px;
  left: 2px;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: #fff;
  box-shadow: 0 1px 3px rgb(0 0 0 / 25%);
  transition: transform 150ms var(--ease);
}
.switch.on i {
  background: var(--ty-accent);
}
.switch.on i::after {
  transform: translateX(12px);
}
.rows {
  display: grid;
  gap: 6px;
  margin: 0;
  padding: 0;
  list-style: none;
}
.row {
  display: flex;
  align-items: center;
  gap: 12px;
  min-height: 52px;
  padding: 8px 10px 8px 14px;
  border-radius: calc(var(--ty-radius) * 0.75);
  background: var(--ty-raised);
  box-shadow: 0 1px 0 var(--ty-line);
  transition: background 150ms var(--ease), opacity 150ms var(--ease);
}
.row.next {
  box-shadow: inset 3px 0 0 var(--ty-accent), 0 1px 0 var(--ty-line);
}
.row.done {
  opacity: 0.6;
}
.box {
  display: grid;
  place-items: center;
  width: 24px;
  height: 24px;
  flex: none;
  border-radius: 50%;
  box-shadow: inset 0 0 0 2px var(--ty-faint);
  transition: background 150ms var(--ease), box-shadow 150ms var(--ease), transform 100ms var(--ease);
}
.box:active {
  transform: scale(0.88);
}
.box svg {
  width: 15px;
  height: 15px;
  fill: none;
  stroke: #fff;
  stroke-width: 3;
  stroke-linecap: round;
  stroke-linejoin: round;
  stroke-dasharray: 22;
  stroke-dashoffset: 22;
  transition: stroke-dashoffset 220ms var(--ease) 60ms;
}
.done .box {
  background: var(--ty-accent);
  box-shadow: none;
}
.done .box svg {
  stroke-dashoffset: 0;
}
.time {
  width: 74px;
  flex: none;
  color: var(--ty-accent);
  font: 700 12.5px var(--font-ui);
  font-variant-numeric: tabular-nums;
}
.time.empty {
  color: var(--ty-faint);
}
.past .time {
  color: var(--ty-muted);
}
.title {
  flex: 1;
  min-width: 0;
  display: grid;
  gap: 1px;
  color: inherit;
  font: inherit;
  font-size: 15px;
  text-align: left;
}
.title small {
  color: var(--ty-muted);
  font: 12px var(--font-ui);
}
.done .title > span {
  text-decoration: line-through;
  text-decoration-color: var(--ty-faint);
}
.bell.on {
  color: var(--ty-accent);
}
.remove {
  opacity: 0;
}
.row:hover .remove {
  opacity: 1;
}
.add {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 8px;
  padding: 0 14px;
  min-height: 48px;
  border-radius: calc(var(--ty-radius) * 0.75);
  box-shadow: inset 0 0 0 1.5px var(--ty-line);
  color: var(--ty-faint);
}
.add:focus-within {
  box-shadow: inset 0 0 0 1.5px var(--ty-accent);
}
.bare {
  flex: 1;
  border: 0;
  background: none;
  color: var(--ty-ink);
  font: 14px var(--font-ui);
  outline: none;
}
.bare::placeholder {
  color: var(--ty-faint);
}
.foot {
  margin: 14px 4px 0;
  color: var(--ty-faint);
  font: 12px var(--font-ui);
}
.row-enter-from,
.row-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}
.row-enter-active,
.row-leave-active {
  transition: all 180ms var(--ease);
}
.row-move {
  transition: transform 250ms var(--ease);
}
</style>
