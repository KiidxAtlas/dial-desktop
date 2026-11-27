<script setup lang="ts">
// The Files panel: the project as a lazily loaded tree beside a viewer for one file. A file can
// be mentioned in the prompt so the agent reads it. Git-ignored entries are hidden unless asked for,
// and changed files carry their Git status letter.
import { computed, onMounted, reactive, ref, watch } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import { files as fileActions, showToast, workspaceChange, workspaceFiles } from "../../bridge";
import { useFileTreeSearch } from "../../fileTreeSearch";
import { openMenu, ui, type MenuItem } from "../../ui";
import { addToPrompt, canGoBackFromFile, goBackFromFile, mention, projectPref, setProjectPref, showFile, showPanel, workspaceDrafts } from "../../workspace";
import { paneError, paneErrorText, type PaneError } from "../../paneErrors";
import { joinPath } from "../../paths";
import { gitMarkers, type GitFileStatus, type GrepMatch } from "../../workspaceGit";
import { workspaceGitApi, type GitDirEntry } from "../../workspaceGitApi";
import type { DirEntry, FileView, OpenSnapshot } from "../../types";
import CodeBlock from "../CodeBlock.vue";
import Icon from "../Icon.vue";

const props = defineProps<{ snap: OpenSnapshot }>();

/** Loaded folders' entries, by relative path (`""` is the root). */
const children = reactive(new Map<string, GitDirEntry[]>());
const expanded = reactive(new Set<string>());
const loading = reactive(new Map<string, number>());
let request = 0, fileRequest = 0, statusRequest = 0;
const { filter, query, paths, status, refreshSearch } = useFileTreeSearch(workspaceFiles.search);
const file = ref<FileView | null>(null);
const fileError = ref<PaneError | null>(null);
const editing = ref(false);
const draft = ref("");
const saving = ref(false);
const dirty = computed(() => {
  if (!editing.value || !file.value) return false;
  const saved = workspaceDrafts.get(props.snap.workspace.root, file.value.path);
  const baseline = saved ? saved.baseText : file.value.text;
  return baseline !== null && draft.value !== baseline;
});
const canEdit = computed(() => !!file.value && file.value.text !== null && !file.value.truncated);
const reader = computed(() => !!ui.fileLines || canGoBackFromFile());
const IGNORED_KEY = "dial.files.showIgnored";
const WIDTH_KEY = "dial.files.treeWidth";
function stored(key: string): string | null {
  try { return localStorage.getItem(key); } catch { return null; }
}
function store(key: string, value: string) {
  try { localStorage.setItem(key, value); } catch { /* Storage may be unavailable. */ }
}
/** List `.gitignore`d entries too (dimmed). */
const showIgnored = ref(stored(IGNORED_KEY) === "1");
/** Changed paths from the latest `git status`; empty outside Git. */
const gitFiles = ref<GitFileStatus[]>([]);
const marker = computed(() => gitMarkers(gitFiles.value));
/** The tree column's width in pixels, dragged with the splitter. */
const treeWidth = ref(Number(stored(WIDTH_KEY)) || 220);

/** Search file contents instead of paths. */
const contentMode = ref(false);
const GREP_LIMIT = 200;
const hits = ref<GrepMatch[]>([]);
const hitStatus = ref("");
const grepRevision = ref(0);
watch([query, contentMode, grepRevision, () => props.snap.workspace.root], ([q, on, , root], _, cleanup) => {
  hits.value = [];
  hitStatus.value = "";
  if (!on || !q) return;
  if (q.length < 2) return void (hitStatus.value = "Type at least two characters");
  hitStatus.value = "Searching…";
  let current = true;
  const timer = setTimeout(async () => {
    try {
      const found = await workspaceGitApi.grep(q, root, GREP_LIMIT);
      if (!current) return;
      hits.value = found;
      hitStatus.value = found.length ? "" : "No file contains that text";
    } catch (error) {
      if (current) hitStatus.value = `Search failed: ${paneErrorText(error)}`;
    }
  }, 250);
  cleanup(() => { current = false; clearTimeout(timer); });
});
function toggleContent() {
  contentMode.value = !contentMode.value;
}

