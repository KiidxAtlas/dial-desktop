<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { conversationFindOpen } from "../commands";
import { entryKey, focusComposer, toggle, ui } from "../ui";
import { engine } from "../bridge";
import { canRetry, canUndo, copyText, editFromHere, editLastPrompt, promptOccurrence, regenerateBlocker, regenerateLast, sentImagesFor } from '../sessionActions';
import { draftFor } from "../drafts";
import { isLongPrompt, withQuote } from "../quote";
import type { OpenSnapshot } from "../types";
import Markdown from "./Markdown.vue";
import ToolRun from "./ToolRun.vue";
import EmptyState from "./EmptyState.vue";
import Icon from "./Icon.vue";
import { pendingSave, saveMenu } from "../savedTypes";
import WorkingIndicator from "./WorkingIndicator.vue";
import { turnReviews } from "../turnReview";
import RecordedTurns from './RecordedTurns.vue';
import CodeBlock from "./CodeBlock.vue";
import { showPanel } from "../workspace";
import { phaseOf, toolRuns, type ToolRun as Run } from "../toolFolds";
import { turnProgress } from "../turnProgress";
import { findOccurrences, transcriptCandidates } from '../chatFind';
import FindText from './FindText.vue';
import { viewAttachment } from '../imageViewer';
import { renderLazily, rememberScroll, savedAnchor, savedScroll, scrollKey, type ScrollAnchor } from "../chatScroll";

const props = defineProps<{ snap: OpenSnapshot }>();
const recorded = ref<InstanceType<typeof RecordedTurns> | null>(null);
// Folds/receipts depend on row kinds, tools, received messages and notices, not on the contents
// of a streaming answer. The bridge shares unchanged rows. Reuse the grouping input instead of
// rebuilding every run and handing every historical ToolRun new props while only the last answer grows.
const folds = (kind: string) => kind === 'tool' || kind === 'received' || kind === 'notice';
const grouping = computed<OpenSnapshot['entries']>((previous) => {
  const entries = props.snap.entries;
  return previous?.length === entries.length && entries.every((e, i) => e === previous[i] || (!folds(e.kind) && e.kind === previous[i].kind)) ? previous : entries;
});
const reviews = computed(() => turnReviews(grouping.value, props.snap.turnRunning));
function forkAt(index: number, prompt: string) {
  const snap = props.snap;
  if (!snap.sessionId) return;
  const { occurrence, matches } = promptOccurrence(snap.entries, index);
  void engine.forkBefore(snap.workspace.root, snap.sessionId, prompt, occurrence, matches);
}
function evidence(index: number, key: string) {
  pinned.value = false;
  ui.expanded.add(key);
  revealEntry(index);
}
/** The entry being scrolled to; it always renders fully (see `renderLazily`). */
const revealing = ref(-1);
/**
 * Scroll entry `index` to the middle of the view. Older entries off screen are drawn at an
 * estimated height until seen, so the ones it passes can settle to their real height and move it;
 * it is centred again once they have.
 */
let revealVersion = 0;
function revealEntry(index: number, occurrence = false) {
  const version = ++revealVersion;
  revealing.value = index;
  const center = () => {
    if (version !== revealVersion) return;
    const row = scroller.value?.querySelector<HTMLElement>(`[data-entry="${index}"]`);
    (occurrence ? row?.querySelector('.find-current') ?? row : row)?.scrollIntoView({ block: 'center', inline: 'nearest' });
  };
  void nextTick(() => { center(); requestAnimationFrame(() => requestAnimationFrame(center)); });
}
function workingTree(path: string) {
  ui.diffPath = path;
  showPanel("changes");
}

const scroller = ref<HTMLElement | null>(null);
/** Follow new output only while the reader is at the bottom. */
const pinned = ref(true);
const searchInput = ref<HTMLInputElement | null>(null);
const query = ref("");
const activeMatch = ref(-1);
const transcriptMatch = ref(-1);
const transcriptHit = computed(() => {
  const hit = ui.transcriptJump;
  return hit?.root === props.snap.workspace.root && hit.session_id === props.snap.sessionId ? hit : null;
});

const candidates = computed(() => transcriptHit.value && !props.snap.historyLoading ? transcriptCandidates(props.snap.entries, transcriptHit.value) : []);
function visitCandidate(index: number) {
  transcriptMatch.value = index;
  pinned.value = false;
  revealEntry(index);
}
watch([transcriptHit, candidates], ([hit, rows], old) => {
  if (hit === old?.[0] && rows.includes(transcriptMatch.value)) return;
  transcriptMatch.value = -1;
  if (rows.length === 1) visitCandidate(rows[0]);
}, { flush: 'post', immediate: true });
function nextCandidate() {
  const rows = candidates.value;
  if (rows.length) visitCandidate(rows[(rows.indexOf(transcriptMatch.value) + 1) % rows.length]);
}
const matches = computed(() => conversationFindOpen.value ? findOccurrences(props.snap.entries, query.value) : []);
const current = computed(() => matches.value[activeMatch.value]);
const matchNumber = computed(() => current.value ? activeMatch.value + 1 : 0);

function showMatch(index: number) {
  activeMatch.value = index;
  if (!current.value) return;
  pinned.value = false;
  revealEntry(current.value.entry, true);
}

function stepMatch(step: number) {
  const list = matches.value;
  if (list.length) showMatch((activeMatch.value + step + list.length) % list.length);
}

function closeFind() {
  conversationFindOpen.value = false;
  if (searchInput.value?.parentElement?.contains(document.activeElement)) scroller.value?.focus({ preventScroll: true });
  onScroll();
}

