// The only place the UI talks to the Rust side.

import { getVersion } from "@tauri-apps/api/app";
import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { downloadDir } from "@tauri-apps/api/path";
import { listen } from "@tauri-apps/api/event";
import { ask, open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { check as checkAppUpdate, type Update } from "@tauri-apps/plugin-updater";
import { reactive, ref, shallowRef } from "vue";
import { readTrash, trashKey, updateTrash, type Deletable } from './deferredDelete';
import { attachmentPayload } from './attachmentFormats';
import type { KeyProvider } from './providerKeys';
import { LastSubmission } from './promptEdit';
import { nameFailure, saveRejected, settleSaves, startSave } from './settingsFeedback';
import type {
  Answer,
  ApprovalMode,
  Attachment,
  Bounds,
  BrowserState,
  Decision,
  DiffView,
  EngineUpdateView,
  DirEntry,
  FileView,
  ModelOption,
  PageInfo,
  Setting,
  SettingScope,
  SettingsPage,
  Snapshot,
  TerminalEvent,
  Workspace,
} from "./types";
import type { WatchPeerBoard, WatchWorker } from './activityWatch';

/** Latest state published by the app; `null` until the first one arrives. Parts unchanged since
 * the previous snapshot keep their identity (see {@link share}), so Vue skips redrawing them. */
export const snapshot = shallowRef<Snapshot | null>(null);
export const lastSubmission = new LastSubmission();
export const startupError = ref<string | null>(null);
export const connecting = ref(false);
/** Incremented whenever the native workspace filesystem watcher reports external changes. */
export const workspaceChange = ref<{ root: string; rev: number }>({ root: "", rev: 0 });

/**
 * `next`, reusing every object or array of `prev` that holds the same data. The app republishes the
 * whole transcript about once a frame while a turn streams; without this every message and tool row
 * would get a new object, and be redrawn, on every update. Returns `prev` itself when nothing changed.
 */
export function share<T>(prev: unknown, next: T): T {
  if (prev === next || typeof prev !== "object" || typeof next !== "object" || !prev || !next) return next;
  if (Array.isArray(prev) !== Array.isArray(next)) return next;
  if (Array.isArray(next)) {
    const old = prev as unknown[];
    let same = old.length === next.length;
    for (let i = 0; i < next.length; i++) {
      next[i] = share(old[i], next[i]);
      if (next[i] !== old[i]) same = false;
    }
    return same ? (prev as T) : next;
  }
  const old = prev as Record<string, unknown>;
  const fresh = next as Record<string, unknown>;
  const keys = Object.keys(fresh);
  let same = keys.length === Object.keys(old).length;
  for (const key of keys) {
    fresh[key] = share(old[key], fresh[key]);
    if (fresh[key] !== old[key] || !(key in old)) same = false;
  }
  return same ? (prev as T) : next;
}

/** Models the user can switch to; empty until the engine listed them. */
export const models = shallowRef<ModelOption[]>([]);

/** Both the recipient and the sending session are explicit; ids on the two transports are unrelated. */
export const activityWatchApi = {
  workers: (workspaceRoot: string, sessionId: string) => invoke<WatchWorker[]>('watch_workers', { workspaceRoot, sessionId }),
  peers: (workspaceRoot: string, sessionId: string) => invoke<WatchPeerBoard>('watch_peers', { workspaceRoot, sessionId }),
  message: (workspaceRoot: string, sessionId: string, id: string, text: string, watchToken: string) => invoke<unknown>('watch_message', { workspaceRoot, sessionId, id, text, watchToken }),
  peerMessage: (workspaceRoot: string, sessionId: string, id: string, text: string) => invoke<unknown>('watch_peer_message', { workspaceRoot, sessionId, id, text }),
};

export interface DownloadStatus {
  repo: string; file: string | null; expectedBytes: number; running: boolean; message: string;
  percent: number | null; receivedBytes: number | null; completedBytes: number | null; totalBytes: number | null;
  completedFiles: number | null; totalFiles: number | null; elapsedSecs: number; retryable: boolean;
}
export interface CachedModelFile { revision: string; path: string; bytes: number }
export interface CachedModel { repo: string; bytes: number; files: CachedModelFile[] }
export interface HubModel { id: string; downloads: number; likes: number; pipeline_tag: string | null }
export interface HubFile { path: string; size: number; split: boolean }
export const modelDownloads = {
  setup: () => invoke<boolean>('model_hub_setup'),
  status: () => invoke<DownloadStatus>('model_download_status'),
  cancel: () => invoke<DownloadStatus>('model_download_cancel'),
  cached: () => invoke<CachedModel[]>('model_cache_list'),
  search: (query: string, format: 'MLX' | 'GGUF') => invoke<HubModel[]>('model_hub_search', { query, format }),
  files: (repo: string) => invoke<HubFile[]>('model_hub_files', { repo }),
  async start(repo: string, size: string, bytes: number, file?: string) {
    const what = file ? `${file} from ${repo}` : repo;
    if (!await ask(`Download ${what} (about ${size}) from Hugging Face to this computer? This does not switch your session model.`, { title: 'Download model?' })) return null;
    const start = () => invoke<DownloadStatus | null>('model_download_start', { repo, file: file ?? null, expectedBytes: bytes });
    const started = await start();
    if (started) return started;
    // Null means the helper executable is missing (other failures reject). Keep the confirmed
    // repo/file and retry only after separate native consent for the helper installation.
    if (!await modelDownloads.setup()) return null;
    const retried = await start();
    if (!retried) throw new Error('The model helper was not found after setup.');
    return retried;
  },
  async removeFile(repo: string, f: CachedModelFile) {
    if (!await ask(`Remove ${f.path} from ${repo} at revision ${f.revision}?\n\nOther files and revisions are kept. Shared cache blobs are not deleted, so this may not free disk space.`, { title: 'Remove GGUF file?', kind: 'warning', okLabel: 'Remove file' })) return false;
    await invoke('model_cache_remove_file', { repo, revision: f.revision, file: f.path });
    return true;
  },
  /** Delete a downloaded model from the Hugging Face cache after confirming; the bytes freed, or `null` if cancelled. */
  async remove(repo: string, size: string) {
    if (!await ask(`Delete the entire ${repo} repository (${size}) from this computer?\n\nAll its cached files and revisions will be removed. They can be downloaded again later.`, { title: 'Delete entire repository?', kind: 'warning', okLabel: 'Delete repository' })) return null;
    return invoke<number>('model_cache_remove', { repo });
  },
};

/** The settings page; `null` until the engine sent it. */
export const settingsPage = shallowRef<SettingsPage | null>(null);

export interface ToastAction {
  label: string;
  run: () => void;
}

export interface Toast {
  id: number;
  message: string;
  /** A button on the toast, like Undo; pressing it also dismisses the toast. */
  action?: ToastAction;
}

const TOAST_MS = 6000;
export const toasts = ref<Toast[]>([]);
let lastToast = 0;

const timers = new Map<number, ReturnType<typeof setTimeout>>();
/** How long each toast stays when not the default. */
const durations = new Map<number, number>();

function dismissLater(id: number) {
  clearTimeout(timers.get(id));
  timers.set(
    id,
    setTimeout(() => {
      timers.delete(id);
      durations.delete(id);
      toasts.value = toasts.value.filter((t) => t.id !== id);
    }, durations.get(id) ?? TOAST_MS),
  );
}

/**
 * Show `message`; one already on screen stays single and just stays longer. A toast with an
 * `action` is always its own. Returns the toast's id.
 */
export function showToast(message: string, options: { action?: ToastAction; ms?: number } = {}): number {
  const shown = !options.action && toasts.value.find((t) => t.message === message && !t.action);
  if (shown) {
    dismissLater(shown.id);
    return shown.id;
  }
  const id = ++lastToast;
  if (options.ms) durations.set(id, options.ms);
  // At most three; an older plain toast makes room before one that offers an action (Undo).
  let kept = toasts.value;
  while (kept.length > 2) {
    const drop = kept.findIndex((t) => !t.action);
    const gone = kept[drop < 0 ? 0 : drop];
    clearTimeout(timers.get(gone.id));
    timers.delete(gone.id);
    durations.delete(gone.id);
    kept = kept.filter((t) => t !== gone);
  }
  toasts.value = [...kept, { id, message, action: options.action }];
  dismissLater(id);
  return id;
}

/** Run a toast's action button, then dismiss it. */
export function runToastAction(id: number) {
  const toast = toasts.value.find((t) => t.id === id);
  dismissToast(id);
  toast?.action?.run();
}

/** Remove a toast now (its close button). */
export function dismissToast(id: number) {
  clearTimeout(timers.get(id));
  timers.delete(id);
  durations.delete(id);
  toasts.value = toasts.value.filter((t) => t.id !== id);
}

/** Keep a toast while the pointer rests on it, so it can be read or copied. */
export function holdToast(id: number) {
  clearTimeout(timers.get(id));
  timers.delete(id);
}

/** Resume a held toast's countdown. */
export function releaseToast(id: number) {
  if (toasts.value.some((t) => t.id === id)) dismissLater(id);
}

/** The `snapshot` event (`view::SnapshotEvent`): the whole snapshot, or what changed since `base`. */
interface SnapshotEvent {
  seq: number;
  base: number;
  full?: Snapshot;
  patch?: Partial<Snapshot> & { entriesFrom?: number; entriesTail?: Snapshot["entries"] };
}

/** Number of the snapshot on screen; `0` before the first. */
let snapshotSeq = 0;
let resyncing = false;
let resyncAgain = false;

function accept(seq: number, next: Snapshot) {
  snapshotSeq = seq;
  if (next.workspace?.root !== snapshot.value?.workspace?.root) loadTrash(next.workspace?.root);
  snapshot.value = share(snapshot.value, next);
  lastSubmission.observe(snapshot.value);
  startupError.value = null;
}

/** `prev` with `patch` applied; unchanged entries keep their identity. */
export function applyPatch(prev: Snapshot, patch: NonNullable<SnapshotEvent["patch"]>): Snapshot {
  const { entriesFrom, entriesTail, ...fields } = patch;
  const next = { ...prev, ...fields };
  if (entriesFrom !== undefined) next.entries = prev.entries.slice(0, entriesFrom).concat(entriesTail ?? []);
  return next;
}

function receive(event: SnapshotEvent) {
  if (event.seq <= snapshotSeq) return; // Already included in a snapshot loaded meanwhile.
  if (event.full) accept(event.seq, event.full);
  else if (snapshot.value && event.base === snapshotSeq && event.patch) accept(event.seq, applyPatch(snapshot.value, event.patch));
  else void resync().catch(connectionError);
}

/** Load the latest snapshot whole, after missing an event (or at start). */
async function resync() {
  if (resyncing) {
    resyncAgain = true;
    return;
  }
  resyncing = true;
  try {
    do {
      resyncAgain = false;
      const latest = await invoke<{ seq: number; snapshot: Snapshot }>("snapshot");
      if (latest.seq > snapshotSeq) accept(latest.seq, latest.snapshot);
    } while (resyncAgain);
  } finally {
    resyncing = false;
  }
}

function connectionError(error: unknown) {
  if (snapshot.value) showToast(`Could not refresh Dial: ${error}`);
  else startupError.value = String(error);
}

let subscribed = false;
/** Retry only bridge setup. Keep successful listeners across snapshot retries. */
export async function connect() {
  if (connecting.value) return;
  connecting.value = true;
  const partial: (() => void)[] = [];
  try {
    if (!subscribed) {
      partial.push(await listen<SnapshotEvent>("snapshot", e => receive(e.payload)));
      partial.push(await listen<ModelOption[]>("models", e => { models.value = e.payload; }));
      partial.push(await listen<SettingsPage>("settings", e => { settingsPage.value = e.payload; settleSaves(); }));
      partial.push(await listen<string>("toast", e => showToast(nameFailure(e.payload))));
      partial.push(await listen<{ root: string; session: string }>("attach-session", async ({ payload }) => {
        if (snapshot.value?.workspace?.root !== payload.root) return;
        const { openTaskTerminal } = await import("./commandRunner");
        openTaskTerminal({ kind: "attach", session: payload.session }, payload.root);
      }));
      partial.push(await listen<string>("workspace-changed", e => {
        workspaceChange.value = { root: e.payload, rev: workspaceChange.value.rev + 1 };
      }));
      subscribed = true;
    }
    await resync();
    const list = await invoke<ModelOption[]>("models");
    if (!models.value.length) models.value = list;
  } catch (error) {
    if (!subscribed) partial.forEach(unlisten => unlisten());
    connectionError(error);
  } finally {
    connecting.value = false;
  }
}

async function call(command: string, args?: Record<string, unknown>) {
  try {
    await invoke(command, args);
    return true;
  } catch (error) {
    showToast(String(error));
    return false;
  }
}

/** Project-scoped local trash; no timer deletes a transcript. */
export const hiddenSessions = reactive(new Set<string>());
function replaceTrash(ids: string[]) {
  hiddenSessions.clear();
  for (const id of ids) hiddenSessions.add(id);
}
function loadTrash(root?: string) {
  try { replaceTrash(root ? readTrash(localStorage.getItem(trashKey(root))) : []); }
  catch { replaceTrash([]); showToast('Could not read Recently Deleted on this computer'); }
}
if (typeof window !== 'undefined') window.addEventListener('storage', e => {
  const root = snapshot.value?.workspace?.root;
  if (root && (!e.key || e.key === trashKey(root))) loadTrash(root);
});
function changeTrash(root: string, ids: string[], restore: boolean): boolean {
  try {
    const next = updateTrash(localStorage, root, ids, restore);
    if (snapshot.value?.workspace?.root === root) replaceTrash(next);
    return true;
  } catch { showToast('Could not save Recently Deleted. No local change was made.'); return false; }
}
export function restoreSessions(root: string, ids: string[]) { return changeTrash(root, ids, true); }
export const trashPrompt = shallowRef<{ title: string; message: string; action: string; finish: (yes: boolean) => void } | null>(null);
function confirmTrash(title: string, message: string, action: string): Promise<boolean> {
  if (trashPrompt.value) return Promise.resolve(false);
  return new Promise(resolve => { trashPrompt.value = { title, message, action, finish(yes) { trashPrompt.value = null; resolve(yes); } }; });
}
export async function purgeSessions(root: string, sessions: Deletable[]) {
  if (!sessions.length || snapshot.value?.workspace?.root !== root) return;
  const subject = sessions.length === 1 ? `“${sessions[0].title || 'Untitled session'}”` : `${sessions.length} sessions`;
  if (!await confirmTrash('Delete permanently?', `${subject} and all associated history will be deleted. This cannot be undone.`, 'Delete permanently')) return;
  for (const { id } of sessions) {
    if (snapshot.value?.workspace?.root !== root) break;
    if (!hiddenSessions.has(id)) continue;
    if (!await call('delete_sessions', { workspaceRoot: root, sessionIds: [id], confirmRemoval: true }) || !restoreSessions(root, [id])) break;
  }
}

export const engine = {
  async send(text: string, attachments: Attachment[], scope: { workspaceRoot: string; sessionId: string | null; typeId?: string | null }) {
    const settle = lastSubmission.start(snapshot.value, scope, text, attachments);
    try {
      const displayed = await invoke<string | null>('send_prompt', { ...scope, text, ...attachmentPayload(attachments, snapshot.value?.model?.acceptsImages) });
      settle?.(displayed ?? undefined);
      return true;
    } catch (error) {
      settle?.();
      showToast(String(error));
      return false;
    }
  },
  approve: (approvalId: string, decision: Decision) => call("approve", { approvalId, decision }),
  abort: () => call("abort"),
  newSession: () => call("new_session"),
  resume: (sessionId: string, workspaceRoot?: string, waitForClose = false) => call("resume", { sessionId, workspaceRoot, waitForClose }),
  searchTranscripts: (workspaceRoot: string, query: string, limit: number) =>
    invoke<import("./types").TranscriptResults>("search_transcripts", { workspaceRoot, query, limit }),
  setupAtomic: () => invoke<boolean>('atomic_engine_setup'),
  localModelDetails: (workspaceRoot: string, model: string) =>
    invoke<import("./types").LocalModelRemoval>("local_model_removal", { workspaceRoot, model, dryRun: true }),
  async removeLocalModel(workspaceRoot: string, model: string, preview: import("./types").LocalModelRemoval) {
    const confirmed = await ask(`Remove ${preview.label} from disk?\n\n${preview.folder}\n\nThis removes the entire model folder and cannot be undone.`, { title: "Remove downloaded model?", kind: "warning" });
    if (!confirmed) return false;
    const result = await invoke<import("./types").LocalModelRemoval>("local_model_removal", { workspaceRoot, model, dryRun: false });
    if (!result.deleted) throw new Error("Dial did not confirm removal");
    await engine.refreshModels();
    return true;
  },
  async deleteSessions(sessions: Deletable[]): Promise<boolean> {
    const snap = snapshot.value, root = snap?.workspace?.root;
    if (!root || !sessions.length) return false;
    const ids = sessions.map(s => s.id);
    const busy = () => snapshot.value?.sessions.some(s => ids.includes(s.id) && (s.running || s.waiting));
    if (busy()) { showToast('Stop the running session before moving it to Recently Deleted'); return false; }
    if (ids.length > 1 && !await confirmTrash(`Move ${ids.length} sessions?`, 'You can restore these sessions from Recently Deleted in the sidebar.', 'Move sessions')) return false;
    if (snapshot.value?.workspace?.root !== root || busy()) return false;
    if (!changeTrash(root, ids, false)) return false;
    if (snapshot.value.sessionId && ids.includes(snapshot.value.sessionId) && !await engine.newSession()) {
      restoreSessions(root, ids);
      return false;
    }
    showToast(`Moved ${ids.length === 1 ? 'session' : `${ids.length} sessions`} to Recently Deleted`, { action: { label: 'Undo', run: () => restoreSessions(root, ids) } });
    return true;
  },
  renameSession: (sessionId: string, title: string) => call("rename_session", { sessionId, title }),
  pinSession: (sessionId: string, pinned: boolean) => call("pin_session", { sessionId, pinned }),
  /** Copy a session into a new one and open the copy. */
  forkSession: (sessionId: string) => call("fork_session", { sessionId }),
  /**
   * Copy the conversation up to (not including) a prompt and open the copy; resolves to the copy's
   * id, or `null` when cancelled or it failed. `edit` words the confirmation for "Edit from here".
   */
  async forkBefore(root: string, sessionId: string, prompt: string, occurrence: number, matches: number, edit = false): Promise<string | null> {
    try {
      const question = edit
        ? "Create a copy ending before this prompt and put the prompt in the box to edit? The original conversation and working files stay unchanged."
        : "Create a copy ending before this prompt? The original conversation and working files stay unchanged.";
      if (!await ask(question, { title: edit ? "Edit from this message?" : "Fork from this message?", okLabel: edit ? "Edit in a copy" : undefined })) return null;
      const id = await invoke<string>("fork_before", { workspaceRoot: root, sessionId, prompt, occurrence, matches });
      if (snapshot.value?.workspace?.root === root && snapshot.value.sessionId === sessionId) await engine.resume(id, root);
      else showToast("Fork created in the original project");
      return id;
    } catch (e) {
      showToast(String(e));
      return null;
    }
  },
  /** Ask where to save, inside the project folder, and export the session on screen as Markdown
   * or as a standalone HTML page. */
  async exportSession(root: string, title: string, format: "markdown" | "html" = "markdown") {
    const slug = title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 60) || "session";
    const html = format === "html";
    try {
      const path = await saveDialog({
        title: `Export session as ${html ? "HTML" : "Markdown"}`,
        defaultPath: `${root}/${slug}.${html ? "html" : "md"}`,
        filters: [html ? { name: "HTML", extensions: ["html"] } : { name: "Markdown", extensions: ["md"] }],
      });
      if (path) await call("export_session", { path });
    } catch (error) {
      showToast(`Could not open the save dialog: ${error}`);
    }
  },
  /** Confirm before undoing; only report success after the engine accepts the undo. `regenerate`
   * words the confirmation for undoing so the same prompt can be sent again. */
  async undoTurn(edit?: { missingImages: number; stillCurrent: () => boolean; regenerate?: boolean }): Promise<boolean> {
    const current = snapshot.value;
    if (!current?.workspace) return false;
    const { sessionId } = current;
    const workspaceRoot = current.workspace.root;
    const regenerate = !!edit?.regenerate;
    const confirmed = await ask(
      (regenerate
        ? 'Dial restores the files the last turn changed, removes its answer, and sends the same prompt again.'
        : 'Dial restores the files the last turn changed and removes that prompt and its answer from the conversation.') +
        (edit?.missingImages ? `\n\nOnly the prompt text can be restored. You’ll need to attach ${edit.missingImages === 1 ? 'the image' : `all ${edit.missingImages} images`} again. Cancel to keep the original turn.` : ''),
      {
        title: regenerate ? 'Regenerate the last answer?' : edit ? 'Edit the last prompt?' : 'Undo the last turn?',
        kind: 'warning',
        okLabel: regenerate ? 'Regenerate' : edit ? (edit.missingImages ? 'Edit text only' : 'Edit prompt') : 'Undo',
        cancelLabel: 'Cancel',
      },
    ).catch(() => false);
    return confirmed && (!edit || edit.stillCurrent()) && await call('undo_turn', { workspaceRoot, sessionId });
  },
  /** Add guidance to the running turn; Dial reads it at its next step. */
  steer: (text: string, scope: { workspaceRoot: string; sessionId: string | null; typeId?: string | null }) => call("steer", { ...scope, text }),
  /** Send the last prompt again after its turn failed. */
  retryTurn: () => call("retry_turn"),
  /** Summarize the conversation so far to free context. */
  compact: () => call("compact_session"),
  /** Start voice mode, or talk again while it waits. */
  voiceStart: () => call("voice_start"),
  /** Talk over the reply being read, or stop recording. */
  voiceTap: () => call("voice_tap"),
  /** Stop the turn voice mode started from a misheard request; voice then waits for a tap. */
  voiceCancel: () => call("voice_cancel"),
  voiceMute: (muted: boolean) => call("voice_mute", { muted }),
  voiceStop: () => call("voice_stop"),
  /** Read a reply aloud; with no text, stop reading. */
  readAloud: (text: string | null) => call("read_aloud", { text }),
  refreshSessions: () => call("refresh_sessions"),
  refreshModels: () => call("discover_models", { force: true }),
  discoverModels: () => call("discover_models"),
  setModel: (snap: import("./types").OpenSnapshot, provider: string, modelId: string) => call("set_model", { workspaceRoot: snap.workspace.root, sessionId: snap.sessionId, provider, modelId }),
  /** `default` restores the model's own level. */
  setThinking: (snap: import("./types").OpenSnapshot, level: string) => call("set_thinking", { workspaceRoot: snap.workspace.root, sessionId: snap.sessionId, level }),
  setApproval: (mode: ApprovalMode) => call("set_approval", { mode }),
  reconnect: () => call("reconnect"),
  /** Reconnect without a toast; resolves to `null`, or why the app refused (work still protected). */
  reconnectQuietly: () => invoke("reconnect").then(() => null, (error) => String(error)),
  /** The question is on screen; the engine starts its answer timeout. */
  questionPresented: (questionId: string) => call("question_presented", { questionId }),
  answerQuestion: (questionId: string, answers: Answer[]) =>
    call("answer_question", { questionId, reply: "answers", answers }),
  skipQuestion: (questionId: string) => call("answer_question", { questionId, reply: "cancel", answers: [] }),
};

