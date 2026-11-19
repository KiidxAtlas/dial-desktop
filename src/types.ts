// Mirrors `src-tauri/src/view.rs`: the snapshot the Rust side publishes as the `snapshot` event.

export type Connection =
  | { state: "starting" }
  | { state: "live"; model: string; engineVersion: string | null }
  | { state: "preview" }
  | { state: "failed"; error: string }
  | { state: "ended" };

export interface Workspace {
  root: string;
  display: string;
  name: string;
}

export interface Session {
  id: string;
  title: string;
  meta: string;
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
  state: ToolState;
  progress: string | null;
  input: Block | null;
  output: Block | null;
  changedPaths: string[];
  /** Absolute paths of images an `image_gen` call wrote; always shown in the row. */
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
}

/** An image attached to the draft: a file sent by path, or pasted bytes sent inline. */
export interface Attachment {
  key: string;
  name: string;
  /** `data:` URL for the thumbnail. */
  url: string;
  path: string | null;
  /** Base64 bytes of a pasted image. */
  data: string | null;
}

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
}

export type SettingControl =
  | { type: "toggle" }
  | { type: "select"; options: Choice[] }
  | { type: "number"; min: number | null; max: number | null; step: number | null; integer: boolean; auto: boolean }
  | { type: "text"; suggestions: Choice[] }
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