function findKey(event: KeyboardEvent) {
  if (event.defaultPrevented || event.isComposing || ui.settingsOpen || ui.menu || ui.paletteOpen || ui.shortcutsOpen || ui.modelPickerOpen) return;
  if (event.target instanceof Element && event.target.closest(".xterm")) return;
  const mod = props.snap.platform === 'macos' ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
  if (mod && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "f" && !empty.value) {
    event.preventDefault();
    conversationFindOpen.value = true;
    nextTick(() => { searchInput.value?.focus(); searchInput.value?.select(); });
  } else if (mod && event.altKey && !event.shiftKey && (event.key === "ArrowUp" || event.key === "ArrowDown") && !empty.value) {
    event.preventDefault();
    stepPrompt(event.key === "ArrowUp" ? -1 : 1);
  }
}

/** Scroll to the previous or next prompt you sent, relative to the top of the view. */
function stepPrompt(step: 1 | -1) {
  const el = scroller.value;
  if (!el) return;
  const top = el.getBoundingClientRect().top + 8;
  const rows = [...el.querySelectorAll<HTMLElement>(".row.k-user")];
  const target = step < 0
    ? rows.filter((row) => row.getBoundingClientRect().top < top - 4).at(-1)
    : rows.find((row) => row.getBoundingClientRect().top > top + 4);
  if (!target) {
    if (step > 0) jumpToEnd();
    return;
  }
  pinned.value = false;
  el.scrollTo({ top: el.scrollTop + target.getBoundingClientRect().top - top, behavior: "smooth" });
  target.classList.remove("flash");
  void target.offsetWidth;
  target.classList.add("flash");
}

// ── Quoting into the prompt box ──
function quote(text: string) {
  const draft = draftFor(props.snap);
  draft.text = withQuote(draft.text, text);
  window.getSelection()?.removeAllRanges();
  selection.value = null;
  focusComposer();
}
/** Selected conversation text, with where to show its "Quote" button (window coordinates). */
const selection = ref<{ text: string; x: number; y: number } | null>(null);
function onSelect() {
  // After the click lands, so a click that clears the selection hides the button.
  setTimeout(() => {
    const sel = window.getSelection();
    const text = sel?.toString() ?? "";
    const node = sel?.anchorNode instanceof Element ? sel.anchorNode : sel?.anchorNode?.parentElement;
    if (!sel || sel.isCollapsed || !text.trim() || !node?.closest(".row.k-user, .row.k-assistant, .received-detail") || !scroller.value?.contains(node)) {
      selection.value = null;
      return;
    }
    const rect = sel.getRangeAt(0).getBoundingClientRect();
    selection.value = { text, x: rect.left + rect.width / 2, y: rect.top };
  });
}

onMounted(() => {
  window.addEventListener("keydown", findKey);
  document.addEventListener("selectionchange", onSelectionChange);
  window.addEventListener('pagehide', saveReadingPosition);
});
function onSelectionChange() {
  if (selection.value && window.getSelection()?.isCollapsed) selection.value = null;
}
onBeforeUnmount(() => {
  window.removeEventListener("keydown", findKey);
  window.removeEventListener('pagehide', saveReadingPosition);
  saveReadingPosition();
  revealVersion++;
  document.removeEventListener("selectionchange", onSelectionChange);
  conversationFindOpen.value = false;
});
watch(searchInput, (el) => el?.focus());
watch([query, matches], ([q, list], [old]) => {
  if (q !== old || activeMatch.value < 0 || activeMatch.value >= list.length) showMatch(list.length ? 0 : -1);
}, { flush: 'post' });

const empty = computed(() => props.snap.entries.length === 0 && !props.snap.turnRunning);
const progress = computed(() => turnProgress(props.snap));
/** The indicator names what the running call does ("Reading…", "Editing…") rather than "Running a tool…". */
const progressLabel = computed(() => {
  const p = progress.value;
  return p?.tool ? `${phaseOf(p.tool)}…` : p?.label ?? "";
});

/** Last seen scroll position, to tell scrolling up from scrolling down. */
let lastTop = 0;
function onScroll() {
  const el = scroller.value;
  if (el) {
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    // At the end: follow. Moving up: the reader is leaving, even within a few lines of the end
    // (otherwise the next streamed update snaps them straight back down). Moving down near the
    // end: follow again. Moving down further up (a smooth jump to the end) keeps the current state.
    if (distance <= 2) pinned.value = true;
    else if (el.scrollTop < lastTop - 1) pinned.value = false;
    else if (distance < 48) pinned.value = true;
    lastTop = el.scrollTop;
  }
  if (pinned.value) seen.value = props.snap.entries.length;
  if (selection.value) selection.value = null;
  if (!pinned.value || savedScroll(positionKey.value) !== null) {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(saveReadingPosition, 200);
  }
}
/** Scrolling up by wheel, trackpad or keys stops following at once, before the scroll lands, so a
 * streamed update arriving meanwhile does not pull the view back to the end. */
function onWheel(event: WheelEvent) {
  if (event.deltaY < 0 && (scroller.value?.scrollTop ?? 0) > 0) pinned.value = false;
}
function onScrollKey(event: KeyboardEvent) {
  if (event.target instanceof HTMLElement && event.target.closest("input, textarea, [contenteditable]")) return;
  if (["ArrowUp", "PageUp", "Home"].includes(event.key) || (event.key === " " && event.shiftKey)) pinned.value = false;
}