/** Folders open in the tree, remembered per project. */
function saveExpanded() {
  setProjectPref("expanded", props.snap.workspace.root, [...expanded]);
}
function restoreExpanded() {
  expanded.clear();
  const saved = projectPref<unknown>("expanded", props.snap.workspace.root);
  if (Array.isArray(saved)) for (const path of saved) if (typeof path === "string" && path) expanded.add(path);
}
/** Load the root and every remembered open folder; ones that are gone are forgotten quietly. */
function loadTree() {
  void load("");
  for (const path of expanded) void load(path, true);
}

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

async function load(path: string, quiet = false) {
  const root = props.snap.workspace.root, id = ++request;
  loading.set(path, id);
  const current = () => props.snap.workspace.root === root && loading.get(path) === id;
  try {
    const entries = await workspaceGitApi.list(path, root, showIgnored.value);
    if (current()) children.set(path, entries);
  } catch (error) {
    if (!current()) return;
    if (!quiet) showToast(paneErrorText(error));
    else if (expanded.delete(path)) saveExpanded();
  } finally {
    if (current()) loading.delete(path);
  }
}

/** Re-read Git status markers across the workspace's worktrees. */
async function loadStatus() {
  const root = props.snap.workspace.root, id = ++statusRequest;
  try {
    const view = await workspaceGitApi.status(root);
    if (id === statusRequest && props.snap.workspace.root === root) gitFiles.value = view.files;
  } catch {
    // Markers are a hint; the tree still works without them.
    if (id === statusRequest) gitFiles.value = [];
  }
}

function toggleIgnored() {
  showIgnored.value = !showIgnored.value;
  store(IGNORED_KEY, showIgnored.value ? "1" : "0");
  void refresh();
}

function toggle(entry: DirEntry) {
  if (expanded.delete(entry.path)) return saveExpanded();
  expanded.add(entry.path);
  saveExpanded();
  if (!children.has(entry.path)) void load(entry.path);
}

async function confirmDiscardChanges(): Promise<boolean> {
  if (!dirty.value) return true;
  return ask(`Discard unsaved changes to ${file.value?.path ?? "this file"}?`, {
    title: "Unsaved changes",
    kind: "warning",
    okLabel: "Discard",
  });
}

async function openFile(path: string) {
  const root = props.snap.workspace.root;
  // Never overwrite the active file's baseline sha256 while the user is editing it; doing so
  // would defeat optimistic conflict detection on save and clobber external or agent edits.
  if (editing.value && file.value?.path === path) return;
  const id = ++fileRequest;
  const current = () => id === fileRequest && ui.filePath === path && props.snap.workspace.root === root;
  if (file.value?.path !== path) {
    file.value = null;
    editing.value = false;
  }
  fileError.value = null;
  try {
    const result = await workspaceFiles.read(path, root);
    if (current() && !(editing.value && file.value?.path === path)) {
      file.value = result;
      const saved = workspaceDrafts.get(root, path);
      if (saved && result.text !== null) {
        draft.value = saved.text;
        editing.value = true;
      } else if (result.text !== null) {
        draft.value = result.text;
        editing.value = false;
      }
    }
  } catch (error) {
    if (!current()) return;
    file.value = null;
    editing.value = false;
    fileError.value = paneError(error);
  }
}

function startEdit() {
  if (!file.value || file.value.text === null || file.value.truncated) return;
  const saved = workspaceDrafts.get(props.snap.workspace.root, file.value.path);
  draft.value = saved ? saved.text : file.value.text;
  editing.value = true;
}

async function cancelEdit() {
  if (!(await confirmDiscardChanges())) return;
  if (file.value) workspaceDrafts.clear(props.snap.workspace.root, file.value.path);
  editing.value = false;
  if (file.value?.text !== null && file.value?.text !== undefined) draft.value = file.value.text;
}

function closeFile() {
  editing.value = false;
  ui.filePath = null;
}

async function saveEdit() {
  if (!file.value || !editing.value || saving.value) return;
  const path = file.value.path, root = props.snap.workspace.root;
  const saved = workspaceDrafts.get(root, path);
  const expectedSha = saved?.baseSha256 ?? file.value.sha256;
  saving.value = true;
  try {
    const updated = await workspaceGitApi.write(path, draft.value, root, expectedSha);
    workspaceDrafts.clear(root, path);
    if (props.snap.workspace.root === root && ui.filePath === path) {
      file.value = updated;
      draft.value = updated.text ?? draft.value;
      editing.value = false;
    }
    showToast(`Saved ${path}`);
    void loadStatus();
  } catch (error) {
    showToast(paneErrorText(error));
  } finally {
    saving.value = false;
  }
}

