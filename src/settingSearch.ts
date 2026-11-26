// What a settings search matches for one engine setting. Everything comes from the engine's own
// registry (the same source as `dial config reference --json`), so names stay in step with the terminal.

import type { Setting } from "./types";

/** The words a search looks through: label, key, description, place, and every offered choice. */
export function settingHaystack(s: Pick<Setting, "label" | "key" | "description" | "section" | "tab" | "control">, tabLabel = ""): string {
  const choices =
    s.control.type === "select" ? s.control.options : s.control.type === "text" ? s.control.suggestions : [];
  return [s.label, s.key, s.description, s.section, s.tab, tabLabel, ...choices.flatMap((c) => [c.label, c.value])].join(" ");
}

/** True when every search term appears somewhere in the setting, ignoring case. */
export function settingMatches(terms: string[], s: Parameters<typeof settingHaystack>[0], tabLabel = ""): boolean {
  const hay = settingHaystack(s, tabLabel).toLowerCase();
  return terms.every((t) => hay.includes(t.toLowerCase()));
}