/** Entries the reader has had in view; answers that arrived after scrolling up count as new. */
const seen = ref(0);
const unseen = computed(() => {
  if (pinned.value) return 0;
  return props.snap.entries.slice(seen.value).filter((e) => e.kind === "assistant" && e.text.trim() && !e.streaming).length;
});
watch(pinned, (now) => { if (!now) seen.value = Math.min(seen.value || props.snap.entries.length, props.snap.entries.length); });

/** Keep following the end when content grows or the view shrinks without new entries
 * (images and highlighting finishing, an approval card opening below). */
let resizeObserver: ResizeObserver | null = null;
watch(scroller, (el) => {
  resizeObserver?.disconnect();
  resizeObserver = null;
  if (!el) return;
  resizeObserver = new ResizeObserver(() => {
    if (restoreTop !== null) return applyRestore();
    if (pinned.value && !matches.value.length) el.scrollTop = el.scrollHeight;
  });
  resizeObserver.observe(el);
  if (el.firstElementChild) resizeObserver.observe(el.firstElementChild);
}, { flush: "post" });
onBeforeUnmount(() => resizeObserver?.disconnect());

/**
 * Older entries skip rendering off screen and start at an estimated height (`renderLazily`), so
 * one above the view can change height when it first comes near. Where the web view does not keep
 * the reader's place by itself (`overflow-anchor`), shift the scroll by the change.
 */
const nativeAnchoring = typeof CSS !== "undefined" && CSS.supports("overflow-anchor", "auto");
const heights = new WeakMap<Element, number>();
let watched = new WeakSet<Element>();
let lazyObserver: ResizeObserver | null = null;
function keepPlace(changes: ResizeObserverEntry[]) {
  const el = scroller.value;
  if (!el) return;
  const top = el.getBoundingClientRect().top;
  let shift = 0;
  for (const change of changes) {
    const height = change.borderBoxSize?.[0]?.blockSize ?? change.contentRect.height;
    const was = heights.get(change.target);
    heights.set(change.target, height);
    if (was !== undefined && !pinned.value && change.target.getBoundingClientRect().bottom <= top) shift += height - was;
  }
  if (shift) {
    el.scrollTop += shift;
    lastTop = el.scrollTop;
  }
}
function watchLazyRows() {
  const el = scroller.value;
  if (nativeAnchoring || !el) return;
  lazyObserver ??= new ResizeObserver(keepPlace);
  for (const row of el.querySelectorAll(".row.lazy")) {
    if (watched.has(row)) continue;
    watched.add(row);
    lazyObserver.observe(row);
  }
}
watch([() => props.snap.entries.length, scroller], () => void nextTick(watchLazyRows), { flush: "post" });
onBeforeUnmount(() => lazyObserver?.disconnect());

/** The first entry at least partly in view, and where its top sits relative to the view's top. */
function topAnchor(el: HTMLElement): ScrollAnchor | null {
  const top = el.getBoundingClientRect().top;
  const rows = el.querySelectorAll<HTMLElement>('.column > .row');
  let lo = 0, hi = rows.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (rows[mid].getBoundingClientRect().bottom <= top) lo = mid + 1; else hi = mid;
  }
  const row = rows[lo];
  return row ? { entry: Number(row.dataset.entry ?? row.dataset.run), offset: row.getBoundingClientRect().top - top } : null;
}
/** Scroll so `anchor`'s entry sits where it was; false when that entry is not drawn. */
function placeAnchor(el: HTMLElement, anchor: ScrollAnchor): boolean {
  const row = el.querySelector(`.row[data-entry="${anchor.entry}"], .row[data-run="${anchor.entry}"]`);
  if (!row) return false;
  el.scrollTop += row.getBoundingClientRect().top - el.getBoundingClientRect().top - anchor.offset;
  lastTop = el.scrollTop;
  return true;
}

function jumpToEnd() {
  const el = scroller.value;
  if (!el) return;
  pinned.value = true;
  el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
}

/** Index of the last entry that matches, or -1. */
function lastIndex(test: (entry: OpenSnapshot["entries"][number]) => boolean) {
  const entries = props.snap.entries;
  for (let i = entries.length - 1; i >= 0; i--) if (test(entries[i])) return i;
  return -1;
}
/** The last prompt can be undone (files included) once its turn has ended. */
const lastUser = computed(() => lastIndex((e) => e.kind === "user"));
const undoable = computed(() => props.snap.connection.state === "live" && canUndo(props.snap));
/** The last turn failed: its error row offers to send the prompt again. */
const lastError = computed(() => (canRetry(props.snap) ? lastIndex((e) => e.kind === "error") : -1));
/** The latest finished answer keeps its actions in view; earlier ones show them on hover. */
const lastAnswer = computed(() =>
  props.snap.turnRunning ? -1 : lastIndex((e) => e.kind === "assistant" && !!e.text.trim()),
);

/** The reply "Read aloud" was pressed on, while it is being read. */
const readingEntry = ref(-1);
watch(() => props.snap.voice.reading, (reading, was) => { if (was && !reading) readingEntry.value = -1; });
function toggleReading(index: number, text: string) {
  const stop = readingEntry.value === index && props.snap.voice.reading;
  readingEntry.value = stop ? -1 : index;
  void engine.readAloud(stop ? null : text);
}

