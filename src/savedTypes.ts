// Types: the user's saved collections, each opened as its own small app. Mirrors
// `src-tauri/src/types.rs`; the Rust side owns the files and checks every value.

import { reactive } from "vue";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { engine, showToast, snapshot } from "./bridge";
import { focusComposer, openMenu, ui, type MenuItem } from "./ui";
import {
  isDone, occurrence, today, markdownImages,
  type Field, type Fields, type Layout, type TypeView,
} from "./typeExtract";

export type { Field, FieldKind, Fields, Layout, TypeView } from "./typeExtract";

export type Font = "sans" | "serif" | "mono" | "hand" | "display" | "rounded";
export type Texture = "none" | "paper" | "linen" | "grid" | "dots" | "noise";

export interface Theme {
  accent: string;
  paper: string;
  ink: string;
  font: Font;
  texture: Texture;
  radius: number;
}

export interface TypeDef {
  version: number;
  id: string;
  name: string;
  description: string;
  icon: string;
  created: number;
  updated: number;
  fields: Field[];
  view: TypeView;
  theme: Theme;
  customHtml: string;
  /** Dial has chosen its layout and look; a type named in the sidebar has not yet. */
  designed: boolean;
  /** The type's own conversation per project: workspace root to session id. */
  sessions: Record<string, string>;
}

export interface Source { text: string; sessionId: string | null; workspace: string | null }

export interface Item {
  id: string;
  created: number;
  updated: number;
  fields: Fields;
  done: boolean;
  doneOn: string | null;
  remind: boolean;
  source: Source | null;
}

export interface Summary {
  id: string;
  name: string;
  icon: string;
  description: string;
  layout: Layout;
  accent: string;
  count: number;
  updated: number;
  designed: boolean;
  sessions: string[];
}

export interface Full { def: TypeDef; items: Item[]; assets: string }

export interface ItemPatch { fields?: Fields; done?: boolean; doneOn?: string; remind?: boolean }

export const typeApi = {
  list: () => invoke<Summary[]>("types_list"),
  get: (id: string) => invoke<Full>("types_get", { id }),
  create: (def: Partial<TypeDef>) => invoke<TypeDef>("types_create", { def }),
  update: (id: string, patch: Record<string, unknown>) => invoke<TypeDef>("types_update", { id, patch }),
  remove: (id: string) => invoke<void>("types_delete", { id }),
  addItem: (id: string, fields: Fields, source: Source | null = null) =>
    invoke<Item>("types_add_item", { id, fields, source }),
  updateItem: (id: string, itemId: string, patch: ItemPatch) =>
    invoke<Item>("types_update_item", { id, itemId, patch }),
  removeItem: (id: string, itemId: string) => invoke<void>("types_remove_item", { id, itemId }),
  moveItem: (id: string, itemId: string, to: number) => invoke<void>("types_move_item", { id, itemId, to }),
  importImage: (id: string, path: string) => invoke<string>("types_import_image", { id, path }),
  importImageData: (id: string, data: string) => invoke<string>("types_import_image_data", { id, data }),
  allowReminders: () => invoke<void>("type_reminders_allow"),
  remind: (typeId: string, title: string, body: string) => invoke<void>("type_reminder", { typeId, title, body }),
};

/** The sidebar's list, and a counter per type that views reload on. */
export const types = reactive({
  list: [] as Summary[],
  loaded: false,
  revision: {} as Record<string, number>,
});

export async function refreshTypes() {
  if (!isTauri()) return;
  try {
    types.list = await typeApi.list();
    types.loaded = true;
  } catch (error) {
    showToast(`Could not load your types: ${error}`);
  }
}

// A type's own conversation. Opening a type shows its session (the composer talks to it, and the
// Chat switch shows its transcript); those sessions stay out of the session list. Prompts sent
// there start with a `[[type:ID]]` line, so Dial knows which type "this" is.
//
// A type gets its conversation only when something is sent in it: opening or creating a type
// never starts a session, so no empty "Untitled session" is left behind, and opening several new
// types in a row never races several new sessions. The session is claimed as the type's before
// the first prompt goes out, so it never shows in the list as an ordinary one.

export const typeState = reactive({
  returnTo: null as string | null,
  /** The type whose new conversation is starting: the next session on screen is its own. */
  pending: null as string | null,
  /** An answer to save into the next type made in the sidebar. */
  saveAfterCreate: null as PendingSave | null,
});

const TAG = /^\[\[type:([a-z0-9-]+)\]\]\n?/;

