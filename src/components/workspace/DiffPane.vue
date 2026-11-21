<script setup lang="ts">
// Review the current Git diff for one file, retaining both sides' line numbers.
import { computed, ref, watch } from "vue";
import { diffHunks, type DiffHunk } from "../../diff";
import { showToast, workspaceFiles } from "../../bridge";
import { addToPrompt, mention, showFile } from "../../workspace";
import type { DiffView } from "../../types";
import Icon from "../Icon.vue";

const props = defineProps<{
  path: string;
  root: string;
  expanded: boolean;
  position: { index: number; total: number } | null;
}>();
const emit = defineEmits<{ back: []; expand: []; move: [step: -1 | 1] }>();

const view = ref<DiffView | null>(null);
const error = ref<string | null>(null);

let request = 0;
const scroll = ref<HTMLElement | null>(null);
async function load() {
  const current = ++request;
  view.value = null;
  error.value = null;
  try {
    const result = await workspaceFiles.diff(props.path, props.root);
    if (current === request) view.value = result;
  } catch (e) {
    if (current === request) error.value = String(e);
  }
}

const hunks = computed(() => diffHunks(view.value?.diff ?? ""));
const rows = computed(() => hunks.value.flatMap((hunk) => hunk.lines));
const added = computed(() => rows.value.filter((line) => line.kind === "add").length);
const removed = computed(() => rows.value.filter((line) => line.kind === "del").length);
const baseLabel = computed(() => {
  switch (view.value?.base) {
    case "head": return "Working tree vs. HEAD";
    case "index": return "Working tree vs. index (no commits yet)";
    case "untracked": return "Untracked file vs. empty file";
  }
});
const relative = computed(() => view.value?.path ?? props.path.replace(`${props.root}/`, ""));

function openInFiles() {
  showFile(relative.value);
}

watch(() => [props.root, props.path], () => {
  scroll.value?.scrollTo({ top: 0, left: 0 });
  void load();
}, { immediate: true });