/** A remembered position to put the reader back at once the returned-to session's entries show. */
const positionKey = computed(() => scrollKey(props.snap.workspace.root, props.snap.sessionId));
let saveTimer: ReturnType<typeof setTimeout> | undefined;
let restoreTop = transcriptHit.value ? null : savedScroll(positionKey.value);
let restoreAnchor = restoreTop === null ? null : savedAnchor(positionKey.value);
if (restoreTop !== null) pinned.value = false;
function savePosition(key: string | null) {
  // Flushed on session change, trailing scroll, unmount and window close.
  clearTimeout(saveTimer);
  const el = scroller.value;
  // Don't replace a valid anchor with measurements from a hidden view or a partial restore.
  if (!key || !el?.clientHeight || restoreTop !== null) return;
  rememberScroll(key, el.scrollTop, pinned.value, pinned.value ? null : topAnchor(el));
}
function saveReadingPosition() { savePosition(positionKey.value); }
function applyRestore() {
  const el = scroller.value;
  if (restoreTop === null || !el?.clientHeight || props.snap.historyLoading || !props.snap.entries.length) return;
  el.scrollTop = restoreTop;
  lastTop = el.scrollTop;
  restoreTop = null;
  // Entries above may be drawn at an estimated height, so the pixel offset alone can land
  // elsewhere: put the entry that was at the top back there, again once heights settle.
  const anchor = restoreAnchor;
  restoreAnchor = null;
  if (anchor && placeAnchor(el, anchor)) {
    const key = positionKey.value, version = revealVersion;
    requestAnimationFrame(() => requestAnimationFrame(() => { if (!pinned.value && key === positionKey.value && version === revealVersion && scroller.value === el) placeAnchor(el, anchor); }));
  }
}

// The ResizeObserver follows actual content growth (including image loads). Reading scrollHeight
// after each token flushes layout even when the current line hasn't changed height.
watch([() => props.snap.entries.length, scroller, () => props.snap.historyLoading], () => {
  if (restoreTop !== null) applyRestore();
}, { flush: 'post' });

watch(
  positionKey,
  (key, previous) => {
    // Before Vue patches the DOM for the next session, this still measures the old one.
    savePosition(previous);
    revealVersion++;
    lazyObserver?.disconnect();
    watched = new WeakSet();
    conversationFindOpen.value = false;
    query.value = "";
    activeMatch.value = -1;
    restoreTop = transcriptHit.value ? null : savedScroll(key);
    restoreAnchor = restoreTop === null ? null : savedAnchor(key);
    revealing.value = -1;
    pinned.value = restoreTop === null;
    seen.value = props.snap.entries.length;
    selection.value = null;
    void nextTick(() => {
      if (key !== positionKey.value) return;
      if (restoreTop !== null) applyRestore();
      else if (pinned.value && scroller.value) scroller.value.scrollTop = scroller.value.scrollHeight;
    });
  },
);

// A tool call ends a streaming row, not the reader's choice to see its reasoning.
const thoughtOverrides = ref(new Map<number, boolean>());
watch([positionKey, () => props.snap.showThinking], () => thoughtOverrides.value.clear());
function reasoningOpen(index: number) {
  if (current.value?.entry === index && current.value.field === 'reasoning') return true;
  return thoughtOverrides.value.get(index) ?? (props.snap.showThinking !== false);
}
function toggleThought(index: number) {
  thoughtOverrides.value.set(index, !reasoningOpen(index));
}

/** Words of reasoning, for the "Thought" label. */
function words(text: string) {
  const n = text.trim().split(/\s+/).filter(Boolean).length;
  return n >= 1000 ? `${(n / 1000).toFixed(1)}k words` : n === 1 ? "1 word" : `${n} words`;
}

const runs = computed(() => toolRuns(grouping.value, props.snap.sessionId));
/** Entries drawn by their run's line rather than as rows of their own. */
const inRun = computed(() => {
  const out = new Set<number>();
  for (const run of runs.value.values()) for (let i = run.start; i <= run.end; i++) out.add(i);
  return out;
});
/** Where a row of a run keeps its own open state; status lines have none. */
function rowKey({ index, entry }: Run['items'][number]) {
  return entry.kind === 'tool' ? entry.key : entry.kind === 'received' ? entryKey(props.snap.sessionId, 'received', index) : '';
}
/** A run opens when it was opened, one of its rows is expanded, or a search lands in it. */
function folded(run: Run) {
  if (ui.expanded.has(run.key) || run.items.some((item) => ui.expanded.has(rowKey(item)))) return false;
  const inside = (i: number) => i >= run.start && i <= run.end;
  return !inside(transcriptMatch.value) && !(current.value && inside(current.value.entry));
}
function setOpen(run: Run, open: boolean) {
  if (open) ui.expanded.add(run.key);
  else {
    ui.expanded.delete(run.key);
    for (const item of run.items) ui.expanded.delete(rowKey(item));
  }
}
/** ⌥-click opens or closes every run in the conversation, as in Finder's outline views. */
function toggleFold(run: Run, event: MouseEvent) {
  const open = folded(run);
  if (!event.altKey) return setOpen(run, open);
  for (const each of runs.value.values()) if (each.items.length > 1) setOpen(each, open);
}
/** The entry a search highlights, or -1. */
const highlighted = computed(() => current.value?.entry ?? transcriptMatch.value);
/** Entries a search or jump is taking the reader to: drawn fully even when old. */
const targets = computed(() => [highlighted.value, revealing.value]);
/** Older entries render only near the view (see `renderLazily`). */
function lazy(index: number) {
  return renderLazily(index, props.snap.entries.length, targets.value);
}
function lazyRun(run: Run) {
  return lazy(run.end) && !targets.value.some((t) => t >= run.start && t <= run.end);
}
/** "Regenerate" on the last answer: why not, or `""` when it can. */
const regenerateNote = computed(() => regenerateBlocker(props.snap));
</script>