watch(draft, (text) => {
  if (!editing.value || !file.value || file.value.text === null) return;
  workspaceDrafts.update(
    props.snap.workspace.root,
    file.value.path,
    text,
    file.value.text,
    file.value.sha256,
  );
});
function onEditorKeydown(event: KeyboardEvent) {
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
    event.preventDefault();
    if (dirty.value) void saveEdit();
  } else if (event.key === "Escape") {
    event.preventDefault();
    void cancelEdit();
  }
}

/** Reload every open folder (Dial may have added or removed files). */
async function refresh() {
  refreshSearch();
  grepRevision.value++;
  const open = ["", ...expanded];
  children.clear();
  loading.clear();
  await Promise.all([
    ...open.map((path) => load(path, path !== "")),
    loadStatus(),
    ui.filePath && !editing.value && openFile(ui.filePath),
  ]);
}
/** Search rows show ranked project-relative paths; browsing keeps the expanded tree intact. */
const rows = computed(() => {
  const out: { entry: GitDirEntry; depth: number }[] = [];
  if (query.value) return contentMode.value ? [] : paths.value.map(path => ({ entry: { path, name: path, dir: false } as GitDirEntry, depth: 0 }));
  const walk = (path: string, depth: number) => {
    for (const entry of children.get(path) ?? []) {
      out.push({ entry, depth });
      // A folder may have become a link since its children were loaded. Never walk back up.
      if (entry.dir && expanded.has(entry.path) && entry.path && (!path || entry.path.startsWith(`${path}/`))) walk(entry.path, depth + 1);
    }
  };
  walk("", 0);
  return out;
});

function absolute(path: string) {
  return joinPath(props.snap.workspace.root, path);
}

function openInEditor(path: string, line?: number) {
  workspaceGitApi.open(path, props.snap.workspace.root, line).catch((error) => showToast(paneErrorText(error)));
}

function showChanges(path: string) {
  ui.diffPath = path;
  showPanel("changes");
}
async function createWorkspaceFile(rawPath: string) {
  const path = rawPath.trim().replace(/^\/+/, "");
  if (!path) return;
  const root = props.snap.workspace.root;
  try {
    const created = await workspaceGitApi.createFile(path, "", root);
    if (root !== props.snap.workspace.root) return;
    await refresh();
    ui.filePath = created.path;
    file.value = created;
    draft.value = created.text ?? "";
    editing.value = true;
    showToast(`Created ${created.path}`);
  } catch (error) {
    showToast(paneErrorText(error));
  }
}

async function renameEntry(from: string, rawTo: string) {
  const to = rawTo.trim().replace(/^\/+/, "");
  if (!to || to === from) return;
  const root = props.snap.workspace.root;
  try {
    const next = await workspaceGitApi.renameFile(from, to, root);
    if (root !== props.snap.workspace.root) return;
    workspaceDrafts.rename(root, from, next);
    if (ui.filePath === from) ui.filePath = next;
    await refresh();
    showToast(`Renamed to ${next}`);
  } catch (error) {
    showToast(paneErrorText(error));
  }
}

async function deleteEntry(entryPath: string, isDir: boolean) {
  const root = props.snap.workspace.root;
  const confirmed = await ask(
    isDir
      ? `Delete empty folder ${entryPath}?`
      : `Delete ${entryPath}? This removes the file from disk.`,
    { title: isDir ? "Delete folder" : "Delete file", kind: "warning", okLabel: "Delete" },
  );
  if (!confirmed || root !== props.snap.workspace.root) return;
  try {
    await workspaceGitApi.deleteFile(entryPath, root);
    if (root !== props.snap.workspace.root) return;
    workspaceDrafts.clear(root, entryPath);
    if (ui.filePath === entryPath) {
      editing.value = false;
      file.value = null;
      ui.filePath = null;
    }
    await refresh();
    showToast(`Deleted ${entryPath}`);
  } catch (error) {
    showToast(paneErrorText(error));
  }
}

function newFileMenu(event: MouseEvent, dir = "") {
  const prefix = dir ? `${dir}/` : "";
  openMenu(
    event,
    [
      {
        label: dir ? `New file in ${dir}` : "New file",
        icon: "plus",
        prompt: {
          label: "File path",
          placeholder: `${prefix}example.ts`,
          maxLength: 240,
          confirm: "Create",
          submit: (value) => void createWorkspaceFile(value),
        },
      },
    ],
    event.currentTarget as HTMLElement,
  );
}

