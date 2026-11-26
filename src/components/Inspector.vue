<script setup lang="ts">
import { computed, nextTick, reactive, ref, watch } from "vue";
import type { Change, OpenSnapshot, ToolEntry } from "../types";
import { openMenu, ui, type InspectorTab } from "../ui";
import { addToPrompt, inspectorWidth, mention, MIN_WIDTH, projectPref, setInspectorWidth, setProjectPref, showPanel } from "../workspace";
import { changeRows, gitLabel, inNestedRepo, otherNestedChanges, stagedCount, stageState, type GitFileStatus } from "../workspaceGit";
import { workspaceGitApi, type CommitReview } from "../workspaceGitApi";
import { showToast } from "../bridge";
import { aiReviewView, gitLogView, worktreesView } from "../commandRunner";
import { ask } from "@tauri-apps/plugin-dialog";
import { needsGitIdentity, paneErrorText } from "../paneErrors";
import Icon from "./Icon.vue";
import BrowserPane from "./workspace/BrowserPane.vue";
import DiffPane from "./workspace/DiffPane.vue";
import FilesPane from "./workspace/FilesPane.vue";
import TerminalPane from "./workspace/TerminalPane.vue";
import AppPane from "./workspace/AppPane.vue";
import JobDiscovery from './JobDiscovery.vue';
import ActivityWatch from './ActivityWatch.vue';
import { toolFiles } from "../toolFolds";

const props = defineProps<{ snap: OpenSnapshot; open: boolean }>();

const pinned = [
  { id: "plan", label: "Plan", icon: "list" },
  { id: "changes", label: "Changes", icon: "diff" },
] as const;
const rosters = [
  { id: "workers", label: "Workers", icon: "chip" },
  { id: "peers", label: "Peers", icon: "chat" },
] as const;
/** Workspace tools: one click each, shown as icons beside the two main tabs. */
const toolTabs = [
  { id: "files", label: "Files", icon: "files" },
  { id: "terminal", label: "Terminal", icon: "terminal" },
  { id: "browser", label: "Browser", icon: "globe" },
] as const;
/** Less used panels, in the overflow menu. */
const auxiliary = [
  { id: "activity", label: "Activity", icon: "activity" },
  ...rosters,
  { id: "app", label: "App", icon: "monitor" },
] as const;
const watchKind = computed(() => ui.inspectorTab === "workers" ? "worker" : ui.inspectorTab === "peers" ? "peer" : null);
const modifier = navigator.platform.includes("Mac") ? "⌘" : "Ctrl+";
const hints: Partial<Record<InspectorTab, string>> = { files: `${modifier}⇧E`, terminal: modifier === "⌘" ? "⌃`" : "Ctrl+`", browser: `${modifier}⇧B` };
const panelButton = ref<HTMLButtonElement>();
function panelMenu(event: MouseEvent) {
  openMenu(event, auxiliary.map(tab => ({ label: tab.label, icon: tab.icon, hint: hints[tab.id], run: () => showPanel(tab.id) })), event.currentTarget as HTMLElement);
}

/** Heavy panels mount on first visit and stay mounted, so a shell or a page survives tab switches. */
const visited = reactive(new Set<InspectorTab>());
watch(
  () => [ui.inspectorTab, props.open] as const,
  ([tab, open]) => open && visited.add(tab),
  { immediate: true },
);
const shown = (tab: InspectorTab) => props.open && !ui.settingsOpen && ui.inspectorTab === tab;

const done = computed(() => props.snap.plan.filter((t) => t.status === "completed").length);
const progress = computed(() => (props.snap.plan.length ? (done.value / props.snap.plan.length) * 100 : 0));
/** Activity shows the newest calls first, a page at a time. */
const ACTIVITY_PAGE = 50;
const activityLimit = ref(ACTIVITY_PAGE);
const allTools = computed(() => props.snap.entries.filter((e): e is ToolEntry => e.kind === "tool").reverse());
const tools = computed(() => allTools.value.slice(0, activityLimit.value));
watch(() => props.snap.workspace.root, () => { activityLimit.value = ACTIVITY_PAGE; });

