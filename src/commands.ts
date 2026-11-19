// Everything the palette (⌘K) can do, and the shortcut list. Keys here are the ones App.vue handles.

import { ref } from "vue";
import { engine, folders } from "./bridge";
import type { OpenSnapshot } from "./types";
import { focusComposer, ui } from "./ui";
import { appearance, zoomBy, type Theme } from "./appearance";
import { showPanel, workspace } from "./workspace";
import { checkApp, checkEngine } from "./updates";
import {
  canCompact,
  canRetry,
  canUndo,
  conversationMarkdown,
  copyText,
  editLastPrompt,
  openSession,
  startRename,
} from "./sessionActions";

export interface Command {
  id: string;
  label: string;
  group: string;
  icon?: string;
  /** Shown on the right, like `⌘N`. */
  hint?: string;
  /** Extra words the filter matches. */
  keywords?: string;
  run: () => void;
}

export const conversationFindOpen = ref(false);

function openConversationFind() {
  ui.settingsOpen = false;
  conversationFindOpen.value = true;
}
export function startNewSession(snap: OpenSnapshot) {
  if (snap.connection.state !== "live") return;
  ui.settingsOpen = false;
  engine.newSession();
  focusComposer();
}

const THEMES: { theme: Theme; label: string; icon: string }[] = [
  { theme: "system", label: "Match the system", icon: "monitor" },
  { theme: "light", label: "Light", icon: "sun" },
  { theme: "dark", label: "Dark", icon: "moon" },
];

export function commands(snap: OpenSnapshot): Command[] {
  const live = snap.connection.state === "live";
  const session = openSession(snap);
  const list: Command[] = [];
  const add = (command: Command, when = true) => when && list.push(command);

  add({ id: "new", group: "Session", label: "New session", icon: "plus", hint: "⌘N", run: () => startNewSession(snap) }, live);
  if (session) {
    add({ id: "rename", group: "Session", label: "Rename session", icon: "pencil", run: () => startRename(session.id, "title") });
    add({
      id: "pin",
      group: "Session",
      label: session.pinned ? "Unpin session" : "Pin session",
      icon: session.pinned ? "unpin" : "pin",
      run: () => engine.pinSession(session.id, !session.pinned),
    });
    add(
      { id: "fork", group: "Session", label: "Duplicate session", icon: "duplicate", run: () => engine.forkSession(session.id) },
      live && !snap.turnRunning,
    );
    add(
      {
        id: "export",
        group: "Session",
        label: "Export as Markdown…",
        icon: "download",
        run: () => engine.exportSession(snap.workspace.root, session.title),
      },
      live && snap.entries.length > 0,
    );
  }
  add(
    {
      id: "copy",
      group: "Session",
      label: "Copy conversation as Markdown",
      icon: "copy",
      run: () => copyText(conversationMarkdown(snap), "Conversation copied as Markdown"),
    },
    snap.entries.length > 0,
  );
  add(
    { id: "edit", group: "Session", label: "Edit last prompt", icon: "pencil", keywords: "undo resend change", run: () => editLastPrompt(snap) },
    live && canUndo(snap),
  );
  add({ id: "undo", group: "Session", label: "Undo last turn", icon: "undo", run: () => engine.undoTurn() }, live && canUndo(snap));
  add({ id: "retry", group: "Session", label: "Retry last prompt", icon: "refresh", keywords: "again failed", run: () => engine.retryTurn() }, canRetry(snap));
  add(
    { id: "compact", group: "Session", label: "Compact conversation", icon: "compress", keywords: "context summarize free tokens", run: () => engine.compact() },
    canCompact(snap),
  );
  add({ id: "stop", group: "Session", label: "Stop the turn", icon: "stop", hint: "Esc", run: () => engine.abort() }, snap.turnRunning);
  add({ id: "find", group: "Session", label: "Find in conversation", icon: "search", hint: "⌘F", run: openConversationFind }, snap.entries.length > 0);

  add({
    id: "model",
    group: "Model",
    label: "Change model…",
    icon: "sparkle",
    hint: "⇧⌘M",
    keywords: "provider",
    run: () => {
      ui.settingsOpen = false;
      ui.modelPickerOpen = true;
    },
  });

  add({ id: "sidebar", group: "View", label: ui.sidebarOpen ? "Hide sidebar" : "Show sidebar", icon: "panelLeft", hint: "⌘B", run: () => (ui.sidebarOpen = !ui.sidebarOpen) });
  add({
    id: "inspector",
    group: "View",
    label: ui.inspectorOpen ? "Hide plan and changes" : "Show plan and changes",
    icon: "sidebar",
    hint: "⌥⌘I",
    keywords: "inspector diff",
    run: () => (ui.inspectorOpen = !ui.inspectorOpen),
  });
  add({ id: "browser", group: "View", label: "Open the browser", icon: "globe", hint: "⇧⌘B", keywords: "web page preview localhost url", run: () => showPanel("browser") });
  add({ id: "files", group: "View", label: "Browse files", icon: "folder", hint: "⇧⌘E", keywords: "explorer tree project open file", run: () => showPanel("files") });
  add({ id: "terminal", group: "View", label: "Open the terminal", icon: "terminal", hint: "⌃`", keywords: "shell console commands run", run: () => showPanel("terminal") });
  add({ id: "changes", group: "View", label: "Show changed files", icon: "diff", keywords: "diff review edits", run: () => showPanel("changes") });
  add({
    id: "auto-previews",
    group: "View",
    label: workspace.autoOpenPreviews ? "Stop opening dev servers in the browser" : "Open dev servers in the browser automatically",
    icon: "globe",
    keywords: "preview localhost auto open",
    run: () => (workspace.autoOpenPreviews = !workspace.autoOpenPreviews),
  });
  for (const t of THEMES)
    add(
      { id: `theme-${t.theme}`, group: "View", label: `Theme: ${t.label}`, icon: t.icon, keywords: "appearance color mode", run: () => (appearance.theme = t.theme) },
      appearance.theme !== t.theme,
    );
  add({ id: "zoom-in", group: "View", label: "Zoom in", icon: "plus", hint: "⌘+", keywords: "bigger text size", run: () => zoomBy(1) });
  add({ id: "zoom-out", group: "View", label: "Zoom out", icon: "minus", hint: "⌘−", keywords: "smaller text size", run: () => zoomBy(-1) });
  add({ id: "zoom-reset", group: "View", label: "Actual size", hint: "⌘0", keywords: "zoom reset", run: () => zoomBy(0) }, appearance.zoom !== 1);

  add({ id: "settings", group: "App", label: "Settings", icon: "gear", hint: "⌘,", keywords: "preferences background", run: () => (ui.settingsOpen = true) });
  add({
    id: "updates",
    group: "App",
    label: "Check for updates",
    icon: "download",
    keywords: "upgrade version engine dial desktop",
    run: () => {
      ui.settingsTab = "updates";
      ui.settingsOpen = true;
      void Promise.all([checkApp(), checkEngine()]);
    },
  });
  add({ id: "shortcuts", group: "App", label: "Keyboard shortcuts", icon: "keyboard", hint: "⌘/", keywords: "help keys", run: () => (ui.shortcutsOpen = true) });
  add({ id: "folder", group: "App", label: "Open folder…", icon: "folder", keywords: "project workspace", run: () => folders.choose() });
  add({ id: "reconnect", group: "App", label: "Reconnect the engine", icon: "refresh", run: () => engine.reconnect() }, !live && snap.connection.state !== "preview");

  // Switching sessions, most recent first.
  for (const s of snap.sessions)
    if (!s.selected)
      list.push({
        id: `session-${s.id}`,
        group: "Go to session",
        label: s.title,
        icon: s.pinned ? "pin" : "chat",
        keywords: s.meta,
        run: () => {
          ui.settingsOpen = false;
          engine.resume(s.id);
        },
      });
  return list;
}

