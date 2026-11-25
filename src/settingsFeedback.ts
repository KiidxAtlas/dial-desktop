// What happened to a saved setting: a short state on its row, failure toasts that name it, and
// the restart-tagged changes still waiting for the engine to restart.
//
// The engine answers a save asynchronously: success shows up as a fresh settings page, failure as
// a toast ("Could not save the setting: …"). Saves are answered in order, so the oldest pending
// one is the one a failure is about.
import { reactive } from "vue";

/** `turn`: saved, applied once the running turn ends. `restart`: saved, needs an engine restart. */
export type RowState = "saving" | "saved" | "turn" | "restart" | "failed";

interface Pending {
  key: string;
  label: string;
  restart: boolean;
  turn: boolean;
}

/** How long a row keeps saying "Saved" (and the rest, except a pending restart). */
const SHOWN_MS = 3000;
const FAILED_MS = 8000;

export const settingFeedback = reactive({
  rows: {} as Record<string, RowState>,
  /** Labels of restart-tagged settings changed since the engine last restarted. */
  restart: [] as string[],
});

const pending: Pending[] = [];
const timers = new Map<string, ReturnType<typeof setTimeout>>();

/** `message` reworded to name `label` when it reports a failed save or reset; else `null`. */
export function namedFailure(message: string, label: string): string | null {
  const m = /^Could not (save|reset) the setting(?::\s*([\s\S]*))?$/.exec(message.trim());
  if (!m) return null;
  return `Could not ${m[1]} “${label}”${m[2] ? `: ${m[2]}` : ""}`;
}

/** What a row says once the engine accepted its change. */
export function savedState(p: { restart: boolean; turn: boolean }): RowState {
  return p.restart ? "restart" : p.turn ? "turn" : "saved";
}

/** Row text for each state. */
export const ROW_TEXT: Record<RowState, string> = {
  saving: "Saving…",
  saved: "Saved",
  turn: "Applies after this turn",
  restart: "Restart needed",
  failed: "Not saved",
};

function show(key: string, state: RowState) {
  clearTimeout(timers.get(key));
  timers.delete(key);
  settingFeedback.rows[key] = state;
  if (state === "saving" || state === "restart") return;
  timers.set(
    key,
    setTimeout(() => {
      timers.delete(key);
      if (settingFeedback.rows[key] === state) delete settingFeedback.rows[key];
    }, state === "failed" ? FAILED_MS : SHOWN_MS),
  );
}

/** A save or reset of `key` was sent. */
export function startSave(s: { key: string; label: string; restart: boolean }, turnRunning: boolean) {
  pending.push({ key: s.key, label: s.label, restart: s.restart, turn: turnRunning });
  show(s.key, "saving");
}

/** The app refused the request outright (already reported by a toast). */
export function saveRejected(key: string) {
  const i = pending.findIndex((p) => p.key === key);
  if (i >= 0) pending.splice(i, 1);
  show(key, "failed");
}

/** A fresh settings page arrived: every save still pending went through. */
export function settleSaves() {
  for (const p of pending.splice(0)) {
    const state = savedState(p);
    if (state === "restart" && !settingFeedback.restart.includes(p.label)) settingFeedback.restart.push(p.label);
    show(p.key, state);
  }
}

/** The toast to show for an engine `message`, naming the setting when it is about a pending save. */
export function nameFailure(message: string): string {
  const p = pending[0];
  const named = p && namedFailure(message, p.label);
  if (!p || !named) return message;
  pending.shift();
  show(p.key, "failed");
  return named;
}

/** The engine restarted: restart-tagged changes are in effect. */
export function restartDone() {
  settingFeedback.restart = [];
  for (const [key, state] of Object.entries(settingFeedback.rows)) if (state === "restart") delete settingFeedback.rows[key];
}
