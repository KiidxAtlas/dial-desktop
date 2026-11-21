<script setup lang="ts">
// The Files panel: the project as a lazily loaded tree beside a viewer for one file. A file can
// be mentioned in the prompt so the agent reads it.
import { computed, onMounted, reactive, ref, watch } from "vue";
import { files as fileActions, showToast, workspaceFiles } from "../../bridge";
import { openMenu, ui, type MenuItem } from "../../ui";
import { addToPrompt, mention, showFile } from "../../workspace";
import type { DirEntry, FileView, OpenSnapshot } from "../../types";
import CodeBlock from "../CodeBlock.vue";
import Icon from "../Icon.vue";

const props = defineProps<{ snap: OpenSnapshot }>();

/** Loaded folders' entries, by relative path (`""` is the root). */
const children = reactive(new Map<string, DirEntry[]>());
const expanded = reactive(new Set<string>());
const loading = reactive(new Map<string, number>());
let request = 0, fileRequest = 0;
const filter = ref("");
const file = ref<FileView | null>(null);
const fileError = ref<string | null>(null);

const LANGS: Record<string, string> = {
  rs: "rust", ts: "typescript", mts: "typescript", cts: "typescript", tsx: "tsx", js: "javascript", mjs: "javascript",
  cjs: "javascript", jsx: "tsx", vue: "vue", json: "json", toml: "toml", yaml: "yaml", yml: "yaml", sh: "shellscript",
  bash: "shellscript", zsh: "shellscript", py: "python", go: "go", c: "c", h: "c", cpp: "cpp", cc: "cpp", hpp: "cpp",
  java: "java", swift: "swift", html: "html", css: "css", sql: "sql", diff: "diff", patch: "diff", md: "markdown",
};

function lang(path: string): string {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return LANGS[ext] ?? "";
}

async function load(path: string) {
  const root = props.snap.workspace.root, id = ++request;
  loading.set(path, id);
  const current = () => props.snap.workspace.root === root && loading.get(path) === id;
  try {
    const entries = await workspaceFiles.list(path, root);
    if (current()) children.set(path, entries);
  } catch (error) {
    if (current()) showToast(String(error));
  } finally {
    if (current()) loading.delete(path);
  }
}

function toggle(entry: DirEntry) {
  if (expanded.delete(entry.path)) return;
  expanded.add(entry.path);
  if (!children.has(entry.path)) void load(entry.path);
}

async function openFile(path: string) {
  const root = props.snap.workspace.root;
  const id = ++fileRequest;
  const current = () => id === fileRequest && ui.filePath === path && props.snap.workspace.root === root;
  if (file.value?.path !== path) file.value = null;
  fileError.value = null;
  try {
    const result = await workspaceFiles.read(path, root);
    if (current()) file.value = result;
  } catch (error) {
    if (!current()) return;
    file.value = null;
    fileError.value = String(error);
  }
}

/** Reload every open folder (Dial may have added or removed files). */
async function refresh() {
  const open = ["", ...expanded];
  children.clear();
  loading.clear();
  await Promise.all([...open.map(load), ui.filePath && openFile(ui.filePath)]);
}

/** The visible rows: open folders' entries, depth-first. */
const rows = computed(() => {
  const out: { entry: DirEntry; depth: number }[] = [];
  const q = filter.value.trim().toLowerCase();
  const walk = (path: string, depth: number) => {
    for (const entry of children.get(path) ?? []) {
      if (q && !entry.dir && !entry.path.toLowerCase().includes(q)) continue;
      out.push({ entry, depth });
      if (entry.dir && expanded.has(entry.path)) walk(entry.path, depth + 1);
    }
  };
  walk("", 0);
  return out;
});

function absolute(path: string) {
  return `${props.snap.workspace.root}/${path}`;
}

function menu(event: MouseEvent, entry: DirEntry) {
  const items: MenuItem[] = [
    { label: "Mention in prompt", icon: "chat", run: () => addToPrompt(mention(entry.path)) },
    {
      label: "Copy path",
      icon: "copy",
      run: () => navigator.clipboard.writeText(entry.path).catch(() => showToast("Could not copy to the clipboard")),
    },
    { label: "Show in file manager", icon: "folder", run: () => fileActions.reveal(absolute(entry.path)) },
  ];
  openMenu(event, items);
}