<template>
  <p v-if="empty && snap.historyLoading" class="history-loading" role="status">Loading conversation…</p>
  <EmptyState v-else-if="empty" :snap="snap" />
  <div v-if="conversationFindOpen && !empty" class="find-bar" @keydown.esc.stop.prevent="closeFind">
    <Icon name="search" :size="15" />
    <input
      ref="searchInput" v-model="query" placeholder="Find in conversation" aria-label="Find in conversation"
      @keydown.enter.prevent="stepMatch($event.shiftKey ? -1 : 1)"
    />
    <span class="match-count" role="status">{{ query.trim() ? (matches.length ? `${matchNumber} of ${matches.length}` : 'No matches') : '' }}</span>
    <button aria-label="Previous occurrence" title="Previous occurrence" :disabled="!matches.length" @click="stepMatch(-1)"><Icon name="arrowUp" :size="14" /></button>
    <button aria-label="Next occurrence" title="Next occurrence" :disabled="!matches.length" @click="stepMatch(1)"><Icon name="arrowDown" :size="14" /></button>
    <button aria-label="Close conversation search" title="Close conversation search" @click="closeFind"><Icon name="cross" :size="14" /></button>
  </div>
  <div v-if="transcriptHit && !conversationFindOpen && (transcriptMatch < 0 || candidates.length > 1)" class="search-preview" role="status">
    <Icon name="search" :size="14" />
    <span class="truncate"><strong>{{ candidates.length > 1 ? `${candidates.length} identical messages` : 'Saved search result' }}</strong> · {{ transcriptHit.snippet }}</span>
    <button v-if="candidates.length > 1" class="btn btn-secondary" @click="nextCandidate">{{ transcriptMatch < 0 ? 'Show first' : `Next (${candidates.indexOf(transcriptMatch) + 1}/${candidates.length})` }}</button>
    <button class="btn btn-secondary" @click="ui.transcriptJump = null">Dismiss</button>
  </div>
  <div v-if="!empty" ref="scroller" class="scroller" tabindex="-1" @scroll.passive="onScroll" @wheel.passive="onWheel" @keydown="onScrollKey" @mouseup="onSelect" @keyup.shift="onSelect">
    <div class="column">
      <template v-for="(entry, index) in snap.entries" :key="entry.kind === 'tool' ? entry.key : index">
        <div v-if="runs.get(index)" class="row k-run" :data-run="index" :class="{ lazy: lazyRun(runs.get(index)!) }">
          <ToolRun :run="runs.get(index)!" :open="!folded(runs.get(index)!)" :session="snap.sessionId" :match="highlighted" :occurrence="current" @toggle="toggleFold(runs.get(index)!, $event)" />
        </div>
        <div v-else-if="!inRun.has(index)" class="row" :class="[`k-${entry.kind}`, { 'current-match': index === highlighted, lazy: lazy(index) }]" :data-entry="index">
          <div v-if="entry.kind === 'user'" class="user">
            <div class="bubble">
              <div class="user-text" :class="{ clamped: highlighted !== index && isLongPrompt(entry.text) && !ui.expanded.has(entryKey(snap.sessionId, 'prompt', index)) }"><FindText v-if="current?.entry === index" :text="entry.text" :match="current" /><template v-else>{{ entry.text }}</template></div>
              <button v-if="highlighted !== index && isLongPrompt(entry.text)" class="more" :aria-expanded="ui.expanded.has(entryKey(snap.sessionId, 'prompt', index))" @click="toggle(entryKey(snap.sessionId, 'prompt', index))">
                {{ ui.expanded.has(entryKey(snap.sessionId, 'prompt', index)) ? "Show less" : `Show all ${entry.text.split("\n").length} lines` }}
              </button>
              <div v-if="entry.images" class="images">
                <Icon name="image" :size="13" />
                {{ entry.images === 1 ? "1 image" : `${entry.images} images` }}
                <button v-for="(image, n) in sentImagesFor(entry)" :key="n" class="sent-image" :title="`Enlarge ${image.name}`" @click="viewAttachment(image)"><img :src="image.url" :alt="image.name" loading="lazy" /></button>
              </div>
            </div>
            <div class="actions" :class="{ shown: index === lastUser && !snap.turnRunning }">
              <button class="act" title="Copy" aria-label="Copy prompt" @click="copyText(entry.text)">
                <Icon name="copy" :size="14" />
              </button>
              <button v-if="snap.sessionId && !snap.turnRunning && snap.connection.state === 'live'" class="act" title="Fork conversation before this prompt (files unchanged)" aria-label="Fork conversation before this prompt" @click="forkAt(index, entry.text)"><Icon name="duplicate" :size="14" /></button>
              <template v-if="index === lastUser && undoable">
                <button class="act" title="Edit: undo this turn and put the prompt back in the box" aria-label="Edit prompt" @click="editLastPrompt(snap)">
                  <Icon name="pencil" :size="14" />
                </button>
                <button class="act" title="Undo this turn: restore its files and remove it" aria-label="Undo this turn" @click="engine.undoTurn()">
                  <Icon name="undo" :size="14" />
                </button>
              </template>
              <button
                v-else-if="snap.sessionId && !snap.turnRunning && snap.connection.state === 'live'"
                class="act"
                title="Edit from here: copy the conversation before this prompt and put the prompt in the box (files unchanged)"
                aria-label="Edit from here in a copy"
                @click="editFromHere(snap, index)"
              >
                <Icon name="pencil" :size="14" />
              </button>
            </div>
          </div>

          <template v-else-if="entry.kind === 'assistant'">
            <div v-if="entry.reasoning" class="reasoning">
              <button class="thinking" :aria-expanded="reasoningOpen(index)" @click="toggleThought(index)">
                <span>{{ entry.streaming && !entry.text ? "Thinking…" : `Thought · ${words(entry.reasoning)}` }}</span>
                <Icon
                  :name="reasoningOpen(index) ? 'chevronDown' : 'chevronRight'"
                  :size="12"
                />
              </button>
              <div v-if="reasoningOpen(index)" class="thought">
                <FindText v-if="current?.entry === index && current.field === 'reasoning'" :text="entry.reasoning" :match="current" /><template v-else>{{ entry.reasoning }}</template>
              </div>
            </div>
            <div v-if="entry.abandoned" class="aside">Discarded attempt</div>
            <div v-if="current?.entry === index && current.field === 'text'" class="search-source"><small>Markdown source · Find match</small><FindText :text="entry.text" :match="current" /></div>
            <Markdown v-show="current?.entry !== index || current.field !== 'text'" :class="{ abandoned: entry.abandoned }" :text="entry.text" :streaming="entry.streaming" :workspace-root="snap.workspace.root" />
            <div
              v-if="!entry.streaming && entry.text.trim()"
              class="actions"
              :class="{ shown: index === lastAnswer }"
            >
              <button
                class="act save"
                title="Save to a type…"
                @click="saveMenu($event, pendingSave(entry.text, [], snap.workspace.root, snap.sessionId), $event.currentTarget as HTMLElement)"
              >
                <Icon name="bookmark" :size="14" /> Save
              </button>
              <button class="act" title="Quote in your reply" aria-label="Quote in your reply" @click="quote(entry.text)">
                <Icon name="quote" :size="14" />
              </button>
              <button class="act" title="Copy answer as Markdown" aria-label="Copy answer as Markdown" @click="copyText(entry.text)">
                <Icon name="copy" :size="14" />
              </button>
              <button
                v-if="index === lastAnswer && index > lastUser && undoable"
                class="act"
                :class="{ off: !!regenerateNote }"
                :title="regenerateNote || 'Regenerate: undo this turn (files included) and send the same prompt again'"
                aria-label="Regenerate answer"
                :aria-disabled="!!regenerateNote"
                @click="regenerateLast(snap)"
              >
                <Icon name="refresh" :size="14" />
              </button>
              <button
                v-if="snap.connection.state === 'live'"
                class="act"
                :title="readingEntry === index && snap.voice.reading ? 'Stop reading' : 'Read aloud'"
                :aria-label="readingEntry === index && snap.voice.reading ? 'Stop reading' : 'Read aloud'"
                @click="toggleReading(index, entry.text)"
              >
                <Icon :name="readingEntry === index && snap.voice.reading ? 'volumeOff' : 'volume'" :size="14" />
              </button>
            </div>
          </template>

          <div v-else-if="entry.kind === 'notice' && entry.noticeKind === 'steer'" class="user" aria-label="Your mid-turn note">
            <div class="bubble user-text"><FindText v-if="current?.entry === index" :text="entry.message" :match="current" /><template v-else>{{ entry.message }}</template></div>
          </div>
          <div v-else-if="entry.kind === 'notice' && entry.lane === 'alert'" class="alert">
            <FindText v-if="current?.entry === index" :text="entry.message" :match="current" /><template v-else>{{ entry.message }}</template>
          </div>
          <div v-else-if="entry.kind === 'notice'" class="divider">
            <FindText v-if="current?.entry === index" :text="entry.message" :match="current" /><span v-else>{{ entry.message }}</span>
          </div>

          <div v-else-if="entry.kind === 'error'" class="error">
            <span><strong>Error:</strong> <FindText v-if="current?.entry === index" :text="entry.message" :match="current" /><template v-else>{{ entry.message }}</template></span>
            <button class="act copy-error" title="Copy error" aria-label="Copy error" @click="copyText(entry.message, 'Error copied')">
              <Icon name="copy" :size="14" />
            </button>
            <button v-if="index === lastError" class="btn btn-secondary retry" @click="engine.retryTurn()">
              <Icon name="refresh" :size="13" /> Retry
            </button>
          </div>
        </div>
        <details v-for="review in reviews.get(index)" :key="review.start" class="turn-review">
          <summary>
            <Icon class="review-chevron" name="chevronRight" :size="12" />
            <span>Tool receipts</span>
            <span class="review-counts">{{ review.files.length }} files · {{ review.checks.length }} verification calls</span>
          </summary>
          <section class="review-section">
            <h4>Verification calls</h4>
            <p v-if="!review.checks.length">No explicit verification calls in this transcript.</p>
            <button v-for="call in review.checks" :key="call.tool.key" class="check-row" title="Show recorded output" @click="evidence(call.index, call.tool.key)">
              <span class="check-target">{{ call.tool.target || call.tool.label }}</span>
              <span class="check-state" :class="call.tool.state">{{ call.tool.state === 'running' ? 'unknown' : call.tool.state }}</span>
              <Icon name="chevronRight" :size="12" />
            </button>
          </section>
          <section v-if="review.files.length" class="review-section"><h4>Files</h4>
          <details v-for="file in review.files" :key="file.path">
            <summary :title="file.path"><Icon class="review-chevron" name="chevronRight" :size="12" />{{ file.path.startsWith(snap.workspace.root + '/') ? file.path.slice(snap.workspace.root.length + 1) : file.path }}</summary>
            <div v-for="call in file.calls" :key="call.tool.key">
              <CodeBlock v-if="call.tool.output?.caption === 'diff' && call.tool.changedPaths.length === 1" caption="Recorded change · this turn" :text="call.tool.output.text" lang="diff" :max-height="280" />
              <p v-else>No separate recorded patch for this file.</p>
              <button class="btn btn-secondary" @click="evidence(call.index, call.tool.key)">Show tool receipt · {{ call.tool.state }}</button>
            </div>
            <button v-if="recorded" class="btn btn-secondary" @click="recorded.chooseCheckpoint(file.path)">Review retained checkpoint…</button>
            <button class="btn btn-secondary" @click="workingTree(file.path)">Compare current working tree</button>
          </details>
          </section>
        </details>
      </template>

      <RecordedTurns ref="recorded" v-if="snap.connection.state === 'live' && snap.sessionId && !snap.turnRunning && !snap.historyLoading" :key="JSON.stringify([snap.workspace.root, snap.sessionId])" :root="snap.workspace.root" :session="snap.sessionId" />
      <WorkingIndicator v-if="progress" :key="JSON.stringify([snap.workspace.root, snap.sessionId])" :label="progressLabel" :detail="progress.detail" :waiting="progress.waiting" />
      <div v-if="!pinned && !matches.length" class="jump">
        <button :class="{ wide: unseen }" :title="unseen ? 'Jump to the new replies' : 'Jump to the latest message'" aria-label="Jump to the latest message" @click="jumpToEnd">
          <Icon name="arrowDown" :size="16" />
          <span v-if="unseen">{{ unseen === 1 ? "1 new reply" : `${unseen} new replies` }}</span>
        </button>
      </div>
    </div>
  </div>
  <Teleport to="body">
    <button
      v-if="selection"
      class="quote-pop"
      :style="{ left: `${selection.x}px`, top: `${selection.y}px` }"
      title="Quote the selection in your reply"
      @mousedown.prevent
      @click="quote(selection.text)"
    >
      <Icon name="quote" :size="13" /> Quote
    </button>
  </Teleport>