/** Git status with +/- counts for the Changes list; refreshed when it is shown or a file changes. */
const gitFiles = ref<GitFileStatus[]>([]);
/** Whether the project is a git repository; `false` until a status says so. */
const gitRepo = ref(false);
const nestedRepos = ref<string[]>([]);
/** The checked-out branch from the latest status. */
const gitBranch = ref<string | null>(null);
/** Files with something staged, across the whole project (a commit takes them all). */
const staged = computed(() => stagedCount(gitFiles.value.filter(f => !inNestedRepo(f.path, nestedRepos.value))));
const commitMessage = ref("");
/** A stage, unstage or commit is running; the controls wait for it. */
const gitBusy = ref(false);
const review = ref<CommitReview | null>(null);
let reviewRevision = 0;
function dismissReview() {
  reviewRevision++;
  review.value = null;
}
watch([() => props.snap.workspace.root, () => props.snap.sessionId, commitMessage, () => shown("changes")], dismissReview, { flush: "sync" });
watch(() => props.snap.workspace.root, () => { commitMessage.value = ""; author.value = null; });
let gitRequest = 0;
async function loadGit() {
  const root = props.snap.workspace.root, id = ++gitRequest;
  try {
    const view = await workspaceGitApi.status(root, true);
    if (id === gitRequest && root === props.snap.workspace.root) {
      gitFiles.value = view.files;
      nestedRepos.value = view.nestedRepos ?? [];
      gitRepo.value = view.repo;
      gitBranch.value = view.branch ?? null;
    }
  } catch {
    if (id === gitRequest) {
      gitFiles.value = [];
      nestedRepos.value = [];
      gitRepo.value = false;
      gitBranch.value = null;
    }
  }
}

/** Stage a file whose changes are not all staged; unstage it otherwise. */
async function toggleStaged(path: string, git: GitFileStatus) {
  const root = props.snap.workspace.root;
  dismissReview();
  gitBusy.value = true;
  try {
    await workspaceGitApi.stage([path], stageState(git) !== true, root);
  } catch (e) {
    showToast(paneErrorText(e));
  } finally {
    gitBusy.value = false;
    if (root === props.snap.workspace.root) await loadGit();
  }
}

/** Read the entire index, including paths untouched by this session. Confirmation is separate. */
async function reviewStaged() {
  if (gitBusy.value || !commitMessage.value.trim()) return;
  const root = props.snap.workspace.root, revision = ++reviewRevision;
  gitBusy.value = true;
  try {
    const result = await workspaceGitApi.review(root);
    if (revision === reviewRevision) review.value = result;
  } catch (e) {
    if (revision === reviewRevision) showToast(paneErrorText(e));
  } finally {
    gitBusy.value = false;
  }
}

async function commitStaged() {
  const reviewed = review.value;
  if (gitBusy.value || !reviewed || !commitMessage.value.trim()) return;
  const root = props.snap.workspace.root, message = commitMessage.value;
  gitBusy.value = true;
  try {
    const sha = await workspaceGitApi.commit(message, root, reviewed);
    if (root === props.snap.workspace.root && commitMessage.value === message) commitMessage.value = "";
    showToast(`Committed ${sha}`);
  } catch (e) {
    if (needsGitIdentity(e)) void askAuthor(root);
    else showToast(paneErrorText(e));
  } finally {
    dismissReview();
    gitBusy.value = false;
    if (root === props.snap.workspace.root) await loadGit();
  }
}

/** Fill the message box with the engine's message for what is staged (`dial commit --dry-run`). */
const writing = ref(false);
const aiWarning = ref("");
watch(commitMessage, () => { if (!writing.value) aiWarning.value = ""; });
async function writeMessage() {
  if (writing.value || gitBusy.value || !staged.value) return;
  if (commitMessage.value.trim() && !(await ask("Replace the message you wrote with one from Dial?", { title: "Write with AI", kind: "warning", okLabel: "Replace" }))) return;
  const root = props.snap.workspace.root;
  writing.value = true;
  try {
    const preview = await workspaceGitApi.aiMessage(root);
    if (root !== props.snap.workspace.root) return;
    commitMessage.value = preview.message;
    aiWarning.value = preview.warning ?? "";
  } catch (e) {
    showToast(`Could not write a message: ${paneErrorText(e)}`);
  } finally {
    writing.value = false;
  }
}

