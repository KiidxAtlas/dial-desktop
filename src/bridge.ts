// The only place the UI talks to the Rust side.

import { getVersion } from "@tauri-apps/api/app";
import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { downloadDir } from "@tauri-apps/api/path";
import { listen } from "@tauri-apps/api/event";
import { ask, open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { check as checkAppUpdate, type Update } from "@tauri-apps/plugin-updater";
import { ref, shallowRef } from "vue";
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
  SettingScope,
  SettingsPage,
  Snapshot,
  TerminalEvent,
  Workspace,
} from "./types";

/** Latest state published by the app; `null` until the first one arrives. Parts unchanged since
 * the previous snapshot keep their identity (see {@link share}), so Vue skips redrawing them. */
export const snapshot = shallowRef<Snapshot | null>(null);
export const startupError = ref<string | null>(null);
export const connecting = ref(false);

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

export interface DownloadStatus { repo: string; running: boolean; message: string }
export interface CachedModel { repo: string; bytes: number }
export interface HubModel { id: string; downloads: number; likes: number; pipeline_tag: string | null }
export interface HubFile { path: string; size: number; split: boolean }
export const modelDownloads = {
  status: () => invoke<DownloadStatus>('model_download_status'),
  cancel: () => invoke<DownloadStatus>('model_download_cancel'),
  cached: () => invoke<CachedModel[]>('model_cache_list'),
  search: (query: string, format: 'MLX' | 'GGUF') => invoke<HubModel[]>('model_hub_search', { query, format }),
  files: (repo: string) => invoke<HubFile[]>('model_hub_files', { repo }),
  async start(repo: string, size: string, file?: string) {
    const what = file ? `${file} from ${repo}` : repo;
    if (!await ask(`Download ${what} (about ${size}) from Hugging Face to this computer? This does not switch your session model.`, { title: 'Download model?' })) return null;
    return invoke<DownloadStatus>('model_download_start', { repo, file: file ?? null });
  },
  /** Delete a downloaded model from the Hugging Face cache after confirming; the bytes freed, or `null` if cancelled. */
  async remove(repo: string, size: string) {
    if (!await ask(`Delete ${repo} (${size}) from this computer?\n\nIt is removed from the Hugging Face cache and can be downloaded again later.`, { title: 'Delete model?', kind: 'warning', okLabel: 'Delete' })) return null;
    return invoke<number>('model_cache_remove', { repo });
  },
};

/** The settings page; `null` until the engine sent it. */
export const settingsPage = shallowRef<SettingsPage | null>(null);

export interface Toast {
  id: number;
  message: string;
}

const TOAST_MS = 6000;
export const toasts = ref<Toast[]>([]);
let lastToast = 0;

const timers = new Map<number, ReturnType<typeof setTimeout>>();

function dismissLater(id: number) {
  clearTimeout(timers.get(id));
  timers.set(
    id,
    setTimeout(() => {
      timers.delete(id);
      toasts.value = toasts.value.filter((t) => t.id !== id);
    }, TOAST_MS),
  );
}

/** Show `message`; one already on screen stays single and just stays longer. */
export function showToast(message: string) {
  const shown = toasts.value.find((t) => t.message === message);
  if (shown) return dismissLater(shown.id);
  const id = ++lastToast;
  toasts.value = [...toasts.value.slice(-2), { id, message }];
  dismissLater(id);
}

/** Remove a toast now (its close button). */
export function dismissToast(id: number) {
  clearTimeout(timers.get(id));
  timers.delete(id);
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
  snapshot.value = share(snapshot.value, next);
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
      partial.push(await listen<SettingsPage>("settings", e => { settingsPage.value = e.payload; }));
      partial.push(await listen<string>("toast", e => showToast(e.payload)));
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

export const engine = {
  send: (text: string, attachments: Attachment[], scope: { workspaceRoot: string; sessionId: string | null }) =>
    call("send_prompt", {
      ...scope, text,
      images: attachments.flatMap((a) => (a.path ? [a.path] : [])),
      inlineImages: attachments.flatMap((a) => (a.data ? [a.data] : [])),
    }),
  approve: (approvalId: string, decision: Decision) => call("approve", { approvalId, decision }),
  abort: () => call("abort"),
  newSession: () => call("new_session"),
  resume: (sessionId: string, workspaceRoot?: string, waitForClose = false) => call("resume", { sessionId, workspaceRoot, waitForClose }),
  searchTranscripts: (workspaceRoot: string, query: string) =>
    invoke<import("./types").TranscriptResults>("search_transcripts", { workspaceRoot, query }),
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
  /** Delete sessions after the user confirms; resolves to whether they were asked to go. */
  async deleteSessions(sessions: { id: string; title: string }[]): Promise<boolean> {
    if (!sessions.length) return false;
    const one = sessions.length === 1;
    const confirmed = await ask(
      one
        ? `“${sessions[0].title}” and its history will be removed. This cannot be undone.`
        : `${sessions.length} sessions and their history will be removed. This cannot be undone.`,
      {
        title: one ? "Delete session?" : `Delete ${sessions.length} sessions?`,
        kind: "warning",
        okLabel: "Delete",
        cancelLabel: "Cancel",
      },
    ).catch(() => false);
    return confirmed && await call("delete_sessions", { sessionIds: sessions.map((s) => s.id) });
  },
  renameSession: (sessionId: string, title: string) => call("rename_session", { sessionId, title }),
  pinSession: (sessionId: string, pinned: boolean) => call("pin_session", { sessionId, pinned }),
  /** Copy a session into a new one and open the copy. */
  forkSession: (sessionId: string) => call("fork_session", { sessionId }),
  async forkBefore(root: string, sessionId: string, prompt: string, occurrence: number, matches: number) {
    try {
      if (!await ask("Create a copy ending before this prompt? The original conversation and working files stay unchanged.", { title: "Fork from this message?" })) return;
      const id = await invoke<string>("fork_before", { workspaceRoot: root, sessionId, prompt, occurrence, matches });
      if (snapshot.value?.workspace?.root === root && snapshot.value.sessionId === sessionId) await engine.resume(id, root);
      else showToast("Fork created in the original project");
    } catch (e) { showToast(String(e)); }
  },
  /** Ask where to save, inside the project folder, and export the session on screen as Markdown. */
  async exportSession(root: string, title: string) {
    const slug = title.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 60) || "session";
    try {
      const path = await saveDialog({
        title: "Export session as Markdown",
        defaultPath: `${root}/${slug}.md`,
        filters: [{ name: "Markdown", extensions: ["md"] }],
      });
      if (path) await call("export_session", { path });
    } catch (error) {
      showToast(`Could not open the save dialog: ${error}`);
    }
  },
  /** Undo the last turn, files included, after the user confirms; resolves to whether it was sent. */
  async undoTurn(): Promise<boolean> {
    const current = snapshot.value;
    if (!current?.workspace) return false;
    const { sessionId } = current;
    const workspaceRoot = current.workspace.root;
    const confirmed = await ask(
      "Dial restores the files the last turn changed and removes that prompt and its answer from the conversation.",
      { title: "Undo the last turn?", kind: "warning", okLabel: "Undo", cancelLabel: "Cancel" },
    ).catch(() => false);
    return confirmed && await call("undo_turn", { workspaceRoot, sessionId });
  },
  /** Add guidance to the running turn; Dial reads it at its next step. */
  steer: (text: string, scope: { workspaceRoot: string; sessionId: string | null }) => call("steer", { ...scope, text }),
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
  /** The question is on screen; the engine starts its answer timeout. */
  questionPresented: (questionId: string) => call("question_presented", { questionId }),
  answerQuestion: (questionId: string, answers: Answer[]) =>
    call("answer_question", { questionId, reply: "answers", answers }),
  skipQuestion: (questionId: string) => call("answer_question", { questionId, reply: "cancel", answers: [] }),
};

