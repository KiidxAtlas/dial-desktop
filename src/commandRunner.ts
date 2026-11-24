// Typed slash commands with their arguments (`/goal set …`, `/effort high`, `/skills install …`),
// run the way the terminal runs them: each one calls the engine method `get_available_commands`
// names for it, and the answer shows in the command panel. Commands only the terminal serves run
// for real in the Terminal panel (`dial '/cmd args'` opens the terminal UI and runs it there).

import { ref } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import { engine, showToast, snapshot, type TerminalTask } from "./bridge";
import { applySaved, engineCall, loadModes, readable, setMode, setPlanMode, type EngineCommand } from "./engineTools";
import { openSession, startRename } from "./sessionActions";
import { editQueued, queueFor, removeQueued } from "./queue";
import { showPanel, showSidebar } from "./workspace";
import { ui, type SettingsTab } from "./ui";
import { saveSchedule, typedSchedule } from "./schedules";
import type { OpenSnapshot } from "./types";

// ── The command panel ─────────────────────────────────────────────────────────

export interface ResultAction { label: string; run: () => void; danger?: boolean }
export interface ResultRow { title: string; detail?: string; actions?: ResultAction[] }
export interface CommandResult {
  title: string;
  /** Running; the panel shows a spinner. */
  busy?: boolean;
  error?: string;
  text?: string;
  rows?: ResultRow[];
  /** The raw reply, for "Show raw". */
  data?: unknown;
}

export const commandResult = ref<CommandResult | null>(null);

function showResult(result: CommandResult) {
  commandResult.value = result;
}

/** Call `method`, showing progress, then the reply (`render` picks what to show). */
async function call(
  title: string,
  method: string,
  params: Record<string, unknown> = {},
  render?: (reply: any) => Partial<CommandResult> | null,
  root?: string,
) {
  showResult({ title, busy: true });
  try {
    const reply = await engineCall<any>(method, params, root);
    if (commandResult.value?.title !== title) return reply;
    const shown = render ? render(reply) : { text: describe(reply) };
    if (shown === null) commandResult.value = null;
    else showResult({ title, data: reply, ...shown });
    return reply;
  } catch (error) {
    if (commandResult.value?.title === title || !commandResult.value) showResult({ title, error: readable(error) });
    return null;
  }
}

/** A reply as readable text: `key: value` lines, nested by indentation, long lists cut. */
export function describe(value: unknown, depth = 0): string {
  const pad = "  ".repeat(depth);
  if (value == null) return `${pad}—`;
  if (typeof value === "string") return value.length > 4000 ? `${value.slice(0, 4000)}…` : value;
  if (typeof value !== "object") return String(value);
  if (Array.isArray(value)) {
    if (!value.length) return `${pad}(none)`;
    const shown = value.slice(0, 60).map((item) =>
      item && typeof item === "object" ? `${pad}-\n${describe(item, depth + 1)}` : `${pad}- ${describe(item)}`,
    );
    if (value.length > 60) shown.push(`${pad}… ${value.length - 60} more`);
    return shown.join("\n");
  }
  const entries = Object.entries(value as Record<string, unknown>);
  if (!entries.length) return `${pad}(empty)`;
  return entries
    .map(([key, v]) => {
      const label = key.replace(/_/g, " ");
      if (v && typeof v === "object" && (Array.isArray(v) ? v.length : Object.keys(v).length))
        return `${pad}${label}:\n${describe(v, depth + 1)}`;
      const text = describe(v);
      return text.includes("\n") ? `${pad}${label}:\n${text.replace(/^/gm, pad + "  ")}` : `${pad}${label}: ${text}`;
    })
    .join("\n");
}

/** The engine's human text when a reply carries one, else the whole reply described. */
function textOf(reply: any): string {
  for (const key of ["text", "message", "output", "report", "summary", "answer"]) {
    if (typeof reply?.[key] === "string" && reply[key].trim()) return reply[key];
  }
  return describe(reply);
}

// ── The Terminal panel ────────────────────────────────────────────────────────

/** A slash command for a dedicated engine PTY, never input to the user's shell. */
export const terminalRequest = ref<{ task: TerminalTask; root: string } | null>(null);

export function openTaskTerminal(task: TerminalTask, root: string) {
  if (terminalRequest.value) return showToast("A command is already opening");
  terminalRequest.value = { task, root };
  ui.settingsOpen = false;
  ui.inspectorOpen = true;
  showPanel("terminal");
}

export function runInTerminal(command: string) {
  const root = snapshot.value?.workspace?.root;
  if (!root) return showToast("Open a project first");
  openTaskTerminal({ kind: "slash", command: command.trim() }, root);
}

