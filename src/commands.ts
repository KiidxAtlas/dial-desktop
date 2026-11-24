// Everything the palette (⌘K) can do, and the shortcut list. Keys here are the ones App.vue handles.

import { ref } from "vue";
import { engine, files, folders } from "./bridge";
import { recentProjects } from "./projects";
import type { OpenSnapshot } from "./types";
import { focusComposer, ui, type SettingsTab } from "./ui";
import { appearance, zoomBy, type Theme } from "./appearance";
import { showPanel, showSidebar, sidebarShown, toggleSidebar, workspace } from "./workspace";
import { checkApp, checkEngine } from "./updates";
import { canUseTaskRecipes, insertTaskRecipe, taskRecipes } from "./taskRecipes";
import { markTurnAborted } from "./queue";
import {
  canCompact,
  canRetry,
  canUndo,
  conversationMarkdown,
  copyText,
  editLastPrompt,
  openSession,
  listedSession,
  sessionOrder,
  startRename,
  stepSession,
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
  add(
    snap.voice.phase === "off"
      ? { id: "voice", group: "Session", label: "Talk to Dial (voice mode)", icon: "mic", hint: "⇧⌘V", keywords: "voice speak talk microphone", run: () => void engine.voiceStart() }
      : { id: "voice-end", group: "Session", label: "End voice mode", icon: "mic", keywords: "voice stop", run: () => void engine.voiceStop() },
    live && (snap.voice.phase !== "off" || !snap.turnRunning),
  );
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
  add({ id: "stop", group: "Session", label: "Stop the turn", icon: "stop", hint: "Esc", run: () => { markTurnAborted(snap); return engine.abort(); } }, snap.turnRunning);
  add({ id: "find", group: "Session", label: "Find in conversation", icon: "search", hint: "⌘F", run: openConversationFind }, snap.entries.length > 0);
  add({ id: "focus-composer", group: "Session", label: "Focus the message box", icon: "pencil", hint: "⌘L", keywords: "type prompt input", run: () => { ui.settingsOpen = false; focusComposer(); } });
  add({ id: "next-session", group: "Session", label: "Next session", icon: "arrowDown", hint: "⇧⌘]", keywords: "switch down", run: () => stepSession(snap, 1) }, sessionOrder(snap).length > 1);
  add({ id: "prev-session", group: "Session", label: "Previous session", icon: "arrowUp", hint: "⇧⌘[", keywords: "switch up", run: () => stepSession(snap, -1) }, sessionOrder(snap).length > 1);
  add({ id: "search-sessions", group: "Session", label: "Search sessions", icon: "search", hint: "⇧⌘F", keywords: "filter find sidebar", run: () => { showSidebar(); ui.focusSearch++; } });

  if (canUseTaskRecipes(snap)) {
    for (const recipe of taskRecipes()) {
      add({
        id: `recipe-${recipe.id}`,
        group: "Task recipes",
        label: recipe.title,
        icon: "pencil",
        keywords: recipe.detail,
        run: () => insertTaskRecipe(recipe.prompt),
      });
    }
  }

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

  add({ id: "sidebar", group: "View", label: sidebarShown() ? "Hide sidebar" : "Show sidebar", icon: "panelLeft", hint: "⌘B", run: toggleSidebar });
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
  add({ id: "quick-open", group: "View", label: "Quick-open project file", icon: "file", hint: "⌘P", run: () => { ui.filePalette = true; ui.paletteOpen = true; } });
  add({ id: "terminal", group: "View", label: "Open the terminal", icon: "terminal", hint: "⌃`", keywords: "shell console commands run", run: () => showPanel("terminal") });
  add({ id: "changes", group: "View", label: "Show changed files", icon: "diff", keywords: "diff review edits", run: () => showPanel("changes") });
  add({ id: "activity", group: "View", label: "Open Activity", icon: "activity", keywords: "watch jobs tool calls /watch", run: () => showPanel("activity") });
  add({ id: "workers", group: "View", label: "Open Workers", icon: "chip", keywords: "session tasks messages", run: () => showPanel("workers") });
  add({ id: "peers", group: "View", label: "Open Peers", icon: "chat", keywords: "project sessions messages", run: () => showPanel("peers") });
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
  const page = (tab: SettingsTab) => () => { ui.settingsTab = tab; ui.settingsOpen = true; };
  add({ id: "roles", group: "Project", label: "Model roles & presets", icon: "gear", keywords: "roles presets small task coach tester memory /modelpreset", run: page("roles") }, live);
  add({ id: "library", group: "App", label: "Model library", icon: "gear", keywords: "download local models mlx gguf hugging face delete", run: page("library") });
  add({ id: "usage", group: "Project", label: "Usage limits & stats", icon: "gear", keywords: "usage cost tokens stats limits context /usage /stats /cost", run: page("usage") }, live);
  add({ id: "memory", group: "Project", label: "What Dial remembers", icon: "gear", keywords: "memory facts remember forget /memory", run: page("memory") }, live);
  add({ id: "skills", group: "Project", label: "Skills", icon: "gear", keywords: "skills install enable /skills", run: page("skills") }, live);
  add({ id: "extensions", group: "Project", label: "Extensions & MCP servers", icon: "gear", keywords: "mcp extensions plugins /extensions", run: page("extensions") }, live);
  add({ id: "checks", group: "Project", label: "Checks after edits", icon: "gear", keywords: "checks tests lint verify /checks", run: page("checks") }, live);
  add({ id: "folders", group: "Project", label: "Extra folders", icon: "gear", keywords: "dirs add-dir directories folders /dirs", run: page("folders") }, live);
  add({ id: "schedules", group: "Project", label: "Schedules", icon: "gear", keywords: "schedule cron recurring timer /schedule", run: page("schedules") }, live);
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
  add({ id: "project-window", group: "App", label: "Open project in new window…", icon: "folder", run: () => folders.choose(true) });
  add({ id: "folder", group: "App", label: "Open folder…", icon: "folder", keywords: "project workspace", run: () => folders.choose() });
  add({ id: "reconnect", group: "App", label: "Reconnect the engine", icon: "refresh", run: () => engine.reconnect() }, !live && snap.connection.state !== "preview");

  // The open project, and switching to a recent one.
  add({ id: "reveal-project", group: "Project", label: "Reveal project in Finder", icon: "folder", keywords: "show explorer folder", run: () => void files.reveal(snap.workspace.root) });
  add({ id: "copy-project-path", group: "Project", label: "Copy project path", icon: "copy", keywords: "folder directory", run: () => void copyText(snap.workspace.root, "Project path copied") });
  for (const w of recentProjects.value)
    if (w.root !== snap.workspace.root)
      add({ id: `project-${w.root}`, group: "Switch project", label: w.name, icon: "folder", keywords: `${w.display} project open recent`, run: () => void folders.open(w.root) });

  // Switching sessions, most recent first.
  for (const s of snap.sessions)
    if (!s.selected && listedSession(s, snap))
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
    else if (subsequence(label, q.replace(/\s+/g, "")) !== null) score = 5;
    if (score >= 0) scored.push({ command, score, at });
  });
  return scored.sort((a, b) => a.score - b.score || a.at - b.at).map((s) => s.command);
}

