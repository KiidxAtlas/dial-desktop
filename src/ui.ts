// State only the interface owns.

import { reactive } from "vue";

export type InspectorTab = "plan" | "changes" | "activity" | "files" | "terminal" | "browser";

/** Settings pages the desktop draws itself; `""` opens the usual first one. */
export type SettingsTab =
  | ""
  | "updates"
  | "library"
  | "roles"
  | "images"
  | "appearance"
  | "usage"
  | "skills"
  | "extensions"
  | "checks"
  | "folders"
  | "memory"
  | "schedules";

export const ui = reactive({
  /** Bumped to move keyboard focus to the composer. */
  focusComposer: 0,
  sessionFilter: "",
  transcriptJump: null as (import("./types").TranscriptHit & { root: string }) | null,
  inspectorOpen: false,
  inspectorTab: "plan" as InspectorTab,
  /** The changed file whose diff the Changes panel shows, or `null` for the list. */
  diffPath: null as string | null,
  diffExpanded: false,
  /** The file open in the Files panel's viewer. */
  filePath: null as string | null,
  /** Expanded tool calls and reasoning sections, by key. */
  expanded: new Set<string>(),
  /** The composer's model picker. */
  modelPickerOpen: false,
  /** The settings page replaces the conversation while open. */
  settingsOpen: false,
  /** The Desktop tab to open settings on (`updates`), or `""` for the usual first tab. */
  settingsTab: "" as SettingsTab,
  /** Sessions picked in the sidebar with ⌘/Ctrl- or Shift-click, by id. */
  picked: new Set<string>(),
  /** The session whose name is being edited, and where: the sidebar row or the title bar. */
  renaming: null as { id: string; place: "sidebar" | "title" } | null,
  /** The open context menu, at a point in the window. */
  menu: null as { x: number; y: number; items: MenuItem[] } | null,
  /** The command palette (⌘K). */
  paletteOpen: false,
  filePalette: false,
  /** The keyboard shortcuts sheet (⌘/). */
  shortcutsOpen: false,
  sidebarOpen: true,
  /** Bumped to focus the sidebar's session search (⇧⌘F). */
  focusSearch: 0,
  /** ⌘ is held down: sessions show their ⌘1–9 numbers. */
  jumpHints: false,
});

export type MenuItem =
  | {
      label: string;
      icon?: string;
      /** A key hint shown on the right, like `⌘⌫`. */
      hint?: string;
      danger?: boolean;
      disabled?: boolean;
      run: () => void;
    }
  | { divider: true };

/** Open a menu at the pointer (a right-click) or under `anchor` (a "…" button). */
export function openMenu(event: MouseEvent, items: MenuItem[], anchor?: HTMLElement | null) {
  event.preventDefault();
  event.stopPropagation();
  if (anchor) {
    const box = anchor.getBoundingClientRect();
    ui.menu = { x: box.left, y: box.bottom + 4, items };
  } else {
    ui.menu = { x: event.clientX, y: event.clientY, items };
  }
}

export function toggle(key: string) {
  if (!ui.expanded.delete(key)) ui.expanded.add(key);
}

export function focusComposer() {
  ui.focusComposer++;
}
