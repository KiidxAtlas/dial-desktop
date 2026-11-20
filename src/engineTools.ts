// Engine features the terminal offers that the desktop reaches through `engine_call`: modes,
// folders, jobs, checks, skills, extensions, memory, schedules, usage, stats and checkpoints.

import { invoke } from "@tauri-apps/api/core";
import { ref } from "vue";
import { showToast, snapshot } from "./bridge";

/** Call an engine method the desktop allows in the open project; throws a readable message. */
export async function engineCall<T = Record<string, unknown>>(method: string, params?: Record<string, unknown>): Promise<T> {
  const root = snapshot.value?.workspace?.root;
  if (!root) throw new Error("Open a folder first");
  return invoke<T>("engine_call", { workspaceRoot: root, method, params: params ?? {} });
}

/** Like `engineCall`, but a failure becomes a toast and `null`. */
export async function tryEngine<T = Record<string, unknown>>(method: string, params?: Record<string, unknown>, what?: string): Promise<T | null> {
  try {
    return await engineCall<T>(method, params);
  } catch (error) {
    showToast(`${what ?? "Dial could not do that"}: ${readable(error)}`);
    return null;
  }
}

/** An engine error as one sentence (v2 errors arrive as `code: message`). */
export function readable(error: unknown): string {
  const text = String(error instanceof Error ? error.message : error);
  return text.replace(/^(busy|invalid_params|failed|not_found|not_pending)\s*:\s*/i, (_, code: string) =>
    code.toLowerCase() === "busy" ? "Dial is busy; try again when the turn ends. " : "",
  );
}

/** Apply settings saved through `engineCall` to the running session (one rebuild). */
export async function applySaved() {
  await invoke("apply_saved_settings");
}

// ── Slash commands ────────────────────────────────────────────────────────────

export interface EngineCommand {
  name: string;
  usage: string;
  description: string;
  kind: "builtin" | "custom" | "skill";
  rpc: { surface: "method"; method: string } | { surface: "client" | "terminal"; reason: string };
}

/** The engine's slash commands for the open project; empty until loaded. */
export const engineCommands = ref<EngineCommand[]>([]);
let commandsFor = "";

/** Load the command list once per project (custom commands and skills differ by project). */
export async function loadEngineCommands(force = false) {
  const root = snapshot.value?.workspace?.root ?? "";
  if (!root || snapshot.value?.connection.state !== "live" || (!force && commandsFor === root)) return;
  commandsFor = root;
  try {
    engineCommands.value = (await engineCall<{ commands: EngineCommand[] }>("get_available_commands")).commands ?? [];
  } catch {
    commandsFor = "";
  }
}

// ── Modes ─────────────────────────────────────────────────────────────────────

export interface ModesState {
  plan: boolean;
  external_thinking: boolean;
  advisor: { enabled: boolean; completion_only: boolean; model: string | null };
  modes: { vibe: boolean; skillful: unknown; goal: { goal: unknown } };
  settings: { extended_context: boolean; fast_mode: boolean; slow_mode: boolean; turn_optimizer: boolean; service_tier: string };
}

export const modes = ref<ModesState | null>(null);

export async function loadModes() {
  try {
    modes.value = await engineCall<ModesState>("get_modes");
  } catch {
    modes.value = null;
  }
  return modes.value;
}

/** Switch one mode, then re-read every mode (one change can turn another off). */
export async function setMode(params: Record<string, unknown>) {
  const done = await tryEngine("set_mode", params, "Could not change the mode");
  await loadModes();
  return done !== null;
}

export async function setPlanMode(enabled: boolean) {
  const done = await tryEngine("plan", { enabled }, "Could not change plan mode");
  await loadModes();
  return done !== null;
}

// ── Jobs ──────────────────────────────────────────────────────────────────────

export interface Job { name: string; command: string; status: string; running: boolean; stop_requested: boolean; terminal: boolean }

export async function listJobs() {
  return (await engineCall<{ jobs: Job[] }>("list_jobs")).jobs ?? [];
}

// ── Checkpoints ───────────────────────────────────────────────────────────────

export interface Checkpoint { turn: number; label: string; files: number | string[] }
export interface CheckpointFile { path: string; writers: unknown; later_turns: unknown; overlap: boolean }

/** Bytes as a short size. */
export function bytes(n: number) {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}

/** A token count as `1.2k`, `3.4M`, `5.6B`. */
export function count(n: number | null | undefined) {
  if (n == null || !Number.isFinite(n)) return "—";
  const abs = Math.abs(n);
  if (abs < 1000) return String(Math.round(n));
  if (abs < 1e6) return `${(n / 1e3).toFixed(abs < 1e4 ? 1 : 0)}k`;
  if (abs < 1e9) return `${(n / 1e6).toFixed(abs < 1e7 ? 1 : 0)}M`;
  return `${(n / 1e9).toFixed(abs < 1e10 ? 1 : 0)}B`;
}

export function usd(n: number | null | undefined) {
  if (n == null) return "—";
  return n < 0.01 && n > 0 ? "<$0.01" : `$${n.toFixed(2)}`;
}