function menu(event: MouseEvent, entry: GitDirEntry) {
  const changed = !entry.dir && !!marker.value(entry.path, false);
  const items: MenuItem[] = [
    { label: "Mention in prompt", icon: "chat", run: () => addToPrompt(mention(entry.path)) },
    ...(entry.dir
      ? [{
          label: "New file here…",
          icon: "plus",
          prompt: {
            label: "New file path",
            placeholder: `${entry.path}/file.ts`,
            maxLength: 240,
            confirm: "Create",
            submit: (value) => void createWorkspaceFile(value.includes("/") ? value : `${entry.path}/${value}`),
          },
        } as MenuItem]
      : [{ label: "Open in editor", icon: "external", run: () => openInEditor(entry.path) }]),
    ...(changed ? [{ label: "Show changes", icon: "diff", run: () => showChanges(entry.path) }] : []),
    {
      label: "Rename…",
      icon: "file",
      prompt: {
        label: `Rename ${entry.path}`,
        placeholder: entry.path,
        maxLength: 240,
        confirm: "Rename",
        submit: (value) => void renameEntry(entry.path, value),
      },
    },
    {
      label: "Copy path",
      icon: "copy",
      run: () => navigator.clipboard.writeText(entry.path).catch(() => showToast("Could not copy to the clipboard")),
    },
    { label: "Show in file manager", icon: "folder", run: () => fileActions.reveal(absolute(entry.path)) },
    { divider: true },
    { label: entry.dir ? "Delete empty folder…" : "Delete file…", icon: "trash", danger: true, run: () => void deleteEntry(entry.path, entry.dir) },
  ];
  openMenu(event, items);
}
/** Drag the splitter to resize the tree; the width is kept for next time. */
function startResize(event: PointerEvent) {
  event.preventDefault();
  const handle = event.currentTarget as HTMLElement;
  const left = (handle.parentElement?.getBoundingClientRect().left ?? 0);
  handle.setPointerCapture(event.pointerId);
  const move = (e: PointerEvent) => { treeWidth.value = Math.round(Math.min(600, Math.max(140, e.clientX - left))); };
  const stop = () => {
    handle.removeEventListener("pointermove", move);
    handle.removeEventListener("pointerup", stop);
    store(WIDTH_KEY, String(treeWidth.value));
  };
  handle.addEventListener("pointermove", move);
  handle.addEventListener("pointerup", stop);
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
    filter.value = "";
    children.clear();
    loading.clear();
    fileRequest++;
    fileError.value = null;
    ui.fileLines = null;
    restoreExpanded();
    file.value = null;
    editing.value = false;
    ui.filePath = null;
    loadTree();
    void loadStatus();
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
    if (path === ui.filePath && !editing.value) void openFile(path);
    // Refresh the nearest visible parent, including when a new subtree was created.
    let dir = path.split('/').slice(0, -1).join('/');
    while (dir && !expanded.has(dir)) dir = dir.split('/').slice(0, -1).join('/');
    folders.add(dir);
  }
  if (affected.length) { refreshSearch(); void loadStatus(); }
  for (const dir of folders) void load(dir);
});
watch(() => ui.filePath, (path) => {
  if (path) void openFile(path);
});
watch(workspaceChange, ({ root }) => {
  if (!root || root !== props.snap.workspace.root) return;
  refreshSearch();
  void loadStatus();
  for (const dir of ["", ...expanded]) void load(dir, dir !== "");
  if (ui.filePath && !editing.value) void openFile(ui.filePath);
});

onMounted(() => {
  restoreExpanded();
  loadTree();
  void loadStatus();
  if (ui.filePath) void openFile(ui.filePath);
});

</script>

