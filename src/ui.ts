// State only the interface owns.

import { reactive, watch } from "vue";

const INSPECTOR_TABS = ["plan", "changes", "activity", "workers", "peers", "files", "terminal", "browser", "app", "boards"] as const;
export type InspectorTab = (typeof INSPECTOR_TABS)[number];
export interface LineRange { start: number; end: number }

const LAYOUT_KEY = "dial.layout";

/** The panels' open state that survives a restart. */
export interface Layout {
  sidebarOpen: boolean;
  inspectorOpen: boolean;
  inspectorTab: InspectorTab;
}

/** Read a stored layout, falling back to the defaults for anything missing or malformed. */
export function parseLayout(raw: string | null): Layout {
  const layout: Layout = { sidebarOpen: true, inspectorOpen: false, inspectorTab: "plan" };
  try {
    const value = JSON.parse(raw ?? "{}");
    if (typeof value?.sidebarOpen === "boolean") layout.sidebarOpen = value.sidebarOpen;
    if (typeof value?.inspectorOpen === "boolean") layout.inspectorOpen = value.inspectorOpen;
    if (INSPECTOR_TABS.includes(value?.inspectorTab)) layout.inspectorTab = value.inspectorTab;
  } catch {
    // A damaged value opens the usual layout.
  }
  return layout;
}

function storedLayout(): Layout {
  try {
    return parseLayout(localStorage.getItem(LAYOUT_KEY));
  } catch {
    return parseLayout(null);
  }
}
const layout = storedLayout();

/** Settings pages the desktop draws itself; `""` opens the usual first one. */
export type SettingsTab =
  | ""
  | "updates"
  | "health"
  | "maintenance"
  | "library"
  | "providers"
  | "roles"
  | "images"
  | "training"
  | "appearance"
  | "usage"
  | "skills"
  | "extensions"
  | "checks"
  | "folders"
  | "memory"
  | "schedules"
  | "permissions"
  | "integrations";

export const ui = reactive({
  /** Bumped to move keyboard focus to the composer. */
  focusComposer: 0,
  sessionFilter: "",
  transcriptJump: null as (import("./types").TranscriptHit & { root: string }) | null,
  inspectorOpen: layout.inspectorOpen,
  inspectorTab: layout.inspectorTab,
  /** The side panel was put away for an empty session (not by you), so a restart keeps it open. */
  inspectorParked: false,
  /** The changed file whose diff the Changes panel shows, or `null` for the list. */
  diffPath: null as string | null,
  diffExpanded: false,
  /** The file open in the Files panel's viewer. */
  filePath: null as string | null,
  fileLines: null as LineRange | null,
  /** Expanded tool calls and reasoning sections, by key. */
  expanded: new Set<string>(),
  /** The composer's model picker. */
  modelPickerOpen: false,
  modelPickerFilter: 'all' as import('./modelPicker').PickerFilter,
  /** The settings page replaces the conversation while open. */
  settingsOpen: false,
  /** The Desktop tab to open settings on (`updates`), or `""` for the usual first tab. */
  settingsTab: "" as SettingsTab,
  /** The saved type (`savedTypes`) shown in place of the transcript while open, by id. */
  typeOpen: null as string | null,
  /** With a type open: the type itself, or its own conversation. */
  typeMode: "type" as "type" | "chat",
  /** Sessions picked in the sidebar with ⌘/Ctrl- or Shift-click, by id. */
  picked: new Set<string>(),
  /** The session whose name is being edited, and where: the sidebar row or the title bar. */
  renaming: null as { id: string; place: "sidebar" | "title" } | null,
  /** The open context menu, at a point in the window. */
  menu: null as { x: number; y: number; items: MenuItem[]; title?: string; anchor?: HTMLElement | null } | null,
  /** The command palette (⌘K). */
  paletteOpen: false,
  filePalette: false,
  /** The keyboard shortcuts sheet (⌘/). */
  shortcutsOpen: false,
  sidebarOpen: layout.sidebarOpen,
  /** Bumped to focus the sidebar's session search (⇧⌘F). */
  focusSearch: 0,
  /** ⌘ is held down: sessions show their ⌘1–9 numbers. */
  jumpHints: false,
});

watch(
  () => [ui.sidebarOpen, ui.inspectorOpen || ui.inspectorParked, ui.inspectorTab] as const,
  ([sidebarOpen, inspectorOpen, inspectorTab]) => {
    try {
      localStorage.setItem(LAYOUT_KEY, JSON.stringify({ sidebarOpen, inspectorOpen, inspectorTab }));
    } catch {
      // Storage may be unavailable; the layout still holds for this run.
    }
  },
);

export type MenuItem =
  | ({
      label: string;
      icon?: string;
      /** A key hint shown on the right, like `⌘⌫`. */
      hint?: string;
      danger?: boolean;
      disabled?: boolean;
    } & ({ run: () => void; prompt?: never } | {
      prompt: { label: string; placeholder: string; maxLength: number; confirm: string; submit: (value: string) => void };
    }))
  | { divider: true };

/** Open a menu at the pointer (a right-click) or under `anchor` (a "…" button). */
export function openMenu(event: MouseEvent, items: MenuItem[], anchor?: HTMLElement | null, title?: string) {
  event.preventDefault();
  event.stopPropagation();
  if (anchor) {
    const box = anchor.getBoundingClientRect();
    ui.menu = { x: box.left, y: box.bottom + 4, items, anchor, title };
  } else {
    ui.menu = { x: event.clientX, y: event.clientY, items, title };
  }
}

export function toggle(key: string) {
  if (!ui.expanded.delete(key)) ui.expanded.add(key);
}

/** Key for an entry's expanded state, scoped to its session: another session's row at the same index stays closed. */
export function entryKey(sessionId: string | null | undefined, part: string, index: number) {
  return `${sessionId ?? "new"}:${part}:${index}`;
}

export function focusComposer() {
  ui.focusComposer++;
}