export const folders = {
  recent: () => invoke<Workspace[]>("recent_workspaces").catch(() => [] as Workspace[]),
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
  async open(path: string) {
    try {
      let from = await invoke<string | null>("open_workspace", { path });
      while (from) {
        if (!(await ask("Switching projects will stop active turns and close open terminals in the current project.", { title: "Switch projects?", kind: "warning" }))) return false;
        from = await invoke<string | null>("open_workspace", { path, confirmedFrom: from });
      }
      return true;
    } catch (error) {
      showToast(String(error));
      return false;
    }
  },
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
  readClipboardImage: () => isTauri() ? invoke<string | null>("read_clipboard_image") : Promise.resolve(null),
  readTranscriptImage: (workspaceRoot: string, path: string) =>
    invoke<ImageFile>("read_transcript_image", { workspaceRoot, path }),
  /** Open a project image in the system's default viewer (Preview on a Mac). */
  openTranscriptImage: (workspaceRoot: string, path: string) =>
    invoke<void>("open_transcript_image", { workspaceRoot, path }).catch((error) => showToast(String(error))),
  /** Show the file in the system file manager (Finder, Explorer). */
  reveal: (path: string) => revealItemInDir(path).catch((error) => showToast(`Could not show the file: ${error}`)),
  /** Ask for images with the system dialog; empty when cancelled. */
  async chooseImages(): Promise<string[]> {
    const picked = await openDialog({
      multiple: true,
      title: "Attach images",
      filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "gif", "webp"] }],
    });
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

interface NotificationDestination { workspaceRoot: string; sessionId: string }

export const notificationApi = {
  setEnabled: (enabled: boolean, interactive: boolean) => invoke<void>("set_notifications", { enabled, interactive }),
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
  diff: (path: string, workspaceRoot: string) => invoke<DiffView>("file_diff", { path, workspaceRoot }),
  /** Fuzzy-search workspace files for `@` mentions in the composer. */
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

/** Shells for the Terminal panel, running in the project folder. Calls throw. */
export const terminalApi = {
  open: (cols: number, rows: number) => invoke<number>("terminal_open", { cols, rows }),
  openProviderSetup(workspaceRoot: string, cols: number, rows: number, handler: (event: TerminalEvent) => void) {
    const output = new Channel<TerminalEvent>(handler);
    return invoke<number>("provider_setup_open", { workspaceRoot, cols, rows, output });
  },
  write: (id: number, data: string) => invoke<void>("terminal_write", { id, data }),
  resize: (id: number, cols: number, rows: number) => invoke<void>("terminal_resize", { id, cols, rows }),
  close: (id: number) => invoke<void>("terminal_close", { id }),
  onEvent: (handler: (event: TerminalEvent) => void) => listen<TerminalEvent>("terminal", (e) => handler(e.payload)),
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
  /** Download, verify and install `update`, reporting progress as 0..1, then restart into it. */
  async installApp(update: Update, progress: (fraction: number | null) => void) {
    let total = 0;
    let done = 0;
    await update.downloadAndInstall((event) => {
      if (event.event === "Started") total = event.data.contentLength ?? 0;
      else if (event.event === "Progress") {
        done += event.data.chunkLength;
        progress(total ? Math.min(done / total, 1) : null);
      }
    });
    await relaunch();
  },
  checkEngine: () => invoke<EngineUpdateView>("engine_update_check"),
  /** Run `dial update`; resolves to its output. */
  updateEngine: () => invoke<string>("engine_update"),
  /** Install the engine with Dial's signed installer and connect to it; resolves to its output. */
  installEngine: () => invoke<string>("engine_install"),
};