/** Git refused a commit for want of an author: asked for under the message, then commit again. */
const author = ref<{ name: string; email: string; everywhere: boolean; saving: boolean; error: string } | null>(null);
async function askAuthor(root: string) {
  const known = await workspaceGitApi.identity(root).catch(() => ({ name: "", email: "" }));
  if (root === props.snap.workspace.root) author.value = { ...known, everywhere: true, saving: false, error: "" };
}
async function saveAuthor() {
  const form = author.value, root = props.snap.workspace.root;
  if (!form || form.saving || !form.name.trim() || !form.email.trim()) return;
  form.saving = true;
  form.error = "";
  try {
    const saved = await workspaceGitApi.setIdentity(form.name, form.email, form.everywhere, root);
    if (author.value === form) author.value = null;
    showToast(`Commits are now recorded as ${saved.name} <${saved.email}>. Review and commit again.`);
  } catch (e) {
    form.error = String(e).replace(/^Error:\s*/, "");
  } finally {
    form.saving = false;
  }
}
watch(
  [() => shown("changes"), () => props.snap.workspace.root, () => props.snap.changes.map((c) => c.revision).join()],
  ([visible]) => { if (visible) void loadGit(); },
  { immediate: true },
);
/** Session-touched files; those git no longer reports as changed (committed or reverted) last. */
const sessionPath = (c: Change) => (c.dir + c.name).replaceAll("\\", "/");
const rows = computed(() => changeRows(props.snap.changes, sessionPath, gitFiles.value, gitRepo.value, nestedRepos.value));
const nestedChanges = computed(() => otherNestedChanges(gitFiles.value, nestedRepos.value, props.snap.changes.map(sessionPath)));
const pending = computed(() => rows.value.filter((r) => !r.settled).length);
/** Line counts Dial's own calls reported, for rows git has no numbers for (no repository yet). */
const reported = computed(() => toolFiles([...allTools.value].reverse()));
function reportedFor(change: Change) {
  const path = sessionPath(change);
  return reported.value.find((f) => { const p = f.path.replaceAll("\\", "/"); return p === path || p.endsWith(`/${path}`) || path.endsWith(`/${p}`); });
}
const width = computed(() => inspectorWidth());
/** Show a tool call in the conversation, opened, and flash it. */
async function reveal(key: string) {
  ui.expanded.add(key);
  await nextTick();
  const row = document.querySelector<HTMLElement>(`[data-tool="${CSS.escape(key)}"]`);
  if (!row) return;
  row.scrollIntoView({ block: "center", behavior: "smooth" });
  row.classList.remove("flash");
  void row.offsetWidth;
  row.classList.add("flash");
}
const expanded = computed(() => shown("changes") && !!ui.diffPath && ui.diffExpanded);
// Opening a diff restores this project's last Expand/Shrink choice.
watch(
  () => ui.diffPath,
  (path, before) => {
    if (path && !before) ui.diffExpanded = projectPref<boolean>("diffExpanded", props.snap.workspace.root) === true;
  },
  { immediate: true },
);
function toggleExpanded() {
  ui.diffExpanded = !ui.diffExpanded;
  setProjectPref("diffExpanded", props.snap.workspace.root, ui.diffExpanded);
}
const fileIndex = computed(() => props.snap.changes.findIndex((file) => file.path === ui.diffPath));
function moveFile(step: -1 | 1) {
  if (fileIndex.value < 0) return;
  const file = props.snap.changes[fileIndex.value + step];
  if (file) ui.diffPath = file.path;
}

function taskClass(status: string) {
  if (["abandoned", "cancelled", "blocked"].includes(status)) return "dropped";
  return status;
}

/** `text` split on backticks: odd parts were code spans. An unmatched backtick stays literal. */
function codeParts(text: string): { text: string; code: boolean }[] {
  const parts = text.split("`");
  if (parts.length % 2 === 0) return [{ text, code: false }];
  return parts.map((part, i) => ({ text: part, code: i % 2 === 1 })).filter((p) => p.text);
}

/** Drag the left edge to resize. */
function startResize(event: PointerEvent) {
  event.preventDefault();
  const handle = event.currentTarget as HTMLElement;
  handle.setPointerCapture(event.pointerId);
  const move = (e: PointerEvent) => setInspectorWidth(window.innerWidth - e.clientX);
  const stop = () => {
    handle.removeEventListener("pointermove", move);
    handle.removeEventListener("pointerup", stop);
    document.body.classList.remove("resizing");
  };
  document.body.classList.add("resizing");
  handle.addEventListener("pointermove", move);
  handle.addEventListener("pointerup", stop);
}