/** `text` without the type line it was sent with, for the transcript. */
export function withoutTypeTag(text: string): string {
  return text.replace(TAG, "");
}

export function isTypeSession(id: string | null | undefined): boolean {
  return !!id && types.list.some((t) => t.sessions.includes(id));
}

/** Whether the session on screen is the open type's own conversation. */
export function typeSessionOnScreen(): boolean {
  const id = snapshot.value?.sessionId;
  const open = types.list.find((t) => t.id === ui.typeOpen);
  return !!open && !!id && open.sessions.includes(id);
}

/** A prompt for session `sessionId`: tagged when it is the open type's conversation. */
export function typedPrompt(text: string, sessionId: string | null = snapshot.value?.sessionId ?? null): string {
  const open = types.list.find((t) => t.id === ui.typeOpen);
  return open && sessionId && open.sessions.includes(sessionId) ? `[[type:${open.id}]]\n${text}` : text;
}

/** Open type `id` in place of the transcript, with its own conversation behind it if it has one. */
export async function openType(id: string) {
  ui.settingsOpen = false;
  ui.typeMode = "type";
  const snap = snapshot.value;
  if (!ui.typeOpen) typeState.returnTo = snap?.sessionId && !isTypeSession(snap.sessionId) ? snap.sessionId : null;
  ui.typeOpen = id;
  if (!snap?.workspace) return;
  const root = snap.workspace.root;
  let session: string | undefined;
  try {
    session = (await typeApi.get(id)).def.sessions[root];
  } catch (error) {
    return showToast(String(error));
  }
  // No conversation yet: the first prompt starts one (`sendInType`).
  if (ui.typeOpen !== id || !session || snapshot.value?.sessionId === session) return;
  // Opening runs in the engine; a session that is gone leaves the old one on screen.
  if ((await engine.resume(session, root)) && (await until(() => snapshot.value?.sessionId === session, 8000))) return;
  if (ui.typeOpen === id) await forgetSession(id, root);
}

async function until(test: () => boolean, timeout: number): Promise<boolean> {
  const end = Date.now() + timeout;
  while (!test()) {
    if (Date.now() > end) return false;
    await new Promise((r) => setTimeout(r, 100));
  }
  return true;
}

/** Leave the type view without choosing a destination session. */
export function dismissType() {
  ui.typeOpen = null;
  typeState.returnTo = null;
  typeState.pending = null;
}

/** Back to the conversation the type was opened from. */
export function closeType() {
  const back = typeState.returnTo;
  const onType = typeSessionOnScreen();
  dismissType();
  if (back && snapshot.value?.sessionId !== back) void engine.resume(back);
  else if (!back && onType) void engine.newSession();
}

async function setSession(id: string, root: string, session: string | null) {
  const { def } = await typeApi.get(id);
  const sessions = { ...def.sessions };
  if (session) sessions[root] = session;
  else delete sessions[root];
  await typeApi.update(id, { sessions });
  await refreshTypes();
}

function forgetSession(id: string, root: string) {
  return setSession(id, root, null).catch((error) => showToast(String(error)));
}

/** The session on screen can take a prompt. */
function ready(): boolean {
  const s = snapshot.value;
  return !!s?.sessionId && s.canSend && !s.turnRunning && s.connection.state === "live";
}

/**
 * Start type `id`'s conversation in this project and claim it as the type's. An untouched new
 * session on screen that belongs to nobody is used as it is; otherwise a new one is started.
 */
async function startTypeSession(id: string, root: string): Promise<string | null> {
  const now = snapshot.value;
  let session: string | null = null;
  if (now?.sessionId && !now.entries.length && !now.turnRunning && !isTypeSession(now.sessionId)) {
    session = now.sessionId;
    if (typeState.returnTo === session) typeState.returnTo = null;
  } else {
    const before = now?.sessionId ?? null;
    typeState.pending = id;
    try {
      await engine.newSession();
      const started = await until(() => snapshot.value?.sessionId !== before && !snapshot.value?.entries.length && ready(), 10000);
      if (!started || ui.typeOpen !== id) return null;
      session = snapshot.value!.sessionId;
    } finally {
      typeState.pending = null;
    }
  }
  if (!session) return null;
  await setSession(id, root, session);
  return session;
}

/**
 * Send `text` in type `id`'s own conversation, starting it first when the type has none in this
 * project. The type must be the one open.
 */
