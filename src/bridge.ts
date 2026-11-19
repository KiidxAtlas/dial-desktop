// The only place the UI talks to the Rust side.

import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
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

/** Latest state published by the app; `null` until the first one arrives. */
export const snapshot = shallowRef<Snapshot | null>(null);

/** Models the user can switch to; empty until the engine listed them. */
export const models = shallowRef<ModelOption[]>([]);

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

/** Subscribe to updates, then load the current state. */
export async function connect() {
  await listen<Snapshot>("snapshot", (event) => {
    snapshot.value = event.payload;
  });
  await listen<ModelOption[]>("models", (event) => {
    models.value = event.payload;
  });
  await listen<SettingsPage>("settings", (event) => {
    settingsPage.value = event.payload;
  });
  await listen<string>("toast", (event) => showToast(event.payload));
  const [initial, list] = await Promise.all([
    invoke<Snapshot>("snapshot"),
    invoke<ModelOption[]>("models"),
  ]);
  // An event that arrived meanwhile is newer than these replies.
  if (!snapshot.value) snapshot.value = initial;
  if (!models.value.length) models.value = list;
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
  resume: (sessionId: string, workspaceRoot?: string) => call("resume", { sessionId, workspaceRoot }),
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
    if (confirmed) await call("delete_sessions", { sessionIds: sessions.map((s) => s.id) });
    return confirmed;
  },
  renameSession: (sessionId: string, title: string) => call("rename_session", { sessionId, title }),
  pinSession: (sessionId: string, pinned: boolean) => call("pin_session", { sessionId, pinned }),
  /** Copy a session into a new one and open the copy. */
  forkSession: (sessionId: string) => call("fork_session", { sessionId }),
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
  refreshSessions: () => call("refresh_sessions"),
  refreshModels: () => call("refresh_models"),
  setModel: (provider: string, modelId: string) => call("set_model", { provider, modelId }),
  /** `default` restores the model's own level. */
  setThinking: (level: string) => call("set_thinking", { level }),
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
  async choose() {
    try {
      const path = await openDialog({ directory: true, multiple: false, title: "Open a project folder" });
      if (typeof path === "string") await folders.open(path);
    } catch (error) {
      showToast(`Could not open the folder picker: ${error}`);
    }
  },
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

/** The open project's files, for the Files and Changes panels. Calls throw a readable message. */
export const workspaceFiles = {
  /** Folder `path` relative to the project; `""` is its root. */
  list: (path: string) => invoke<DirEntry[]>("list_dir", { path }),
  read: (path: string) => invoke<FileView>("read_file", { path }),
  /** Uncommitted changes of `path` (relative or absolute inside the project). */
  diff: (path: string) => invoke<DiffView>("file_diff", { path }),
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
