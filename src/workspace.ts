// The workspace panels' shared state: panel widths, the dev servers Dial started (previews), and
// handing things to the composer. Panel components keep their own details.

import { reactive, watch } from "vue";
import type { Entry, OpenSnapshot } from "./types";
import { focusComposer, ui, type InspectorTab, type LineRange } from "./ui";
import { appRevealRequest, appRunnerApi } from "./appRunner";
import { browserApi, showToast, snapshot } from "./bridge";
import { draftFor } from "./drafts";

const KEY = "dial.workspace";

/** Panels that need room: the inspector widens while they show. */
const WIDE: InspectorTab[] = ["files", "terminal", "browser", "app"];
export const MIN_WIDTH = 260;

interface Prefs {
  /** Inspector width for the plan, changes and activity lists. */
  narrowWidth: number;
  /** Inspector width for the files, terminal and browser panels (and a diff). */
  wideWidth: number;
  /** Open a dev server Dial starts in the browser panel at once. */
  autoOpenPreviews: boolean;
}

function load(): Prefs {
  const defaults: Prefs = { narrowWidth: 300, wideWidth: 620, autoOpenPreviews: false };
  try {
    const value = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    return {
      narrowWidth: Number.isFinite(value?.narrowWidth) ? value.narrowWidth : defaults.narrowWidth,
      wideWidth: Number.isFinite(value?.wideWidth) ? value.wideWidth : defaults.wideWidth,
      // The old autoOpenPreviews value was also persisted on resize, so true wasn't an opt-in.
      autoOpenPreviews: value?.previewAutoOpenOptIn === true,
    };
  } catch {
    return defaults;
  }
}

export const workspace = reactive({
  ...load(),
  /** Local addresses seen in this session's tool output, oldest first. */
  previews: [] as string[],
  /** An address the browser panel should open when it next shows. */
  pendingUrl: null as string | null,
  /** What the agent is doing in the browser right now (its MCP tools), or null. */
  agentActivity: null as string | null,
});

watch(
  () => [workspace.narrowWidth, workspace.wideWidth, workspace.autoOpenPreviews],
  () => {
    try {
      const { narrowWidth, wideWidth, autoOpenPreviews } = workspace;
      localStorage.setItem(KEY, JSON.stringify({ narrowWidth, wideWidth, previewAutoOpenOptIn: autoOpenPreviews }));
    } catch {
      // Storage may be unavailable; the choice still holds for this run.
    }
  },
);

/** Whether the inspector shows a panel that wants the wide width. */
export function isWide(): boolean {
  return WIDE.includes(ui.inspectorTab) || (ui.inspectorTab === "changes" && !!ui.diffPath);
}

/** Room the conversation keeps beside the inspector (and the sidebar's, while it shows). */
const CONVERSATION_MIN = 540;
const SIDEBAR = 280;

/** The window's width in CSS pixels, kept current so the inspector's width follows a resize. */
export const viewport = reactive({ width: typeof window === "undefined" ? 1200 : window.innerWidth });
if (typeof window !== "undefined") window.addEventListener("resize", () => (viewport.width = window.innerWidth));

function maxWidth(): number {
  const used = CONVERSATION_MIN + (ui.sidebarOpen ? SIDEBAR : 0);
  return Math.max(MIN_WIDTH, viewport.width - used);
}

/** The inspector's width: the chosen one, narrowed when the window leaves too little room. */
export function inspectorWidth(): number {
  const chosen = isWide() ? workspace.wideWidth : workspace.narrowWidth;
  return Math.min(chosen, maxWidth());
}

/** Resize the inspector to `width` CSS pixels, keeping room for the conversation. */
export function setInspectorWidth(width: number) {
  const clamped = Math.round(Math.min(maxWidth(), Math.max(MIN_WIDTH, width)));
  if (isWide()) workspace.wideWidth = clamped;
  else workspace.narrowWidth = clamped;
}

/** Show panel `tab` in the inspector. */
export function showPanel(tab: InspectorTab) {
  if (tab === "files") ui.fileLines = null;
  ui.settingsOpen = false;
  ui.inspectorOpen = true;
  ui.inspectorTab = tab;
}

export function showFile(path: string, lines: LineRange | null = null) {
  ui.filePath = path;
  showPanel("files");
  ui.fileLines = lines;
}

/** Open `url` (an address or a search) in the browser panel. */
export function openInBrowser(url: string) {
  workspace.pendingUrl = url;
  showPanel("browser");
}