export const folders = {
  // Always an array: a null reply (a backend not ready yet, or a mock) must not crash the menus.
  recent: () => invoke<Workspace[] | null>("recent_workspaces").then((list) => list ?? []).catch(() => [] as Workspace[]),
  /** Drop a folder from the recent projects list; the folder itself stays on disk. */
  async forget(path: string) {
    try {
      if (!(await invoke<boolean>("forget_workspace", { path }))) showToast("That project was already gone from the list");
      return true;
    } catch (error) {
      showToast(`Could not remove the project from the list: ${error}`);
      return false;
    }
  },
  /** Switch projects in this window without stopping the previous project's sessions. */
  open: (path: string) => call("open_workspace", { path }),
  /** Ask for a folder with the system dialog and open it. */
  async choose(newWindow = false) {
    try {
      const path = await openDialog({ directory: true, multiple: false, title: "Open a project folder" });
      if (typeof path === "string") {
        if (newWindow) await folders.openInWindow(path); else await folders.open(path);
      }
    } catch (error) {
      showToast(`Could not open the folder picker: ${error}`);
    }
  },
  /** Ask for a folder with the system dialog without opening it; `null` when cancelled. */
  async pick(title: string): Promise<string | null> {
    try {
      const path = await openDialog({ directory: true, multiple: false, title });
      return typeof path === "string" ? path : null;
    } catch (error) {
      showToast(`Could not open the folder picker: ${error}`);
      return null;
    }
  },
  /** Open `path` in another Dial window, leaving this one as it is. */
  openInWindow: (path: string) => call("project_window", { path }),
};

