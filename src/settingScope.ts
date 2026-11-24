// Which saved layer sets a setting (`get_settings` entries carry `scope` on engines that report
// it; the settings page passes it on as `source`). Older engines send nothing, so every helper
// falls back to the scope the user picked on the page.

import type { SettingScope } from "./types";

interface Sourced {
  source?: SettingScope | null;
  modified?: boolean;
}

/** Whether the engine reported layers for any row (older engines never do). */
export function hasSources(rows: readonly Sourced[]): boolean {
  return rows.some((r) => r.source === "project" || r.source === "global");
}

/** Where a change saves by default: a value this project overrides stays a project value,
 *  so the edit is not hidden behind the override; anything else goes to the picked scope. */
export function saveScope(source: SettingScope | null | undefined, selected: SettingScope): SettingScope {
  return source === "project" ? "project" : selected;
}

/** Where a reset goes: the layer that set the value, else the picked scope. */
export function resetScope(source: SettingScope | null | undefined, selected: SettingScope): SettingScope {
  return source === "project" || source === "global" ? source : selected;
}

/** Changed rows, split by the layer that changed them when known. */
export function changedCounts(rows: readonly Sourced[]) {
  let total = 0, project = 0, global = 0;
  for (const r of rows) {
    if (!r.modified) continue;
    total++;
    if (r.source === "project") project++;
    else if (r.source === "global") global++;
  }
  return { total, project, global };
}

/** Hover text for the "N changed" button. */
export function changedTitle(counts: ReturnType<typeof changedCounts>, sourced: boolean): string {
  if (!sourced) return "Show every setting changed from its default";
  const parts = [counts.project && `${counts.project} in this project`, counts.global && `${counts.global} everywhere`].filter(Boolean);
  return `Show every setting changed from its default${parts.length ? ` (${parts.join(", ")})` : ""}`;
}

/** The badge a row shows for its layer, and a hint when the page saves somewhere it will not show. */
export function sourceBadge(source: SettingScope | null | undefined, pageScope: SettingScope | undefined) {
  if (source === "project") {
    return {
      label: "This project",
      title: "Saved for this project; it overrides the everywhere value",
      hint: pageScope === "global" ? "Overridden by this project: an everywhere value saved here will not apply in this project until you reset the project value." : "",
    };
  }
  if (source === "global") return { label: "Everywhere", title: "Saved for every project", hint: "" };
  return null;
}

/** The reset arrow's hover text. */
export function resetTitle(source: SettingScope | null | undefined): string | undefined {
  if (source === "project") return "Reset this project's value";
  if (source === "global") return "Reset the value saved for every project";
  return undefined;
}
