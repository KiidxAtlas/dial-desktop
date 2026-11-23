// Mirrors `src-tauri/src/view.rs`: the snapshot the Rust side publishes as the `snapshot` event.

export type Connection =
  | { state: "starting" }
  | { state: "live"; model: string; engineVersion: string | null }
  | { state: "preview" }
  | { state: "failed"; error: string; problem: EngineProblem }
  | { state: "ended" };

/** What the UI can offer for a failed connection; mirrors `updates::EngineProblem`. */
export type EngineProblem = "missing" | "incompatible" | "other";

/** `dial update --check --json`, as the engine reports it (`updates::EngineStatus`). */
export interface EngineStatus {
  current: string;
  latest: string | null;
  update_available: boolean;
  installation: string;
  method: string;
  can_update: boolean;
  update_command: string;
  error: string | null;
}

/** Mirrors `updates::EngineUpdateView`. */
export type EngineUpdateView =
  | { state: "checked"; status: EngineStatus }
  | { state: "unsupported"; detail: string }
  | { state: "missing"; error: string }
  | { state: "failed"; error: string };

export interface Workspace {
  root: string;
  display: string;
  name: string;
}

export interface Session {
  id: string;
  title: string;
  meta: string;
  empty?: boolean;
  pinned: boolean;
  selected: boolean;
  /** A turn runs in it, on screen or in the background. */
  running: boolean;
  /** An approval or a question in it waits for the user. */
  waiting: boolean;
}

export type ToolState = "running" | "succeeded" | "failed" | "unknown";

export interface Block {
  caption: string;
  text: string;
}

export interface ToolEntry {
  kind: "tool";
  key: string;
  label: string;
  target: string;
  tone: string | null;
  check: boolean;
  state: ToolState;
  progress: string | null;
  input: Block | null;
  output: Block | null;
  changedPaths: string[];
  /** Absolute paths of images the call wrote or viewed (an `image_gen`, a written image, a `read`); shown in the row. */
  images: string[];
  facts: string[];
}

export type Entry =
  | { kind: "user"; text: string; images: number }
  | { kind: "assistant"; text: string; reasoning: string; streaming: boolean; abandoned: boolean }
  | ToolEntry
  | { kind: "notice"; message: string; lane: string | null; noticeKind: string | null }
  | { kind: "error"; message: string };

export interface Approval {
  approvalId: string;
  tool: string;
  summary: string;
}

export interface QuestionOption {
  label: string;
  description: string | null;
  preview: string | null;
}

export interface QuestionItem {
  id: string;
  header: string | null;
  text: string;
  options: QuestionOption[];
  /** Several options may be chosen. */
  multi: boolean;
  /** Index of the option the agent suggests. */
  recommended: number | null;
}

/** The agent's open question; an empty `id` means it cannot be answered from the app. */
export interface Question {
  id: string;
  items: QuestionItem[];
}

/** The user's answer to one `QuestionItem`. */
export interface Answer {
  id: string;
  selected: string[];
  custom: string | null;
}

export interface Task {
  text: string;
  status: string;
}

export interface Change {
  path: string;
  dir: string;
  name: string;
  revision: number;
}

export interface Stats {
  running: number;
  succeeded: number;
  failed: number;
}

/** The model in force. `provider` is empty until the engine reported its full state. */
export interface ModelInfo {
  provider: string;
  modelId: string;
  label: string;
  /** The session's reasoning level; `null` runs the model's default. */
  thinking: string | null;
  defaultThinking: string | null;
  levels: string[];
  thinkingSettable: boolean;
  /** Whether the model takes image attachments; `null` while unknown. */
  acceptsImages: boolean | null;
}

/** One model the user can switch to (the `models` event). */
export interface ModelOption {
  provider: string;
  modelId: string;
  label: string;
  contextWindow: number | null;
  reasoning: boolean;
  vision: boolean;
  /** The main model, a favorite, a profile or a role in the user's Dial config. */
  configured: boolean;
  local: boolean;
  /** Present in the provider's live `/models` or account listing. */
  listed?: boolean;
  /** Only from a static bundled catalog (not configured, local, or live-listed). */
  catalogOnly?: boolean;
}

export interface TranscriptHit {
  session_id: string;
  entry_id: string;
  role: string;
  snippet: string;
  preview: string;
  title: string;
}