<template>
  <div class="files">
    <div v-show="!reader" class="tree" :class="{ solo: !ui.filePath }" :style="ui.filePath ? { width: `${treeWidth}px` } : undefined">
      <div class="tree-head">
        <input v-model="filter" type="search" class="filter" :aria-label="contentMode ? 'Search inside project files' : 'Search project files'" :placeholder="contentMode ? 'Search inside files' : 'Search project files'" spellcheck="false" />
        <!-- Says what is searched, like the sidebar's Titles / Messages switch. -->
        <button class="icon-btn contents" :class="{ on: contentMode }" :aria-pressed="contentMode" :title="contentMode ? 'Searching file contents; click to search file names' : 'Searching file names; click to search file contents'" @click="toggleContent">
          {{ contentMode ? "Contents" : "Names" }}
        </button>
        <button class="icon-btn" :class="{ on: showIgnored }" :aria-pressed="showIgnored" :title="showIgnored ? 'Hide files .gitignore excludes' : 'Show files .gitignore excludes'" @click="toggleIgnored">
          <Icon name="files" :size="14" />
        </button>
        <button class="icon-btn" title="New file" aria-label="New file" @click="newFileMenu($event)">
          <Icon name="plus" :size="14" />
        </button>
        <button class="icon-btn" title="Refresh" @click="refresh">
          <Icon name="refresh" :size="14" />
        </button>
      </div>
      <div class="rows">
        <template v-if="query && contentMode">
          <div v-if="hitStatus" class="faint pad" role="status">{{ hitStatus }}</div>
          <button
            v-for="hit in hits"
            :key="`${hit.path}:${hit.line}`"
            class="row hit"
            :class="{ on: hit.path === ui.filePath }"
            :title="`${hit.path}:${hit.line}`"
            @click="showFile(hit.path, { start: hit.line, end: hit.line }, $event.currentTarget as HTMLElement)"
          >
            <span class="mono truncate hit-where">{{ hit.path }}:{{ hit.line }}</span>
            <span class="mono truncate hit-text">{{ hit.text }}</span>
          </button>
          <div v-if="hits.length >= GREP_LIMIT" class="faint pad">Showing the first {{ GREP_LIMIT }} matches; type more to narrow them.</div>
        </template>
        <div v-else-if="query && status" class="faint pad" role="status">{{ status }}</div>
        <div v-else-if="!query && loading.has('') && !rows.length" class="faint pad">Loading…</div>
        <button
          v-for="{ entry, depth } in rows"
          :key="entry.path"
          class="row"
          :class="{ on: !entry.dir && entry.path === ui.filePath, ignored: entry.ignored }"
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
          <span v-if="!entry.dir && workspaceDrafts.isDirty(snap.workspace.root, entry.path)" class="draft-dot" title="Unsaved draft">●</span>
          <span v-if="marker(entry.path, entry.dir)" class="git" :class="'git-' + marker(entry.path, entry.dir)!.code" :title="marker(entry.path, entry.dir)!.title">{{ marker(entry.path, entry.dir)!.text }}</span>
        </button>
      </div>
    </div>
    <div v-if="!reader && ui.filePath" class="splitter" title="Drag to resize" @pointerdown="startResize" />

    <!-- The viewer shows once a file is picked; until then the tree has the whole panel. -->
    <div v-if="ui.filePath || ui.fileLines" class="viewer" :class="{ reader }">
      <div v-if="reader" class="reader-head">
        <button v-if="canGoBackFromFile()" class="btn btn-secondary" @click="goBackFromFile">Back</button>
        <button class="btn btn-secondary" @click="showPanel('files')">Browse files</button>
        <span class="mono truncate" :title="ui.filePath ?? ''">{{ ui.filePath }}</span>
        <span v-if="ui.fileLines" class="faint">{{ ui.fileLines.end === ui.fileLines.start ? `line ${ui.fileLines.start}` : `lines ${ui.fileLines.start}–${ui.fileLines.end}` }}</span>
        <button v-if="canEdit && !editing" class="btn btn-secondary" @click="startEdit">Edit</button>
        <template v-else-if="editing">
          <button class="btn btn-secondary" :disabled="saving" @click="cancelEdit">Cancel</button>
          <button class="btn" :disabled="!dirty || saving" @click="saveEdit">{{ saving ? "Saving…" : "Save" }}</button>
        </template>
        <button v-if="ui.filePath && ui.fileLines && file && !editing" class="btn btn-secondary" @click="openInEditor(ui.filePath, ui.fileLines.start)">Open line in VS Code</button>
        <button v-if="ui.filePath && file" class="icon-btn" title="Open in default app" aria-label="Open in default app" @click="openInEditor(ui.filePath)"><Icon name="external" :size="14" /></button>
      </div>
      <template v-if="file">
        <div v-if="!reader" class="viewer-head">
          <span class="mono truncate" :title="file.path">{{ file.path }}</span>
          <span class="faint">{{ size(file.size) }}</span>
          <button v-if="canEdit && !editing" class="btn btn-secondary" title="Edit this file inline" @click="startEdit">Edit</button>
          <template v-else-if="editing">
            <button class="btn btn-secondary" :disabled="saving" @click="cancelEdit">Cancel</button>
            <button class="btn" :disabled="!dirty || saving" @click="saveEdit">{{ saving ? "Saving…" : "Save" }}</button>
          </template>
          <button class="icon-btn" title="Mention in prompt" @click="addToPrompt(mention(file.path))">
            <Icon name="chat" :size="14" />
          </button>
          <button v-if="marker(file.path, false)" class="icon-btn" title="Show changes" @click="showChanges(file.path)">
            <Icon name="diff" :size="14" />
          </button>
          <button class="icon-btn" title="Open in editor (the system's default app)" @click="openInEditor(file.path)">
            <Icon name="external" :size="14" />
          </button>
          <button class="icon-btn" title="Show in file manager" @click="fileActions.reveal(absolute(file.path))">
            <Icon name="folder" :size="14" />
          </button>
          <button class="icon-btn" title="Close the file" aria-label="Close the file" @click="closeFile()">
            <Icon name="cross" :size="14" />
          </button>
        </div>
        <div v-if="file.text === null" class="empty">
          <strong>Binary file</strong>
          <span>It cannot be shown as text.</span>
          <button class="btn btn-secondary" @click="openInEditor(file.path)">Open in its app</button>
        </div>
        <div v-else-if="editing" class="code editing">
          <textarea
            v-model="draft"
            class="editor mono"
            :aria-label="`Edit ${file.path}`"
            spellcheck="false"
            @keydown="onEditorKeydown"
          />
        </div>
        <div v-else class="code">
          <CodeBlock :caption="lang(file.path) || 'text'" :text="file.text" :lang="lang(file.path)" :max-height="100000" :lines="ui.fileLines" />
          <div v-if="file.truncated" class="faint pad">Only the first 512 KB are shown.</div>
        </div>
      </template>
      <div v-else-if="fileError" class="empty">
        <strong>Cannot show the file: {{ fileError.title.toLowerCase() }}</strong>
        <span v-if="fileError.hint">{{ fileError.hint }}</span>
        <span class="mono detail">{{ fileError.detail }}</span>
      </div>
      <div v-else class="empty faint" role="status">Opening…</div>
    </div>
  </div>
