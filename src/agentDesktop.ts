// What the agent sees of and does in the app itself, through the `desktop_*` tools of the
// `dial_desktop` MCP server (`src-tauri/src/agent_browser/desktop.rs`). The backend runs a fixed
// script in this window that calls `window.__dialDesktop.call(tool, args)`; the agent supplies
// only JSON arguments, never code.
//
// Actions are the command palette's own list (`commands.ts`), so every feature added there is
// reachable by the agent too, with the same availability rules. Approvals, Agent access, VM grants
// and credentials are not palette commands, and stay the user's.

import { snapshot, showToast, toasts } from "./bridge";
import { commands, filterCommands } from "./commands";
import { appearance } from "./appearance";
import { sidebarShown, workspace } from "./workspace";
import { ui } from "./ui";
import type { OpenSnapshot } from "./types";

/** Sessions the view lists; the rest are counted. */
const SESSION_LIMIT = 60;

type Outcome = { ok: true; value: unknown } | { ok: false; error: string };

function opened(): OpenSnapshot {
  const snap = snapshot.value;
  if (!snap?.workspace) throw new Error("No project is open in Dial Desktop");
  return snap as OpenSnapshot;
}

/** The app as the user sees it: project, session, panels, and what is waiting on them. */
export function desktopView(snap: OpenSnapshot) {
  return {
    project: snap.workspace.root,
    connection: snap.connection.state,
    session: { id: snap.sessionId, title: snap.title, turnRunning: snap.turnRunning, entries: snap.entries.length },
    model: snap.model && { provider: snap.model.provider, id: snap.model.modelId, label: snap.model.label, thinking: snap.model.thinking },
    approvalMode: snap.approval,
    waitingOnUser: { approvals: snap.approvals.length, question: !!snap.question },
    context: snap.context,
    plan: snap.plan,
    changedFiles: snap.changes.map((c) => c.path),
    voice: snap.voice.phase,
    sessions: {
      total: snap.sessions.length,
      shown: snap.sessions.slice(0, SESSION_LIMIT).map((s) => ({
        id: s.id,
        title: s.title,
        pinned: s.pinned,
        selected: s.selected,
        running: s.running,
        waiting: s.waiting,
      })),
    },
    layout: {
      sidebar: sidebarShown(),
      inspector: ui.inspectorOpen ? ui.inspectorTab : null,
      settings: ui.settingsOpen ? ui.settingsTab || "general" : null,
      palette: ui.paletteOpen,
      modelPicker: ui.modelPickerOpen,
      shortcuts: ui.shortcutsOpen,
      type: ui.typeOpen,
      file: ui.filePath,
      diff: ui.diffPath,
    },
    appearance: { theme: appearance.theme, zoom: appearance.zoom, cards: appearance.cards, diagrams: appearance.diagrams },
    browser: { previews: workspace.previews, autoOpenPreviews: workspace.autoOpenPreviews },
    toasts: toasts.value.map((t) => t.message),
  };
}

/** Palette commands available right now, best matches first when `query` is given. */
export function desktopCommands(snap: OpenSnapshot, query = "") {
  return filterCommands(commands(snap), query).map(({ id, group, label, hint }) => ({ id, group, label, ...(hint ? { shortcut: hint } : {}) }));
}

/** Run palette command `id`, as if the user picked it; tells the user it was the agent. */
export function runDesktopCommand(snap: OpenSnapshot, id: string) {
  const command = commands(snap).find((c) => c.id === id);
  if (!command) throw new Error(`No command "${id}" is available now; list them with desktop_commands`);
  command.run();
  showToast(`Dial: ${command.label}`);
  return `Ran "${command.label}"`;
}

function call(tool: string, args: Record<string, unknown>): unknown {
  const snap = opened();
  switch (tool) {
    case "view":
      return desktopView(snap);
    case "commands":
      return desktopCommands(snap, typeof args.query === "string" ? args.query : "");
    case "run":
      if (typeof args.id !== "string" || !args.id) throw new Error("Say which command: id");
      return runDesktopCommand(snap, args.id);
    case "notice":
      if (typeof args.text === "string" && args.text) showToast(args.text);
      return null;
    default:
      throw new Error(`Unknown desktop call "${tool}"`);
  }
}

/** Answer the backend's calls; the result is a JSON string the backend parses. */
export function installAgentDesktop() {
  (window as unknown as { __dialDesktop: unknown }).__dialDesktop = {
    call(tool: string, args: Record<string, unknown>): string {
      let outcome: Outcome;
      try {
        outcome = { ok: true, value: call(tool, args ?? {}) ?? null };
      } catch (error) {
        outcome = { ok: false, error: error instanceof Error ? error.message : String(error) };
      }
      return JSON.stringify(outcome);
    },
  };
}
