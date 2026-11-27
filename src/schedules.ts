/** The engine validates interval syntax, watch patterns and ids when it saves. */
export type ScheduleDraft = {
  prompt: string;
  id?: string;
  every?: string;
  watch?: string[];
  allow?: string[];
  approve_all?: boolean;
}

/** Parse the engine's `/schedule add` flags, stopping at the first prompt word. */
export function typedSchedule(words: string[]): ScheduleDraft {
  const draft: ScheduleDraft = { prompt: "" };
  for (let i = 0; i < words.length; i++) {
    const flag = words[i];
    if (flag === "--yes" || flag === "-y") draft.approve_all = true;
    else if (["--every", "--id", "--watch", "--allow"].includes(flag)) {
      const value = words[++i];
      if (!value) throw new Error(`${flag} needs a value`);
      if (flag === "--every") draft.every = value;
      else if (flag === "--id") draft.id = value;
      else if (flag === "--watch") (draft.watch ??= []).push(value);
      else (draft.allow ??= []).push(value);
    } else {
      draft.prompt = words.slice(flag === "--" ? i + 1 : i).join(" ");
      break;
    }
  }
  return draft;
}

export async function saveSchedule(
  draft: ScheduleDraft, root: string, ask: (summary: string) => Promise<boolean>,
  save: (request: ScheduleDraft & { confirm: true }) => Promise<unknown>,
) {
  if (!draft.prompt.trim() || (!draft.every && !draft.watch?.length)) throw new Error("Enter a prompt and an interval or file pattern");
  const access = draft.approve_all ? "ALL tools pre-approved" : draft.allow?.length ? `Read-only plus ${draft.allow.join(", ")}` : "Read-only";
  const summary = [`Save this schedule for ${root}?`, draft.prompt, draft.every && `Every ${draft.every}`, draft.watch?.length && `When files change: ${draft.watch.join(", ")}`, `Access: ${access}`, "A running scheduler may pick it up immediately."].filter(Boolean).join("\n\n");
  if (!(await ask(summary))) return false;
  await save({ ...draft, confirm: true });
  return true;
}

/** The app-owned background scheduler (`src-tauri/src/scheduler.rs`, `scheduler` event). */
export type SchedulerPhase = "stopped" | "running" | "restarting" | "failed" | "elsewhere";
export type SchedulerStatus = {
  root: string;
  phase: SchedulerPhase;
  pid: number | null;
  startedAtMs: number | null;
  restarts: number;
  retryAtMs: number | null;
  lastExit: { code: number | null; atMs: number; ranMs: number } | null;
  message: string | null;
  /** Start with the project; null until the user chose. */
  background: boolean | null;
  log: string[];
}

/** The OS service (launchd, systemd --user) that runs a project's schedules after the app quits. */
export type SchedulerService = {
  supported: boolean;
  installed: boolean;
  log: string | null;
  detail: string | null;
}

/** When an interval schedule is next due (Unix seconds), as the engine decides: the later of its
 * creation and last run, plus the interval. Watch-only schedules have no time. */
export function nextRun(entry: { every_secs?: number | null; created?: number | null }, lastRun?: number | null): number | null {
  if (!entry.every_secs || entry.created == null) return null;
  return Math.max(lastRun ?? entry.created, entry.created) + entry.every_secs;
}

/** "in 5m", "in 2h", a date, or "due now" (it runs once a scheduler gets to it). */
export function dueLabel(dueSecs: number, nowMs = Date.now()): string {
  const mins = Math.round((dueSecs * 1000 - nowMs) / 60000);
  if (mins <= 0) return "due now";
  if (mins < 60) return `in ${mins}m`;
  if (mins < 48 * 60) return `in ${Math.round(mins / 60)}h`;
  return new Date(dueSecs * 1000).toLocaleDateString();
}

/** One line for the scheduler card. `engineRunning` is the engine's own lease probe
 * (`schedule_status.running`), which also sees schedulers this app does not own. */
export function schedulerHeadline(status: SchedulerStatus | null, engineRunning?: boolean): { title: string; detail: string; ours: boolean } {
  const phase = status?.phase ?? "stopped";
  if (phase === "running") return { title: "Running in the background", detail: "Saved schedules run while this project is open, without a terminal.", ours: true };
  if (phase === "restarting") {
    const secs = status?.retryAtMs ? Math.max(0, Math.round((status.retryAtMs - Date.now()) / 1000)) : 0;
    return { title: "Restarting", detail: `The scheduler exited${status?.lastExit?.code != null ? ` (code ${status.lastExit.code})` : ""}; trying again${secs ? ` in ${secs}s` : ""}.`, ours: true };
  }
  if (engineRunning) return { title: "Running elsewhere", detail: "Another scheduler (a Terminal tab, another window or a shell) is running this project's schedules.", ours: false };
  if (phase === "elsewhere") return { title: "Scheduler stopped", detail: status?.message ?? "Another scheduler was running this project's schedules.", ours: false };
  if (phase === "failed") return { title: "Scheduler stopped after errors", detail: status?.message ?? "See the log.", ours: false };
  return { title: "Scheduler stopped", detail: "Saved schedules run only while a scheduler is running.", ours: false };
}
