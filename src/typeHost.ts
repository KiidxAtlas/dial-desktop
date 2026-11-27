// What the agent's `types_*` tools see of an open Type's app (`src-tauri/src/types/live.rs`):
// whether it is on screen and loaded, what it logged, and scripts run inside it. Rust evaluates
// fixed calls on `window.__dialTypes` in this window; each answer is a JSON string.

export interface Log { level: string; text: string }

interface Entry {
  /** The app version (`Full.appVersion`) the frame shows. */
  version: string;
  loaded: boolean;
  logs: Log[];
  canvas: () => HTMLElement | null;
  /** Send a message into the app's frame, while it has one. */
  post: ((message: object) => void) | null;
}

type Outcome = { ok: true; value: unknown } | { ok: false; error: string };
interface Run { id: string; logs: Log[]; outcome: Outcome | null; timer: ReturnType<typeof setTimeout> }

const MAX_LOGS = 200;
const entries = new Map<string, Entry>();
const runs = new Map<number, Run>();
let seq = 0;

/** Follow type `id` while its view is mounted; returns the detach. */
export function attach(id: string, canvas: () => HTMLElement | null): () => void {
  const entry: Entry = { version: "", loaded: false, logs: [], canvas, post: null };
  entries.set(id, entry);
  return () => {
    if (entries.get(id) === entry) entries.delete(id);
    fail(id, "The Type was closed while the script ran");
  };
}

export function update(id: string, patch: Partial<Omit<Entry, "logs" | "canvas">>) {
  const entry = entries.get(id);
  if (!entry) return;
  const reloading = patch.loaded === false && entry.loaded;
  Object.assign(entry, patch);
  if (patch.loaded === false) entry.logs = [];
  if (reloading) fail(id, "The app reloaded while the script ran");
}

export function log(id: string, entry: Log) {
  const target = entries.get(id);
  if (!target) return;
  target.logs.push(entry);
  if (target.logs.length > MAX_LOGS) target.logs.splice(0, target.logs.length - MAX_LOGS);
  for (const run of runs.values()) {
    if (run.id === id && !run.outcome) {
      run.logs.push(entry);
      if (run.logs.length > MAX_LOGS) run.logs.splice(0, run.logs.length - MAX_LOGS);
    }
  }
}

export function settle(key: number, outcome: Outcome) {
  const run = runs.get(key);
  if (run && !run.outcome) run.outcome = outcome;
}

function fail(id: string, error: string) {
  for (const run of runs.values()) if (run.id === id && !run.outcome) run.outcome = { ok: false, error };
}

function visible(entry: Entry): boolean {
  const el = entry.canvas();
  if (!el?.isConnected || el.closest("[hidden], [inert]") || document.visibilityState !== "visible") return false;
  const box = el.getBoundingClientRect();
  return box.width > 2 && box.height > 2;
}

export const host = {
  status(id: string) {
    const entry = entries.get(id);
    return JSON.stringify(entry ? { open: visible(entry), loaded: entry.loaded, version: entry.version } : { open: false });
  },
  logs(id: string) {
    return JSON.stringify(entries.get(id)?.logs ?? []);
  },
  run(id: string, script: string) {
    const entry = entries.get(id);
    if (!entry?.post || !entry.loaded) return JSON.stringify({ error: "The Type's app is not open; open it with types_open" });
    const key = ++seq;
    const timer = setTimeout(() => runs.delete(key), 60_000);
    (timer as unknown as { unref?: () => void }).unref?.();
    runs.set(key, { id, logs: [], outcome: null, timer });
    entry.post({ kind: "eval", id: key, script });
    return JSON.stringify({ key });
  },
  result(key: number) {
    const run = runs.get(key);
    if (!run) return JSON.stringify({ error: "The script's result was lost" });
    if (!run.outcome) return JSON.stringify({ pending: true });
    clearTimeout(run.timer);
    runs.delete(key);
    return JSON.stringify({ ...run.outcome, logs: run.logs });
  },
};

declare global {
  interface Window { __dialTypes?: typeof host }
}
if (typeof window !== "undefined") window.__dialTypes = host;
