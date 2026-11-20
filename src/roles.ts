// Model roles and presets, as the terminal's Roles tab and /modelpreset keep them in settings:
// `model_roles.<role>` is one helper model with its thinking level, and `model_presets.<name>`
// a saved set of the conversation model plus every role. Reads come flattened from
// `get_settings`; writes go through `set_setting`, then one `apply_settings`.

import { invoke } from "@tauri-apps/api/core";
import { applySaved, engineCall, readable } from "./engineTools";
import { engine, showToast } from "./bridge";

export const ROLES: { id: string; label: string; detail: string; chat: boolean }[] = [
  { id: "small", label: "Small", detail: "Fast background work: compaction, commit messages, scouts", chat: true },
  { id: "task", label: "Task", detail: "Implementation tasks and workers", chat: true },
  { id: "coach", label: "Coach", detail: "Stronger advisor: escalation, feedback, advisor mode", chat: true },
  { id: "tester", label: "Tester", detail: "Review and verification workers", chat: true },
  { id: "memory", label: "Memory", detail: "Extracts and consolidates what Dial remembers", chat: true },
  { id: "decision", label: "Decision", detail: "Quick typed choices; unassigned follows Small, then main", chat: true },
  { id: "embedding", label: "Embedding", detail: "Vectors for memory and search", chat: false },
  { id: "image", label: "Image", detail: "Generates images", chat: false },
];

export interface Selection { provider: string; model: string; thinking?: string | null; [key: string]: unknown }
export interface Preset { name: string; main: Selection | null; roles: Record<string, Selection> }

type Flat = Map<string, unknown>;

/** Engine settings as `key → value`, leaving out redacted values. */
export async function readSettings(): Promise<Flat> {
  const reply = await engineCall<{ settings: { key: string; value: unknown; redacted: boolean }[] }>("get_settings");
  return new Map(reply.settings.filter((s) => !s.redacted).map((s) => [s.key, s.value]));
}

/** The keys under `prefix.` as a nested object (`a.b.c` → `{a: {b: {c}}}`). */
export function nest(flat: Flat, prefix: string): Record<string, any> {
  const out: Record<string, any> = {};
  const head = `${prefix}.`;
  for (const [key, value] of flat) {
    if (!key.startsWith(head)) continue;
    const path = key.slice(head.length).split(".");
    let at = out;
    for (const part of path.slice(0, -1)) at = at[part] ??= {};
    at[path[path.length - 1]] = value;
  }
  return out;
}

function selection(value: any): Selection | null {
  return value && typeof value.provider === "string" && typeof value.model === "string" ? value : null;
}

/** Each assigned helper role. */
export function rolesIn(flat: Flat): Record<string, Selection> {
  const out: Record<string, Selection> = {};
  for (const [id, value] of Object.entries(nest(flat, "model_roles"))) {
    const s = selection(value);
    if (s) out[id] = s;
  }
  return out;
}

/** Saved presets, by name. */
export function presetsIn(flat: Flat): Preset[] {
  return Object.entries(nest(flat, "model_presets"))
    .map(([name, p]: [string, any]) => ({
      name,
      main: selection(p?.main),
      roles: Object.fromEntries(Object.entries(p?.roles ?? {}).filter(([, v]) => selection(v))) as Record<string, Selection>,
    }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

/** The terminal's rule: a letter, then letters, digits, `-` or `_`. */
export function validPresetName(name: string) {
  return /^[A-Za-z][A-Za-z0-9_-]*$/.test(name);
}

async function write(key: string, value: unknown) {
  await engineCall("set_setting", { key, value, scope: "global" });
}

async function remove(key: string) {
  return (await engineCall<{ removed: boolean }>("reset_setting", { key, scope: "global" })).removed;
}

/** Do `work`, then re-read and apply the saved settings once and refresh the model list. */
async function run(what: string, work: () => Promise<unknown>, done?: string) {
  try {
    await work();
    await applySaved();
    void engine.refreshModels();
    if (done) showToast(done);
    return true;
  } catch (error) {
    showToast(`${what}: ${readable(error)}`);
    return false;
  }
}

/** Assign `role` (provider and model, keeping the level only when the model stays). `baseUrl` is
 *  the endpoint a listed non-chat model (embedding, image) is served from, saved as the terminal
 *  saves it with the role. */
export function assignRole(role: string, provider: string, model: string, thinking: string | null, baseUrl?: string | null) {
  const value: Selection = { provider, model };
  if (thinking) value.thinking = thinking;
  if (baseUrl) value.base_url = baseUrl;
  return run("Could not assign the role", () => write(`model_roles.${role}`, value));
}

export function unassignRole(role: string) {
  return run("Could not clear the role", () => remove(`model_roles.${role}`));
}

/** Save the conversation model and every role as preset `name`. */
export function savePreset(name: string, main: Selection, roles: Record<string, Selection>) {
  return run("Could not save the preset", () => write(`model_presets.${name}`, { main, roles }), `Saved preset ${name}`);
}

export function deletePreset(name: string) {
  return run("Could not delete the preset", async () => {
    if (!(await remove(`model_presets.${name}`))) throw new Error(`${name} is defined in the project config; change it there`);
  }, `Deleted preset ${name}`);
}

export function renamePreset(preset: Preset, to: string) {
  return run("Could not rename the preset", async () => {
    await write(`model_presets.${to}`, { ...(preset.main ? { main: preset.main } : {}), roles: preset.roles });
    await remove(`model_presets.${preset.name}`);
  }, `Renamed to ${to}`);
}

/** Use a preset: its roles everywhere, and its conversation model and level in this session. */
export async function applyPreset(preset: Preset, workspaceRoot: string, sessionId: string) {
  try {
    await invoke("apply_model_preset", { workspaceRoot, sessionId, roles: preset.roles, main: preset.main });
    void engine.refreshModels();
    showToast(`Using preset ${preset.name}`);
    return true;
  } catch (error) {
    showToast(`Could not apply the preset: ${readable(error)}`);
    return false;
  }
}
