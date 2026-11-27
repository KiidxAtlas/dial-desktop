<script setup lang="ts">
// Review the current Git diff for one file, retaining both sides' line numbers.
import { computed, ref, watch } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import { diffHunks, type DiffHunk } from "../../diff";
import { showToast, workspaceChange, workspaceFiles } from "../../bridge";
import { addToPrompt, mention, showFile } from "../../workspace";
import { openMenu } from "../../ui";
import { paneError, paneErrorText, type PaneError } from "../../paneErrors";
import { relativeTo } from "../../paths";
import { workspaceGitApi } from "../../workspaceGitApi";
import type { DiffScope, DiffView } from "../../types";
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
const scope = ref<DiffScope>("all");
const error = ref<PaneError | null>(null);
const reverting = ref(false);

let request = 0;
async function load() {
  const current = ++request;
  error.value = null;
  try {
    const result = await workspaceFiles.diff(props.path, props.root, scope.value);
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
  // A staged addition can be diffed against HEAD but still has no committed version.
  const untracked = view.value?.deleteOnRevert;
  const confirmed = await ask(
    untracked
      ? `Delete ${path}? It has no committed version, so this removes the file. Your own edits to it are lost too, not just Dial's.`
      : `Revert ${path} to the last commit? This discards every uncommitted change to the file, including your own edits and staged changes, not just Dial's.`,
    { title: untracked ? "Delete new file" : "Revert file", kind: "warning", okLabel: untracked ? "Delete file" : "Revert file" },
  );
  if (!confirmed || root !== props.root || path !== relative.value) return;
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
  if (!confirmed || root !== props.root || path !== relative.value) return;
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

/** Stage just this hunk into Git's index. */
async function stageHunk(hunk: DiffHunk) {
  const path = relative.value, root = props.root;
  reverting.value = true;
  try {
    await workspaceGitApi.stageHunk(path, hunkText(hunk), root);
    showToast(`Staged a change in ${path}`);
    await load();
  } catch (e) {
    showToast(paneErrorText(e));
  } finally {
    reverting.value = false;
  }
}

/** Unstage just this hunk from Git's index while keeping the working tree edit. */
async function unstageHunk(hunk: DiffHunk) {
  const path = relative.value, root = props.root;
  reverting.value = true;
  try {
    await workspaceGitApi.unstageHunk(path, hunkText(hunk), root);
    showToast(`Unstaged a change in ${path}`);
    await load();
  } catch (e) {
    showToast(paneErrorText(e));
  } finally {
    reverting.value = false;
  }
}

/** Resolve an active merge conflict on this file using ours, theirs, or manual resolution. */
async function resolveConflict(strategy: "ours" | "theirs" | "resolved") {
  const path = relative.value, root = props.root;
  if (strategy === "ours" || strategy === "theirs") {
    const label = strategy === "ours" ? "Keep ours" : "Take theirs";
    const confirmed = await ask(
      `${label} for ${path}? This replaces the file with ${strategy === "ours" ? "your branch's" : "the incoming"} version and stages it as resolved.`,
      { title: "Resolve merge conflict", kind: "warning", okLabel: label },
    );
    if (!confirmed || root !== props.root || path !== relative.value) return;
  }
  reverting.value = true;
  try {
    await workspaceGitApi.resolveConflict(path, strategy, root);
    showToast(
      strategy === "resolved"
        ? `Marked ${path} as resolved`
        : `Resolved ${path} (${strategy === "ours" ? "kept ours" : "took theirs"})`,
    );
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
  if (scope.value === "staged") return "Staged changes (index vs. HEAD)";
  if (scope.value === "unstaged") {
    return view.value?.base === "untracked" ? "Untracked file vs. empty file" : "Unstaged changes (working tree vs. index)";
  }
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

watch([() => props.root, () => props.path, () => props.revision, scope], ([root, path], previous) => {
  // Changing files unmounts the old scroll container. Refreshing the same file keeps it in place.
  if (root !== previous[0] || path !== previous[1]) view.value = null;
  void load();
}, { immediate: true });
watch(workspaceChange, ({ root }) => {
  if (root && root === props.root) void load();
});
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

/** The less-used file actions, behind "⋯" so the header stays one row. */
function more(event: MouseEvent) {
  openMenu(event, [
    { label: "Ask Dial about this change", icon: "chat", run: () => addToPrompt(`About the current working-tree diff for ${mention(relative.value)}: `) },
    { label: "Open in Files", icon: "file", run: openInFiles },
    { label: "Copy diff", icon: "copy", disabled: !view.value?.diff, run: copy },
    { label: "Refresh", icon: "refresh", run: () => void load() },
    ...(view.value?.nested ? [] : [
      { divider: true as const },
      { label: view.value?.deleteOnRevert ? "Delete file…" : "Revert file…", icon: "undo", danger: true, disabled: !view.value?.diff || reverting.value, run: () => void revert() },
    ]),
  ], event.currentTarget as HTMLElement);
}
</script>

<template>
  <div class="diff">
    <!-- One row: back, the file and its counts, file-to-file stepping, expand, and "⋯" for the rest. -->
    <div class="head">
      <button class="icon-btn" title="All changed files" aria-label="All changed files" @click="emit('back')">
        <Icon name="arrowLeft" :size="14" />
      </button>
      <span class="mono truncate" :title="relative">{{ relative }}</span>
      <span v-if="view" class="counts"><b class="add">+{{ added }}</b> <b class="del">−{{ removed }}</b></span>
      <div v-if="position && position.total > 1" class="file-nav" aria-label="Changed files navigation">
        <button class="icon-btn" title="Previous file" aria-label="Previous file" :disabled="position.index === 0" @click="emit('move', -1)"><Icon name="chevronLeft" :size="14" /></button>
        <span>{{ position.index + 1 }}/{{ position.total }}</span>
        <button class="icon-btn" title="Next file" aria-label="Next file" :disabled="position.index + 1 === position.total" @click="emit('move', 1)"><Icon name="chevronRight" :size="14" /></button>
      </div>
      <button
        class="icon-btn"
        :title="expanded ? 'Shrink diff' : 'Expand diff'"
        :aria-label="expanded ? 'Shrink diff' : 'Expand diff'"
        :aria-pressed="expanded"
        @click="emit('expand')"
      ><Icon :name="expanded ? 'compress' : 'expand'" :size="14" /></button>
      <button class="icon-btn" title="More: ask Dial, open in Files, copy, revert" aria-label="More file actions" @click="more"><Icon name="more" :size="14" /></button>
    </div>
    <div v-if="view" class="scope-bar">
      <div class="scopes" role="tablist" aria-label="Diff view">
        <button v-for="mode in (['all', 'unstaged', 'staged'] as const)" :key="mode" role="tab" :aria-selected="scope === mode" class="scope-tab" :class="{ on: scope === mode }" @click="scope = mode">
          {{ mode === 'all' ? 'All' : mode === 'unstaged' ? 'Unstaged' : 'Staged' }}
        </button>
      </div>
      <p class="note"><strong>{{ baseLabel }}</strong><template v-if="view.nested"> · Open this repository as a project to stage, revert or commit changes.</template></p>
    </div>
    <div v-if="view?.conflicted && !view.nested" class="conflict-bar" role="region" aria-label="Merge conflict resolution">
      <strong>Merge conflict</strong>
      <div class="conflict-actions">
        <button class="btn btn-secondary" :disabled="reverting" @click="resolveConflict('ours')">Keep ours</button>
        <button class="btn btn-secondary" :disabled="reverting" @click="resolveConflict('theirs')">Take theirs</button>
        <button class="btn btn-secondary" :disabled="reverting" @click="resolveConflict('resolved')">Mark resolved</button>
        <button class="btn btn-secondary" @click="openInFiles">Edit in Files</button>
      </div>
    </div>
    <div v-if="error" class="empty">
      <strong>{{ error.title }}</strong>
      <span v-if="error.hint">{{ error.hint }}</span>
      <span class="mono detail">{{ error.detail }}</span>
    </div>
    <div v-else-if="view && !view.diff" class="empty">
      <strong>{{ scope === 'staged' ? 'No staged changes' : scope === 'unstaged' ? 'No unstaged changes' : 'No uncommitted changes' }}</strong>
      <span>{{ scope === 'staged' ? 'Stage a file or hunk to include it in the next commit.' : scope === 'unstaged' ? 'Every change in this file is already staged.' : `This file matches ${view.base === 'head' ? 'the last commit' : 'Git’s index'}. Dial’s edits may have been committed or undone.` }}</span>
    </div>
    <div v-else-if="view" class="lines mono">
      <template v-if="hunks.length">
        <section v-for="(hunk, i) in hunks" :key="i">
          <header class="hunk">
            <span class="truncate" :title="hunk.header">{{ hunk.header }}</span>
            <span class="hunk-actions">
              <button
                v-if="scope === 'staged' && !view.nested"
                class="btn btn-secondary"
                title="Unstage just this hunk from Git's index"
                :disabled="reverting"
                @click="unstageHunk(hunk)"
              >Unstage hunk</button>
              <button
                v-if="scope !== 'staged' && view.base !== 'untracked' && !view.nested"
                class="btn btn-secondary"
                title="Stage just this hunk into Git's index"
                :disabled="reverting"
                @click="stageHunk(hunk)"
              >Stage hunk</button>
              <button
                v-if="scope !== 'staged' && view.base !== 'untracked' && !view.nested"
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
    <!-- Lines shaped like a diff while it loads, so the pane does not jump when it arrives. -->
    <div v-else class="lines skeleton" role="status" aria-label="Loading diff">
      <span v-for="(w, i) in [62, 48, 80, 35, 70, 55, 44, 66]" :key="i" :class="{ add: i === 2 || i === 3, del: i === 5 }" :style="{ width: `${w}%` }" />
    </div>
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
.file-nav {
  display: flex;
  flex: none;
  align-items: center;
  gap: 0;
  font-size: var(--text-xs);
  font-variant-numeric: tabular-nums;
  color: var(--text-faint);
}
.counts {
  font-size: var(--text-xs);
  white-space: nowrap;
}
.skeleton {
  display: flex;
  flex-direction: column;
  gap: 9px;
  padding: 12px 10px;
}
.skeleton span {
  height: 8px;
  border-radius: 4px;
  background: var(--hover);
  animation: shimmer 1.4s ease-in-out infinite;
}
.skeleton span.add { background: color-mix(in srgb, var(--success) 16%, transparent); }
.skeleton span.del { background: color-mix(in srgb, var(--danger) 16%, transparent); }
@keyframes shimmer { 50% { opacity: 0.5; } }
@media (prefers-reduced-motion: reduce) { .skeleton span { animation: none; } }
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
.scope-bar {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  padding-bottom: 6px;
}
.scopes {
  display: inline-flex;
  gap: 2px;
  padding: 2px;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--surface);
}
.scope-tab {
  border: 0;
  background: transparent;
  color: var(--text-faint);
  font: inherit;
  font-size: var(--text-xs);
  padding: 2px 7px;
  border-radius: calc(var(--radius-sm) - 1px);
  cursor: pointer;
}
.scope-tab.on {
  background: var(--active);
  color: var(--text);
  font-weight: 500;
}
.note {
  margin: 0;
  padding: 0 4px;
  font-family: var(--font-ui);
  font-size: var(--text-xs);
  color: var(--text-faint);
}
.conflict-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 8px;
  margin-bottom: 8px;
  padding: 7px 10px;
  border: 1px solid color-mix(in srgb, var(--warning) 45%, var(--border));
  border-radius: var(--radius-sm);
  background: color-mix(in srgb, var(--warning) 10%, var(--surface));
  font-size: var(--text-xs);
}
.conflict-bar strong {
  color: var(--warning);
  font-weight: 600;
}
.conflict-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
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