export interface TranscriptResults {
  hits: TranscriptHit[];
  has_more: boolean;
}

export interface LocalModelRemoval {
  label: string;
  folder: string;
  bytes: number;
  source: string;
  deleted: boolean;
}

/** A file chosen by path or pasted inline. Only supported raster images have a preview URL. */
export type Attachment = {
  key: string;
  name: string;
  url: string | null;
} & ({ path: string; data: null } | { path: null; data: string });

export type ApprovalMode = "readonly" | "ask" | "write" | "auto";

export interface Snapshot {
  platform: string;
  /** `null` until a folder is opened: the UI shows the welcome screen. */
  workspace: Workspace | null;
  connection: Connection;
  title: string;
  model: ModelInfo | null;
  approval: ApprovalMode | null;
  sessionId: string | null;
  historyLoading: boolean;
  turnRunning: boolean;
  canSend: boolean;
  sessions: Session[];
  entries: Entry[];
  approvals: Approval[];
  question: Question | null;
  plan: Task[];
  changes: Change[];
  stats: Stats;
  /** How full the conversation is; `null` until the engine reports it. */
  context: ContextView | null;
  voice: VoiceView;
}

export type VoicePhase = "off" | "starting" | "listening" | "thinking" | "speaking" | "paused";

/** Hands-free voice mode: Dial listens, works, then reads its answer aloud. */
export interface VoiceView {
  phase: VoicePhase;
  muted: boolean;
  /** What the microphone last heard. */
  heard: string | null;
  /** Why voice mode paused, when it was not the user's choice. */
  problem: string | null;
  /** The microphone while listening, 0 to 100. */
  level: number;
  /** An utterance is being recorded. */
  hearing: boolean;
  /** Something is being read aloud. */
  reading: boolean;
}

export interface ContextView {
  tokens: number;
  /** The model's context window, when the model list says. */
  window: number | null;
  estimated: boolean;
}

/** A snapshot with an open folder; everything but the welcome screen renders one. */
export type OpenSnapshot = Snapshot & { workspace: Workspace };

export type Decision = "allow" | "always" | "project" | "deny";

// Mirrors `src-tauri/src/settings.rs`: the settings page, published as the `settings` event.

export interface Choice {
  value: string;
  label: string;
  /** Distinguishes choices whose persisted values are identical. */
  id?: string;
  /** Other settings saved with this choice (an image model's endpoint and protocol). */
  with?: Record<string, unknown>;
}

/** What a text setting names, so the page can offer a picker instead of a text box. */
export type TextHint = "plain" | "model" | "provider" | "program" | "file" | "folder" | "url" | "envVar" | "microphone";

export type SettingControl =
  | { type: "toggle" }
  | { type: "select"; options: Choice[] }
  | { type: "number"; min: number | null; max: number | null; step: number | null; integer: boolean; auto: boolean; unit: string | null }
  | { type: "text"; suggestions: Choice[]; hint: TextHint }
  | { type: "list" }
  | { type: "table" };

export interface Setting {
  key: string;
  label: string;
  description: string;
  tab: string;
  section: string;
  control: SettingControl;
  /** The effective value: saved, or the default. */
  value: unknown;
  default: unknown;
  modified: boolean;
  /** Holds a credential: hidden and not editable here. */
  redacted: boolean;
  /** Takes effect when the engine restarts. */
  restart: boolean;
}

export interface SettingsPage {
  tabs: string[];
  settings: Setting[];
}

/** Where a setting is saved: for every project, or this one only. */
export type SettingScope = "global" | "project";

// The workspace panels (files.rs, browser.rs, terminal.rs).

export interface DirEntry {
  name: string;
  /** Relative to the workspace, `/`-separated. */
  path: string;
  dir: boolean;
}

export interface FileView {
  path: string;
  /** `null` for binary files. */
  text: string | null;
  size: number;
  truncated: boolean;
}

export interface DiffView {
  path: string;
  /** Unified diff; empty without uncommitted changes. */
  diff: string;
  base: "head" | "index" | "untracked";
}

/** Where the browser panel sits, in logical window pixels. */
export interface Bounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface BrowserState {
  url: string;
  title: string;
  loading: boolean;
}

export interface PageInfo {
  url: string;
  title: string;
  selection: string;
  text: string;
  errors: string[];
}

export type TerminalEvent = { kind: "data"; id: number; data: string } | { kind: "exit"; id: number };