/** Rank `commands` by how well they match `query`: label start, word start, then anywhere; else dropped. */
export function filterCommands(list: Command[], query: string): Command[] {
  const q = query.trim().toLowerCase();
  if (!q) return list;
  const scored: { command: Command; score: number; at: number }[] = [];
  list.forEach((command, at) => {
    const label = command.label.toLowerCase();
    const extra = `${command.group} ${command.keywords ?? ""}`.toLowerCase();
    let score = -1;
    if (label.startsWith(q)) score = 0;
    else if (label.split(/[^a-z0-9]+/).some((word) => word.startsWith(q))) score = 1;
    else if (label.includes(q)) score = 2;
    else if (extra.includes(q)) score = 3;
    else if (q.split(/\s+/).every((part) => label.includes(part) || extra.includes(part))) score = 4;
    if (score >= 0) scored.push({ command, score, at });
  });
  return scored.sort((a, b) => a.score - b.score || a.at - b.at).map((s) => s.command);
}

export const SHORTCUTS: { group: string; keys: [string, string][] }[] = [
  {
    group: "General",
    keys: [
      ["⌘K", "Command palette"],
      ["⌘N", "New session"],
      ["⌘,", "Settings"],
      ["⌘/", "Keyboard shortcuts"],
      ["⌘B", "Show or hide the sidebar"],
      ["⌥⌘I", "Show or hide plan and changes"],
      ["⇧⌘B", "Browser"],
      ["⇧⌘E", "Files"],
      ["⌃`", "Terminal"],
      ["⌘+  ⌘−  ⌘0", "Zoom in, out, actual size"],
    ],
  },
  {
    group: "Conversation",
    keys: [
      ["Enter", "Send (while Dial works: add a note)"],
      ["⇧Enter", "New line"],
      ["⌘F", "Find in conversation"],
      ["↑", "Previous prompt (in an empty box)"],
      ["Esc", "Stop the turn"],
      ["⇧⌘M", "Change model"],
      ["/", "Slash commands"],
    ],
  },
  {
    group: "Sessions",
    keys: [
      ["⌘-click  ⇧-click", "Select several"],
      ["⌘A", "Select all (with a selection)"],
      ["⌫", "Delete the selected"],
      ["F2  double-click", "Rename"],
      ["Right-click", "Session actions"],
    ],
  },
];