function size(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

// Another folder: start over.
watch(
  () => props.snap.workspace.root,
  () => {
    children.clear();
    loading.clear();
    fileRequest++;
    fileError.value = null;
    ui.fileLines = null;
    expanded.clear();
    file.value = null;
    ui.filePath = null;
    void load("");
  },
);

// Each settled tool result advances its paths' revisions, including repeat edits.
watch(() => props.snap.changes, (changes, previous) => {
  const old = new Map(previous.map(c => [c.path, c]));
  const affected = [...changes.filter(c => old.get(c.path)?.revision !== c.revision),
    ...previous.filter(c => !changes.some(next => next.path === c.path))];
  const folders = new Set<string>();
  for (const c of affected) {
    const path = (c.dir + c.name).replaceAll('\\', '/');
    if (path === ui.filePath) void openFile(path);
    // Refresh the nearest visible parent, including when a new subtree was created.
    let dir = path.split('/').slice(0, -1).join('/');
    while (dir && !expanded.has(dir)) dir = dir.split('/').slice(0, -1).join('/');
    folders.add(dir);
  }
  for (const dir of folders) void load(dir);
});
watch(() => ui.filePath, path => { if (path) void openFile(path); });

onMounted(() => {
  void load("");
  if (ui.filePath) void openFile(ui.filePath);
});

</script>

<template>
  <div class="files">
    <div v-if="!ui.fileLines" class="tree">
      <div class="tree-head">
        <input v-model="filter" class="filter" placeholder="Filter open folders" spellcheck="false" />
        <button class="icon-btn" title="Refresh" @click="refresh">
          <Icon name="refresh" :size="14" />
        </button>
      </div>
      <div class="rows">
        <div v-if="loading.has('') && !rows.length" class="faint pad">Loading…</div>
        <button
          v-for="{ entry, depth } in rows"
          :key="entry.path"
          class="row"
          :class="{ on: !entry.dir && entry.path === ui.filePath }"
          :style="{ paddingLeft: `${6 + depth * 12}px` }"
          :title="entry.path"
          @click="entry.dir ? toggle(entry) : showFile(entry.path)"
          @contextmenu="menu($event, entry)"
        >
          <Icon
            v-if="entry.dir"
            :name="expanded.has(entry.path) ? 'chevronDown' : 'chevronRight'"
            :size="12"
            class="chev"
          />
          <span v-else class="chev" />
          <Icon :name="entry.dir ? 'folder' : 'file'" :size="13" class="kind" />
          <span class="truncate">{{ entry.name }}</span>
        </button>
      </div>
    </div>

    <div class="viewer" :class="{ reader: ui.fileLines }">
      <div v-if="ui.fileLines" class="reader-head">
        <button class="btn btn-secondary" @click="ui.fileLines = null">Browse files</button>
        <span class="mono truncate" :title="ui.filePath ?? ''">{{ ui.filePath }}</span>
        <span class="faint">{{ ui.fileLines.end === ui.fileLines.start ? `line ${ui.fileLines.start}` : `lines ${ui.fileLines.start}–${ui.fileLines.end}` }}</span>
      </div>
      <template v-if="file">
        <div v-if="!ui.fileLines" class="viewer-head">
          <span class="mono truncate" :title="file.path">{{ file.path }}</span>
          <span class="faint">{{ size(file.size) }}</span>
          <button class="icon-btn" title="Mention in prompt" @click="addToPrompt(mention(file.path))">
            <Icon name="chat" :size="14" />
          </button>
          <button class="icon-btn" title="Show in file manager" @click="fileActions.reveal(absolute(file.path))">
            <Icon name="folder" :size="14" />
          </button>
        </div>
        <div v-if="file.text === null" class="empty">
          <strong>Binary file</strong>
          <span>It cannot be shown as text.</span>
        </div>
        <div v-else class="code">
          <CodeBlock :caption="lang(file.path) || 'text'" :text="file.text" :lang="lang(file.path)" :max-height="100000" :lines="ui.fileLines" />
          <div v-if="file.truncated" class="faint pad">Only the first 512 KB are shown.</div>
        </div>
      </template>
      <div v-else-if="fileError" class="empty">
        <strong>Cannot show the file</strong>
        <span>{{ fileError }}</span>
      </div>
      <div v-else class="empty">
        <strong>No file open</strong>
        <span>Pick a file to read it. Right-click one to mention it in your prompt.</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.files {
  display: flex;
  height: 100%;
  min-height: 0;
  gap: 10px;
}
.viewer.reader { padding: 0; border: 0; }
.reader-head { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; padding-bottom: 8px; font-size: var(--text-sm); }
.tree {
  width: 42%;
  max-width: 260px;
  min-width: 150px;
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.tree-head {
  display: flex;
  align-items: center;
  gap: 4px;
  padding-bottom: 6px;
}
.filter {
  flex: 1;
  min-width: 0;
  height: 26px;
  padding: 0 8px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text);
  font: inherit;
  font-size: var(--text-sm);
  outline: none;
}
.filter:focus {
  border-color: var(--focus-border);
}
.rows {
  flex: 1;
  overflow: auto;
}
.row {
  display: flex;
  align-items: center;
  gap: 5px;
  width: 100%;
  height: 24px;
  padding-right: 6px;
  border-radius: 5px;
  font-size: 12.5px;
  color: var(--text-muted);
  text-align: left;
}
.row:hover {
  background: var(--hover);
  color: var(--text);
}
.row.on {
  background: var(--active);
  color: var(--text);
}
.chev {
  width: 12px;
  flex: none;
  color: var(--text-faint);
}
.kind {
  color: var(--text-faint);
}
.viewer {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  min-height: 0;
  border-left: 1px solid var(--border);
  padding-left: 10px;
}
.viewer-head {
  display: flex;
  align-items: center;
  gap: 6px;
  padding-bottom: 6px;
  font-size: var(--text-sm);
}
.viewer-head .mono {
  flex: 1;
  min-width: 0;
}
.code {
  flex: 1;
  overflow: auto;
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
.empty span,
.faint {
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.pad {
  padding: 6px;
}
</style>
