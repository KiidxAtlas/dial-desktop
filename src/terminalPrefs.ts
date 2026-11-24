// How your own terminals look and how much they keep: font size and scrollback, set in
// Settings › Appearance and applied to open terminals at once. Kept in localStorage like the
// other appearance choices.

import { reactive, watch } from "vue";

export interface TerminalPrefs {
  /** Font size in CSS pixels. */
  fontSize: number;
  /** Lines kept above the screen. */
  scrollback: number;
}

const KEY = "dial.terminal";
export const TERMINAL_FONT_SIZE = { min: 9, max: 24, step: 0.5, default: 12.5 } as const;
export const TERMINAL_SCROLLBACK = { min: 500, max: 100_000, step: 500, default: 5000 } as const;

function inRange(value: unknown, range: { min: number; max: number; default: number }): number {
  return typeof value === "number" && Number.isFinite(value) ? Math.min(range.max, Math.max(range.min, value)) : range.default;
}

/** Stored prefs, each field falling back to its default when missing or unusable. */
export function parseTerminalPrefs(raw: string | null): TerminalPrefs {
  let value: Record<string, unknown> = {};
  try {
    const parsed = raw ? JSON.parse(raw) : {};
    if (parsed && typeof parsed === "object") value = parsed;
  } catch {
    // A damaged entry falls back to the defaults.
  }
  return {
    fontSize: inRange(value.fontSize, TERMINAL_FONT_SIZE),
    scrollback: Math.round(inRange(value.scrollback, TERMINAL_SCROLLBACK)),
  };
}

function stored(): string | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

export const terminalPrefs = reactive<TerminalPrefs>(parseTerminalPrefs(stored()));

watch(
  () => ({ ...terminalPrefs }),
  (now) => {
    try {
      localStorage.setItem(KEY, JSON.stringify(now));
    } catch {
      // Storage may be unavailable; the choice still holds for this run.
    }
  },
);
