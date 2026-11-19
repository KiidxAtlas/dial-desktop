// The only place the UI talks to the Rust side.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ask, open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { ref, shallowRef } from "vue";
import type {
  Answer,
  ApprovalMode,
  Attachment,
  Decision,
  ModelOption,
  SettingScope,
  SettingsPage,
  Snapshot,
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
  } catch (error) {
    showToast(String(error));
  }
}

export const engine = {
  send: (text: string, attachments: Attachment[] = []) =>
    call("send_prompt", {
      text,
      images: attachments.flatMap((a) => (a.path ? [a.path] : [])),
      inlineImages: attachments.flatMap((a) => (a.data ? [a.data] : [])),
    }),
  approve: (approvalId: string, decision: Decision) => call("approve", { approvalId, decision }),
  abort: () => call("abort"),
  newSession: () => call("new_session"),
  resume: (sessionId: string) => call("resume", { sessionId }),
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
    const confirmed = await ask(
      "Dial restores the files the last turn changed and removes that prompt and its answer from the conversation.",
      { title: "Undo the last turn?", kind: "warning", okLabel: "Undo", cancelLabel: "Cancel" },
    ).catch(() => false);
    if (confirmed) await call("undo_turn");
    return confirmed;
  },
  /** Add guidance to the running turn; Dial reads it at its next step. */
  steer: (text: string) => call("steer", { text }),
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
  open: (path: string) => call("open_workspace", { path }),
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
