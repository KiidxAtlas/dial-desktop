<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";
import type { OpenSnapshot } from "../types";
import { toggle, ui } from "../ui";
import { engine } from "../bridge";
import { canRetry, canUndo, copyText, editLastPrompt } from "../sessionActions";
import Markdown from "./Markdown.vue";
import ToolRow from "./ToolRow.vue";
import EmptyState from "./EmptyState.vue";
import Icon from "./Icon.vue";

const props = defineProps<{ snap: OpenSnapshot }>();

const scroller = ref<HTMLElement | null>(null);
/** Follow new output only while the reader is at the bottom. */
const pinned = ref(true);

const empty = computed(() => props.snap.entries.length === 0 && !props.snap.turnRunning);
const streaming = computed(() => {
  const last = props.snap.entries.at(-1);
  return last?.kind === "assistant" && last.streaming && last.text.length > 0;
});

// The working line: a spinning glyph and the turn's elapsed time, like a terminal agent.
const GLYPHS = ["·", "✢", "✳", "✶", "✻", "✽", "✻", "✶", "✳", "✢"];
const frame = ref(0);
const elapsed = ref(0);
let started = 0;
let timer: ReturnType<typeof setInterval> | null = null;
watch(
  () => props.snap.turnRunning,
  (running) => {
    if (timer) clearInterval(timer);
    timer = null;
    if (!running) return;
    started = Date.now();
    elapsed.value = 0;
    timer = setInterval(() => {
      frame.value = (frame.value + 1) % GLYPHS.length;
      elapsed.value = Math.floor((Date.now() - started) / 1000);
    }, 120);
  },
  { immediate: true },
);
onBeforeUnmount(() => timer && clearInterval(timer));
const elapsedLabel = computed(() => {
  const s = elapsed.value;
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
});

function onScroll() {
  const el = scroller.value;
  if (el) pinned.value = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
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

watch(
  () => props.snap.entries,
  async () => {
    if (!pinned.value) return;
    await nextTick();
    const el = scroller.value;
    if (el) el.scrollTop = el.scrollHeight;
  },
  { flush: "post" },
);

watch(
  () => props.snap.sessionId,
  () => (pinned.value = true),
);

function reasoningOpen(index: number, streamingAnswer: boolean, text: string) {
  return ui.expanded.has(`reasoning:${index}`) || (streamingAnswer && !text);
}

/** Consecutive tool calls sit close together. */
function gap(index: number) {
  const entries = props.snap.entries;
  if (index === 0) return "";
  return entries[index].kind === "tool" && entries[index - 1].kind === "tool" ? "tight" : "";
}
</script>

<template>
  <EmptyState v-if="empty" :snap="snap" />
  <div v-else ref="scroller" class="scroller" @scroll.passive="onScroll">
    <div class="column">
      <template v-for="(entry, index) in snap.entries" :key="entry.kind === 'tool' ? entry.key : index">
        <div class="row" :class="[`k-${entry.kind}`, gap(index)]">
          <div v-if="entry.kind === 'user'" class="user">
            <div class="bubble">
              <div class="user-text">{{ entry.text }}</div>
              <div v-if="entry.images" class="images">
                <Icon name="image" :size="13" />
                {{ entry.images === 1 ? "1 image" : `${entry.images} images` }}
              </div>
            </div>
            <div class="actions">
              <button class="act" title="Copy" @click="copyText(entry.text)">
                <Icon name="copy" :size="14" />
              </button>
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
                <span>{{ entry.streaming && !entry.text ? "Thinking…" : "Thought for a moment" }}</span>
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
            <Markdown :class="{ abandoned: entry.abandoned }" :text="entry.text" />
            <div
              v-if="!entry.streaming && entry.text.trim()"
              class="actions"
              :class="{ shown: index === lastAnswer }"
            >
              <button class="act" title="Copy answer as Markdown" @click="copyText(entry.text)">
                <Icon name="copy" :size="14" />
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
      </template>

      <div v-if="snap.turnRunning && !streaming" class="working">
        <span class="glyph">{{ GLYPHS[frame] }}</span>
        <span class="shimmer">Working…</span>
        <span class="hint">({{ elapsedLabel }} · esc to interrupt)</span>
      </div>
      <div v-if="!pinned" class="jump">
        <button title="Jump to the latest message" aria-label="Jump to the latest message" @click="jumpToEnd">
          <Icon name="arrowDown" :size="16" />
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
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
  border-radius: var(--radius-lg);
  background: var(--bubble);
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
  background: var(--raised);
  color: var(--text);
  box-shadow: var(--shadow-pop);
  pointer-events: auto;
  animation: rise 160ms var(--ease) both;
}
.jump button:hover {
  background: var(--surface);
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
.working {
  display: flex;
  align-items: baseline;
  gap: 8px;
  margin-top: 18px;
  font-size: 14px;
}
.glyph {
  width: 14px;
  text-align: center;
  color: var(--accent);
}
.shimmer {
  color: var(--accent);
  background: linear-gradient(90deg, var(--accent) 0%, var(--accent-soft) 50%, var(--accent) 100%) 0 0 / 200% 100%;
  -webkit-background-clip: text;
  background-clip: text;
  -webkit-text-fill-color: transparent;
  animation: shimmer 2s linear infinite;
}
.hint {
  font-size: 13px;
  color: var(--text-faint);
}
@keyframes shimmer {
  to {
    background-position: -200% 0;
  }
}
</style>