export async function sendInType(id: string, text: string, images: Parameters<typeof engine.send>[1] = []): Promise<boolean> {
  const root = snapshot.value?.workspace?.root;
  if (!root) return false;
  try {
    let session: string | null = (await typeApi.get(id)).def.sessions[root] ?? null;
    if (session && snapshot.value?.sessionId !== session) {
      await openType(id);
      if (snapshot.value?.sessionId !== session) session = null;
    }
    const first = !session;
    if (!session) session = await startTypeSession(id, root);
    if (!session || !(await until(ready, 10000))) throw new Error("Dial is not ready yet; try again in a moment");
    if (first) {
      // Named while the session is idle: the engine refuses a rename during a turn.
      const t = types.list.find((x) => x.id === id);
      const title = `${t?.icon ?? ""} ${t?.name ?? "Type"}`.trim();
      try {
        await engine.renameSession(session, title);
        await until(() => snapshot.value?.title === title && ready(), 3000);
      } catch {
        // An unnamed conversation still works.
      }
    }
    return await engine.send(`[[type:${id}]]\n${text}`, images, { workspaceRoot: root, sessionId: session });
  } catch (error) {
    showToast(`Could not send: ${error instanceof Error ? error.message : error}`);
    return false;
  }
}

/**
 * Follow the session on screen while a type is open: its own conversation, or the new one being
 * started for it, keep it open; any other session (picked in the sidebar, the palette, ⌘N)
 * closes the type.
 */
export function followTypeSession(root: string | undefined, id: string | null, previousRoot: string | undefined, previousId: string | null) {
  const open = ui.typeOpen;
  if (!open) return;
  if (root !== previousRoot) {
    ui.typeOpen = null;
    return;
  }
  if (!id || id === previousId) return;
  if (types.list.find((t) => t.id === open)?.sessions.includes(id)) return;
  if (typeState.pending === open) return;
  ui.typeOpen = null;
}
/** Make a type from just a name, open it, and save the answer waiting for it, if any. */
export async function createType(name: string): Promise<boolean> {
  try {
    const def = await typeApi.create({ name });
    await refreshTypes();
    const save = typeState.saveAfterCreate;
    typeState.saveAfterCreate = null;
    if (save) return saveTo(def.id, save);
    await openType(def.id);
    focusComposer();
    return true;
  } catch (error) {
    showToast(String(error));
    return false;
  }
}

/** The absolute path of asset `name` of a type. */
export function assetPath(full: Full, name: unknown): string | null {
  return typeof name === "string" && name ? `${full.assets.replace(/[\\/]+$/, "")}/${name}` : null;
}

/** Text of a field for a line of UI: lists joined, numbers as written. */
export function fieldText(value: unknown): string {
  if (Array.isArray(value)) return value.join(", ");
  if (value === null || value === undefined) return "";
  return String(value);
}