/** Follow the agent's browser tools: show the panel and open the pages they ask for. */
export async function followAgentBrowser(): Promise<() => void> {
  try {
    return await browserApi.onAgent((event) => {
      if (event.kind === "open") openInBrowser(event.url);
      else if (event.kind === "show") showPanel("browser");
      else workspace.agentActivity = event.busy ? (event.text ?? "Using the browser") : null;
    });
  } catch {
    // Outside Tauri there is no browser.
    return () => {};
  }
}

/**
 * Reveal the App panel when the agent starts a guest for the open project, even if the panel was
 * closed. Listens only to the reveal notification, never the frame stream, and checks the open
 * project's root before showing anything.
 */
export async function followAgentApp(): Promise<() => void> {
  try {
    return await appRunnerApi.onReveal((event) => {
      if (event.workspaceRoot !== snapshot.value?.workspace?.root) return;
      // Select the target the agent booted, so the pane shows that run; without the field, nothing moves.
      if (event.guestTarget) appRevealRequest.value = { workspaceRoot: event.workspaceRoot, guestTarget: event.guestTarget };
      showPanel("app");
    });
  } catch {
    // Outside Tauri there is no app runner.
    return () => {};
  }
}

/** Add `text` to the end of the open session's draft as its own paragraph, and focus the composer. */
export function addToPrompt(text: string) {
  const snap = snapshot.value;
  if (!snap?.workspace) return;
  const draft = draftFor(snap as OpenSnapshot);
  const current = draft.text.replace(/\s+$/, "");
  draft.text = current ? `${current}\n\n${text}` : text;
  focusComposer();
}

/** `path` as a Markdown code span the agent reads as a file reference. */
export function mention(path: string): string {
  const ticks = path.match(/`+/g);
  if (!ticks) return `\`${path}\``;
  const fence = "`".repeat(1 + Math.max(...ticks.map(run => run.length)));
  return `${fence} ${path} ${fence}`;
}

// Dev servers announce themselves in tool output ("Local: http://localhost:5173/").
const ANSI = /\x1b\[[0-9;?]*[A-Za-z]/g;
const LOCAL_URL =
  /\bhttps?:\/\/(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\]|[a-z0-9-]+\.localhost)(?::\d{2,5})?(?:\/[^\s"'<>`)\]]*)?/gi;
/** Tauri's own webview hosts: never a page to preview, even when a config file or log mentions them. */
const INTERNAL_HOST = /^https?:\/\/(?:ipc|tauri|asset)\.localhost(?:[:/]|$)/i;

/** The local addresses in `text`, in order and without repeats. */
export function previewUrlsIn(text: string): string[] {
  const out: string[] = [];
  for (const match of text.replace(ANSI, "").matchAll(LOCAL_URL)) {
    let url = match[0].replace(/[.,;:!?]+$/, "").replace("://0.0.0.0", "://localhost");
    if (INTERNAL_HOST.test(url)) continue;
    if (!/\/./.test(url.replace(/^https?:\/\//i, "")) && !url.endsWith("/")) url += "/";
    if (!out.includes(url)) out.push(url);
  }
  return out;
}

// Only consume a URL immediately following a server startup label, not links in source or errors.
const SERVER_ANNOUNCEMENT = /^\s*(?:[➜-]\s*)?(?:Local:|(?:Server )?(?:running|listening) (?:on|at):?|Starting development server at)\s+(https?:\/\/\S+)/i;

/** Local dev-server addresses in the session's tool output, oldest first. */
export function previewUrls(entries: Entry[]): string[] {
  const out: string[] = [];
  for (const entry of entries) {
    if (entry.kind !== "tool") continue;
    const text = [entry.output?.text, entry.progress].filter(Boolean).join("\n");
    for (const line of text.replace(ANSI, "").split("\n")) {
      const address = SERVER_ANNOUNCEMENT.exec(line)?.[1];
      if (!address) continue;
      const [url] = previewUrlsIn(address);
      if (url && /^https?:\/\/[^/]*:\d{2,5}(?:\/|$)/i.test(url) && !out.includes(url)) out.push(url);
    }
  }
  return out;
}

let seenSession: string | null | undefined;
const seen = new Set<string>();

/** Follow the session on screen: list its previews, and open a newly started one. */
export function trackPreviews(snap: OpenSnapshot) {
  const urls = previewUrls(snap.entries);
  workspace.previews = urls;
  if (snap.sessionId !== seenSession) {
    // Opening a session shows its history; only servers started from now on open by themselves.
    seenSession = snap.sessionId;
    seen.clear();
    urls.forEach((url) => seen.add(url));
    return;
  }
  for (const url of urls) {
    if (seen.has(url)) continue;
    seen.add(url);
    if (workspace.autoOpenPreviews) openInBrowser(url);
    else showToast(`Dial started ${url}; open it from the Browser panel`);
  }
}
