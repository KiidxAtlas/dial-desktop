// The composer's slash menu: the terminal's commands, run the desktop way.
//
// The engine lists every slash command with where a client finds it (`get_available_commands`).
// Commands with a desktop control run it; custom commands and skills go into the box so the
// task can be typed after them (sending expands them as in the terminal); terminal-only commands
// stay discoverable and say why.

import { engine, folders } from "./bridge";
import { copyText, conversationMarkdown, canCompact, canRetry, canUndo, openSession, startRename } from "./sessionActions";
import { modes, loadModes, setMode, setPlanMode, type EngineCommand } from "./engineTools";
import { hasHandler, runInTerminal, runTyped } from "./commandRunner";
import { showPanel } from "./workspace";
import type { OpenSnapshot } from "./types";
import { ui, type SettingsTab } from "./ui";

export interface SlashCommand {
  name: string;
  detail: string;
  /** Arguments hint, like `<message>`. */
  usage?: string;
  kind: "desktop" | "custom" | "skill" | "terminal";
  /** Why it cannot run here (terminal-only), shown instead of running. */
  reason?: string;
  /** Put `name ` in the box for the user to finish, instead of running. */
  insert?: boolean;
  run?: () => void;
}

export interface SlashContext {
  snap: OpenSnapshot;
  showApprovals: () => void;
  showModes: () => void;
}

function settingsAt(tab: SettingsTab) {
  ui.settingsTab = tab;
  ui.settingsOpen = true;
}

async function toggleMode(id: string, label: string) {
  const m = modes.value ?? (await loadModes());
  if (!m) return;
  const on: Record<string, boolean> = {
    plan: m.plan,
    vibe: m.modes.vibe,
    fast: m.settings.fast_mode,
    slow: m.settings.slow_mode,
    "extended-context": m.settings.extended_context,
    drafter: m.settings.turn_optimizer,
    "external-thinking": m.external_thinking,
    advisor: m.advisor.enabled,
    skillful: m.modes.skillful === true,
  };
  const next = !on[id];
  const done =
    id === "plan" ? await setPlanMode(next)
    : id === "fast" ? await setMode({ mode: "fast", state: next ? "on" : "off" })
    : id === "advisor" ? await setMode({ mode: "advisor", state: next ? "on" : "off" })
    : await setMode({ mode: id.replace("-", "_"), enabled: next });
  if (done) (await import("./bridge")).showToast(`${label} ${next ? "on" : "off"}`);
}

/** Desktop actions for engine command names; `null` hides the command (nothing to show here). */
function desktopAction(name: string, c: SlashContext): (() => void) | null | undefined {
  const { snap } = c;
  const session = openSession(snap);
  const live = snap.connection.state === "live";
  const map: Record<string, (() => void) | null> = {
    model: () => (ui.modelPickerOpen = true),
    switch: () => (ui.modelPickerOpen = true),
    effort: () => (ui.modelPickerOpen = true),
    new: live ? () => void engine.newSession() : null,
    fork: session && live && !snap.turnRunning ? () => void engine.forkSession(session.id) : null,
    rename: session ? () => startRename(session.id, "title") : null,
    export: session && live && snap.entries.length ? () => void engine.exportSession(snap.workspace.root, session.title) : null,
    compact: canCompact(snap) ? () => void engine.compact() : null,
    handoff: canCompact(snap) ? () => void engine.compact() : null,
    undo: live && canUndo(snap) ? () => void engine.undoTurn() : null,
    retry: canRetry(snap) ? () => void engine.retryTurn() : null,
    approval: c.showApprovals,
    mode: c.showModes,
    settings: () => settingsAt(""),
    set: () => settingsAt(""),
    sessions: () => { ui.sidebarOpen = true; ui.focusSearch++; },
    search: () => { ui.sidebarOpen = true; ui.focusSearch++; },
    delete: session ? () => void engine.deleteSessions([session]) : null,
    pin: session ? () => void engine.pinSession(session.id, !session.pinned) : null,
    cost: () => settingsAt("usage"),
    modelpreset: () => settingsAt("roles"),
    context: () => settingsAt("usage"),
    usage: () => settingsAt("usage"),
    stats: () => settingsAt("usage"),
    processes: () => showPanel("activity"),
    activity: () => showPanel("activity"),
    rollback: () => { ui.diffPath = null; showPanel("changes"); },
    "review-file": () => { ui.diffPath = null; showPanel("changes"); },
    checks: () => settingsAt("checks"),
    skills: () => settingsAt("skills"),
    extensions: () => settingsAt("extensions"),
    mcp: () => settingsAt("extensions"),
    memory: () => settingsAt("memory"),
    dirs: () => settingsAt("folders"),
    "add-dir": () => settingsAt("folders"),
    "remove-dir": () => settingsAt("folders"),
    schedule: () => settingsAt("schedules"),
    help: () => (ui.shortcutsOpen = true),
    hotkeys: () => (ui.shortcutsOpen = true),
    controls: () => (ui.shortcutsOpen = true),
    theme: () => settingsAt("appearance"),
    copy: snap.entries.length ? () => void copyText(conversationMarkdown(snap), "Conversation copied as Markdown") : null,
    restart: () => void engine.reconnect(),
    // The desktop does these its own way already, without a command.
    queue: null, draft: null, attachments: null, watch: null, exit: null, commands: null, session: null, tools: null, reload: null,
  };
  for (const [id, label] of [
    ["plan", "Plan mode"], ["vibe", "Vibe mode"], ["fast", "Fast mode"], ["slow", "Slow mode"],
    ["extended-context", "Extended context"], ["drafter", "Prompt drafting"], ["external-thinking", "Private thinking"],
    ["advisor", "Advisor"], ["skillful", "Skill catalog"],
  ] as const)
    map[id] = live && !snap.turnRunning ? () => void toggleMode(id, label) : null;
  return name in map ? map[name] : undefined;
}

