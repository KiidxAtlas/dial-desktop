<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { showToast } from "../../bridge";
import { itemTitle, typeApi, type Full, type Item } from "../../savedTypes";
import { dateOf, dayKey, findTime, withoutTime } from "../../typeExtract";
import Icon from "../Icon.vue";
import { pageArrow } from "./pageKeys";

/**
 * A calendar: a month of days with each item on the day its date falls, and beside it the picked
 * day's agenda. Items move to another day by dragging them there; a line typed under the agenda
 * ("3pm Coffee with Sam") adds one on the picked day.
 */
const props = defineProps<{ full: Full }>();
const emit = defineEmits<{ edit: [item: Item]; add: [] }>();

const def = computed(() => props.full.def);
const timeKey = computed(() => def.value.view.timeField);
const dateOnly = computed(() => def.value.fields.find((f) => f.key === timeKey.value)?.kind === "date");
const subtitleKey = computed(() => def.value.view.subtitleField);

const now = ref(new Date());
let clock = 0;
onMounted(() => (clock = window.setInterval(() => (now.value = new Date()), 60_000)));
onBeforeUnmount(() => window.clearInterval(clock));
const todayKey = computed(() => dayKey(now.value));

/** The first day of the month on screen, and the day picked in it. */
const month = ref(new Date(now.value.getFullYear(), now.value.getMonth(), 1));
const picked = ref(todayKey.value);

interface Entry { item: Item; at: Date; allDay: boolean; day: string }

const entries = computed<Entry[]>(() => {
  const key = timeKey.value;
  if (!key) return [];
  const list: Entry[] = [];
  for (const item of props.full.items) {
    const when = dateOf(item.fields[key]);
    if (when) list.push({ item, ...when, day: dayKey(when.at) });
  }
  // By day; within a day all-day items first, then by time.
  return list.sort((a, b) =>
    a.day !== b.day ? a.at.getTime() - b.at.getTime() : a.allDay !== b.allDay ? (a.allDay ? -1 : 1) : a.at.getTime() - b.at.getTime(),
  );
});
const unplaced = computed(() => props.full.items.filter((item) => !entries.value.some((e) => e.item.id === item.id)));
const byDay = computed(() => {
  const map = new Map<string, Entry[]>();
  for (const e of entries.value) {
    const list = map.get(e.day);
    if (list) list.push(e);
    else map.set(e.day, [e]);
  }
  return map;
});

/** The week starts where the user's locale starts it. */
const firstDay = (() => {
  try {
    const locale = new Intl.Locale(navigator.language) as Intl.Locale & {
      weekInfo?: { firstDay: number };
      getWeekInfo?: () => { firstDay: number };
    };
    return (locale.getWeekInfo?.().firstDay ?? locale.weekInfo?.firstDay ?? 7) % 7;
  } catch {
    return 0;
  }
})();
const weekdays = computed(() =>
  Array.from({ length: 7 }, (_, i) => new Date(2024, 0, 7 + ((firstDay + i) % 7)).toLocaleDateString(undefined, { weekday: "short" })),
);

/** Whole weeks covering the month: 4 to 6 rows. */
const days = computed(() => {
  const start = new Date(month.value);
  start.setDate(1 - ((start.getDay() - firstDay + 7) % 7));
  const last = new Date(month.value.getFullYear(), month.value.getMonth() + 1, 0);
  const cells: { date: Date; key: string; inMonth: boolean }[] = [];
  for (const d = new Date(start); cells.length % 7 !== 0 || d <= last; d.setDate(d.getDate() + 1)) {
    cells.push({ date: new Date(d), key: dayKey(d), inMonth: d.getMonth() === month.value.getMonth() });
  }
  return cells;
});
const weeks = computed(() => days.value.length / 7);
const monthName = computed(() => month.value.toLocaleDateString(undefined, { month: "long" }));
const year = computed(() => month.value.getFullYear());
const MAX_CHIPS = 3;

