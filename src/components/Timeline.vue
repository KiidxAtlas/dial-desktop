<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { conversationFindOpen } from "../commands";
import { focusComposer, toggle, ui } from "../ui";
import { engine } from "../bridge";
import { canRetry, canUndo, copyText, editLastPrompt } from "../sessionActions";
import { draftFor } from "../drafts";
import { isLongPrompt, withQuote } from "../quote";
import type { OpenSnapshot } from "../types";
import Markdown from "./Markdown.vue";
import ToolRow from "./ToolRow.vue";
import EmptyState from "./EmptyState.vue";
import Icon from "./Icon.vue";
import { pendingSave, saveMenu, withoutTypeTag } from "../savedTypes";
import WorkingIndicator from "./WorkingIndicator.vue";
import { turnReviews } from "../turnReview";
import CodeBlock from "./CodeBlock.vue";
import GeneratedImage from "./GeneratedImage.vue";
import { showPanel } from "../workspace";
import { runSummary, toolRuns, type ToolRun } from "../toolFolds";

const props = defineProps<{ snap: OpenSnapshot }>();
const reviews = computed(() => turnReviews(props.snap.entries, props.snap.turnRunning));
function forkAt(index: number, prompt: string) {
  const snap = props.snap;
  if (!snap.sessionId) return;
  let occurrence = 0, matches = 0;
  for (const [i, entry] of snap.entries.entries()) {
    if (entry.kind !== 'user' || entry.text !== prompt) continue;
    matches++;
    if (i === index) occurrence = matches;
  }
  void engine.forkBefore(snap.workspace.root, snap.sessionId, prompt, occurrence, matches);
}
function evidence(index: number, key: string) {
  pinned.value = false;
  ui.expanded.add(key);
  nextTick(() => scroller.value?.querySelector(`[data-entry="${index}"]`)?.scrollIntoView({ block: "center" }));
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

watch([transcriptHit, () => props.snap.entries], ([hit, entries], old) => {
  if (!hit || hit !== old?.[0]) transcriptMatch.value = -1;
  if (!hit || transcriptMatch.value >= 0 || !hit.preview.trim()) return;
  const needle = hit.preview.trim();
  const matches = entries.flatMap((entry, index) =>
    ((entry.kind === "user" || entry.kind === "assistant") && entry.kind === hit.role && entry.text.trim() === needle) ? [index] : [],
  );
  if (matches.length !== 1) return;
  transcriptMatch.value = matches[0];
  pinned.value = false;
  nextTick(() => scroller.value?.querySelector(`[data-entry="${matches[0]}"]`)?.scrollIntoView({ block: "center" }));
}, { flush: "post", immediate: true });
const matches = computed(() => {
  const term = query.value.trim().toLowerCase();
  if (!conversationFindOpen.value || !term) return [];
  return props.snap.entries.flatMap((entry, index) => {
    const text = entry.kind === "tool" ? `${entry.label} ${entry.target}`
      : entry.kind === "notice" || entry.kind === "error" ? entry.message
      : entry.text;
    return text.toLowerCase().includes(term) ? [index] : [];
  });
});
const matchNumber = computed(() => matches.value.indexOf(activeMatch.value) + 1);

function showMatch(index: number) {
  activeMatch.value = index;
  if (index < 0) return;
  pinned.value = false;
  scroller.value?.querySelector(`[data-entry="${index}"]`)?.scrollIntoView({ block: "center" });
}

function stepMatch(step: number) {
  const list = matches.value;
  if (list.length) showMatch(list[(matchNumber.value - 1 + step + list.length) % list.length]);
}

function closeFind() {
  conversationFindOpen.value = false;
  if (searchInput.value?.parentElement?.contains(document.activeElement)) scroller.value?.focus({ preventScroll: true });
  onScroll();
}

function findKey(event: KeyboardEvent) {
  if (event.defaultPrevented || event.isComposing || ui.settingsOpen || ui.menu || ui.paletteOpen || ui.shortcutsOpen || ui.modelPickerOpen) return;
  if (event.target instanceof Element && event.target.closest(".xterm")) return;
  if ((event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === "f" && !empty.value) {
    event.preventDefault();
    conversationFindOpen.value = true;
    nextTick(() => { searchInput.value?.focus(); searchInput.value?.select(); });
  } else if ((event.metaKey || event.ctrlKey) && event.altKey && !event.shiftKey && (event.key === "ArrowUp" || event.key === "ArrowDown") && !empty.value) {
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
    if (!sel || sel.isCollapsed || !text.trim() || !node?.closest(".row.k-user, .row.k-assistant") || !scroller.value?.contains(node)) {
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
});
function onSelectionChange() {
  if (selection.value && window.getSelection()?.isCollapsed) selection.value = null;
}
onBeforeUnmount(() => {
  window.removeEventListener("keydown", findKey);
  document.removeEventListener("selectionchange", onSelectionChange);
  conversationFindOpen.value = false;
});
watch(searchInput, (el) => el?.focus());
watch(query, async () => {
  await nextTick();
  showMatch(matches.value[0] ?? -1);
});
watch(matches, (list) => {
  if (!list.includes(activeMatch.value)) showMatch(list[0] ?? -1);
}, { flush: "post" });

const empty = computed(() => props.snap.entries.length === 0 && !props.snap.turnRunning);
const streaming = computed(() => {
  const last = props.snap.entries.at(-1);
  return last?.kind === "assistant" && last.streaming && last.text.length > 0;
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
    if (pinned.value && !matches.value.length) el.scrollTop = el.scrollHeight;
  });
  resizeObserver.observe(el);
  if (el.firstElementChild) resizeObserver.observe(el.firstElementChild);
}, { flush: "post" });
onBeforeUnmount(() => resizeObserver?.disconnect());

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

watch(
  () => props.snap.entries,
  async () => {
    if (!pinned.value) return;
    await nextTick();
    const el = scroller.value;
    if (el && pinned.value && !matches.value.length) el.scrollTop = el.scrollHeight;
  },
  { flush: "post" },
);

watch(
  () => [props.snap.workspace.root, props.snap.sessionId],
  ([root, id], [wasRoot, wasId]) => {
    if (root === wasRoot && id === wasId) return;
    conversationFindOpen.value = false;
    query.value = "";
    activeMatch.value = -1;
    pinned.value = true;
    seen.value = props.snap.entries.length;
    selection.value = null;
  },
);

function reasoningOpen(index: number, streamingAnswer: boolean, text: string) {
  return ui.expanded.has(`reasoning:${index}`) || (streamingAnswer && !text);
}

/** Words of reasoning, for the "Thought" label. */
function words(text: string) {
  const n = text.trim().split(/\s+/).filter(Boolean).length;
  return n >= 1000 ? `${(n / 1000).toFixed(1)}k words` : n === 1 ? "1 word" : `${n} words`;
}

/** Consecutive tool calls sit close together. */
function gap(index: number) {
  const entries = props.snap.entries;
  if (index === 0) return "";
  return entries[index].kind === "tool" && entries[index - 1].kind === "tool" ? "tight" : "";
}

const runs = computed(() => toolRuns(props.snap.entries, props.snap.turnRunning));
/** A run stays open while it was opened, one of its calls is expanded, or a search lands in it. */
function folded(run: ToolRun) {
  if (ui.expanded.has(run.key) || run.tools.some((t) => ui.expanded.has(t.key))) return false;
  const inside = (i: number) => i >= run.start && i <= run.end;
  return !inside(transcriptMatch.value) && !(matches.value.length && inside(activeMatch.value));
}
/** Entries hidden inside a folded run. */
const hidden = computed(() => {
  const out = new Set<number>();
  for (const run of runs.value.values()) if (folded(run)) for (let i = run.start; i <= run.end; i++) out.add(i);
  return out;
});
function toggleFold(run: ToolRun) {
  if (folded(run)) ui.expanded.add(run.key);
  else {
    ui.expanded.delete(run.key);
    for (const t of run.tools) ui.expanded.delete(t.key);
  }
}
function failedIn(run: ToolRun) {
  return run.tools.some((t) => t.state === "failed");
}
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
    <button aria-label="Previous matching message" title="Previous matching message" :disabled="!matches.length" @click="stepMatch(-1)"><Icon name="arrowUp" :size="14" /></button>
    <button aria-label="Next matching message" title="Next matching message" :disabled="!matches.length" @click="stepMatch(1)"><Icon name="arrowDown" :size="14" /></button>
    <button aria-label="Close conversation search" title="Close conversation search" @click="closeFind"><Icon name="cross" :size="14" /></button>
  </div>
  <div v-if="!empty" ref="scroller" class="scroller" tabindex="-1" @scroll.passive="onScroll" @wheel.passive="onWheel" @keydown="onScrollKey" @mouseup="onSelect" @keyup.shift="onSelect">
    <div class="column">
      <p v-if="transcriptHit && transcriptMatch < 0" class="search-preview" role="status">
        <Icon name="search" :size="14" />
        <span class="truncate"><strong>Search result</strong> · {{ transcriptHit.snippet }}</span>
        <button class="btn btn-secondary" @click="ui.transcriptJump = null">Dismiss</button>
      </p>
      <template v-for="(entry, index) in snap.entries" :key="entry.kind === 'tool' ? entry.key : index">
        <button
          v-if="runs.get(index)"
          class="fold"
          :class="{ open: !hidden.has(index), failed: failedIn(runs.get(index)!) }"
          :aria-expanded="!hidden.has(index)"
          :title="hidden.has(index) ? 'Show every tool call in this run' : 'Fold these tool calls into one line'"
          @click="toggleFold(runs.get(index)!)"
        >
          <span class="fold-icon"><Icon :name="hidden.has(index) ? 'chevronRight' : 'chevronDown'" :size="12" /></span>
          <span class="truncate">{{ runSummary(runs.get(index)!) }}</span>
        </button>
        <div v-if="runs.get(index) && hidden.has(index) && runs.get(index)!.tools.some((t) => t.images.length)" class="fold-images">
          <template v-for="t in runs.get(index)!.tools" :key="t.key">
            <GeneratedImage v-for="path in t.images" :key="path" :path="path" />
          </template>
        </div>
        <div v-if="!hidden.has(index)" class="row" :class="[`k-${entry.kind}`, gap(index), { 'current-match': index === transcriptMatch || (matches.length && index === activeMatch) }]" :data-entry="index">
          <div v-if="entry.kind === 'user'" class="user">
            <div class="bubble">
              <div class="user-text" :class="{ clamped: isLongPrompt(entry.text) && !ui.expanded.has(`prompt:${index}`) }">{{ withoutTypeTag(entry.text) }}</div>
              <button v-if="isLongPrompt(entry.text)" class="more" @click="toggle(`prompt:${index}`)">
                {{ ui.expanded.has(`prompt:${index}`) ? "Show less" : `Show all ${entry.text.split("\n").length} lines` }}
              </button>
              <div v-if="entry.images" class="images">
                <Icon name="image" :size="13" />
                {{ entry.images === 1 ? "1 image" : `${entry.images} images` }}
              </div>
            </div>
            <div class="actions">
              <button class="act" title="Copy" @click="copyText(withoutTypeTag(entry.text))">
                <Icon name="copy" :size="14" />
              </button>
              <button v-if="snap.sessionId && !snap.turnRunning && snap.connection.state === 'live'" class="act" title="Fork conversation before this prompt (files unchanged)" @click="forkAt(index, entry.text)"><Icon name="duplicate" :size="14" /></button>
              <template v-if="index === lastUser && undoable">
                <button class="act" title="Edit: undo this turn and put the prompt back in the box" @click="editLastPrompt(snap)">
                  <Icon name="pencil" :size="14" />
                </button>
                <button class="act" title="Undo this turn: restore its files and remove it" @click="engine.undoTurn()">
                  <Icon name="undo" :size="14" />
                </button>
              </template>
            </div>
          </div>

          <template v-else-if="entry.kind === 'assistant'">
            <div v-if="entry.reasoning" class="reasoning">
              <button class="thinking" @click="toggle(`reasoning:${index}`)">
                <span>{{ entry.streaming && !entry.text ? "Thinking…" : `Thought · ${words(entry.reasoning)}` }}</span>
                <Icon
                  :name="reasoningOpen(index, entry.streaming, entry.text) ? 'chevronDown' : 'chevronRight'"
                  :size="12"
                />
              </button>
              <div v-if="reasoningOpen(index, entry.streaming, entry.text)" class="thought">
                {{ entry.reasoning }}
              </div>
            </div>
            <div v-if="entry.abandoned" class="aside">Discarded attempt</div>
            <Markdown :class="{ abandoned: entry.abandoned }" :text="entry.text" :streaming="entry.streaming" :workspace-root="snap.workspace.root" />
            <div
              v-if="!entry.streaming && entry.text.trim()"
              class="actions"
              :class="{ shown: index === lastAnswer }"
            >
              <button class="act" title="Quote in your reply" @click="quote(entry.text)">
                <Icon name="quote" :size="14" />
              </button>
              <button class="act" title="Copy answer as Markdown" @click="copyText(entry.text)">
                <Icon name="copy" :size="14" />
              </button>
              <button
                class="act"
                title="Save to a type…"
                @click="saveMenu($event, pendingSave(entry.text, [], snap.workspace.root, snap.sessionId), $event.currentTarget as HTMLElement)"
              >
                <Icon name="bookmark" :size="14" />
              </button>
              <button
                v-if="snap.connection.state === 'live'"
                class="act"
                :title="readingEntry === index && snap.voice.reading ? 'Stop reading' : 'Read aloud'"
                @click="toggleReading(index, entry.text)"
              >
                <Icon :name="readingEntry === index && snap.voice.reading ? 'volumeOff' : 'volume'" :size="14" />
              </button>
            </div>
          </template>

          <ToolRow v-else-if="entry.kind === 'tool'" :tool="entry" />

          <div v-else-if="entry.kind === 'notice' && entry.lane === 'alert'" class="alert">
            {{ entry.message }}
          </div>
          <div v-else-if="entry.kind === 'notice'" class="divider">
            <span>{{ entry.message }}</span>
          </div>

          <div v-else-if="entry.kind === 'error'" class="error">
            <span>{{ entry.message }}</span>
            <button v-if="index === lastError" class="btn btn-secondary retry" @click="engine.retryTurn()">
              <Icon name="refresh" :size="13" /> Retry
            </button>
          </div>
        </div>
        <details v-for="review in reviews.get(index)" :key="review.start" class="turn-review">
          <summary>
            <Icon class="review-chevron" name="chevronRight" :size="12" />
            <span>Turn review</span>
            <span class="review-counts">{{ review.files.length }} {{ review.files.length === 1 ? 'file' : 'files' }} · {{ review.checks.length ? `${review.checks.length} ${review.checks.length === 1 ? 'check' : 'checks'}` : 'No checks' }}</span>
          </summary>
          <section class="review-section">
            <h4>Checks</h4>
            <p v-if="!review.checks.length">No verification recorded.</p>
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
            <button class="btn btn-secondary" @click="workingTree(file.path)">Compare current working tree</button>
          </details>
          </section>
        </details>
      </template>

      <WorkingIndicator v-if="snap.turnRunning" v-show="!streaming" />
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
.column {
  max-width: var(--content);
  margin: 0 auto;
  padding: var(--space-lg) var(--space-xl) var(--space-xxl);
}
.row {
  margin-top: 18px;
  animation: rise 200ms var(--ease) both;
}
.row:first-child {
  margin-top: 0;
}
.fold {
  display: flex;
  align-items: center;
  gap: 8px;
  max-width: 100%;
  margin: 18px 0 0 -6px;
  padding: 4px 10px 4px 6px;
  border-radius: var(--radius-sm);
  font-size: 13px;
  color: var(--text-muted);
  text-align: left;
  animation: rise 200ms var(--ease) both;
  transition: background 120ms var(--ease), color 120ms var(--ease);
}
.fold:hover {
  background: var(--hover);
  color: var(--text);
}
.fold-icon {
  display: grid;
  place-items: center;
  width: 16px;
  height: 16px;
  flex: none;
  border-radius: 50%;
  background: color-mix(in srgb, var(--success) 22%, transparent);
  color: var(--success);
}
.fold.failed .fold-icon {
  background: color-mix(in srgb, var(--danger) 22%, transparent);
  color: var(--danger);
}
.fold-images {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin: 8px 0 0 18px;
}
.fold.open + .row {
  margin-top: 6px;
}
.row.tight {
  margin-top: 4px;
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
.act:hover {
  background: var(--hover);
  color: var(--text);
}
.jump {
  position: sticky;
  bottom: 14px;
  display: flex;
  justify-content: center;
  height: 0;
  overflow: visible;
  pointer-events: none;
}
.jump button {
  display: grid;
  place-items: center;
  width: 34px;
  height: 34px;
  margin-top: -48px;
  border-radius: 50%;
  background: var(--glass);
  -webkit-backdrop-filter: var(--glass-blur);
  backdrop-filter: var(--glass-blur);
  color: var(--text);
  box-shadow: var(--shadow-pop);
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
  background: var(--raised);
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
  padding-top: 2px;
}
.error .retry {
  flex: none;
  gap: 6px;
  user-select: none;
  -webkit-user-select: none;
}
</style>