/** A checked image file and its thumbnail. */
export interface ImageFile {
  path: string;
  name: string;
  url: string;
}

export const files = {
  /** Check the image at `path` and read its thumbnail; throws a readable message. */
  readImage: (path: string) => invoke<ImageFile>("read_image", { path }),
  validateAttachment: (path: string) => invoke<void>("validate_attachment", { path }),
  readClipboardImage: () => isTauri() ? invoke<string | null>("read_clipboard_image") : Promise.resolve(null),
  readClipboardFilePaths: () => isTauri() ? invoke<string[]>('read_clipboard_file_paths') : Promise.resolve([]),
  readTranscriptImage: (workspaceRoot: string, path: string) =>
    invoke<ImageFile>("read_transcript_image", { workspaceRoot, path }),
  /** Open a project image in the system's default viewer (Preview on a Mac). */
  openTranscriptImage: (workspaceRoot: string, path: string) =>
    invoke<void>("open_transcript_image", { workspaceRoot, path }).catch((error) => showToast(String(error))),
  /** Show the file in the system file manager (Finder, Explorer). */
  reveal: (path: string) => revealItemInDir(path).catch((error) => showToast(`Could not show the file: ${error}`)),
  /** Ask for files with the system dialog; empty when cancelled. */
  async chooseFiles(): Promise<string[]> {
    const picked = await openDialog({ multiple: true, title: 'Attach files' });
    return Array.isArray(picked) ? picked : picked ? [picked] : [];
  },
  /** Ask for one background image with the system dialog; `null` when cancelled. */
  async chooseBackground(): Promise<string | null> {
    const picked = await openDialog({
      multiple: false,
      title: "Choose a background image",
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "gif", "webp"] }],
    });
    return typeof picked === "string" ? picked : null;
  },
  /** Ask where to save an image (Downloads by default) and write its base64 bytes there. */
  async saveImage(fileName: string, extensions: string[], data: string) {
    try {
      const folder = await downloadDir().catch(() => "");
      const path = await saveDialog({
        title: "Save image",
        defaultPath: folder ? `${folder.replace(/\/+$/, "")}/${fileName}` : fileName,
        filters: [{ name: "Image", extensions }],
      });
      if (!path) return;
      await invoke("save_image", { path, data });
      showToast("Image saved");
    } catch (error) {
      showToast(`Could not save the image: ${error}`);
    }
  },
};