/** The title of an item: its title field, or its first text. */
export function itemTitle(def: TypeDef, item: Item): string {
  const key = def.view.titleField;
  const title = key ? fieldText(item.fields[key]) : "";
  if (title) return title;
  const first = def.fields.find((f) => ["text", "longtext", "markdown"].includes(f.kind) && item.fields[f.key]);
  return first ? fieldText(item.fields[first.key]).split("\n")[0].replace(/^#+\s*/, "").slice(0, 120) : "Untitled";
}

/** A 12- or 24-hour label for `HH:MM`, as the system shows times. */
export function timeLabel(value: unknown): string {
  const at = occurrence(value);
  if (!at) return fieldText(value);
  const full = /^\d{4}-/.test(String(value));
  return at.toLocaleString(undefined, full ? { dateStyle: "medium", timeStyle: "short" } : { timeStyle: "short" });
}

export interface PendingSave {
  text: string;
  /** Absolute image paths the answer showed or produced. */
  images: string[];
  sessionId: string | null;
  workspace: string | null;
}

/** An answer, ready to save: its text and the images it shows. */
export function pendingSave(text: string, images: string[], workspace: string | null, sessionId: string | null): PendingSave {
  const shown = markdownImages(text, workspace ?? "");
  return { text, images: [...new Set([...images, ...shown])], sessionId, workspace };
}

/** What Dial is asked when an answer is saved to `def`. */
function savePrompt(def: TypeDef, save: PendingSave): string {
  const lines = [
    `[[type:${def.id}]]`,
    `Save this to “${def.name}”.${def.designed ? "" : " It has no design yet: design it to suit this first."}`,
    "",
    save.text.trim(),
  ];
  if (save.images.length) lines.push("", "Images it shows (absolute paths, for image fields):", ...save.images.map((p) => `- ${p}`));
  return lines.join("\n");
}

/**
 * Hand an answer to Dial to add to type `id`, styled to fit. It runs in the type's own
 * conversation: in the background when the type has one in this project, or else by opening the
 * type, where its first prompt starts it.
 */
export async function saveTo(id: string, save: PendingSave): Promise<boolean> {
  const snap = snapshot.value;
  if (!snap?.workspace) return false;
  const root = snap.workspace.root;
  try {
    const { def } = await typeApi.get(id);
    const prompt = savePrompt(def, save);
    const session = def.sessions[root];
    if (session && session === snap.sessionId) {
      return await engine.send(prompt, [], { workspaceRoot: root, sessionId: session });
    }
    if (session) {
      await invoke("send_background", { workspaceRoot: root, sessionId: session, text: prompt });
      showToast(`Dial is adding it to ${def.icon} ${def.name}`);
      return true;
    }
    await openType(id);
    if (ui.typeOpen !== id) return false;
    // The prompt carries its own type line, so it is sent as it is.
    return await sendInType(id, prompt.replace(TAG, ""));
  } catch (error) {
    showToast(`Could not save: ${error instanceof Error ? error.message : error}`);
    return false;
  }
}

/** The "Save to…" menu under an answer: every type, then a new one. */
export function saveMenu(event: MouseEvent, save: PendingSave, anchor?: HTMLElement | null) {
  const items: MenuItem[] = types.list.map((t) => ({ label: `${t.icon} ${t.name}`, run: () => void saveTo(t.id, save) }));
  if (items.length) items.push({ divider: true });
  items.push({
    label: "New type…",
    icon: "plus",
    run: () => {
      typeState.saveAfterCreate = save;
      ui.sidebarOpen = true;
      ui.typeNaming = true;
    },
  });
  openMenu(event, items, anchor);
}

// Reminders: while the app runs, items whose type has reminders on notify ahead of their time.

const FIRED_KEY = "dial.type-reminders-fired";
const TICK = 20_000;
/** A reminder that comes due while the app sleeps still shows if it is this recent. */
const LATE = 5 * 60_000;

function firedToday(): Set<string> {
  try {
    const saved = JSON.parse(localStorage.getItem(FIRED_KEY) ?? "{}") as { day?: string; keys?: string[] };
    return new Set(saved.day === today() ? saved.keys ?? [] : []);
  } catch {
    return new Set();
  }
}

let reminderTypes: Full[] = [];

async function loadReminderTypes() {
  const out: Full[] = [];
  for (const summary of types.list) {
    if (summary.layout === "custom" || summary.count === 0) continue;
    try {
      const full = await typeApi.get(summary.id);
      if (full.def.view.reminders && full.def.view.timeField) out.push(full);
    } catch {
      // A damaged type shows its own error when opened.
    }
  }
  reminderTypes = out;
}

async function remindDue() {
  const now = new Date();
  const fired = firedToday();
  let changed = false;
  for (const { def, items } of reminderTypes) {
    for (const item of items) {
      if (!item.remind || isDone(item, def.view.dailyReset, now)) continue;
      const at = occurrence(item.fields[def.view.timeField!], now);
      if (!at) continue;
      for (const lead of def.view.leadMinutes) {
        const due = at.getTime() - lead * 60_000;
        const key = `${def.id}/${item.id}/${at.getTime()}/${lead}`;
        if (now.getTime() < due || now.getTime() - due > LATE || fired.has(key)) continue;
        fired.add(key);
        changed = true;
        const title = itemTitle(def, item);
        const body = lead === 0 ? `${title} — now` : `${title} in ${lead >= 60 && lead % 60 === 0 ? `${lead / 60} h` : `${lead} min`} (${timeLabel(item.fields[def.view.timeField!])})`;
        typeApi.remind(def.id, `${def.icon} ${def.name}`, body).catch((error) => console.warn("reminder", error));
      }
    }
  }
  if (changed) localStorage.setItem(FIRED_KEY, JSON.stringify({ day: today(now), keys: [...fired] }));
}

let started = false;

/** Load the list, follow changes from the agent and the views, and run reminders. */
export async function startTypes() {
  if (started || !isTauri()) return;
  started = true;
  await listen<string | null>("types-changed", async ({ payload }) => {
    if (payload) types.revision[payload] = (types.revision[payload] ?? 0) + 1;
    await refreshTypes();
    await loadReminderTypes();
  });
  await listen<string>("type-open", ({ payload }) => openType(payload));
  await refreshTypes();
  await loadReminderTypes();
  setInterval(() => void remindDue(), TICK);
  void remindDue();
}