/** Arrow keys on the focused edge resize too: Left widens, Right narrows; Shift takes bigger steps. */
function resizeByKey(event: KeyboardEvent) {
  const step = event.shiftKey ? 64 : 16;
  if (event.key === "ArrowLeft") setInspectorWidth(width.value + step);
  else if (event.key === "ArrowRight") setInspectorWidth(width.value - step);
  else if (event.key === "Home") setInspectorWidth(MIN_WIDTH);
  else if (event.key === "End") setInspectorWidth(Number.MAX_SAFE_INTEGER);
  else return;
  event.preventDefault();
}
</script>

<template>
  <aside class="inspector" :class="{ 'wide-review': expanded }" :style="{ '--inspector-width': `${width}px` }">
    <div
      v-if="!expanded"
      class="resize"
      role="separator"
      tabindex="0"
      aria-orientation="vertical"
      aria-label="Resize the panel"
      :aria-valuenow="width"
      :aria-valuemin="MIN_WIDTH"
      title="Drag or use the arrow keys to resize"
      @pointerdown="startResize"
      @keydown="resizeByKey"
    />
    <div class="drag" data-tauri-drag-region />
    <div class="tabs" role="group" aria-label="Workspace panels">
      <button
        v-for="tab in pinned"
        :key="tab.id"
        :title="tab.label"
        :aria-label="tab.label"
        :aria-pressed="ui.inspectorTab === tab.id"
        :class="{ active: ui.inspectorTab === tab.id }"
        @click="showPanel(tab.id)"
      >
        {{ tab.label }}
        <span v-if="tab.id === 'changes' && snap.changes.length" class="badge">{{ snap.changes.length }}</span>
      </button>
      <span class="grow" />
      <button
        v-for="tab in toolTabs"
        :key="tab.id"
        class="tool"
        :title="hints[tab.id] ? `${tab.label} (${hints[tab.id]})` : tab.label"
        :aria-label="tab.label"
        :aria-pressed="ui.inspectorTab === tab.id"
        :class="{ active: ui.inspectorTab === tab.id }"
        @click="showPanel(tab.id)"
      >
        <Icon :name="tab.icon" :size="15" />
      </button>
      <button class="tool" title="More panels" aria-label="More panels" aria-haspopup="menu" :aria-expanded="ui.menu?.anchor === panelButton" ref="panelButton" :class="{ active: auxiliary.some(t => t.id === ui.inspectorTab) }" @click="panelMenu">
        <template v-if="auxiliary.some(t => t.id === ui.inspectorTab)">{{ auxiliary.find(t => t.id === ui.inspectorTab)?.label }}</template>
        <Icon name="more" :size="15" />
      </button>
    </div>

    <div v-show="['files', 'terminal', 'browser', 'app'].includes(ui.inspectorTab)" class="panel">
      <FilesPane v-if="visited.has('files')" v-show="ui.inspectorTab === 'files'" :snap="snap" />
      <KeepAlive>
        <TerminalPane
          v-if="visited.has('terminal')"
          :key="snap.workspace.root"
          v-show="ui.inspectorTab === 'terminal'"
          :snap="snap"
          :active="shown('terminal')"
        />
      </KeepAlive>
      <BrowserPane v-if="visited.has('browser')" v-show="ui.inspectorTab === 'browser'" :active="shown('browser')" />
      <AppPane v-if="visited.has('app')" v-show="ui.inspectorTab === 'app'" :root="snap.workspace.root" :active="shown('app')" />
    </div>

    <div v-if="ui.inspectorTab === 'changes' && ui.diffPath" class="panel">
      <DiffPane
        :path="ui.diffPath" :root="snap.workspace.root" :expanded="expanded"
        :revision="snap.changes[fileIndex]?.revision"
        :position="fileIndex < 0 ? null : { index: fileIndex, total: snap.changes.length }"
        @back="ui.diffPath = null" @expand="toggleExpanded" @move="moveFile"
      />
    </div>

    <div v-else-if="watchKind || ['plan', 'changes', 'activity'].includes(ui.inspectorTab)" class="body">
      <!-- Plan -->
      <template v-if="ui.inspectorTab === 'plan'">
        <div v-if="!snap.plan.length" class="empty">
          <strong>No plan yet</strong>
          <span>Dial shares its task list here as it works.</span>
        </div>
        <template v-else>
          <!-- The tab already says "Plan": the head carries only the progress. -->
          <div class="head progress-head">
            <div class="bar"><span :style="{ width: `${progress}%` }" /></div>
            <span class="faint">{{ done }} of {{ snap.plan.length }} done</span>
          </div>
          <ul class="tasks">
            <li v-for="(task, i) in snap.plan" :key="i" :class="taskClass(task.status)">
              <span class="mark">
                <Icon v-if="task.status === 'completed'" name="check" :size="11" />
                <span v-else-if="task.status === 'in_progress'" class="spinner" />
                <Icon v-else-if="taskClass(task.status) === 'dropped'" name="cross" :size="11" />
              </span>
              <span class="text"><template v-for="(part, j) in codeParts(task.text)" :key="j"><code v-if="part.code">{{ part.text }}</code><template v-else>{{ part.text }}</template></template></span>
            </li>
          </ul>
        </template>
      </template>

      <!-- Changes -->
      <template v-else-if="ui.inspectorTab === 'changes'">
        <!-- Turn results live in the conversation, under the turn they describe. -->
        <div v-if="!snap.changes.length && !nestedChanges.length" class="empty">
          <strong>No changes yet</strong>
          <span>Files Dial edits in this session appear here.</span>
        </div>
        <template v-if="snap.changes.length">
          <div class="head">
            <span class="section-label">{{ snap.changes.length }} {{ snap.changes.length === 1 ? "file" : "files" }} edited</span>
            <button class="btn btn-secondary" @click="addToPrompt('Review the changes to these files for bugs and missing tests:\n' + snap.changes.map(c => mention(c.path)).join('\n'))">Review files</button>
          </div>
          <p class="faint" title="Diffs show every current change to a file, not just Dial’s.">
            Files Dial edited in this session<template v-if="gitRepo && pending < snap.changes.length">; {{ pending }} still {{ pending === 1 ? "differs" : "differ" }} from the last commit</template>.
          </p>
          <div v-for="{ item: change, git, settled } in rows" :key="change.path" class="file-row">
            <input
              v-if="git && !inNestedRepo(git.path, nestedRepos)"
              type="checkbox"
              class="stage"
              :checked="stageState(git) === true"
              :indeterminate="stageState(git) === 'mixed'"
              :disabled="gitBusy"
              :aria-label="`${stageState(git) === true ? 'Unstage' : 'Stage'} ${change.path}`"
              :title="stageState(git) === true ? 'Staged for the next commit; click to unstage' : stageState(git) === 'mixed' ? 'Partly staged; click to stage all of it' : 'Stage for the next commit'"
              @change="toggleStaged((change.dir + change.name).replaceAll('\\', '/'), git)"
            />
            <span v-else class="stage" />
            <button
              class="file"
              :class="{ settled }"
              :title="settled ? `${change.path}: no uncommitted changes (committed or reverted since)` : change.path"
              @click="ui.diffPath = change.path"
            >
              <Icon name="file" :size="14" />
              <div class="file-text">
                <div class="mono truncate">{{ change.name }}</div>
                <div class="faint truncate">{{ change.dir || "./" }}</div>
              </div>
              <span v-if="git" class="change-stat" :title="gitLabel(git.status)">
                <template v-if="git.added !== undefined"><b v-if="git.added || !git.removed" class="add">+{{ git.added }}</b> <b v-if="git.removed" class="del">−{{ git.removed }}</b></template>
                <span class="code" :class="'code-' + git.status">{{ git.status }}</span>
              </span>
              <span v-else-if="settled" class="change-stat faint">no changes</span>
              <span v-else-if="reportedFor(change)?.added != null" class="change-stat">
                <b v-if="reportedFor(change)!.added || !reportedFor(change)!.removed" class="add">+{{ reportedFor(change)!.added }}</b> <b v-if="reportedFor(change)!.removed" class="del">−{{ reportedFor(change)!.removed }}</b>
              </span>
              <Icon name="chevronRight" :size="13" />
            </button>
          </div>
        </template>
        <template v-if="nestedChanges.length">
          <span class="section-label">Other changes in nested repositories</span>
          <div v-for="git in nestedChanges" :key="git.path" class="file-row">
            <button class="file" :title="git.path" @click="ui.diffPath = git.path">
              <Icon name="file" :size="14" />
              <span class="file-text mono truncate">{{ git.path }}</span>
              <span class="change-stat" :title="gitLabel(git.status)"><span class="code" :class="'code-' + git.status">{{ git.status }}</span></span>
              <Icon name="chevronRight" :size="13" />
            </button>
          </div>
        </template>
        <form v-if="gitRepo" class="commit" @submit.prevent="!review && reviewStaged()">
          <div class="commit-head">
            <span class="section-label">Commit</span>
            <span v-if="gitBranch" class="faint mono truncate" :title="`Current branch: ${gitBranch}`">to {{ gitBranch }}</span>
            <span class="commit-ai">
              <button type="button" class="s-link" title="Dial reads your uncommitted changes with the current model and reports; nothing is changed" @click="aiReviewView()">AI review</button>
              <button type="button" class="s-link" title="Recent commits on this branch" @click="gitLogView()">History</button>
              <button type="button" class="s-link" :disabled="gitBusy || writing || !staged" :title="staged ? 'Dial reads the staged changes and writes a message here; nothing is committed until you review it' : 'Stage files first'" @click="writeMessage">{{ writing ? "Writing…" : "Write with AI" }}</button>
              <button type="button" class="s-link" title="Branches checked out in other folders" @click="worktreesView()">Worktrees</button>
            </span>
          </div>
          <textarea
            v-model="commitMessage"
            class="commit-message"
            rows="2"
            aria-label="Commit message"
            placeholder="Commit message"
            spellcheck="true"
            @keydown.meta.enter.prevent="!review && reviewStaged()"
            @keydown.ctrl.enter.prevent="!review && reviewStaged()"
          />
          <p v-if="aiWarning" class="faint commit-warning">Dial’s check of this message: {{ aiWarning }}</p>
          <div v-if="author" class="author" role="group" aria-label="Commit author">
            <strong>Git needs your name and email</strong>
            <span class="faint">Git records them on each commit you make. They are saved in Git’s own settings on this computer.</span>
            <input v-model="author.name" class="s-input" aria-label="Your name" placeholder="Your name" autocomplete="name" @keydown.enter.prevent="saveAuthor" />
            <input v-model="author.email" class="s-input" type="email" aria-label="Email address" placeholder="you@example.com" autocomplete="email" spellcheck="false" @keydown.enter.prevent="saveAuthor" />
            <label class="author-scope faint"><input v-model="author.everywhere" type="checkbox" /> Use for all my repositories</label>
            <p v-if="author.error" role="alert" class="s-error">{{ author.error }}</p>
            <div class="commit-foot">
              <button type="button" class="btn btn-secondary" :disabled="author.saving" @click="author = null">Not now</button>
              <button type="button" class="btn btn-primary" :disabled="author.saving || !author.name.trim() || !author.email.trim()" @click="saveAuthor">{{ author.saving ? "Saving…" : "Save author" }}</button>
            </div>
          </div>
          <template v-if="review">
            <strong>Review {{ review.paths.length }} staged {{ review.paths.length === 1 ? 'path' : 'paths' }}<template v-if="review.branch"> on {{ review.branch }}</template></strong>
            <span class="faint">Includes files staged outside this session. Only staged changes will be committed.</span>
            <ul aria-label="Staged paths" class="staged-paths mono"><li v-for="path in review.paths" :key="path"><bdi>{{ /[\u0000-\u001f\u007f]/.test(path) ? JSON.stringify(path) : path }}</bdi></li></ul>
            <div class="commit-foot">
              <button type="button" class="btn btn-secondary" :disabled="gitBusy" @click="dismissReview">Cancel</button>
              <button type="button" class="btn btn-primary" :disabled="gitBusy" @click="commitStaged">Commit staged</button>
            </div>
          </template>
          <div v-else class="commit-foot">
            <span class="faint">{{ staged ? `${staged} ${staged === 1 ? "file" : "files"} staged in the project` : "Tick files to stage them" }}</span>
            <button type="submit" class="btn btn-primary" :disabled="gitBusy || !commitMessage.trim()">Review staged files</button>
          </div>
        </form>
      </template>

      <template v-else-if="watchKind">
        <ActivityWatch
          v-if="shown(ui.inspectorTab) && snap.connection.state === 'live' && snap.sessionId"
          :key="JSON.stringify([watchKind, snap.workspace.root, snap.sessionId])"
          :kind="watchKind" :root="snap.workspace.root" :session-id="snap.sessionId"
        />
        <div v-else-if="shown(ui.inspectorTab)" class="empty">
          <strong>{{ watchKind === 'worker' ? 'Session workers' : 'Project peers' }}</strong>
          <span>Connect to a live session to see {{ watchKind === 'worker' ? 'its workers' : 'other sessions on this project' }}.</span>
        </div>
      </template>

      <!-- /watch opens Activity; the two rosters are independent destinations. -->
      <template v-else>
        <nav class="rosters" aria-label="Coordination panels">
          <button v-for="tab in rosters" :key="tab.id" class="btn btn-secondary" @click="showPanel(tab.id)">
            <Icon :name="tab.icon" :size="14" />{{ tab.label }}<Icon name="chevronRight" :size="12" />
          </button>
        </nav>
        <JobDiscovery v-if="shown('activity') && snap.connection.state === 'live'" :key="JSON.stringify([snap.workspace.root, snap.sessionId])" :root="snap.workspace.root" />
        <div class="stats">
          <div class="stat"><span class="dot running" /><b>{{ snap.stats.running }}</b> running</div>
          <div class="stat"><span class="dot ok" /><b>{{ snap.stats.succeeded }}</b> done</div>
          <div class="stat"><span class="dot bad" /><b>{{ snap.stats.failed }}</b> failed</div>
        </div>
        <div v-if="!tools.length" class="empty">
          <strong>No tool calls yet</strong>
          <span>Every command, edit and search Dial runs is listed here.</span>
        </div>
        <template v-else>
          <div class="head"><span class="section-label">Recent</span></div>
          <button v-for="tool in tools" :key="tool.key" class="activity" :title="`Show in conversation: ${tool.target}`" @click="reveal(tool.key)">
            <span
              class="dot"
              :class="{
                running: tool.state === 'running',
                ok: tool.state === 'succeeded',
                bad: tool.state === 'failed',
              }"
            />
            <span class="activity-label">{{ tool.label }}</span>
            <span class="mono faint truncate">{{ tool.target }}</span>
          </button>
          <button v-if="allTools.length > tools.length" class="btn btn-secondary more" @click="activityLimit += ACTIVITY_PAGE">
            Show more ({{ allTools.length - tools.length }} older)
          </button>
        </template>
      </template>
    </div>
  </aside>