// ── Arguments ─────────────────────────────────────────────────────────────────

/** Split arguments on spaces, keeping "quoted text" and 'quoted text' together. */
export function words(text: string): string[] {
  const out: string[] = [];
  const re = /"((?:[^"\\]|\\.)*)"|'([^']*)'|(\S+)/g;
  for (let m: RegExpExecArray | null; (m = re.exec(text)); ) out.push(m[1] !== undefined ? m[1].replace(/\\(.)/g, "$1") : m[2] ?? m[3]);
  return out;
}

/** Text after the first `n` words, as typed. */
function after(text: string, n: number) {
  let rest = text.trim();
  for (let i = 0; i < n; i++) rest = rest.replace(/^("(?:[^"\\]|\\.)*"|'[^']*'|\S+)\s*/, "");
  return rest;
}

const ON = ["on", "enable", "enabled", "true", "yes"];
const OFF = ["off", "disable", "disabled", "false", "no"];

function onOff(word: string | undefined): boolean | null {
  const w = (word ?? "").toLowerCase();
  return ON.includes(w) ? true : OFF.includes(w) ? false : null;
}

async function confirm(message: string, title: string, okLabel = "Continue") {
  return ask(message, { title, kind: "warning", okLabel, cancelLabel: "Cancel" }).catch(() => false);
}

function settingsAt(tab: SettingsTab) {
  ui.settingsTab = tab;
  ui.settingsOpen = true;
}

function usage(name: string, text: string) {
  showToast(`Usage: /${name} ${text}`);
}

// ── Commands ──────────────────────────────────────────────────────────────────

export interface RunContext {
  snap: OpenSnapshot;
  name: string;
  /** Everything typed after the command name. */
  rest: string;
  args: string[];
}

type Handler = (c: RunContext) => unknown;

const TOGGLES: Record<string, { mode: string; label: string }> = {
  slow: { mode: "slow", label: "Slow mode" },
  "extended-context": { mode: "extended_context", label: "Extended context" },
  drafter: { mode: "drafter", label: "Prompt drafting" },
  vibe: { mode: "vibe", label: "Vibe mode" },
  skillful: { mode: "skillful", label: "Skill catalog" },
  "external-thinking": { mode: "external_thinking", label: "Private thinking" },
};

async function modeStatus(title: string, pick: (m: any) => unknown) {
  const m = await loadModes();
  if (m) showResult({ title, text: describe(pick(m)), data: m });
}

function toggle(name: string): Handler {
  const { mode, label } = TOGGLES[name];
  return async ({ args }) => {
    if (!args.length || args[0] === "status")
      return modeStatus(label, (m) => ({ [label]: m.modes?.[mode] ?? m.settings?.[mode] ?? m[mode] }));
    const enabled = onOff(args[0]);
    if (enabled === null) return usage(name, "[on|off|status]");
    if (await setMode({ mode, enabled })) showToast(`${label} ${enabled ? "on" : "off"}`);
  };
}

async function sessionByPrefix(prefix: string) {
  const page = await engineCall<{ sessions: { session_id: string; title: string }[] }>("list_sessions", { limit: 500, include_empty: true });
  const found = page.sessions.filter((s) => s.session_id.startsWith(prefix));
  if (found.length !== 1) throw new Error(found.length ? `${found.length} sessions start with ${prefix}; type more of the id` : `No session starts with ${prefix}`);
  return found[0];
}

async function sideAnswer(title: string) {
  // The answer streams in the engine; poll its latest answer until it settles.
  let last = "";
  for (let i = 0; i < 240 && commandResult.value?.title === title; i++) {
    await new Promise((r) => setTimeout(r, 1000));
    try {
      const reply = await engineCall<any>("side_thread", { action: "answer" });
      const answer = typeof reply.answer === "string" ? reply.answer : textOf(reply.answer);
      if (commandResult.value?.title !== title) return;
      const done = !!answer && answer === last;
      showResult({ title, busy: !done, text: answer || "Thinking…", data: reply });
      if (done) return;
      last = answer;
    } catch (error) {
      const text = readable(error);
      if (!/not.?found|no answer|none/i.test(text)) return showResult({ title, error: text });
    }
  }
}