</template>

<style scoped>
.files {
  display: flex;
  height: 100%;
  min-height: 0;
}
.splitter {
  flex: none;
  width: 9px;
  margin: 0 -1px 0 1px;
  cursor: col-resize;
  border-right: 1px solid var(--border);
}
.splitter:hover {
  border-right-color: var(--border-strong);
}
.row.ignored {
  opacity: 0.55;
}
.git {
  flex: none;
  margin-left: auto;
  padding-left: 4px;
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--warning);
}
.git-A,
.git-U {
  color: var(--success);
}
.git-D,
.git-C {
  color: var(--danger);
}
.git-dir {
  color: var(--text-faint);
}
.icon-btn.on {
  background: var(--active);
  color: var(--text);
}
.icon-btn.contents {
  width: auto;
  padding: 0 6px;
  font-size: 11px;
  font-weight: 500;
}
.row.hit {
  flex-direction: column;
  align-items: stretch;
  justify-content: center;
  gap: 0;
  height: auto;
  padding: 3px 6px;
}
.hit-where {
  font-size: 11px;
  color: var(--text-faint);
}
.hit-text {
  font-size: 12px;
}
.detail {
  max-width: 100%;
  overflow-wrap: anywhere;
  font-size: var(--text-xs);
}
.viewer.reader { padding: 0; border: 0; }
.reader-head { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; padding-bottom: 8px; font-size: var(--text-sm); }
.tree {
  max-width: 70%;
  min-width: 140px;
  flex: none;
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.tree.solo {
  flex: 1;
  max-width: none;
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
.code.editing {
  display: flex;
  overflow: hidden;
}
.editor {
  flex: 1;
  width: 100%;
  height: 100%;
  resize: none;
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 10px 12px;
  background: var(--surface);
  color: var(--text);
  font-size: 12.5px;
  line-height: 1.5;
  tab-size: 2;
}
.editor:focus {
  outline: none;
  border-color: var(--border-strong);
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
.draft-dot {
  flex: none;
  font-size: 9px;
  color: var(--accent, var(--text-muted));
}
.pad {
  padding: 6px;
}
</style>
