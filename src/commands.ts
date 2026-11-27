// Everything the palette (⌘K) can do, and the shortcut list. Keys here are the ones App.vue handles.

import { ref } from "vue";
import { engine, files, folders, showToast } from "./bridge";
import { aiReviewView, experienceView, gitLogView, importSessions, learningStatus, runInTerminal, runTyped, sessionTreeView, shareView, understandingView, worktreesView } from "./commandRunner";
import { addToPrompt } from "./workspace";
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

/** The terminal's slash command for the same action, from the first `/word` in `keywords`. */
export function slashOf(command: Pick<Command, "keywords">): string | null {
  return command.keywords?.match(/(?:^|\s)(\/[a-z][a-z-]*)/)?.[1] ?? null;
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
    add({ id: "rename", group: "Session", label: "Rename session", icon: "pencil", hint: "F2", run: () => startRename(session.id, "title") });
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
    add(
      { id: "export-html", group: "Session", label: "Export as HTML…", icon: "download", keywords: "export page web share standalone /export --html", run: () => engine.exportSession(snap.workspace.root, session.title, "html") },
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
  add({ id: "focus-composer", group: "Session", label: "Focus the message box", icon: "chat", hint: "⌘L", keywords: "type prompt input", run: () => { ui.settingsOpen = false; focusComposer(); } });
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
  add({ id: "boards", group: "View", label: "Open Boards", icon: "chip", keywords: "hardware serial USB Arduino ESP32 monitor capture", run: () => showPanel("boards") });
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

  add({ id: "settings", group: "App", label: "Settings", icon: "gear", hint: "⌘,", keywords: "preferences background /settings", run: () => (ui.settingsOpen = true) });
  const page = (tab: SettingsTab) => () => { ui.settingsTab = tab; ui.settingsOpen = true; };
  // Labels match the Settings menu, and each names its terminal command, so one name works in both.
  add({ id: "roles", group: "Project", label: "Roles & presets", icon: "gear", keywords: "/models roles presets small task coach tester memory /modelpreset", run: page("roles") }, live);
  add({ id: "library", group: "App", label: "Model library", icon: "gear", keywords: "/models download local models mlx gguf hugging face delete", run: page("library") });
  add({ id: "providers", group: "App", label: "Providers", icon: "gear", keywords: "login account api key provider settings sign-in /providers", run: page("providers") });
  add({ id: "images", group: "App", label: "Image generation", icon: "gear", keywords: "image model picture flux /images", run: page("images") });
  add({ id: "appearance", group: "App", label: "Appearance", icon: "gear", keywords: "theme background zoom cards diagrams /theme", run: page("appearance") });
  add({ id: "usage", group: "Project", label: "Usage & stats", icon: "gear", keywords: "usage cost tokens stats limits context /stats /usage /cost", run: page("usage") }, live);
  add({ id: "memory", group: "Project", label: "Memory", icon: "gear", keywords: "what dial remembers memory facts remember forget /memory", run: page("memory") }, live);
  add({ id: "skills", group: "Project", label: "Skills", icon: "gear", keywords: "skills install enable /skills", run: page("skills") }, live);
  add({ id: "extensions", group: "Project", label: "Extensions", icon: "gear", keywords: "mcp servers extensions plugins /extensions", run: page("extensions") }, live);
  add({ id: "checks", group: "Project", label: "Checks", icon: "gear", keywords: "checks after edits tests lint verify /checks", run: page("checks") }, live);
  add({ id: "folders", group: "Project", label: "Extra folders", icon: "gear", keywords: "dirs add-dir directories folders /dirs", run: page("folders") }, live);
  add({ id: "schedules", group: "Project", label: "Schedules", icon: "gear", keywords: "schedule cron recurring timer /schedule", run: page("schedules") }, live);
  add({ id: "permissions", group: "Project", label: "Permissions", icon: "gear", keywords: "permission rules allow deny approval always never /permissions", run: page("permissions") }, live);
  // Workflows the engine serves in its own terminal UI; they open in the Terminal panel.
  const inTerminal = (command: string) => () => runInTerminal(command);
  add({ id: "ai-commit", group: "Git", label: "Write a commit message with AI", icon: "diff", keywords: "commit message generate git dry run /commit", run: () => { ui.settingsOpen = false; ui.inspectorOpen = true; showPanel("changes"); showToast("Stage files, then choose Write with AI above the message box"); } }, live);
  add({ id: "ai-commit-push", group: "Git", label: "Commit and push with AI", icon: "diff", keywords: "commit push automatic git /commit --push", run: inTerminal("/commit --push") }, live);
  add({ id: "ai-review-base", group: "Git", label: "Review changes since a branch…", icon: "diff", keywords: "review base branch pull request diff /review", run: () => { ui.settingsOpen = false; addToPrompt("/review base "); } }, live);
  add({ id: "ai-review", group: "Git", label: "Review uncommitted changes with AI", icon: "diff", keywords: "review code diff git /review", run: () => void aiReviewView() }, live);
  add({ id: "worktrees", group: "Git", label: "Worktrees", icon: "folder", keywords: "worktree branch git create switch remove /wt", run: () => { ui.settingsOpen = false; void worktreesView(); } }, live);
  add({ id: "git-log", group: "Git", label: "Git log", icon: "list", keywords: "history commits log git /git", run: () => void gitLogView() }, live);
  add({ id: "import-claude", group: "Session", label: "Import a Claude Code session…", icon: "download", keywords: "import claude code transcript history migrate --from-claude", run: () => { ui.settingsOpen = false; void importSessions("claude"); } }, live);
  add({ id: "import-codex", group: "Session", label: "Import a Codex session…", icon: "download", keywords: "import codex openai transcript history migrate --from-codex", run: () => { ui.settingsOpen = false; void importSessions("codex"); } }, live);
  add({ id: "session-tree", group: "Session", label: "Browse session branches", icon: "list", keywords: "tree forks branches labels /tree", run: () => { ui.settingsOpen = false; void sessionTreeView(); } }, live);
  add({ id: "share-link", group: "Session", label: "Share this session…", icon: "external", keywords: "share upload link gist export /share", run: () => void shareView() }, live);
  add({ id: "anonymize", group: "Session", label: "Write an anonymized copy", icon: "shield", keywords: "anonymize redact private copy /anonymize", run: inTerminal("/anonymize") }, live);
  add({ id: "shake", group: "Session", label: "Drop heavy content from context", icon: "refresh", keywords: "shake context tool results images thinking trim /shake", run: inTerminal("/shake") }, live);
  add({ id: "fresh", group: "Session", label: "Reset the provider stream", icon: "refresh", keywords: "fresh stream state cache reset /fresh", run: inTerminal("/fresh") }, live);
  add({ id: "ssh-hosts", group: "Integrations", label: "SSH hosts", icon: "globe", keywords: "ssh remote host add remove list /ssh", run: page("integrations") }, live);
  add({ id: "hooks", group: "Integrations", label: "Lifecycle hooks", icon: "list", keywords: "hooks lifecycle scripts events /hooks", run: page("integrations") }, live);
  add({ id: "relay-status", group: "Integrations", label: "Collaboration relay status", icon: "chat", keywords: "relay collab local server status /relay", run: page("integrations") }, live);
  add({ id: "relay-start", group: "Integrations", label: "Start the collaboration relay", icon: "chat", keywords: "relay collab local server start /relay", run: page("integrations") }, live);
  add({ id: "relay-stop", group: "Integrations", label: "Stop the collaboration relay", icon: "stop", keywords: "relay collab local server stop /relay", run: page("integrations") }, live);
  add({ id: "profile", group: "Integrations", label: "Choose a capability profile", icon: "shield", keywords: "profile capability tools restrict /profile", run: page("integrations") }, live);
  add({ id: "security", group: "Integrations", label: "Security review", icon: "shield", keywords: "security scan plan review audit /security", run: inTerminal("/security") }, live);
  // Commands that take a task: start them in the message box for the user to finish.
  const startTyping = (text: string) => () => { ui.settingsOpen = false; addToPrompt(text); };
  add({ id: "contract", group: "Autonomy", label: "Start a task as a contract…", icon: "check", keywords: "contract goal constraints done when deliverables /contract", run: startTyping("/contract ") }, live);
  add({ id: "loop", group: "Autonomy", label: "Repeat a prompt in a loop…", icon: "refresh", keywords: "loop repeat count duration until while /loop", run: startTyping("/loop ") }, live);
  add({ id: "plan-review", group: "Autonomy", label: "Re-open the submitted plan", icon: "list", keywords: "plan review approve /plan-review", run: inTerminal("/plan-review") }, live);
  add({ id: "computer-status", group: "Autonomy", label: "Computer use: check status and permissions", icon: "monitor", keywords: "computer use desktop control screenshots input permission /computer", run: page("integrations") }, live);
  add({ id: "computer-on", group: "Autonomy", label: "Computer use: turn on", icon: "monitor", keywords: "computer use desktop control enable /computer", run: page("integrations") }, live);
  add({ id: "computer-off", group: "Autonomy", label: "Computer use: turn off", icon: "monitor", keywords: "computer use desktop control disable /computer", run: page("integrations") }, live);
  add({ id: "learn", group: "Learning", label: "Learning panel", icon: "activity", keywords: "learn train self improvement versions rollback jobs /learn", run: inTerminal("/learn panel") }, live);
  add({ id: "learn-status", group: "Learning", label: "Learning status", icon: "activity", keywords: "learn status jobs cycle /learn", run: () => { ui.settingsOpen = false; void learningStatus(); } }, live);
  add({ id: "understanding", group: "Learning", label: "How this project fits together", icon: "brain", keywords: "understanding architecture components map /understanding", run: () => void understandingView() }, live);
  add({ id: "understanding-build", group: "Learning", label: "Build the project understanding", icon: "brain", keywords: "understanding build index notes /understanding", run: inTerminal("/understanding build") }, live);
  add({ id: "experience", group: "Learning", label: "Learned recipes and failures", icon: "list", keywords: "experience ledger recipes failures /experience", run: () => void experienceView() }, live);
  add({ id: "autoresearch", group: "Learning", label: "Benchmark experiments (autoresearch)", icon: "activity", keywords: "autoresearch experiments branch benchmark /autoresearch", run: inTerminal("/autoresearch status") }, live);
  add({ id: "learn-candidates", group: "Learning", label: "Learned versions and candidates to evaluate", icon: "activity", keywords: "learn research candidate evaluate promote version held-out", run: () => { ui.settingsOpen = false; void learningStatus(); } }, live);
  add({ id: "workers-list", group: "Autonomy", label: "Workers: status and cancel", icon: "chip", keywords: "workers agents subagents tasks cancel stop /workers /cancel", run: () => { ui.settingsOpen = false; runTyped(snap, "workers", "", []); } }, live);
  add({ id: "worker-start", group: "Autonomy", label: "Start a worker…", icon: "chip", keywords: "worker spawn delegate parallel task /worker", run: startTyping("/worker ") }, live);
  add({ id: "benchmark-open", group: "Learning", label: "Open Benchmarks", icon: "activity", keywords: "benchmark suite agents compare results /benchmark", run: () => runTyped(snap, "benchmark", "open", []) }, live);
  add({ id: "benchmark-status", group: "Learning", label: "Benchmark status", icon: "activity", keywords: "benchmark status running /benchmark", run: () => runTyped(snap, "benchmark", "status", []) }, live);
  add({ id: "benchmark-stop", group: "Learning", label: "Stop the benchmark", icon: "stop", keywords: "benchmark stop cancel /benchmark", run: () => runTyped(snap, "benchmark", "stop", []) }, live);
  add({ id: "health", group: "App", label: "Health & setup", icon: "shield", keywords: "doctor setup install capability language server voice path terminal /doctor /setup", run: page("health") });
  add({ id: "maintenance", group: "App", label: "Storage & running processes", icon: "chip", keywords: "storage disk clean gc cache images purge processes ps stop compute rules /gc /images /compute", run: page("maintenance") });
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

/** The shortcuts sheet: keys, a short label, and the fine print shown on hover. */
export const SHORTCUTS: { group: string; keys: [keys: string, what: string, detail?: string][] }[] = [
  {
    group: "General",
    keys: [
      ["⌘K", "Command palette"],
      ["⌘P", "Open a project file"],
      ["⌘N", "New session"],
      ["⌘,", "Settings"],
      ["⌘/", "Keyboard shortcuts"],
      ["⌘B", "Sidebar"],
      ["⌥⌘I", "Plan and changes"],
      ["⇧⌘B", "Browser"],
      ["⇧⌘E", "Files"],
      ["⌃`", "Terminal"],
      ["⌘+  ⌘−  ⌘0", "Zoom", "Zoom in, zoom out, actual size"],
      ["⌘L", "Focus the message box"],
    ],
  },
  {
    group: "Conversation",
    keys: [
      ["Enter", "Send", "While Dial works, Enter adds a note to the running turn"],
      ["⇧Enter", "New line"],
      ["⌘Enter", "Queue a follow-up", "During a turn: sent once the turn ends"],
      ["⌥⌘↑ / ⌥⌘↓", "Previous / next prompt"],
      ["↑  ↓", "Earlier prompts", "In an empty message box"],
      ["@", "Mention a file", "Find and insert a project file path"],
      ["/", "Slash commands"],
      ["⌘F", "Find in conversation"],
      ["Esc", "Stop the turn", "An open menu closes first. Not from the terminal or other text fields"],
      ["⇧⌘M", "Change model"],
      ["⇧⌘V", "Voice", "Start talking, stop recording, or interrupt a spoken reply"],
      ["Enter  ⌘⌫", "Allow / deny", "On an approval card: Enter allows once while Allow has focus; ⌘⌫ denies"],
    ],
  },
  {
    group: "Sessions",
    keys: [
      ["⌘1 … ⌘9", "Open by position", "Hold ⌘ to see the numbers"],
      ["⇧⌘[  ⇧⌘]", "Previous / next session"],
      ["⌃⇧Tab  ⌃Tab", "Cycle sessions"],
      ["⇧⌘F", "Search sessions", "Esc clears the search"],
      ["F2  double-click", "Rename"],
      ["⌘-click  ⇧-click", "Select several"],
      ["⌘A", "Select all", "In the session list, once something is selected"],
      ["⌫", "Delete selected", "Moves them to Recently Deleted"],
      ["Right-click", "Session actions"],
    ],
  },
];