</template>

<style scoped>
.search-source { white-space: pre-wrap; overflow-wrap: anywhere; user-select: text; }
.search-source small { display: block; color: var(--text-muted); }
.images { flex-wrap: wrap; }
.sent-image img { display: block; max-width: 90px; max-height: 66px; border-radius: var(--radius-sm); object-fit: contain; }
.history-loading {
  margin: auto;
  color: var(--text-muted);
  font-size: 13px;
}
.turn-review {
  margin: 12px 0 4px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--glass-soft);
  color: var(--text-muted);
  font-size: var(--text-sm);
}
.review-section { padding: 8px 12px; border-top: 1px solid var(--border); min-width: 0; }
.review-section h4 { margin: 0 0 4px; font-size: inherit; font-weight: 600; }
.review-section p { margin: 4px 0; }
.review-counts { margin-left: auto; color: var(--text-muted); }
.check-row { display: flex; width: 100%; align-items: baseline; gap: 8px; text-align: left; padding: 5px 0; }
.check-target { flex: 1; min-width: 0; overflow-wrap: anywhere; font-family: var(--font-mono); }
.check-state.succeeded { color: var(--success); } .check-state.failed { color: var(--danger); }
.turn-review summary {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 12px;
  cursor: pointer;
  list-style: none;
  border-radius: inherit;
  overflow-wrap: anywhere;
}
.turn-review summary::-webkit-details-marker { display: none; }
.turn-review summary:hover { background: var(--hover); color: var(--text); }
.review-chevron { flex: none; color: var(--text-faint); transition: transform 140ms var(--ease); }
details[open] > summary > .review-chevron { transform: rotate(90deg); }
.review-section summary { padding: 6px 0; font-family: var(--font-mono); }
.review-section .btn { margin: 4px 0; max-width: 100%; height: auto; min-height: 28px; white-space: normal; }
.check-row:hover { background: var(--hover); }
.review-section details > div { padding-inline-start: 14px; }
.search-preview {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0 0 16px;
  padding: 6px 6px 6px 12px;
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--accent) 10%, transparent);
  box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 30%, transparent);
  color: var(--text-muted);
  font-size: var(--text-sm);
}
.search-preview span { flex: 1; }
.search-preview strong { color: var(--text); font-weight: 600; }
.find-bar {
  flex: none;
  display: flex;
  align-items: center;
  gap: var(--space-sm);
  margin: 0 var(--space-xl);
  padding: 4px 8px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-md);
  background: var(--glass);
  color: var(--text-muted);
}
.find-bar input {
  flex: 1;
  min-width: 0;
  height: 30px;
  color: var(--text);
  background: transparent;
  border: 0;
  font: inherit;
}
.match-count { font-size: 12px; white-space: nowrap; }
.find-bar button { display: grid; place-items: center; width: 28px; height: 28px; border-radius: var(--radius-sm); }
.find-bar button:hover:not(:disabled) { background: var(--hover); }
.find-bar button:disabled { opacity: 0.4; }
.row.current-match {
  outline: 1px solid var(--accent);
  outline-offset: 5px;
  border-radius: var(--radius-sm);
}
.scroller {
  flex: 1;
  overflow-y: auto;
  scrollbar-gutter: stable both-edges;
}
/* Focused only from code (closing find), to keep arrow-key scrolling; no ring needed. */
.scroller:focus { outline: none; }
.column {
  max-width: var(--content);
  margin: 0 auto;
  padding: var(--space-lg) var(--space-xl) var(--space-xxl);
}
.row {
  margin-top: 18px;
}
/* Older entries render only near the view; `auto` keeps the height they had once seen, and the
   newest ones (where streaming, scroll-to-end and editing happen) always render. */