function shift(by: number) {
  month.value = new Date(month.value.getFullYear(), month.value.getMonth() + by, 1);
}
function goToday() {
  month.value = new Date(now.value.getFullYear(), now.value.getMonth(), 1);
  picked.value = todayKey.value;
}
function pick(cell: { date: Date; key: string; inMonth: boolean }) {
  picked.value = cell.key;
  if (!cell.inMonth) month.value = new Date(cell.date.getFullYear(), cell.date.getMonth(), 1);
}
function show(e: Entry) {
  picked.value = e.day;
  month.value = new Date(e.at.getFullYear(), e.at.getMonth(), 1);
}

const pickedDate = computed(() => {
  const [y, m, d] = picked.value.split("-").map(Number);
  return new Date(y, m - 1, d);
});
const pickedLabel = computed(() =>
  picked.value === todayKey.value ? "Today" : pickedDate.value.toLocaleDateString(undefined, { weekday: "long" }),
);
const pickedSub = computed(() =>
  pickedDate.value.toLocaleDateString(undefined, {
    month: "long",
    day: "numeric",
    year: pickedDate.value.getFullYear() === now.value.getFullYear() ? undefined : "numeric",
  }),
);
const agenda = computed(() => byDay.value.get(picked.value) ?? []);
/** With nothing on the picked day, what comes next. */
const upcoming = computed(() => {
  const start = new Date(now.value.getFullYear(), now.value.getMonth(), now.value.getDate());
  return entries.value.filter((e) => e.at >= start && e.day !== picked.value).slice(0, 4);
});

const clockLabel = (at: Date) => at.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
/** "10am", "7:15pm" where the locale uses a 12-hour clock; "10:00" where it does not. */
function chipTime(at: Date): string {
  const parts = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).formatToParts(at);
  const get = (type: string) => parts.find((p) => p.type === type)?.value ?? "";
  const period = get("dayPeriod");
  if (!period) return `${get("hour")}:${get("minute")}`;
  const minute = get("minute") === "00" ? "" : `:${get("minute")}`;
  return `${get("hour")}${minute}${period.toLowerCase().replace(/[.\s]/g, "")}`;
}
const shortDay = (at: Date) => at.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
const subtitle = (item: Item) => {
  const key = subtitleKey.value;
  const value = key ? item.fields[key] : null;
  return typeof value === "string" ? value : "";
};

function stamp(day: string, time: string | null) {
  return dateOnly.value || !time ? day : `${day} ${time}`;
}

const draft = ref("");
async function add() {
  const text = draft.value.trim();
  const titleKey = def.value.view.titleField;
  if (!text || !titleKey || !timeKey.value) return;
  const time = findTime(text);
  try {
    await typeApi.addItem(def.value.id, { [titleKey]: (time && withoutTime(text)) || text, [timeKey.value]: stamp(picked.value, time) });
    draft.value = "";
  } catch (error) {
    showToast(String(error));
  }
}

