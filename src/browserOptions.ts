// Browser panel options that live in the frontend: the search engine, the viewport preset and the
// panel session's page history, plus the Rust commands behind them (`browser.rs`).
import { invoke } from "@tauri-apps/api/core";

export type SearchEngineId = "duckduckgo" | "google" | "bing" | "brave" | "kagi" | "custom";

/** Built-in engines; `%s` is where the search words go. */
export const SEARCH_ENGINES: readonly { id: Exclude<SearchEngineId, "custom">; label: string; template: string }[] = [
  { id: "duckduckgo", label: "DuckDuckGo", template: "https://duckduckgo.com/?q=%s" },
  { id: "google", label: "Google", template: "https://www.google.com/search?q=%s" },
  { id: "bing", label: "Bing", template: "https://www.bing.com/search?q=%s" },
  { id: "brave", label: "Brave Search", template: "https://search.brave.com/search?q=%s" },
  { id: "kagi", label: "Kagi", template: "https://kagi.com/search?q=%s" },
];

export interface SearchChoice {
  engine: SearchEngineId;
  /** The custom template, for `custom`. */
  custom: string;
}

const SEARCH_KEY = "dial.browser.search";
const VIEWPORT_KEY = "dial.browser.viewport";

/** Same rule as `check_search_template` in browser.rs: an http(s) address containing `%s`. */
export function isSearchTemplate(template: string): boolean {
  const text = template.trim();
  if (!text.includes("%s")) return false;
  try {
    const url = new URL(text.replaceAll("%s", "test"));
    return url.protocol === "https:" || url.protocol === "http:";
  } catch {
    return false;
  }
}

/** The template a choice searches with; `null` means the backend's default (DuckDuckGo). */
export function searchTemplate(choice: SearchChoice): string | null {
  if (choice.engine === "custom") return isSearchTemplate(choice.custom) ? choice.custom.trim() : null;
  if (choice.engine === "duckduckgo") return null;
  return SEARCH_ENGINES.find((e) => e.id === choice.engine)?.template ?? null;
}

export function searchLabel(choice: SearchChoice): string {
  if (choice.engine === "custom") return "Custom";
  return SEARCH_ENGINES.find((e) => e.id === choice.engine)?.label ?? "DuckDuckGo";
}

export function loadSearch(): SearchChoice {
  try {
    const raw = JSON.parse(localStorage.getItem(SEARCH_KEY) ?? "null") as Partial<SearchChoice> | null;
    const engine = raw?.engine;
    const known = engine === "custom" || SEARCH_ENGINES.some((e) => e.id === engine);
    if (raw && known) return { engine: engine as SearchEngineId, custom: typeof raw.custom === "string" ? raw.custom : "" };
  } catch {
    // Unreadable or no storage: the default engine.
  }
  return { engine: "duckduckgo", custom: "" };
}

export function saveSearch(choice: SearchChoice) {
  try {
    localStorage.setItem(SEARCH_KEY, JSON.stringify(choice));
  } catch {
    // Storage may be unavailable; the choice holds for this run.
  }
}

export type ViewportId = "fit" | "phone" | "tablet" | "desktop";

/** Viewport presets, in CSS pixels; `fit` fills the panel. */
export const VIEWPORTS: readonly { id: ViewportId; label: string; width: number; height: number }[] = [
  { id: "fit", label: "Fit panel", width: 0, height: 0 },
  { id: "phone", label: "Phone · 390×844", width: 390, height: 844 },
  { id: "tablet", label: "Tablet · 820×1180", width: 820, height: 1180 },
  { id: "desktop", label: "Desktop · 1280×800", width: 1280, height: 800 },
];

export function loadViewport(): ViewportId {
  try {
    const id = localStorage.getItem(VIEWPORT_KEY);
    if (VIEWPORTS.some((v) => v.id === id)) return id as ViewportId;
  } catch {
    // Default below.
  }
  return "fit";
}

export function saveViewport(id: ViewportId) {
  try {
    localStorage.setItem(VIEWPORT_KEY, id);
  } catch {
    // Storage may be unavailable.
  }
}

/** Most pages the history menu keeps for a panel session. */
export const HISTORY_LIMIT = 25;

export interface HistoryEntry {
  url: string;
  title: string;
}

/**
 * The panel session's history with `url` (now on show) at the front: a page already listed moves
 * up (keeping a newer title), blank pages are not listed, and the list is capped.
 */
export function pushHistory(list: readonly HistoryEntry[], url: string, title: string): HistoryEntry[] {
  if (!url || url === "about:blank") return [...list];
  const previous = list.find((e) => e.url === url);
  const entry = { url, title: title || previous?.title || "" };
  return [entry, ...list.filter((e) => e.url !== url)].slice(0, HISTORY_LIMIT);
}

export const browserOptionsApi = {
  /** The search template for what is typed into the address bar; `null` returns to DuckDuckGo. */
  setSearch: (template: string | null) => invoke<void>("browser_set_search", { template }),
  features: () => invoke<{ devtools: boolean }>("browser_features"),
  devtools: () => invoke<void>("browser_devtools"),
  /** Pick a local HTML file; resolves to its `http://127.0.0.1:…` address, or null when cancelled. */
  openFile: () => invoke<string | null>("browser_open_file"),
  stopLocal: () => invoke<void>("browser_stop_local"),
};