/** Open a web link in the system browser instead of navigating the app window. */
export function openExternal(url: string) {
  openUrl(url).catch((error) => showToast(`Could not open the link: ${error}`));
}

export const settings = {
  /** Show what is loaded and (re)load from the engine; the fresh page follows as an event. */
  async load() {
    try {
      const page = await invoke<SettingsPage | null>("settings");
      if (page) settingsPage.value = page;
    } catch (error) {
      showToast(String(error));
    }
  },
  save: (key: string, value: unknown, scope: SettingScope) => call("save_setting", { key, value, scope }),
  reset: (key: string, scope: SettingScope) => call("reset_setting", { key, scope }),
};

/** Shared by Settings and inline editors. A fresh engine settings event confirms the write. */
export async function saveSettingRow(s: Setting, value: unknown, scope: SettingScope, turnRunning: boolean, linked?: Record<string, unknown>) {
  startSave(s, turnRunning);
  if (!(await settings.save(s.key, value, scope))) return saveRejected(s.key);
  for (const [key, also] of Object.entries(linked ?? {})) settings.save(key, also, scope);
}

export async function resetSettingRow(s: Setting, scope: SettingScope, turnRunning: boolean) {
  startSave(s, turnRunning);
  if (!(await settings.reset(s.key, scope))) saveRejected(s.key);
}