function skills(): Handler {
  return async ({ args, rest }) => {
    const [action = "list", ...more] = args;
    const target = after(rest, 1);
    if (["list", "sources", "installed"].includes(action)) return call(`Skills · ${action}`, "list_skills", { action }, (r) => ({ text: textOf(r) }));
    if (action === "search") return target ? call(`Skills matching “${target}”`, "list_skills", { action, query: target }, (r) => ({ text: textOf(r) })) : usage("skills", "search <words>");
    if (!["install", "update", "remove", "enable", "disable", "import"].includes(action)) return settingsAt("skills");
    if (!target && action !== "update") return usage("skills", `${action} <${action === "install" ? "source" : action === "import" ? "folder" : "name"}>`);
    const params: Record<string, unknown> = { action, confirm: true };
    if (action === "install") params.spec = target;
    else if (action === "import") params.path = target;
    else if (target) params.name = more[0];
    if (action !== "enable" && !(await confirm(`${action[0].toUpperCase()}${action.slice(1)} ${target || "every installed skill"}?`, `/skills ${action}`))) return;
    if (await call(`Skills · ${action}`, "skill_action", params, (r) => ({ text: textOf(r) }))) await applySaved();
  };
}

function checks(): Handler {
  return async ({ args, rest }) => {
    const [action = "list"] = args;
    if (action === "list") return call("Checks", "list_checks");
    if (action === "remove") {
      if (!args[1]) return usage("checks", "remove <name>");
      if (!(await confirm(`Remove the check ${args[1]} from this project?`, "Remove check", "Remove"))) return;
      if (await call("Checks · remove", "remove_check", { name: args[1], confirm: true })) await applySaved();
      return;
    }
    if (action === "add") {
      const [head, command] = rest.split(/\s--\s/, 2);
      const parts = words(head).slice(1);
      const name = parts[0];
      if (!name || !command?.trim()) return usage("checks", "add <name> [--ext rs,py] [--timeout secs] -- <command>");
      const params: Record<string, unknown> = { name, command: command.trim(), confirm: true };
      const ext = parts.indexOf("--ext");
      if (ext > 0 && parts[ext + 1]) params.file_extensions = parts[ext + 1].split(",").map((x) => x.trim()).filter(Boolean);
      const timeout = parts.indexOf("--timeout");
      if (timeout > 0 && Number(parts[timeout + 1]) > 0) params.timeout_secs = Number(parts[timeout + 1]);
      if (!(await confirm(`Run \`${params.command}\` as the check ${name} after edits in this project?`, "Add check", "Add"))) return;
      if (await call("Checks · add", "add_check", params)) await applySaved();
      return;
    }
    settingsAt("checks");
  };
}

function extension(op: string, id: string, extra: Record<string, unknown> = {}) {
  return call(`MCP · ${op.replace(/_mcp$/, "")} ${id}`, "extension_action", { intent: { op, id, ...extra }, confirm: true }, (r) => ({
    text: [r.message, r.apply_settings ? "Applied to this session." : ""].filter(Boolean).join("\n"),
  }));
}

function mcp(): Handler {
  return async ({ args, rest, name }) => {
    const [action = "list", id] = args;
    if (["list", "add", "help"].includes(action)) return settingsAt("extensions");
    if (action === "reload" || action === "reconnect") return call("MCP · reconnect", "apply_settings", { reconnect: true }, (r) => ({ text: r.reconnected ? "Every MCP server reconnected." : describe(r) }));
    if (action === "login") return runInTerminal(`/${name} ${rest}`);
    if (!id) return usage("mcp", `${action} <server>`);
    if (action === "logout") return call(`MCP · sign out of ${id}`, "mcp_logout", { server: id });
    if (action === "test") return extension("test_mcp", id);
    if (action === "enable" || action === "disable") {
      if (action === "disable" && !(await confirm(`Turn off the MCP server ${id}?`, "Disable MCP server", "Disable"))) return;
      if (await extension("toggle_mcp", id, { enable: action === "enable" })) await applySaved();
      return;
    }
    if (action === "remove") {
      if (!(await confirm(`Remove the MCP server ${id} from your settings?`, "Remove MCP server", "Remove"))) return;
      if (await extension("remove_mcp", id)) await applySaved();
      return;
    }
    usage("mcp", "[list|enable|disable|remove|test|login|logout|reload] <server>");
  };
}