// Dragging an item to another day keeps its time.
const dragging = ref<string | null>(null);
const over = ref<string | null>(null);
function dragStart(event: DragEvent, entry: Entry) {
  dragging.value = entry.item.id;
  event.dataTransfer?.setData("text/plain", entry.item.id);
  if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
}
function dragEnd() {
  dragging.value = null;
  over.value = null;
}
async function drop(day: string) {
  const entry = entries.value.find((e) => e.item.id === dragging.value);
  dragEnd();
  if (!entry || !timeKey.value || entry.day === day) return;
  const time = entry.allDay ? null : `${String(entry.at.getHours()).padStart(2, "0")}:${String(entry.at.getMinutes()).padStart(2, "0")}`;
  picked.value = day;
  try {
    await typeApi.updateItem(def.value.id, entry.item.id, { fields: { [timeKey.value]: stamp(day, time) } });
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
function onKey(event: KeyboardEvent) {
  const step = pageArrow(event);
  if (!step) return;
  event.preventDefault();
  shift(step);
}
onMounted(() => window.addEventListener("keydown", onKey));
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <div class="calendar">
    <section class="month">
      <header class="bar">
        <h2><span class="name">{{ monthName }}</span> <span class="year">{{ year }}</span></h2>
        <div class="nav">
          <button class="today-btn" @click="goToday">Today</button>
          <button class="ty-ghost" title="Previous month (←)" @click="shift(-1)"><Icon name="chevronLeft" :size="16" /></button>
          <button class="ty-ghost" title="Next month (→)" @click="shift(1)"><Icon name="chevronRight" :size="16" /></button>
          <button class="add-btn" title="Add an event" @click="emit('add')"><Icon name="plus" :size="14" /> Add event</button>
        </div>
      </header>

      <div class="weekdays">
        <span v-for="w in weekdays" :key="w">{{ w }}</span>
      </div>
      <div class="grid" :style="{ gridTemplateRows: `repeat(${weeks}, minmax(0, 1fr))` }">
        <div
          v-for="cell in days"
          :key="cell.key"
          class="day"
          :class="{
            out: !cell.inMonth,
            today: cell.key === todayKey,
            picked: cell.key === picked,
            over: over === cell.key,
            weekend: cell.date.getDay() === 0 || cell.date.getDay() === 6,
          }"
          @click="pick(cell)"
          @dragover.prevent="over = cell.key"
          @dragleave="over === cell.key && (over = null)"
          @drop.prevent="drop(cell.key)"
        >
          <span class="num">{{ cell.date.getDate() }}</span>
          <div class="chips">
            <button
              v-for="e in (byDay.get(cell.key) ?? []).slice(0, MAX_CHIPS)"
              :key="e.item.id"
              class="chip"
              :class="{ allday: e.allDay, dragging: dragging === e.item.id }"
              draggable="true"
              :title="`${e.allDay ? '' : clockLabel(e.at) + ' · '}${itemTitle(def, e.item)}`"
              @click.stop="picked = cell.key; emit('edit', e.item)"
              @dragstart="dragStart($event, e)"
              @dragend="dragEnd"
            >
              <i v-if="!e.allDay" />
              <span v-if="!e.allDay" class="t">{{ chipTime(e.at) }}</span>
              <span class="truncate">{{ itemTitle(def, e.item) }}</span>
            </button>
            <span v-if="(byDay.get(cell.key)?.length ?? 0) > MAX_CHIPS" class="more">+{{ byDay.get(cell.key)!.length - MAX_CHIPS }} more</span>
          </div>
        </div>
      </div>
    </section>

    <aside class="agenda">
      <header>
        <span class="eyebrow">{{ pickedLabel }}</span>
        <h3>{{ pickedSub }}</h3>
      </header>

      <ol v-if="agenda.length" class="events">
        <li v-for="e in agenda" :key="e.item.id" class="event" :class="{ past: !e.allDay && e.at < now }">
          <span class="when">{{ e.allDay ? "All day" : clockLabel(e.at) }}</span>
          <button class="what" @click="emit('edit', e.item)">
            <strong class="truncate">{{ itemTitle(def, e.item) }}</strong>
            <small v-if="subtitle(e.item)" class="truncate">{{ subtitle(e.item) }}</small>
          </button>
          <button
            v-if="def.view.reminders && !e.allDay"
            class="ty-ghost bell"
            :class="{ on: e.item.remind }"
            :title="e.item.remind ? 'Reminding you while Dial is open' : 'Remind me'"
            @click="bell(e.item)"
          >
            <Icon :name="e.item.remind ? 'bell' : 'bellOff'" :size="14" />
          </button>
        </li>
      </ol>
      <div v-else class="nothing">
        <p>Nothing planned.</p>
        <template v-if="upcoming.length">
          <span class="eyebrow small">Up next</span>
          <button v-for="e in upcoming" :key="e.item.id" class="next" @click="show(e)">
            <span class="d">{{ shortDay(e.at) }}</span>
            <span class="truncate">{{ itemTitle(def, e.item) }}</span>
          </button>
        </template>
      </div>

      <form v-if="timeKey" class="add" @submit.prevent="add">
        <Icon name="plus" :size="14" />
        <input v-model="draft" class="bare" placeholder="Add — “3pm Coffee with Sam”" />
      </form>

      <section v-if="unplaced.length" class="unplaced">
        <span class="eyebrow small">No date yet</span>
        <button v-for="item in unplaced" :key="item.id" class="next" @click="emit('edit', item)">
          <span class="truncate">{{ itemTitle(def, item) }}</span>
        </button>
      </section>
    </aside>
  </div>
</template>

<style scoped>
.calendar {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 290px;
  gap: 22px;
  height: 100%;
  min-height: 560px;
  padding: 22px 24px 24px;
  font-family: var(--font-ui);
}
.month {
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
}
.bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 14px;
}
.bar h2 {
  margin: 0;
  font-family: var(--ty-font);
  font-size: 30px;
  font-weight: 700;
  letter-spacing: -0.01em;
  white-space: nowrap;
}
.bar .year {
  color: var(--ty-faint);
  font-weight: 400;
}
.nav {
  display: flex;
  align-items: center;
  gap: 2px;
}
.nav .ty-ghost {
  color: var(--ty-muted);
}
.today-btn {
  height: 30px;
  margin-right: 6px;
  padding: 0 13px;
  border-radius: 999px;
  box-shadow: inset 0 0 0 1px var(--ty-line);
  color: var(--ty-ink);
  font: 600 12.5px var(--font-ui);
}
.today-btn:hover {
  background: var(--ty-soft);
}
.add-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 30px;
  margin-left: 10px;
  padding: 0 13px 0 11px;
  border-radius: 999px;
  background: var(--ty-accent);
  color: #fff;
  font: 600 12.5px var(--font-ui);
  box-shadow: 0 6px 16px color-mix(in srgb, var(--ty-accent) 30%, transparent);
  transition: filter 120ms var(--ease), transform 120ms var(--ease);
}
.add-btn:hover {
  filter: brightness(1.06);
  transform: translateY(-1px);
}
.weekdays {
  display: grid;
  grid-template-columns: repeat(7, minmax(0, 1fr));
  padding: 0 0 8px;
}
.weekdays span {
  padding-left: 10px;
  color: var(--ty-faint);
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}
.grid {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: repeat(7, minmax(0, 1fr));
  gap: 1px;
  border-radius: var(--ty-radius);
  overflow: hidden;
  background: var(--ty-line);
  box-shadow: 0 0 0 1px var(--ty-line), 0 14px 34px rgb(0 0 0 / 8%);
}
.day {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
  min-height: 0;
  padding: 6px 6px 4px;
  background: var(--ty-raised);
  cursor: pointer;
  overflow: hidden;
  transition: background 120ms var(--ease);
}
.day.weekend {
  background: color-mix(in srgb, var(--ty-raised) 95%, var(--ty-ink));
}
.day:hover {
  background: color-mix(in srgb, var(--ty-raised) 90%, var(--ty-accent));
}
.day.out {
  background: color-mix(in srgb, var(--ty-paper) 92%, var(--ty-ink));
}
.day.out .num,
.day.out .chips {
  opacity: 0.45;
}
.day.picked {
  box-shadow: inset 0 0 0 2px var(--ty-accent);
}
.day.over {
  background: var(--ty-soft);
}
.num {
  align-self: flex-start;
  display: grid;
  place-items: center;
  min-width: 26px;
  height: 26px;
  padding: 0 6px;
  border-radius: 999px;
  color: var(--ty-ink);
  font-size: 13px;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
.today .num {
  background: var(--ty-accent);
  color: #fff;
}
.chips {
  display: grid;
  gap: 3px;
  min-height: 0;
}
.chip {
  display: flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
  height: 20px;
  padding: 0 6px;
  border-radius: 6px;
  color: var(--ty-ink);
  font: 500 11.5px var(--font-ui);
  text-align: left;
  cursor: grab;
}
.chip:hover {
  background: var(--ty-soft);
}
.chip i {
  width: 6px;
  height: 6px;
  flex: none;
  border-radius: 50%;
  background: var(--ty-accent);
}
.chip .t {
  flex: none;
  color: var(--ty-muted);
  font-variant-numeric: tabular-nums;
}
.chip.allday {
  background: color-mix(in srgb, var(--ty-accent) 88%, black);
  color: #fff;
  font-weight: 600;
}
.chip.dragging {
  opacity: 0.4;
}
.more {
  padding: 0 6px;
  color: var(--ty-muted);
  font-size: 11px;
  font-weight: 600;
}

.agenda {
  display: flex;
  flex-direction: column;
  gap: 14px;
  min-height: 0;
  overflow: auto;
  padding: 18px;
  border-radius: var(--ty-radius);
  background: var(--ty-raised);
  box-shadow: 0 0 0 1px var(--ty-line), 0 14px 34px rgb(0 0 0 / 8%);
}
.eyebrow {
  color: var(--ty-accent);
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.16em;
  text-transform: uppercase;
}
.eyebrow.small {
  color: var(--ty-faint);
  font-size: 10px;
}
.agenda h3 {
  margin: 4px 0 0;
  font-family: var(--ty-font);
  font-size: 21px;
  font-weight: 700;
}
.events {
  display: grid;
  gap: 2px;
  margin: 0;
  padding: 0;
  list-style: none;
}
.event {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 9px 4px 9px 12px;
  border-radius: 10px;
  box-shadow: inset 3px 0 0 var(--ty-accent);
}
.event:hover {
  background: var(--ty-soft);
}
.event.past {
  opacity: 0.55;
}
.when {
  width: 62px;
  flex: none;
  color: var(--ty-accent);
  font-size: 12px;
  font-weight: 700;
  font-variant-numeric: tabular-nums;
}
.what {
  flex: 1;
  min-width: 0;
  display: grid;
  gap: 1px;
  color: var(--ty-ink);
  text-align: left;
}
.what strong {
  font-size: 14px;
  font-weight: 600;
}
.what small {
  color: var(--ty-muted);
  font-size: 12px;
}
.bell {
  color: var(--ty-faint);
}
.bell.on {
  color: var(--ty-accent);
}
.nothing {
  display: grid;
  gap: 6px;
}
.nothing p {
  margin: 0 0 8px;
  color: var(--ty-muted);
  font-size: 13.5px;
}
.next {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
  padding: 7px 10px;
  border-radius: 9px;
  color: var(--ty-ink);
  font-size: 13px;
  text-align: left;
}
.next:hover {
  background: var(--ty-soft);
}
.next .d {
  flex: none;
  width: 82px;
  color: var(--ty-muted);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}
.add {
  display: flex;
  align-items: center;
  gap: 8px;
  min-height: 40px;
  padding: 0 12px;
  border-radius: 10px;
  box-shadow: inset 0 0 0 1.5px var(--ty-line);
  color: var(--ty-faint);
}
.add:focus-within {
  box-shadow: inset 0 0 0 1.5px var(--ty-accent);
}
.bare {
  flex: 1;
  min-width: 0;
  border: 0;
  background: none;
  color: var(--ty-ink);
  font: 13px var(--font-ui);
  outline: none;
}
.bare::placeholder {
  color: var(--ty-faint);
}
.unplaced {
  display: grid;
  gap: 4px;
  margin-top: auto;
  padding-top: 12px;
  border-top: 1px solid var(--ty-line);
}

/* Narrow: the agenda goes under the month. */
@container (max-width: 860px) {
  .calendar {
    grid-template-columns: 1fr;
    height: auto;
  }
  .grid {
    min-height: 460px;
  }
}
</style>