/** How the engine binary was chosen (`launcher::EngineOrigin`). */
export type EngineOrigin = "flag" | "env" | "choice" | "desktop" | "path" | "fallback" | "bundled" | "missing";
/** Which engine Desktop runs and what went wrong while starting (`launcher::Startup`). */
export interface StartupReport {
  engine: string | null;
  origin: EngineOrigin | null;
  warnings: string[];
}
/** The engine picked in Settings › Updates (`launcher::EngineChoice`); `null` is the usual order. */
export type EngineChoice = { kind: "custom"; path: string } | { kind: "path" } | { kind: "desktop" };
export const diagnosticsApi = {
  startup: () => invoke<StartupReport>("startup_diagnostics"),
  /** The saved choice, and `flag`/`env` when `--dial` or `DIAL_BIN` outranks it. */
  engineChoice: () => invoke<{ choice: EngineChoice | null; overriddenBy: EngineOrigin | null }>("engine_choice_get"),
  /** Save (and, unless outranked, use from the next engine on) a choice; rejects a binary that does not run. */
  setEngineChoice: (choice: EngineChoice | null) => invoke<StartupReport>("engine_choice_set", { choice }),
  /** `path --version`: the version line, or a readable failure. */
  validateEngine: (path: string) => invoke<string>("engine_validate", { path }),
};
/** One optional capability from `dial doctor --all` (`engine_cli::Capability`). */
export interface Capability { id: string; name: string; installed: boolean; path: string | null }
export interface DoctorReport { capabilities: Capability[]; notes: string[] }
/** One stored sign-in of an OAuth provider (`engine_cli::Account`). */
export interface StoredAccount { key: string; detail: string; active: boolean }
/** Whether `dial` in a terminal runs Desktop's engine (`engine_cli::CliLink`). `shell` is the
 *  profile line that puts the link's folder on PATH, where Desktop can add it. */
