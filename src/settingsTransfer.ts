// Settings › Overview › Back up and move: export the saved Dial settings to a file without any
// credential, and import such a file after showing what would change. Pure, so the view stays thin.

import type { Setting, SettingScope } from "./types";

export const EXPORT_FORMAT = "dial-settings";

/** The file Desktop writes (JSON). `project` holds the values saved for one project only. */
export interface SettingsExport {
  format: typeof EXPORT_FORMAT;
  version: 1;
  exported: string;
  settings: Record<string, unknown>;
  project?: Record<string, unknown>;
}

/** Credential field names, including common HTTP header and camelCase spellings.
 * `api_key_env` (a variable's name), `max_tokens` and key bindings do not match. */
const SECRET_NAME = /^(api_?key|token|secret|password|passwd|credentials?|bearer|cookies?|set_cookie|(?:proxy_)?authorization|private_key|.+_(api_?key|token|secret|password))$/i;
/** What the engine shows in place of a hidden value. */
const REDACTED_TEXT = /^(\*{3,}|•{3,}|<redacted>|\[redacted\]|redacted)$/i;

export const isSecretKey = (key: string) => key.split(".").some((part) =>
  SECRET_NAME.test(part.replace(/([a-z0-9])([A-Z])/g, "$1_$2").replace(/-/g, "_")));

/** `value` with every credential-looking entry and redacted placeholder removed; `undefined`
 *  when nothing is left to export. */
export function scrub(value: unknown): unknown {
  if (typeof value === "string") return REDACTED_TEXT.test(value.trim()) ? undefined : value;
  if (Array.isArray(value)) return value.map(scrub).filter((v) => v !== undefined);
  if (value && typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      if (isSecretKey(k)) continue;
      const kept = scrub(v);
      if (kept !== undefined) out[k] = kept;
    }
    return out;
  }
  return value;
}

/** The values saved in `scope`: rows that layer sets (or, on engines that do not say, every
 *  changed row as the everywhere layer), without credentials, and without a row whose whole
 *  table is already exported. */
export function exportable(rows: readonly Setting[], scope: SettingScope): Record<string, unknown> {
  const sourced = rows.some((r) => r.source === "global" || r.source === "project");
  const picked = rows.filter((r) => !r.redacted && !isSecretKey(r.key) && (sourced ? r.source === scope : scope === "global" && r.modified));
  const keys = new Set(picked.map((r) => r.key));
  const covered = (key: string) => key.split(".").some((_, i, parts) => i > 0 && keys.has(parts.slice(0, i).join(".")));
  const out: Record<string, unknown> = {};
  for (const r of picked) {
    if (covered(r.key)) continue;
    const value = scrub(r.value);
    if (value !== undefined) out[r.key] = value;
  }
  return out;
}

export function buildExport(rows: readonly Setting[], withProject: boolean, now = new Date()): SettingsExport {
  const file: SettingsExport = { format: EXPORT_FORMAT, version: 1, exported: now.toISOString(), settings: exportable(rows, "global") };
  if (withProject) file.project = exportable(rows, "project");
  return file;
}

/** The settings in an export, or a plain `{ key: value }` object; throws a readable message. */
export function parseImport(text: string): { settings: Record<string, unknown>; project: Record<string, unknown> | null } {
  let data: unknown;
  try {
    data = JSON.parse(text);
  } catch {
    throw new Error("This is not a settings file: it is not valid JSON.");
  }
  const table = (v: unknown) => (v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : null);
  const root = table(data);
  if (!root) throw new Error("This is not a settings file: expected an object of settings.");
  if (root.format === EXPORT_FORMAT) {
    const settings = table(root.settings);
    if (!settings) throw new Error("This settings file has no settings.");
    return { settings, project: table(root.project) };
  }
  return { settings: root, project: null };
}

export type ImportKind = "change" | "same" | "new" | "secret";
export interface ImportItem {
  key: string;
  label: string;
  value: unknown;
  /** The value in effect now; `undefined` for a key this engine does not list. */
  current: unknown;
  kind: ImportKind;
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/** A whole table/list is replaced by set_setting. Never partially import a table containing
 * credentials: deleting a hidden leaf from the payload could wipe the destination's secret. */
function collectCredentialPaths(key: string, value: unknown, hidden: Set<string>) {
  if (isSecretKey(key) || (typeof value === "string" && REDACTED_TEXT.test(value.trim()))) hidden.add(key);
  else if (value && typeof value === "object") {
    for (const [field, child] of Object.entries(value)) collectCredentialPaths(`${key}.${field}`, child, hidden);
  }
}

/** What importing `incoming` would do, by key: change a value, leave an equal one, add a key
 *  the settings page does not list, or skip a credential (never imported). */
export function importPlan(incoming: Record<string, unknown>, rows: readonly Setting[]): ImportItem[] {
  const byKey = new Map(rows.map((r) => [r.key, r]));
  const hidden = new Set<string>();
  for (const row of rows) {
    if (row.redacted) hidden.add(row.key);
    else collectCredentialPaths(row.key, row.value, hidden);
  }
  for (const [key, value] of Object.entries(incoming)) collectCredentialPaths(key, value, hidden);
  const protectedPaths = [...hidden];
  return Object.entries(incoming).map(([key, value]) => {
    const row = byKey.get(key);
    if (protectedPaths.some((h) => h === key || h.startsWith(`${key}.`) || key.startsWith(`${h}.`))) {
      return { key, label: row?.label ?? key, value: undefined, current: undefined, kind: "secret" };
    }
    const kind: ImportKind = !row ? "new" : same(row.value, value) ? "same" : "change";
    return { key, label: row?.label ?? key, value, current: row?.value, kind };
  });
}

/** A value on one line, for the preview. */
export function preview(value: unknown, max = 60): string {
  if (value === undefined) return "—";
  const text = typeof value === "string" ? value : JSON.stringify(value);
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}