</template>

<style scoped>
.inspector {
  position: relative;
  width: var(--inspector-width);
  flex: none;
  display: flex;
  flex-direction: column;
  background: var(--sidebar);
  border-left: 1px solid var(--border);
  padding: 0 var(--space-lg) var(--space-md);
  transition: width 160ms var(--ease);
}
.inspector.wide-review {
  width: min(68%, calc(100% - 320px));
}
:global(body.resizing) .inspector {
  transition: none;
}
:global(body.resizing) {
  cursor: col-resize;
  user-select: none;
}
.resize {
  position: absolute;
  left: -3px;
  top: 0;
  bottom: 0;
  width: 6px;
  cursor: col-resize;
  z-index: 2;
}
.resize:hover {
  background: var(--border-strong);
}
.panel {
  flex: 1;
  min-height: 0;
  padding-top: var(--space-md);
}
.panel > * {
  height: 100%;
}
/* Centres the 32px tab row on the title bar's line (--topbar is 48px). */
.drag {
  height: 8px;
  flex: none;
}
.tabs {
  display: flex;
  gap: 2px;
  height: 32px;
  flex: none;
}
.tabs button {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 8px;
  border-radius: var(--radius-sm);
  font-size: 13px;
  color: var(--text-faint);
  white-space: nowrap;
  transition:
    background 120ms var(--ease),
    color 120ms var(--ease);
}
.tabs button:hover {
  color: var(--text);
}
.tabs button.active {
  background: var(--active);
  color: var(--text);
  box-shadow: inset 0 -2px 0 var(--accent);
}
.tabs .grow {
  flex: 1;
}
.tabs button.tool {
  min-width: 30px;
  justify-content: center;
  padding: 0 7px;
}
.tabs button.tool.active {
  box-shadow: none;
}
.badge {
  font-size: var(--text-xs);
  color: var(--text-muted);
}
.live {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--accent);
  animation: pulse 1.2s ease-in-out infinite;
}
.body {
  flex: 1;
  overflow-y: auto;
  padding-top: var(--space-lg);
  margin-right: -8px;
  padding-right: 8px;
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
.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
}
.faint {
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.bar {
  height: 4px;
  border-radius: 2px;
  background: var(--raised);
  overflow: hidden;
  margin-bottom: var(--space-md);
}
.progress-head { gap: 12px; margin-bottom: var(--space-md); }
.progress-head .bar { flex: 1; margin-bottom: 0; }
.progress-head .faint { flex: none; }
.bar span {
  display: block;
  height: 100%;
  border-radius: 2px;
  background: linear-gradient(90deg, var(--accent), var(--accent-soft));
  transition: width 300ms var(--ease);
}
.tasks {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.tasks li {
  display: flex;
  gap: 10px;
  font-size: 13.5px;
  line-height: 1.45;
  color: var(--text-muted);
}
.mark {
  display: grid;
  place-items: center;
  width: 18px;
  height: 18px;
  margin-top: 1px;
  border-radius: 50%;
  flex: none;
  border: 1.4px solid var(--border-strong);
}
.completed .mark {
  border: none;
  color: var(--success);
}
.in_progress .mark {
  border: none;
  color: var(--accent);
}
.in_progress .text {
  color: var(--text);
}
.dropped .mark {
  border: none;
  color: var(--text-faint);
}
.dropped .text {
  color: var(--text-faint);
  text-decoration: line-through;
}
.tasks code {
  font-family: var(--font-mono);
  font-size: 0.86em;
  padding: 0.1em 0.36em;
  border-radius: 5px;
  background: var(--active);
  color: var(--code-inline);
}
.file-row {
  display: flex;
  align-items: center;
  gap: 4px;
  margin: 0 -8px;
}
.file-row .stage {
  flex: none;
  width: 14px;
  height: 14px;
  margin: 0 0 0 6px;
  accent-color: var(--accent);
}
.file {
  display: flex;
  align-items: center;
  gap: 10px;
  flex: 1;
  min-width: 0;
  padding: 6px 8px 6px 4px;
  border-radius: var(--radius-sm);
  color: var(--text-faint);
  text-align: left;
}
.commit {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: var(--space-lg);
  padding-top: var(--space-md);
  border-top: 1px solid var(--border);
}
.commit-head,
.commit-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.commit-ai { display: flex; gap: 10px; margin-left: auto; flex: none; }
.commit-ai .s-link { color: var(--accent); font-size: var(--text-sm); }
.commit-ai .s-link:disabled { color: var(--text-faint); cursor: default; }
.commit-warning { margin: 0; font-size: var(--text-xs); }
.staged-paths { overflow: auto; max-height: 12rem; padding-left: 1.4rem; overflow-wrap: anywhere; font-size: var(--text-sm); }
.author { display: flex; flex-direction: column; gap: 6px; padding: 10px; border: 1px solid var(--border); border-radius: var(--radius-sm); font-size: var(--text-sm); }
.author .s-input { width: 100%; max-width: 100%; }
.author-scope { display: flex; align-items: center; gap: 6px; }
.commit-message {
  width: 100%;
  min-height: 44px;
  resize: vertical;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text);
  font: inherit;
  font-size: var(--text-sm);
  outline: none;
}
.commit-message:focus {
  border-color: var(--focus-border);
}
.resize:focus-visible {
  outline: none;
  background: var(--accent);
}
.file:hover {
  background: var(--surface);
}
.file.settled .file-text {
  color: var(--text-faint);
}
.file-text {
  flex: 1;
  min-width: 0;
  color: var(--text);
  font-size: 13px;
  line-height: 1.4;
}
.rosters { display: flex; flex-wrap: wrap; gap: 6px; margin-bottom: var(--space-md); }
.stats {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: var(--space-lg);
}
.stat {
  flex: 1 1 auto;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  background: var(--hover);
  font-size: var(--text-sm);
  color: var(--text-faint);
  white-space: nowrap;
}
.stat b {
  color: var(--text);
  font-weight: 600;
}
.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--text-faint);
  flex: none;
}
.dot.running {
  background: var(--accent);
  animation: pulse 1.2s ease-in-out infinite;
}
.dot.ok {
  background: var(--success);
}
.dot.bad {
  background: var(--danger);
}
.activity {
  display: flex;
  align-items: center;
  gap: 8px;
  width: calc(100% + 16px);
  height: 28px;
  margin: 0 -8px;
  padding: 0 8px;
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
  text-align: left;
  transition: background 120ms var(--ease);
}
.activity:hover {
  background: var(--hover);
}
.activity-label {
  white-space: nowrap;
}
.more {
  margin-top: 6px;
}
.change-stat {
  display: flex;
  align-items: center;
  gap: 4px;
  flex: none;
  font-size: var(--text-xs);
  white-space: nowrap;
}
.change-stat .add {
  color: var(--success);
}
.change-stat .del {
  color: var(--danger);
}
.change-stat .code {
  font-family: var(--font-mono);
  color: var(--warning);
}
.change-stat .code-A,
.change-stat .code-U {
  color: var(--success);
}
.change-stat .code-D,
.change-stat .code-C {
  color: var(--danger);
}
</style>
