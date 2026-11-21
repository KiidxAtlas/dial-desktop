// How the window looks on this computer: light, dark or the system's choice, the zoom level, and
// whether answers draw interactive cards and diagrams. Kept in localStorage like the background,
// not in engine settings.

import { reactive, watch } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { invoke, isTauri } from "@tauri-apps/api/core";

export type Theme = "system" | "light" | "dark";

export interface AppearancePrefs {
  theme: Theme;
  /** Webview zoom; 1 is 100%. */
  zoom: number;
  /** Draw `dial-app` blocks as interactive cards; off shows their JSON. */
  cards: boolean;
  /** Draw `mermaid` blocks as diagrams and charts; off shows their source. */
  diagrams: boolean;
}

const KEY = "dial.appearance";
export const ZOOM_STEPS = [0.8, 0.9, 1, 1.1, 1.2, 1.35, 1.5];
const DEFAULTS: AppearancePrefs = { theme: "system", zoom: 1, cards: true, diagrams: true };

/** Read stored prefs, falling back field by field on anything unexpected. */
export function parseAppearance(raw: string | null): AppearancePrefs {
  try {
    const value = raw ? JSON.parse(raw) : {};
    const theme: Theme = ["system", "light", "dark"].includes(value?.theme) ? value.theme : DEFAULTS.theme;
    const zoom = ZOOM_STEPS.includes(value?.zoom) ? value.zoom : DEFAULTS.zoom;
    const flag = (key: "cards" | "diagrams") => (typeof value?.[key] === "boolean" ? value[key] : DEFAULTS[key]);
    return { theme, zoom, cards: flag("cards"), diagrams: flag("diagrams") };
  } catch {
    return { ...DEFAULTS };
  }
}

function stored(): string | null {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

export const appearance = reactive<AppearancePrefs>(parseAppearance(stored()));

const darkQuery = typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: dark)") : null;

/** The theme in effect once "system" is resolved. */
export function resolvedTheme(): "light" | "dark" {
  if (appearance.theme !== "system") return appearance.theme;
  return darkQuery && !darkQuery.matches ? "light" : "dark";
}

function applyTheme() {
  document.documentElement.dataset.theme = resolvedTheme();
}

function applyZoom() {
  getCurrentWebview()
    .setZoom(appearance.zoom)
    .catch(() => {
      // Outside Tauri (a browser preview) there is no webview to zoom.
    });
}

/** Tell the app which rich answers to ask the model for; engines it starts get matching guidance. */
function reportAnswerStyle() {
  if (!isTauri()) return;
  invoke("set_answer_style", { cards: appearance.cards, diagrams: appearance.diagrams }).catch((error) =>
    console.warn("could not apply the answer style", error),
  );
}

/** Step the zoom in (+1) or out (-1), or reset it (0). */
export function zoomBy(step: -1 | 0 | 1) {
  if (step === 0) {
    appearance.zoom = 1;
    return;
  }
  const at = ZOOM_STEPS.indexOf(appearance.zoom);
  const next = Math.min(ZOOM_STEPS.length - 1, Math.max(0, (at < 0 ? ZOOM_STEPS.indexOf(1) : at) + step));
  appearance.zoom = ZOOM_STEPS[next];
}

/** Apply the stored appearance and keep the window in step with later changes. */
export function startAppearance() {
  applyTheme();
  if (appearance.zoom !== 1) applyZoom();
  reportAnswerStyle();
  darkQuery?.addEventListener("change", applyTheme);
  watch(
    () => ({ ...appearance }),
    (now, before) => {
      if (now.theme !== before.theme) applyTheme();
      if (now.zoom !== before.zoom) applyZoom();
      if (now.cards !== before.cards || now.diagrams !== before.diagrams) reportAnswerStyle();
      try {
        localStorage.setItem(KEY, JSON.stringify(now));
      } catch {
        // Private storage may be unavailable; the choice still holds for this run.
      }
    },
  );
}