.row.lazy {
  content-visibility: auto;
  contain-intrinsic-size: auto 120px;
  /* Skipped rows clip their paint; leave room for the action buttons that hang out to the left. */
  margin-inline: -8px;
  padding-inline: 8px;
}
.row:first-child {
  margin-top: 0;
}
.row.k-run {
  margin-top: 14px;
}
.row.k-run:first-child {
  margin-top: 0;
}
.row.k-user {
  display: flex;
  justify-content: flex-end;
  margin-top: 28px;
}
.row.k-user:first-child {
  margin-top: 0;
}
.user {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  max-width: 80%;
}
.k-notice > .user { margin-inline-start: auto; }
.bubble {
  max-width: 100%;
  padding: 10px 16px;
  border-radius: var(--radius-lg) var(--radius-lg) 6px var(--radius-lg);
  background: var(--bubble);
  box-shadow: inset 0 0 0 1px var(--border);
  user-select: text;
  -webkit-user-select: text;
  cursor: text;
}
.actions {
  display: flex;
  gap: 2px;
  margin-top: 4px;
  opacity: 0;
  transition: opacity 120ms var(--ease);
}
.row:hover .actions,
.row:focus-within .actions,
.actions:focus-within,
.actions.shown {
  opacity: 1;
}
.k-assistant .actions {
  margin-left: -6px;
}
.act {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  border-radius: var(--radius-sm);
  color: var(--text-faint);
  transition:
    background 120ms var(--ease),
    color 120ms var(--ease);
}
.act.save {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  width: auto;
  padding: 0 8px;
  font-size: var(--text-sm);
}
.act:hover {
  background: var(--hover);
  color: var(--text);
}
.act.off {
  opacity: 0.45;
}
.error .copy-error {
  flex: none;
  width: 26px;
  height: 26px;
  color: inherit;
  opacity: 0.7;
  user-select: none;
  -webkit-user-select: none;
}
.error .copy-error:hover {
  opacity: 1;
}
/* The round button keeps to the right edge of the column, low in the gap above the composer,
   so it covers the ends of at most a line or two instead of the middle of what's being read.
   The "new replies" pill stays centred: it's a notice worth seeing. */