/** Commands the desktop always has, even before the engine lists its own. */
function localCommands(c: SlashContext): SlashCommand[] {
  const live = c.snap.connection.state === "live";
  const list: SlashCommand[] = [
    { name: "/clear", detail: "Start a fresh session", kind: "desktop", run: live ? () => void engine.newSession() : undefined },
    { name: "/open", detail: "Open another project folder", kind: "desktop", run: () => void folders.choose() },
    { name: "/think", detail: "Model and reasoning level", kind: "desktop", run: () => (ui.modelPickerOpen = true) },
    { name: "/voice", detail: "Talk to Dial and hear the answer (⇧⌘V)", kind: "desktop", run: live && !c.snap.turnRunning && c.snap.voice.phase === "off" ? () => void engine.voiceStart() : undefined },
  ];
  return list.filter((x) => x.run);
}

/** Fallback when the engine list is not loaded: the core terminal commands. */
const CORE = ["model", "new", "compact", "undo", "mode", "retry", "fork", "plan", "usage", "skills", "memory", "checks"];

/** Every slash command, desktop-runnable first, then custom and skills, then terminal-only. */
export function slashCommands(c: SlashContext, engineList: EngineCommand[]): SlashCommand[] {
  const out: SlashCommand[] = [...localCommands(c)];
  const seen = new Set(out.map((x) => x.name));
  const terminal: SlashCommand[] = [];
  const source: EngineCommand[] = engineList.length
    ? engineList
    : CORE.map((name) => ({ name, usage: "", description: "", kind: "builtin", rpc: { surface: "method", method: "" } }));
  for (const cmd of source) {
    const name = `/${cmd.name}`;
    if (seen.has(name)) continue;
    seen.add(name);
    if (cmd.kind === "custom" || cmd.kind === "skill") {
      out.push({ name, detail: cmd.description || (cmd.kind === "skill" ? "Use this skill" : "Custom command"), usage: cmd.usage, kind: cmd.kind, insert: true });
      continue;
    }
    const action = desktopAction(cmd.name, c);
    if (action === null) continue;
    if (action) out.push({ name, detail: capital(cmd.description) || name, usage: cmd.usage, kind: "desktop", run: action });
    else if (cmd.rpc.surface === "terminal" && !hasHandler(cmd.name))
      terminal.push({
        name, detail: capital(cmd.description), usage: cmd.usage, kind: "terminal",
        reason: "Runs in the terminal panel",
        run: cmd.usage ? undefined : () => runInTerminal(name),
        insert: !!cmd.usage,
      });
    else if (hasHandler(cmd.name) || cmd.rpc.surface === "method")
      out.push({
        name, detail: capital(cmd.description) || name, usage: cmd.usage, kind: "desktop",
        run: () => runTyped(c.snap, cmd.name, "", engineList),
        // Commands that need their arguments go into the box to finish.
        insert: /^</.test(cmd.usage.trim()),
      });
  }
  return [...out, ...terminal];
}

/** Commands matching what is typed after `/`: name starts first, then name contains. */
export function matchSlash(list: SlashCommand[], typed: string): SlashCommand[] {
  const q = typed.toLowerCase();
  const starts = list.filter((x) => x.name.toLowerCase().startsWith(q));
  if (q.length < 2) return starts.filter((x) => x.kind !== "terminal");
  const inside = list.filter((x) => !starts.includes(x) && x.name.toLowerCase().includes(q.slice(1)));
  return [...starts, ...inside];
}

function capital(text: string) {
  return text ? text[0].toUpperCase() + text.slice(1) : text;
}