/** Parse a typed setting value: JSON when it reads as JSON (numbers, true, lists), else text. */
function settingValue(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

function codeHistory(): Handler {
  return async ({ args, rest }) => {
    if (args[0] === "clear") return runInTerminal("/code-history clear");
    if (args[0] === "show") {
      if (!args[1]) return usage("code-history", "show <id>");
      const id = /^\d+$/.test(args[1]) ? Number(args[1]) : args[1];
      return call(`Code history · ${args[1]}`, "code_history", { action: "show", id }, (r) => ({ text: [describe(r.version), r.diff].filter(Boolean).join("\n\n") }));
    }
    const params: Record<string, unknown> = { action: "search" };
    const query: string[] = [];
    for (const w of words(rest)) {
      if (w.startsWith("path:")) params.path = w.slice(5);
      else if (w.startsWith("branch:")) params.branch = w.slice(7);
      else query.push(w);
    }
    if (query.length) params.query = query.join(" ");
    return call("Code history", "code_history", params, (r) => ({
      rows: (r.versions ?? []).map((v: any) => ({
        title: `${v.id ?? ""} ${v.path ?? v.summary ?? ""}`.trim(),
        detail: [v.branch, v.when ?? v.created ?? v.time].filter(Boolean).join(" · "),
        actions: v.id != null ? [{ label: "Show", run: () => void codeHistory()({ snap: null as never, name: "code-history", rest: `show ${v.id}`, args: ["show", String(v.id)] }) }] : [],
      })),
      text: (r.versions ?? []).length ? undefined : "No recorded versions match.",
    }));
  };
}

function goalCommand(): Handler {
  return async ({ args, rest }) => {
    const [action = "show"] = args;
    const text = after(rest, 1);
    const show = (r: any) => ({ text: textOf(r.goal ?? r) });
    switch (action) {
      case "show":
      case "get":
        return call("Goal", "goal", { action: "get" }, show);
      case "set":
        if (!text) return usage("goal", "set <objective>");
        return call("Goal", "goal", { action: "set", objective: text }, show);
      case "pause":
      case "resume":
      case "drop":
        return call("Goal", "goal", { action }, show);
      case "budget": {
        const n = args[1] === "off" ? null : Number(args[1]);
        if (n !== null && !(n > 0)) return usage("goal", "budget <tokens|off>");
        return call("Goal", "goal", { action: "budget", token_budget: n }, show);
      }
      default:
        // `/goal <objective>` sets it, as typing an objective does in the terminal.
        return call("Goal", "goal", { action: "set", objective: rest.trim() }, show);
    }
  };
}

function collab(): Handler {
  return async ({ args }) => {
    const [action = "status", link] = args;
    if (["status", "view", "list"].includes(action)) return call("Collaboration", "collaboration_status");
    if (action === "start") {
      if (!(await confirm("Host this conversation in an encrypted room? Guests can read it (read-only).", "Share this session", "Start"))) return;
      return call("Collaboration", "collaboration_start", { confirm: true, read_only: true });
    }
    if (action === "stop" || action === "abort") return call("Collaboration", "collaboration_stop");
    if (action === "leave") return call("Collaboration", "collaboration_leave");
    if (action === "join") return link ? joinRoom(link) : usage("collab", "join <link>");
    usage("collab", "[start|status|stop|join <link>|leave]");
  };
}

async function joinRoom(link: string) {
  if (!(await confirm("Join the shared session behind this link?", "Join session", "Join"))) return;
  return call("Collaboration", "collaboration_join", { link, confirm: true });
}

const HANDLERS: Record<string, Handler> = {
  // Models, modes and approval.
  model: ({ args, rest, snap }) => {
    if (!args.length) return (ui.modelPickerOpen = true);
    if (["local", "backups"].includes(args[0])) return settingsAt("library");
    if (["settings", "defaults"].includes(args[0])) return settingsAt("roles");
    const [provider, ...id] = rest.trim().split("/");
    if (!id.length) return usage("model", "<provider>/<model>");
    void engine.setModel(snap, provider, id.join("/"));
  },
  effort: ({ args, snap }) => {
    if (!args.length) return (ui.modelPickerOpen = true);
    if (args.length > 1) return settingsAt("roles");
    void engine.setThinking(snap, args[0] === "auto" ? "default" : args[0]);
  },
  approval: ({ args }) => {
    const mode = args[0] as "ask" | "write" | "auto" | "readonly";
    if (!["ask", "write", "auto", "readonly"].includes(mode)) return usage("approval", "ask|write|auto|readonly");
    void engine.setApproval(mode);
  },
  plan: async ({ args }) => {
    const on = args.length ? onOff(args[0]) : !(await loadModes())?.plan;
    if (on === null) return usage("plan", "[on|off]");
    if (await setPlanMode(on)) showToast(`Plan mode ${on ? "on" : "off"}`);
  },
  fast: async ({ args }) => {
    const state = (args[0] ?? "on").toLowerCase();
    if (state === "status") return modeStatus("Fast mode", (m) => ({ fast: m.settings.fast_mode, tier: m.settings.service_tier }));
    if (!["on", "ultra", "off"].includes(state)) return usage("fast", "[on|ultra|off|status]");
    if (await setMode({ mode: "fast", state })) showToast(`Fast mode ${state}`);
  },
  advisor: async ({ args }) => {
    const state = (args[0] ?? "status").toLowerCase();
    if (state === "status") return modeStatus("Advisor", (m) => m.advisor);
    if (!["on", "completion", "off"].includes(state)) return usage("advisor", "on|completion|off|status");
    if (await setMode({ mode: "advisor", state })) showToast(`Advisor ${state}`);
  },
  prewalk: async ({ args }) => {
    const action = args[0] === "restart" ? "restart" : args[0] === "off" ? "off" : "arm";
    if (await setMode({ mode: "prewalk", action })) showToast(action === "off" ? "Prewalk off" : "Prewalk armed for the next prompt");
  },
  "plan-yolo": async ({ args }) => {
    if (args[0] === "status") return modeStatus("Plan-yolo", (m) => m.modes?.plan_yolo ?? m);
    const off = args[0] === "off";
    const params: Record<string, unknown> = { mode: "plan_yolo", enabled: !off };
    if (!off && args[0]) params.target = args[0];
    if (await setMode(params)) showToast(off ? "Plan-yolo off" : "Plan-yolo on");
  },
  ...Object.fromEntries(Object.keys(TOGGLES).map((name) => [name, toggle(name)])),

  // This session.
  rename: ({ snap, rest }) => {
    const session = openSession(snap);
    if (!session) return;
    if (!rest.trim()) return startRename(session.id, "title");
    void engine.renameSession(session.id, rest.trim());
  },
  label: ({ rest }) => call("Label", "label_session", { label: rest.trim() }, (r) => (showToast(rest.trim() ? `Labeled “${rest.trim()}”` : "Label cleared"), r && null)),
  export: ({ args }) => {
    const html = args.includes("--html");
    const path = args.find((a) => a !== "--html");
    return call("Export", "export", { format: html ? "html" : "markdown", ...(path ? { path } : {}) }, (r) => ({ text: `Saved to ${r.path}` }));
  },
  compact: ({ args, rest }) => {
    const methods = ["soft", "remote", "snapcompact", "handoff", "shake"];
    const method = methods.includes(args[0]) ? args[0] : undefined;
    const focus = method ? after(rest, 1) : rest.trim();
    return call("Compact", "compact", { ...(method ? { method, require_method: true } : {}), ...(focus ? { instructions: focus } : {}) }, (r) => ({
      text: r.context_reset ? "Started a fresh context with a handoff summary." : r.compacted ? "The conversation was compacted." : textOf(r.outcome ?? r),
    }));
  },
  handoff: ({ rest }) => call("Handoff", "compact", { method: "handoff", require_method: true, ...(rest.trim() ? { instructions: rest.trim() } : {}) }, () => ({ text: "Started a fresh context with a handoff summary." })),
  undo: async ({ args }) => {
    if (args[0] === "files") { ui.diffPath = null; return showPanel("changes"); }
    const turns = Number(args[0] ?? 1);
    if (!(turns >= 1)) return usage("undo", "[n|files]");
    if (!(await confirm(`Restore the files the last ${turns === 1 ? "turn" : `${turns} turns`} changed and drop ${turns === 1 ? "it" : "them"} from the conversation?`, "Undo", "Undo"))) return;
    return call("Undo", "undo", { turns }, (r) => ({ text: r.dropped ? `Undid ${r.dropped} turn${r.dropped === 1 ? "" : "s"}.${r.note ? `\n${r.note}` : ""}` : "Nothing to undo." }));
  },
  rollback: () => { ui.diffPath = null; showPanel("changes"); },
  "review-file": ({ args }) => {
    const turn = Number(args[0]);
    if (!(turn >= 0) || !args[1]) { ui.diffPath = null; return showPanel("changes"); }
    return call(`Turn ${turn} · ${args[1]}`, "checkpoint_diff", { turn, path: args[1] }, (r) => ({ text: r.diff || "No difference." }));
  },
  branch: async ({ args }) => {
    const turns = Number(args[0] ?? 1);
    if (!(turns >= 1)) return usage("branch", "[n]");
    if (!(await confirm(`Rewind the conversation by ${turns} turn${turns === 1 ? "" : "s"}? Files stay as they are.`, "Branch", "Rewind"))) return;
    return call("Branch", "branch_session", { turns, confirm: true }, () => ({ text: "Conversation rewound; files unchanged." }));
  },
  move: async ({ rest }) => {
    if (!rest.trim()) return usage("move", "<dir>");
    if (!(await confirm(`Move this session to ${rest.trim()}?`, "Move session", "Move"))) return;
    return call("Move session", "move_session", { path: rest.trim(), confirm: true });
  },
  pack: ({ rest }) => call("Pack session", "pack_session", rest.trim() ? { path: rest.trim() } : {}, (r) => ({ text: r.path ? `Packed to ${r.path}` : describe(r) })),
  archive: ({ args, rest }) => {
    const view = ["projects", "sessions", "prompts"].includes(args[0]) ? args[0] : "projects";
    const query = view === args[0] ? after(rest, 1) : rest.trim();
    return call(`Archive · ${view}`, "archive", { view, ...(query ? { query } : {}) });
  },
  sessions: async ({ snap, args, rest }) => {
    if (!args.length) { showSidebar(); ui.focusSearch++; return; }
    const [what] = args;
    if (what === "this") return HANDLERS.session({ snap, name: "session", rest: "info", args: ["info"] });
    if (what === "fork") { const s = openSession(snap); if (s) void engine.forkSession(s.id); return; }
    if (what === "branch") return HANDLERS.branch({ snap, name: "branch", rest: after(rest, 1), args: args.slice(1) });
    if (what === "label") return HANDLERS.label({ snap, name: "label", rest: after(rest, 1), args: args.slice(1) });
    try {
      const s = await sessionByPrefix(what);
      void engine.resume(s.session_id, snap.workspace.root);
    } catch (error) {
      showToast(readable(error));
    }
  },
  session: ({ snap, args }) => {
    const s = openSession(snap);
    if (args[0] === "delete" && s) return void engine.deleteSessions([s]);
    if (args[0] === "pin" && s) return void engine.pinSession(s.id, !s.pinned);
    return call("Session", "get_session_stats");
  },
  search: ({ snap, rest }) => {
    const query = rest.trim();
    if (!query || ["results", "back"].includes(query)) { showSidebar(); ui.focusSearch++; return; }
    return call(`Sessions mentioning “${query}”`, "search_sessions", { query, limit: 50 }, (r) => ({
      text: r.hits?.length ? undefined : "No matches.",
      rows: (r.hits ?? []).map((h: any) => ({
        title: h.title || h.session_id.slice(0, 8),
        detail: [h.when, h.role, h.snippet ?? h.preview].filter(Boolean).join(" · "),
        actions: [{ label: "Open", run: () => { commandResult.value = null; void engine.resume(h.session_id, snap.workspace.root); } }],
      })),
    }));
  },
  cost: () => call("Cost", "get_session_stats"),
  context: () => call("Context", "get_session_stats"),
  tools: () => call("Tools", "state", {}, (r) => ({ text: describe(r.tools ?? r) })),
  status: () => call("Status", "state"),

  // Workers and side questions.
  worker: ({ snap, rest }) => {
    const task = rest.trim();
    if (!task) return usage("worker", "<message>");
    if (!snap.turnRunning) return void engine.send(task, [], { workspaceRoot: snap.workspace.root, sessionId: snap.sessionId });
    return call("Worker", "task_start", { task, agent: "worker", name: task.slice(0, 48), solution_space: "Free: any approach that completes the assignment." }, (r) => ({ text: textOf(r) }));
  },
  workers: ({ args }) => call("Workers", "task_status", args[0] ? { id: args[0] } : {}, (r) => ({ text: textOf(r) })),
  agents: ({ args }) => call("Agents", "task_status", args[0] ? { id: args[0] } : {}, (r) => ({ text: textOf(r) })),
  message: ({ args, rest }) => {
    const text = after(rest, 1);
    if (!args[0] || !text) return usage("message", "<id> <text>");
    return call("Message worker", "task_message", { id: args[0], text }, () => ({ text: `Sent to ${args[0]}.` }));
  },
  cancel: ({ args }) => (args[0] ? call("Cancel worker", "task_cancel", { id: args[0] }, () => ({ text: `Cancelled ${args[0]}.` })) : usage("cancel", "<id>")),
  btw: async ({ rest }) => {
    const question = rest.trim();
    if (!question) return call("Side questions", "side_thread", { action: "list" });
    const title = `By the way: ${question.slice(0, 60)}`;
    if (await call(title, "side_thread", { action: "ask", question }, () => ({ busy: true, text: "Thinking…" }))) await sideAnswer(title);
  },
  goal: goalCommand(),
  "guided-goal": ({ rest }) => (rest.trim() ? call("Goal", "goal", { action: "guided", idea: rest.trim() }, (r) => ({ text: textOf(r.goal ?? r) })) : usage("guided-goal", "<idea>")),

  // Project tools.
  skills: skills(),
  checks: checks(),
  mcp: mcp(),
  memory: ({ rest }) => (rest.trim() ? call("Memory", "memory", { command: rest.trim() }, (r) => ({ text: textOf(r) })) : settingsAt("memory")),
  dirs: ({ args, rest }) => {
    if (args[0] === "add" || args[0] === "remove") return HANDLERS[`${args[0]}-dir`]({ snap: null as never, name: "", rest: after(rest, 1), args: args.slice(1) });
    if (args[0] === "list") return call("Folders", "list_dirs");
    settingsAt("folders");
  },
  "add-dir": ({ rest }) => (rest.trim() ? call("Add folder", "add_dir", { path: rest.trim() }, (r) => ({ text: r.changed ? `Added ${r.path}.` : `${r.path} was already in this session.` })) : settingsAt("folders")),
  "remove-dir": ({ rest }) => (rest.trim() ? call("Remove folder", "remove_dir", { path: rest.trim() }, (r) => ({ text: r.changed ? `Removed ${r.path}.` : `${r.path} was not added.` })) : settingsAt("folders")),
  schedule: async ({ snap, args, rest }) => {
    const [action = "list", id] = args;
    if (action === "list") return call("Schedules", "list_schedules");
    if (action === "status") return call("Scheduler", "schedule_status");
    if (action === "remove") {
      if (!id) return usage("schedule", "remove <id>");
      if (!(await confirm(`Remove the schedule ${id}?`, "Remove schedule", "Remove"))) return;
      return call("Schedules", "remove_schedule", { id, confirm: true }, () => ({ text: `Removed ${id}.` }), snap.workspace.root);
    }
    if (action === "add" && args.length === 1) return settingsAt("schedules");
    if (action === "add") {
      const saved = await saveSchedule(typedSchedule(args.slice(1)), snap.workspace.root,
        summary => confirm(summary, "Save schedule", "Save"),
        request => engineCall("add_schedule", request, snap.workspace.root));
      if (saved) { settingsAt("schedules"); showToast("Schedule saved"); }
      return;
    }
    if (action === "run" && !(await confirm(`Run due schedules in ${snap.workspace.root} with their saved tool approvals?`, "Run schedules", "Run"))) return;
    openTaskTerminal({ kind: "slash", command: `/schedule ${rest}` }, snap.workspace.root);
  },
  extensions: ({ args }) => (args[0] === "skills" ? settingsAt("skills") : settingsAt("extensions")),

  // Reports.
  usage: ({ args }) => (args[0] === "reset" ? runInTerminal(`/usage ${args.join(" ")}`) : settingsAt("usage")),
  stats: ({ args }) => (args[0] && !args[0].startsWith("--") ? call(`Stats · ${args[0]}`, "stats", { range: args[0] }, (r) => ({ text: describe({ overall: r.report?.overall, by_model: r.report?.by_model }) })) : settingsAt("usage")),
  policy: () => call("Request policy", "policy", {}, (r) => ({ text: textOf(r) })),
  reliability: () => call("Tool reliability", "reliability", {}, (r) => ({ text: textOf(r) })),
  "code-history": codeHistory(),
  evidence: ({ args, rest }) => {
    const [action = "checks"] = args;
    if (action === "checks") return call("Evidence · checks", "evidence", { action });
    if (action === "check" && args[1]) return call(`Evidence · ${args[1]}`, "evidence", { action, name: args[1] }, (r) => ({ text: textOf(r) }));
    if (action === "inspect" && args[1]) return call(`Evidence · ${args[1]}`, "evidence", { action, reference: args[1] }, (r) => ({ text: textOf(r) }));
    if (action === "lineage" && args[1]) return call("Evidence · lineage", "evidence", { action, references: args.slice(1) }, (r) => ({ text: textOf(r) }));
    if (action === "compare" && args[2]) return call("Evidence · compare", "evidence_compare", { reference: args[1], other: args[2] }, (r) => ({ text: textOf(r) }));
    if (action === "correct" && args[1] && after(rest, 2)) return call("Evidence · correction", "evidence_correct", { reference: args[1], correction: after(rest, 2) }, () => ({ text: "Correction recorded." }));
    usage("evidence", "checks | check <name> | inspect <citation> | lineage <citation>… | compare <a> <b> | correct <citation> <text>");
  },
  benchmark: ({ args }) => {
    const action = args[0] ?? "open";
    if (!["open", "status", "stop"].includes(action)) return usage("benchmark", "[open|status|stop]");
    return call("Benchmarks", "benchmark", { action }, (r) => ({ text: textOf(r) }));
  },
  commands: () => call("Commands", "get_available_commands", {}, (r) => ({
    rows: (r.commands as EngineCommand[]).map((c) => ({ title: `/${c.name} ${c.usage}`.trim(), detail: c.description })),
  })),

  // Settings and accounts.
  set: ({ args, rest }) => {
    const project = args.includes("--project");
    const parts = words(rest.replace(/(^|\s)--project(\s|$)/, " "));
    const key = parts[0];
    if (!key) return settingsAt("");
    const valueText = after(rest.replace(/(^|\s)--project(\s|$)/, " "), 1);
    if (!valueText) return call(key, "get_settings", { key }, (r) => ({ text: describe(r.value) }));
    return (async () => {
      if (await call(key, "set_setting", { key, value: settingValue(valueText), scope: project ? "project" : "global" }, () => ({ text: `Saved ${key}${project ? " for this project" : ""}.` }))) await applySaved();
    })();
  },
  reload: () => applySaved().then(() => showToast("Settings reloaded")).catch((e) => showToast(readable(e))),
  login: () => settingsAt("providers"),
  logout: async ({ args }) => {
    if (!args[0]) return usage("logout", "<provider>");
    if (!(await confirm(`Sign out of ${args[0]}?`, "Sign out", "Sign out"))) return;
    return call("Sign out", "logout", { provider: args[0] }, (r) => ({ text: textOf(r) }));
  },
  collab: collab(),
  join: ({ args }) => (args[0] ? joinRoom(args[0]) : usage("join", "<link>")),
  leave: () => call("Collaboration", "collaboration_leave"),
  modelpreset: async ({ snap, args }) => {
    if (args[0] !== "apply" || !args[1]) return settingsAt("roles");
    const { readSettings, presetsIn, applyPreset } = await import("./roles");
    const preset = presetsIn(await readSettings()).find((p) => p.name === args[1]);
    if (!preset) return showToast(`No preset named ${args[1]}`);
    if (snap.sessionId) await applyPreset(preset, snap.workspace.root, snap.sessionId);
  },

  // The desktop's own views.
  watch: () => showPanel("activity"),
  queue: ({ snap, args }) => {
    const item = queueFor(snap).items[Number(args[1]) - 1];
    if (!item) return showToast(args.length ? "No queued prompt with that number" : "Queued prompts show above the message box");
    if (args[0] === "remove") return void removeQueued(snap, item.id);
    if (args[0] === "edit") return void editQueued(snap, item.id);
  },
  theme: () => settingsAt("appearance"),
  changes: () => { ui.diffPath = null; showPanel("changes"); },
  tree: () => { showSidebar(); },
};

/** Desktop-only names the engine lists as terminal-only but the desktop shows its own way. */
const DESKTOP_TERMINAL = new Set(["changes", "tree", "watch", "modelpreset", "login", "schedule"]);

/** Whether `name` has an argument-aware desktop handler. */
export function hasHandler(name: string) {
  return name in HANDLERS;
}

/** The surface the engine reports for a builtin command, or `undefined` when unlisted. */
function surfaceOf(name: string, list: EngineCommand[]) {
  const cmd = list.find((c) => c.name === name);
  return cmd?.kind === "builtin" ? cmd.rpc.surface : cmd ? "prompt" : undefined;
}

/**
 * A typed `/name args` the desktop runs itself: builtin commands with a handler, and terminal-only
 * ones (run in the Terminal panel). Custom commands and skills return `null`: sending expands them.
 */
export function typedCommand(text: string, list: EngineCommand[]): { name: string; rest: string } | null {
  const m = /^\/([a-z][\w-]*)(?:\s+([\s\S]*))?$/i.exec(text.trim());
  if (!m) return null;
  const name = m[1].toLowerCase();
  const surface = surfaceOf(name, list);
  if (surface === "prompt") return null;
  if (hasHandler(name) || surface === "terminal" || (surface && list.length)) return { name, rest: m[2] ?? "" };
  return null;
}

/** Run a typed command; `fallback` runs the bare desktop action when there is no handler. */
export function runTyped(snap: OpenSnapshot, name: string, rest: string, list: EngineCommand[], fallback?: () => void) {
  const handler = HANDLERS[name];
  const surface = surfaceOf(name, list);
  try {
    if (handler && (surface !== "terminal" || DESKTOP_TERMINAL.has(name))) return void Promise.resolve(handler({ snap, name, rest, args: words(rest) })).catch((e) => showToast(readable(e)));
    if (surface === "terminal") return runInTerminal(`/${name} ${rest}`);
    if (fallback) return fallback();
    // `/exit`, `/draft`…: the desktop does these with its own controls.
    showToast(`/${name} works through the desktop's own controls here`);
  } catch (error) {
    showToast(readable(error));
  }
}