export interface CliLink { engine: string; link: string; linked: boolean; managed: boolean; onPath: boolean; other: string | null; blocked: boolean; shell: { profile: string; line: string; present: boolean } | null }
/** Engine features reached through `dial` subcommands; none needs an open project. */
export const setupApi = {
  doctor: () => invoke<DoctorReport>("doctor_report"),
  /** What setting `id` up would run; with `apply`, run it. Returns the command's output. */
  fix: (id: string, apply: boolean) => invoke<string>("doctor_fix", { id, apply }),
  accounts: (provider: string, action?: "select" | "remove", key?: string) =>
    invoke<StoredAccount[]>("auth_accounts", { provider, action: action ?? null, key: key ?? null }),
  cliLink: () => invoke<CliLink>("cli_link_status"),
  setCliLink: (remove: boolean) => invoke<CliLink>("cli_link_set", { remove }),
  /** Append the PATH line to the login shell's profile; show `shell` to the user first. */
  addCliPath: () => invoke<CliLink>("cli_path_add"),
  /** A maintenance report: gc, images, images_purge, ps, ps_stop, ps_kill, ps_logs, ps_restart or rules. */
  report: <T = any>(kind: string, options: { apply?: boolean; target?: string } = {}) =>
    invoke<T>("engine_report", { kind, apply: options.apply ?? false, target: options.target ?? null }),
};
/** Settings export and import files (picked with the system dialogs). */
export const settingsFiles = {
  write: (path: string, text: string) => invoke<void>("settings_file_write", { path, text }),
  read: (path: string) => invoke<string>("settings_file_read", { path }),
};