.jump {
  position: sticky;
  bottom: 8px;
  display: flex;
  justify-content: flex-end;
  height: 0;
  overflow: visible;
  pointer-events: none;
}
.jump:has(.wide) {
  justify-content: center;
}
.jump button {
  display: grid;
  place-items: center;
  width: 32px;
  height: 32px;
  margin-top: -40px;
  border-radius: 50%;
  background: var(--raised);
  color: var(--text);
  box-shadow: var(--shadow-pop), inset 0 0 0 1px var(--border);
  pointer-events: auto;
  animation: rise 160ms var(--ease) both;
  transition: transform 120ms var(--ease), background 120ms var(--ease);
}
.jump button.wide {
  display: inline-flex;
  gap: 6px;
  width: auto;
  padding: 0 14px 0 10px;
  border-radius: 999px;
  font-size: var(--text-sm);
}
.quote-pop {
  position: fixed;
  z-index: 60;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 4px 10px;
  translate: -50% calc(-100% - 8px);
  border-radius: 999px;
  background: var(--raised);
  box-shadow: var(--shadow-pop);
  color: var(--text);
  font-size: var(--text-sm);
  animation: rise 120ms var(--ease) both;
}
.quote-pop:hover { background: var(--hover); }
.user-text.clamped {
  max-height: calc(1.5em * 10);
  overflow: hidden;
  -webkit-mask-image: linear-gradient(to bottom, #000 70%, transparent);
  mask-image: linear-gradient(to bottom, #000 70%, transparent);
}
.more {
  margin-top: 4px;
  font-size: var(--text-sm);
  color: var(--text-muted);
}
.more:hover { color: var(--text); text-decoration: underline; }
.row.k-user.flash .bubble { animation: flash 900ms var(--ease); }
@keyframes flash {
  0%, 40% { box-shadow: inset 0 0 0 1px var(--accent), 0 0 0 4px color-mix(in srgb, var(--accent) 20%, transparent); }
  100% { box-shadow: inset 0 0 0 1px var(--border); }
}
.jump button:hover {
  background: color-mix(in srgb, var(--raised) 88%, var(--text));
  transform: translateY(-1px);
}
.user-text {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  font-size: 14.5px;
}
.images {
  display: flex;
  align-items: center;
  gap: 5px;
  margin-top: 4px;
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.aside {
  margin-bottom: 4px;
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.reasoning {
  margin-bottom: 8px;
}
.thinking {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 13px;
  color: var(--text-faint);
  transition: color 120ms var(--ease);
}
.thinking:hover {
  color: var(--text-muted);
}
.thought {
  margin: 6px 0 4px;
  padding-left: 14px;
  border-left: 1px solid var(--border-strong);
  font-size: 13.5px;
  line-height: 1.6;
  color: var(--text-faint);
  white-space: pre-wrap;
  user-select: text;
  -webkit-user-select: text;
}
.abandoned {
  color: var(--text-faint);
}
.alert {
  padding: 8px 12px;
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--warning) 8%, transparent);
  color: var(--warning);
  font-size: 13.5px;
  user-select: text;
  -webkit-user-select: text;
}
.divider {
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.divider::before,
.divider::after {
  content: "";
  flex: 1;
  border-top: 1px solid var(--border);
}
.error {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 8px 12px;
  border-radius: var(--radius-md);
  background: color-mix(in srgb, var(--danger) 9%, transparent);
  color: var(--danger);
  font-size: 13.5px;
  white-space: pre-wrap;
  user-select: text;
  -webkit-user-select: text;
}
.error span {
  flex: 1;
  min-width: 0;
  overflow-wrap: anywhere;
  padding-top: 2px;
}
.error .retry {
  flex: none;
  gap: 6px;
  user-select: none;
  -webkit-user-select: none;
}
</style>
