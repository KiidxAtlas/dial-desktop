<script setup lang="ts">
// Review the current Git diff for one file, retaining both sides' line numbers.
import { computed, ref, watch } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import { diffHunks, type DiffHunk } from "../../diff";
import { showToast, workspaceFiles } from "../../bridge";
import { addToPrompt, mention, showFile } from "../../workspace";
import { paneError, paneErrorText, type PaneError } from "../../paneErrors";
import { relativeTo } from "../../paths";
import { workspaceGitApi } from "../../workspaceGitApi";
import type { DiffView } from "../../types";
import Icon from "../Icon.vue";

const props = defineProps<{
  path: string;
  root: string;
  revision?: number;
  expanded: boolean;
  position: { index: number; total: number } | null;
}>();
const emit = defineEmits<{ back: []; expand: []; move: [step: -1 | 1] }>();

const view = ref<DiffView | null>(null);
const error = ref<PaneError | null>(null);
const reverting = ref(false);

let request = 0;
async function load() {
  const current = ++request;
  error.value = null;
  try {
    const result = await workspaceFiles.diff(props.path, props.root);
    if (current === request) view.value = result;
  } catch (e) {
    if (current === request) {
      view.value = null;
      error.value = paneError(e);
    }
  }
}

/** Discard every uncommitted change to this file, after the user confirms. */
async function revert() {
  const path = relative.value, root = props.root;
  // Without a commit to return to, the file is unstaged and deleted.
  const untracked = view.value?.base === "untracked" || view.value?.base === "index";
  const confirmed = await ask(
    untracked
      ? `Delete ${path}? It has no committed version, so this removes the file. Your own edits to it are lost too, not just Dial's.`
      : `Revert ${path} to the last commit? This discards every uncommitted change to the file, including your own edits and staged changes, not just Dial's.`,
    { title: untracked ? "Delete new file" : "Revert file", kind: "warning", okLabel: untracked ? "Delete file" : "Revert file" },
  );
  if (!confirmed || root !== props.root) return;
  reverting.value = true;
  try {
    await workspaceGitApi.revert(path, root);
    showToast(untracked ? `Deleted ${path}` : `Reverted ${path}`);
    await load();
  } catch (e) {
    showToast(paneErrorText(e));
  } finally {
    reverting.value = false;
  }
}

function hunkText(hunk: DiffHunk) {
  return [hunk.header, ...hunk.lines.map((line) => line.text)].join("\n");
}

/** Undo just this hunk in the working tree, after the user confirms. A hunk that no longer matches
 *  the file is refused by the backend rather than guessed at. */
async function revertHunk(hunk: DiffHunk) {
  const path = relative.value, root = props.root;
  const added = hunk.lines.filter((line) => line.kind === "add").length;
  const removed = hunk.lines.filter((line) => line.kind === "del").length;
  const confirmed = await ask(
    `Undo this change in ${path} (+${added} −${removed} lines at ${hunk.header.replace(/^@@ (.*?) @@.*$/, "$1")})? The rest of the file stays as it is. Your own edits in these lines are lost too.`,
    { title: "Revert hunk", kind: "warning", okLabel: "Revert hunk" },
  );
  if (!confirmed || root !== props.root) return;
  reverting.value = true;
  try {
    await workspaceGitApi.revertHunk(path, hunkText(hunk), root);
    showToast(`Reverted a change in ${path}`);
    await load();
  } catch (e) {
    showToast(paneErrorText(e));
  } finally {
    reverting.value = false;
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
const relative = computed(() => view.value?.path ?? relativeTo(props.root, props.path));

function openInFiles() {
  showFile(relative.value);
}

watch([() => props.root, () => props.path, () => props.revision], ([root, path], previous) => {
  // Changing files unmounts the old scroll container. Refreshing the same file keeps it in place.
  if (root !== previous[0] || path !== previous[1]) view.value = null;
  void load();
}, { immediate: true });

function askAbout(hunk: DiffHunk) {
  const patch = hunkText(hunk);
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
      <button class="icon-btn" title="Revert file (discards all its uncommitted changes)" aria-label="Revert file" :disabled="!view?.diff || reverting" @click="revert"><Icon name="undo" :size="14" /></button>
      <button
        class="icon-btn"
        title="Ask Dial about this change"
        @click="addToPrompt(`About the current working-tree diff for ${mention(relative)}: `)"
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
      <strong>{{ error.title }}</strong>
      <span v-if="error.hint">{{ error.hint }}</span>
      <span class="mono detail">{{ error.detail }}</span>
    </div>
    <div v-else-if="view && !view.diff" class="empty">
      <strong>No uncommitted changes</strong>
      <span>This file matches {{ view.base === 'head' ? 'the last commit' : 'Git’s index' }}. Dial’s edits may have been committed or undone.</span>
    </div>
    <div v-else-if="view" class="lines mono">
      <template v-if="hunks.length">
        <section v-for="(hunk, i) in hunks" :key="i">
          <header class="hunk">
            <span class="truncate" :title="hunk.header">{{ hunk.header }}</span>
            <span class="hunk-actions">
              <button
                v-if="view.base !== 'untracked'"
                class="btn btn-secondary"
                title="Undo just this change in the working tree"
                :disabled="reverting"
                @click="revertHunk(hunk)"
              >Revert hunk</button>
              <button class="btn btn-secondary" @click="askAbout(hunk)">Ask Dial</button>
            </span>
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
.hunk-actions { display: flex; gap: 4px; flex: none; }
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
  color: var(--text-faint);
}
.empty .detail {
  max-width: 100%;
  overflow-wrap: anywhere;
  font-size: var(--text-xs);
}
</style>