interface NotificationDestination { workspaceRoot: string; sessionId: string }

export const notificationApi = {
  setEnabled: (enabled: boolean, interactive: boolean, kinds: { approval: boolean; question: boolean; finished: boolean }) => invoke<void>("set_notifications", { enabled, interactive, ...kinds }),
  resume: (destination: NotificationDestination) => call("resume", { ...destination, waitForClose: true }),
  onOpen: (handler: (destination: NotificationDestination) => void) =>
    listen<NotificationDestination>("notification-open", ({ payload }) => handler(payload)),
};

/** The open project's files, for the Files and Changes panels. Calls throw a readable message. */
export const workspaceFiles = {
  /** Folder `path` relative to the project; `""` is its root. */
  list: (path: string, workspaceRoot: string) => invoke<DirEntry[]>("list_dir", { path, workspaceRoot }),
  read: (path: string, workspaceRoot: string) => invoke<FileView>("read_file", { path, workspaceRoot }),
  /** Uncommitted changes of `path` (relative or absolute inside the project). */
  diff: (path: string, workspaceRoot: string, scope: "all" | "unstaged" | "staged" = "all") =>
    invoke<DiffView>("file_diff", { path, workspaceRoot, scope }),
  search: (query: string, limit = 20) => invoke<string[]>("search_files", { query, limit }),
};

/** The integrated browser (a native webview laid over the Browser panel). Calls throw. */
export const browserApi = {
  /** Show it at `bounds`, creating it on first use; opens `url` when given. */
  show: (bounds: Bounds, url?: string) => invoke<BrowserState>("browser_show", { bounds, url: url ?? null }),
  bounds: (bounds: Bounds) => invoke<void>("browser_bounds", { bounds }),
  hide: () => invoke<void>("browser_hide"),
  /** Open an address or a search; resolves to the URL opened. */
  navigate: (url: string) => invoke<string>("browser_navigate", { url }),
  go: (action: "back" | "forward" | "reload" | "stop") => invoke<void>("browser_go", { action }),
  /** The page's address, title, selection, text and logged errors. */
  inspect: () => invoke<PageInfo>("browser_inspect"),
  state: () => invoke<BrowserState>("browser_state"),
  onState: (handler: (state: BrowserState) => void) => listen<BrowserState>("browser", (e) => handler(e.payload)),
  /** What the agent does with the browser (through its MCP tools). */
  onAgent: (handler: (event: AgentBrowserEvent) => void) =>
    listen<AgentBrowserEvent>("agent-browser", (e) => handler(e.payload)),
};

/** The agent's browser tools ask the panel to open a page or show itself, or report a step. */
export type AgentBrowserEvent =
  | { kind: "open"; url: string }
  | { kind: "show" }
  | { kind: "activity"; busy: boolean; text?: string };

export interface ProviderAccount { provider: string; name: string; signed_in: boolean }
export type { KeyProvider } from './providerKeys';
export type ProviderEvent =
  | { event: "auth_url"; provider: string; url: string; instructions: string | null }
  | { event: "auth_progress"; provider: string; message: string }
  | { event: "auth_prompt"; provider: string; prompt_id: string; message: string; placeholder?: string; allow_empty: boolean; secret: boolean }
  | { event: "failed"; message: string };