function askAbout(hunk: DiffHunk) {
  const patch = [hunk.header, ...hunk.lines.map((line) => line.text)].join("\n");
  const excerpt = patch.length > 4000 ? `${patch.slice(0, 4000)}\n[excerpt truncated]` : patch;
  const ticks = excerpt.match(/`+/g) ?? [];
  const fence = "`".repeat(ticks.reduce((n, run) => Math.max(n, run.length + 1), 3));
  addToPrompt(`Review this diff hunk in ${mention(relative.value)} (${baseLabel.value}):\n\n${fence}diff\n${excerpt}\n${fence}`);
}

function copy() {
  navigator.clipboard.writeText(view.value?.diff ?? "").catch(() => showToast("Could not copy to the clipboard"));
}
</script>

<template>
  <div class="diff">
    <div class="head">
      <button class="icon-btn" title="All changed files" aria-label="All changed files" @click="emit('back')">
        <Icon name="arrowLeft" :size="14" />
      </button>
      <span class="mono truncate" :title="relative">{{ relative }}</span>
      <span v-if="view" class="counts"><b class="add">+{{ added }}</b> <b class="del">−{{ removed }}</b></span>
      <button class="icon-btn" title="Refresh" @click="load"><Icon name="refresh" :size="14" /></button>
      <button class="icon-btn" title="Open in Files" @click="openInFiles"><Icon name="file" :size="14" /></button>
      <button class="icon-btn" title="Copy diff" :disabled="!view?.diff" @click="copy"><Icon name="copy" :size="14" /></button>
      <button
        class="icon-btn"
        title="Ask Dial about this change"
        @click="addToPrompt(`About your change to ${mention(relative)}: `)"
      >
        <Icon name="chat" :size="14" />
      </button>
    </div>
    <div class="review-bar">
      <div v-if="position" class="file-nav" aria-label="Changed files navigation">
        <button class="icon-btn" title="Previous file" aria-label="Previous file" :disabled="position.index === 0" @click="emit('move', -1)"><Icon name="arrowLeft" :size="14" /></button>
        <span>{{ position.index + 1 }} / {{ position.total }}</span>
        <button class="icon-btn" title="Next file" aria-label="Next file" :disabled="position.index + 1 === position.total" @click="emit('move', 1)"><Icon name="chevronRight" :size="14" /></button>
      </div>
      <button class="btn btn-secondary" :aria-pressed="expanded" @click="emit('expand')">{{ expanded ? 'Shrink diff' : 'Expand diff' }}</button>
    </div>
    <p v-if="view" class="note"><strong>{{ baseLabel }}</strong> · Not limited to this session or turn.</p>
    <div v-if="error" class="empty">
      <strong>No diff</strong>
      <span>{{ error }}</span>
    </div>
    <div v-else-if="view && !view.diff" class="empty">
      <strong>No changes against this baseline</strong>
    </div>
    <div v-else-if="view" ref="scroll" class="lines mono">
      <template v-if="hunks.length">
        <section v-for="(hunk, i) in hunks" :key="i">
          <header class="hunk">
            <span class="truncate" :title="hunk.header">{{ hunk.header }}</span>
            <button class="btn btn-secondary" @click="askAbout(hunk)">Ask Dial</button>
          </header>
          <div v-for="(line, j) in hunk.lines" :key="j" class="line" :class="line.kind">
            <span class="line-no" aria-label="Old line">{{ line.old }}</span>
            <span class="line-no" aria-label="New line">{{ line.next }}</span>
            <span>{{ line.text }}</span>
          </div>
        </section>
      </template>
      <pre v-else class="note">{{ view.diff }}</pre>
    </div>
    <div v-else class="empty" role="status">Loading diff…</div>
  </div>
</template>

<style scoped>
.diff {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}
.head {
  display: flex;
  align-items: center;
  gap: 4px;
  padding-bottom: 8px;
  font-size: var(--text-sm);
}
.head .mono {
  flex: 1;
  min-width: 0;
}
.review-bar, .file-nav {
  display: flex;
  align-items: center;
  gap: 4px;
}
.review-bar {
  justify-content: space-between;
  flex-wrap: wrap;
  font-size: var(--text-xs);
}
.review-bar > button { margin-left: auto; }
.counts {
  font-size: var(--text-xs);
  white-space: nowrap;
}
.add {
  color: var(--success);
}
.del {
  color: var(--danger);
}
.lines {
  flex: 1;
  overflow: auto;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--bg);
  font-size: 12px;
  line-height: 1.55;
  user-select: text;
  -webkit-user-select: text;
}
.line {
  display: grid;
  grid-template-columns: 5ch 5ch auto;
  white-space: pre;
  padding-right: 10px;
  min-width: max-content;
}
.line-no {
  text-align: right;
  padding-right: 1ch;
  color: var(--text-faint);
  user-select: none;
  -webkit-user-select: none;
}
.line:hover {
  background: var(--hover);
}
.line.add {
  background: color-mix(in srgb, var(--success) 14%, transparent);
  box-shadow: inset 3px 0 0 color-mix(in srgb, var(--success) 70%, transparent);
}
.line.del {
  background: color-mix(in srgb, var(--danger) 14%, transparent);
  box-shadow: inset 3px 0 0 color-mix(in srgb, var(--danger) 70%, transparent);
}
.line.add:hover {
  background: color-mix(in srgb, var(--success) 22%, transparent);
}
.line.del:hover {
  background: color-mix(in srgb, var(--danger) 22%, transparent);
}
.hunk {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
  padding: 4px 8px;
  position: sticky;
  left: 0;
  color: var(--info);
  background: color-mix(in srgb, var(--info) 9%, transparent);
  border-block: 1px solid color-mix(in srgb, var(--info) 18%, transparent);
}
.hunk button { flex: none; }
.note {
  margin: 0;
  padding: 6px 10px;
  font-family: var(--font-ui);
  font-size: var(--text-xs);
  color: var(--text-faint);
}
.empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  margin-top: var(--space-xl);
  text-align: center;
}
.empty strong {
  font-weight: 500;
  color: var(--text-muted);
}
.empty span {
  font-size: var(--text-sm);
  color: var(--text-faint);
}
</style>