/** Positions of `q`'s characters in order within `text` (both lower case), or `null`. */
function subsequence(text: string, q: string): number[] | null {
  const at: number[] = [];
  let from = 0;
  for (const ch of q) {
    const i = text.indexOf(ch, from);
    if (i < 0) return null;
    at.push(i);
    from = i + 1;
  }
  return at;
}

/** `label` split into runs, marking what `query` matched: a substring if there is one, else its letters in order. */
export function highlightMatch(label: string, query: string): { text: string; hit: boolean }[] {
  const q = query.trim().toLowerCase();
  if (!q) return [{ text: label, hit: false }];
  const lower = label.toLowerCase();
  const marked = new Array<boolean>(label.length).fill(false);
  const start = lower.indexOf(q);
  if (start >= 0) marked.fill(true, start, start + q.length);
  else {
    const words = q.split(/\s+/).filter(Boolean);
    let any = false;
    for (const word of words) {
      const i = lower.indexOf(word);
      if (i >= 0) { marked.fill(true, i, i + word.length); any = true; }
    }
    if (!any) for (const i of subsequence(lower, q.replace(/\s+/g, "")) ?? []) marked[i] = true;
  }
  const out: { text: string; hit: boolean }[] = [];
  for (let i = 0; i < label.length; i++) {
    const last = out.at(-1);
    if (last && last.hit === marked[i]) last.text += label[i];
    else out.push({ text: label[i], hit: marked[i] });
  }
  return out;
}

export const SHORTCUTS: { group: string; keys: [string, string][] }[] = [
  {
    group: "General",
    keys: [
      ["⌘K", "Command palette"],
      ["⌘P", "Quick-open project file"],
      ["⌘N", "New session"],
      ["⌘,", "Settings"],
      ["⌘/", "Keyboard shortcuts"],
      ["⌘B", "Show or hide the sidebar"],
      ["⌥⌘I", "Show or hide plan and changes"],
      ["⇧⌘B", "Browser"],
      ["⇧⌘E", "Files"],
      ["⌃`", "Terminal"],
      ["⌘+  ⌘−  ⌘0", "Zoom in, out, actual size"],
      ["⌘L", "Focus the message box"],
    ],
  },
  {
    group: "Conversation",
    keys: [
      ["Enter", "Send (while Dial works: add a note)"],
      ["⇧Enter", "New line"],
      ["⌥⌘↑ / ⌥⌘↓", "Previous / next prompt you sent"],
      ["⌘Enter", "Queue a follow-up during a turn"],
      ["@", "Find and insert a project file path"],
      ["⌘F", "Find in conversation"],
      ["↑  ↓", "Earlier prompts (in an empty box)"],
      ["Esc", "Stop the turn (an open menu closes first; not from the terminal or other fields)"],
      ["⇧⌘M", "Change model"],
      ["⇧⌘V", "Voice: start talking, stop recording, or interrupt a spoken reply"],
      ["/", "Slash commands"],
      ["Enter  ⌘⌫", "Approval card: allow once (Allow has focus) / deny"],
    ],
  },
  {
    group: "Sessions",
    keys: [
      ["⌘-click  ⇧-click", "Select several"],
      ["⌘A", "Select all (in the session list with a selection)"],
      ["⌫", "Move selected to Recently Deleted"],
      ["⌘1 … ⌘9", "Open a session by position (hold the modifier to see numbers)"],
      ["⇧⌘[  ⇧⌘]", "Previous or next session"],
      ["⌃⇧Tab  ⌃Tab", "Cycle sessions backward or forward"],
      ["⇧⌘F", "Search sessions (Esc clears)"],
      ["F2  double-click", "Rename"],
      ["Right-click", "Session actions"],
    ],
  },
];