const setupCall = <T>(id: number, method: string, params = {}) => invoke<T>("provider_setup_call", { id, method, params });
export const providerApi = {
  /** `workspaceRoot` null: no project is open, so sign-in runs in the home folder. */
  open: (workspaceRoot: string | null, handler: (event: ProviderEvent) => void) =>
    invoke<number>("provider_setup_open", { workspaceRoot, output: new Channel<ProviderEvent>(handler) }),
  /** `endpoint_keys`: custom endpoints that take a saved key; engines before 0.3.18 omit it. */
  list: (id: number) => setupCall<{ providers: ProviderAccount[]; key_providers?: KeyProvider[]; endpoint_keys?: KeyProvider[] }>(id, "auth_providers"),
  login: (id: number, provider: string) => setupCall(id, "login", { provider }),
  /** Sign in to a remote MCP server; its events carry the provider `mcp:<server>`. */
  mcpLogin: (id: number, server: string) => setupCall<{ server: string; message: string }>(id, "mcp_login", { server }),
  answer: (id: number, prompt_id: string, code: string) => setupCall(id, "auth_code", { prompt_id, code }),
  cancel: (id: number, provider: string) => setupCall(id, "auth_cancel", { provider }),
  logout: (id: number, provider: string) => setupCall(id, "logout", { provider }),
  removeKey: (id: number, provider: string) => setupCall(id, "remove_api_key", { provider, confirm: true }),
  close: (id: number) => invoke<void>("provider_setup_close", { id }),
};

/** Shells for the Terminal panel, running in the project folder. Calls throw. */
export type TerminalTask = { kind: "slash"; command: string } | { kind: "scheduler"; once?: boolean } | { kind: "attach"; session: string };
/** Session and collaboration tools that run the engine's own commands (`src-tauri/src/engine_cli.rs`). Calls throw a readable message. */
export const sessionToolsApi = {
  /** `dial share`: an encrypted link to the session's active branch, or a secret gist through `gh`. */
  share: (workspaceRoot: string, sessionId: string, gist: boolean) =>
    invoke<{ url: string; notes: string[] }>("share_session", { workspaceRoot, sessionId, gist }),
  /** The collaboration relay Desktop runs: its address while running, else `null`. */
  relay: (action: "status" | "start" | "stop", bind?: string) => invoke<{ bind: string | null }>("relay", { action, bind }),
  /** Reload the open conversation after a move through its tree. */
  reload: (workspaceRoot: string, sessionId: string | null) => invoke<void>("reload_session", { workspaceRoot, sessionId }),
};

export const terminalApi = {
  open: (workspaceRoot: string, cols: number, rows: number, task: TerminalTask | undefined, onEvent: (event: TerminalEvent) => void) =>
    invoke<number>("terminal_open", { workspaceRoot, cols, rows, task, output: new Channel<TerminalEvent>(onEvent) }),
  write: (id: number, data: string) => invoke<void>("terminal_write", { id, data }),
  resize: (id: number, cols: number, rows: number) => invoke<void>("terminal_resize", { id, cols, rows }),
  close: (id: number) => invoke<void>("terminal_close", { id }),
};

/**
 * Keeping Dial current (`src-tauri/src/updates.rs`). The engine updates itself through `dial
 * update`; the app through its signed updater. Calls throw a readable message.
 */
export const updatesApi = {
  /** Whether this build checks for app updates (release builds with the update key only). */
  appEnabled: () => invoke<boolean>("app_updates_enabled"),
  appVersion: () => getVersion(),
  /** The newer signed app release, or `null` when this one is current. */
  checkApp: (): Promise<Update | null> => checkAppUpdate(),
  tryReserveRestart: (confirmed: number[]) => invoke<import("./appInstaller").RestartDecision>("app_try_reserve_restart", { confirmed }),
  releaseRestart: () => invoke<void>("app_release_restart"),
  confirmTerminals: (count: number) => ask(`Updating will close ${count} open terminal session${count === 1 ? "" : "s"} and stop any running commands or scheduler tasks in them. Keep working to review or close them yourself.`, {
    title: "Close terminals and restart Dial Desktop?", okLabel: "Close terminals and restart", cancelLabel: "Keep working", kind: "warning",
  }),
  /** Download and verify now; install only once no turns are running. */
  async downloadApp(update: Update, progress: (fraction: number | null) => void) {
    let total = 0;
    let done = 0;
    await update.download((event) => {
      if (event.event === "Started") total = event.data.contentLength ?? 0;
      else if (event.event === "Progress") {
        done += event.data.chunkLength;
        progress(total ? Math.min(done / total, 1) : null);
      }
    });
  },
  installApp: (update: Update) => update.install(),
  restartApp: relaunch,
  checkEngine: () => invoke<EngineUpdateView>("engine_update_check"),
  /** Run `dial update`; resolves to its output. */
  updateEngine: () => invoke<string>("engine_update"),
  /** Install and select a separate signed release; existing connections stay intact. */
  installEngine: () => invoke<string>("engine_install"),
};
