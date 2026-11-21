// The App panel's calls to the Rust side (`src-tauri/src/app_runner`). The guest is Dial's own bundled
// runner; this module boots it, shows its frames and sends input. No host-shell or desktop fallback,
// and no command launch: commands are typed in the guest's own terminal.
//
// The agent's `app_desktop` tool sends action batches (`src-tauri/src/app_runner/actions.rs`): typed text,
// key combos, clicks, drags, scrolls and waits, checked as a whole and run in order. The live screen here
// streams `pointer`, `wheel`, `key` and `release` (see `AppInput`); the agent never sends those raw events.
//
// Which guest a call targets is decided by the backend's wire fields (`guestTarget`, `unsupported`,
// `openGuestTarget`). This module holds no target policy and parses no backend message text.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { shallowRef } from "vue";

export type AppRunStatus = "idle" | "starting" | "running" | "stopped" | "failed";
export type AppController = "agent" | "user";

/** The guest a run boots, by its wire label. The backend routes by this label, never by the host. */
export type AppGuestTarget = "default" | "linux" | "windows" | "macos";

/** The selector's choices in display order. Labels are display text only; no policy hangs off them. */
export const APP_GUEST_TARGETS: readonly { value: AppGuestTarget; label: string }[] = [
  { value: "default", label: "Imported QEMU guest (unlabelled)" },
  { value: "linux", label: "Linux (QEMU guest)" },
  { value: "windows", label: "Windows (QEMU guest)" },
  { value: "macos", label: "macOS (virtual Mac)" },
];

/**
 * A run's guest: one of the private guests above, or `vm`, a VirtualBox or VMware machine the user picked.
 * The `vm` target is never a private guest choice and is not in the selector.
 */
export type AppRunTarget = AppGuestTarget | "vm";

/** The VM providers Dial lists. Each machine's `id` is the provider's own id, sent back exactly. */
export type AppVmProvider = "virtualbox" | "vmware";

export interface AppVmMachine {
  id: string;
  name: string;
  provider: AppVmProvider;
  /** Running now, per the provider; null when it cannot tell. Connecting to a running VM attaches to it. */
  running: boolean | null;
}

/** The SSH host the agent uses for a VM's commands: one the user linked, or a config alias equal to its name. */
export interface AppVmSshLink {
  host: string;
  source: "linked" | "auto";
}

/** `app_runner_vms`: the registered machines and the notices the backend gives about the search. */
export interface AppVmList {
  machines: AppVmMachine[];
  notices: string[];
  /** VM id → its SSH host, for listed machines that have one. */
  sshLinks: Record<string, AppVmSshLink>;
  /** Concrete `Host` aliases in the user's `~/.ssh/config`, as link suggestions. */
  sshHosts: string[];
}

/** An SSH config host alias Dial accepts (same rule as the backend): no spaces, patterns or leading `-`. */
export function isSshAlias(host: string): boolean {
  return host.length > 0 && host.length <= 255 && !host.startsWith("-") && /^[A-Za-z0-9._\-@:]+$/.test(host);
}

function parseSshLink(raw: unknown): AppVmSshLink | null {
  if (!raw || typeof raw !== "object") return null;
  const { host, source } = raw as Record<string, unknown>;
  if (typeof host !== "string" || !isSshAlias(host)) return null;
  if (source !== "linked" && source !== "auto") return null;
  return { host, source };
}

/** Provider groups in display order. */
export const VM_PROVIDERS: readonly { value: AppVmProvider; label: string }[] = [
  { value: "virtualbox", label: "VirtualBox" },
  { value: "vmware", label: "VMware" },
];

/** The display label for a run target. */
export function runTargetLabel(target: AppRunTarget): string {
  return target === "vm" ? "virtual machine" : guestTargetLabel(target);
}

/** Whether a wire label is a run target: a private guest or `vm`. */
export function isRunTarget(value: unknown): value is AppRunTarget {
  return value === "vm" || isGuestTarget(value);
}

/**
 * Parse `app_runner_vms`. Entries without a usable id, name or known provider are dropped, and a
 * duplicate id keeps its first entry, so a machine can only be chosen by an id the backend listed once.
 */
export function parseVmList(raw: unknown): AppVmList {
  if (!raw || typeof raw !== "object") throw new Error("This build sent a VM list that Dial could not read.");
  const { machines, notices, sshLinks, sshHosts } = raw as {
    machines?: unknown;
    notices?: unknown;
    sshLinks?: unknown;
    sshHosts?: unknown;
  };
  if (!Array.isArray(machines)) throw new Error("This build sent a VM list that Dial could not read.");
  const seen = new Set<string>();
  const out: AppVmMachine[] = [];
  for (const entry of machines) {
    if (!entry || typeof entry !== "object") continue;
    const { id, name, provider, running } = entry as Record<string, unknown>;
    if (typeof id !== "string" || !id || seen.has(id)) continue;
    if (provider !== "virtualbox" && provider !== "vmware") continue;
    seen.add(id);
    out.push({
      id,
      name: typeof name === "string" && name ? name : id,
      provider,
      running: typeof running === "boolean" ? running : null,
    });
  }
  const lines = Array.isArray(notices) ? notices.filter((n): n is string => typeof n === "string" && n !== "") : [];
  const links: Record<string, AppVmSshLink> = {};
  if (sshLinks && typeof sshLinks === "object") {
    for (const [id, value] of Object.entries(sshLinks as Record<string, unknown>)) {
      const link = parseSshLink(value);
      if (link && seen.has(id)) links[id] = link;
    }
  }
  const hosts = Array.isArray(sshHosts) ? sshHosts.filter((h): h is string => typeof h === "string" && isSshAlias(h)) : [];
  return { machines: out, notices: lines, sshLinks: links, sshHosts: hosts };
}

/** The display label for a wire target. */
export function guestTargetLabel(target: AppGuestTarget): string {
  return APP_GUEST_TARGETS.find((choice) => choice.value === target)?.label ?? target;
}

/** Whether a wire label is one of the four targets. Anything else is not a target. */
export function isGuestTarget(value: unknown): value is AppGuestTarget {
  return APP_GUEST_TARGETS.some((choice) => choice.value === value);
}

/**
 * The backend's preferred target, or the reason there is none. A failed call or an unrecognised label is
 * `ok: false`; the caller must then ask the user for a choice. Dial never guesses a target here.
 */
export type AppPreference = { ok: true; target: AppGuestTarget } | { ok: false; detail: string };

export async function fetchPreferredTarget(): Promise<AppPreference> {
  try {
    const label = await appRunnerApi.preferredTarget();
    if (isGuestTarget(label)) return { ok: true, target: label };
    return { ok: false, detail: "This build reported a guest preference that Dial does not recognise." };
  } catch (e) {
    return { ok: false, detail: appRunnerMessage(e) };
  }
}

/**
 * What a `missing_runtime` state lacks: the guest image for the selected target (`image`), the macOS
 * guest helper that this build does not package (`helper`), or other packaged QEMU parts (`packaging`).
 */
export type AppMissing = "image" | "helper" | "packaging";

export function missingOf(state: AppRunnerState): AppMissing | null {
  if (state.status !== "missing_runtime") return null;
  if (state.runtime === "macos-guest-helper") return "helper";
  if (state.runtime === "guest-image" || state.runtime === "macos-guest-image") return "image";
  return "packaging";
}

export type AppButton = "left" | "middle" | "right";
export type AppModifier = "Shift" | "Control" | "Alt" | "Meta";
export type AppOrigin = "user" | "agent";

export interface AppRunInfo {
  runId: string;
  workspaceRoot: string;
  title: string;
  status: AppRunStatus;
  controller: AppController;
  origin: AppOrigin;
  /** The guest disk this run booted (`RunInfo.guestTarget`). */
  guestTarget: AppRunTarget;
  /** The VM this run connected to, when `guestTarget` is `vm`. */
  vmId?: string;
}

export interface AppRunnerState {
  /**
   * `missing_runtime` when a bundled component is absent; `unsupported` when this build has no
   * backend for the target; `failed` when the runtime was refused.
   */
  status: AppRunStatus | "missing_runtime" | "unsupported";
  /** The target this state answers for (`StateView.guestTarget`). */
  guestTarget: AppRunTarget;
  /** The missing component id, when `status` is `missing_runtime`. */
  runtime?: string;
  /** The backend's own sentence for `failed` and `unsupported`, shown as it is. */
  message?: string;
  run?: AppRunInfo;
  /** Another target that has the workspace's one open run, when it is not `guestTarget`. */
  openGuestTarget?: AppRunTarget;
  /** The VM the user allowed the agent to launch once (absent or `null` when none). */
  approvedVmId?: string | null;
  /** For `vm`: the SSH host the agent may use for this VM's commands (shown while Agent access is on). */
  ssh?: AppVmSshLink | null;
}

/** The backend's stable error codes, shown verbatim where a user can act on them. */
export type AppRunnerCode =
  | "stale_frame"
  | "already_running"
  | "stale_workspace"
  | "not_running"
  | "unsupported"
  | "missing_runtime"
  | "start_failed"
  | "unsupported_input"
  | "not_controller"
  | "taken_over";

export interface AppRunnerError {
  code: AppRunnerCode | string;
  message: string;
}

/** The newest frame, as `app_runner_frame` returns it (`FrameView`, camelCase). */
export interface AppFrameView {
  runId: string;
  workspaceRoot: string;
  seq: number;
  width: number;
  height: number;
  mime: "image/png";
  data: string;
}

/** A frame as the live screen holds it: no pixels, only which frame is drawn and its size. */
export interface AppFrame {
  kind: "frame";
  runId: string;
  workspaceRoot: string;
  seq: number;
  width: number;
  height: number;
}

/** One changed rectangle of the live screen, as RGBA rows. */
export interface FrameRect {
  x: number;
  y: number;
  w: number;
  h: number;
  rgba: Uint8ClampedArray<ArrayBuffer>;
}

/** A packet from `app_runner_frame_bytes`: what changed since the frame the window named. */
export interface FramePacket {
  seq: number;
  width: number;
  height: number;
  /** The packet holds the whole frame: the canvas is reset to `width` x `height`. */
  full: boolean;
  rects: FrameRect[];
}

/**
 * Parse a packet: a 24-byte little-endian header (seq u64; width, height, rectangle count, flags
 * u32), then each rectangle's x, y, w, h (u32) and its RGBA rows. Null for an empty or malformed
 * packet; a rectangle outside the frame is malformed. The pixels are views, not copies.
 */
export function parseFramePacket(buffer: ArrayBuffer): FramePacket | null {
  if (buffer.byteLength < 24) return null;
  const view = new DataView(buffer);
  const seq = Number(view.getBigUint64(0, true));
  const width = view.getUint32(8, true);
  const height = view.getUint32(12, true);
  const count = view.getUint32(16, true);
  const full = (view.getUint32(20, true) & 1) === 1;
  if (!width || !height) return null;
  const rects: FrameRect[] = [];
  let at = 24;
  for (let i = 0; i < count; i++) {
    if (at + 16 > buffer.byteLength) return null;
    const x = view.getUint32(at, true);
    const y = view.getUint32(at + 4, true);
    const w = view.getUint32(at + 8, true);
    const h = view.getUint32(at + 12, true);
    at += 16;
    const len = w * h * 4;
    if (!w || !h || x + w > width || y + h > height || at + len > buffer.byteLength) return null;
    rects.push({ x, y, w, h, rgba: new Uint8ClampedArray(buffer, at, len) });
    at += len;
  }
  return at === buffer.byteLength ? { seq, width, height, full, rects } : null;
}

export interface AppStatusEvent {
  kind: "status";
  runId: string;
  workspaceRoot: string;
  status: AppRunStatus;
  controller: AppController;
  origin: AppOrigin;
  message?: string;
  /** The run's target, when the backend sends it. */
  guestTarget?: AppRunTarget;
}

export type AppRunnerEvent = AppFrame | AppStatusEvent;

/** The agent's explicit start asks the window to show the App panel for this project. */
export interface AppRevealEvent {
  kind: "reveal";
  workspaceRoot: string;
  /** The target the agent started, when the backend sends it. */
  guestTarget?: AppRunTarget;
}

/**
 * A reveal waiting for the App pane to take it: the target the agent started, for this project.
 * The pane consumes it (sets its selector, then clears this), so an old reveal never applies twice.
 */
export const appRevealRequest = shallowRef<{ workspaceRoot: string; guestTarget: AppRunTarget } | null>(null);

/**
 * One input, in frame pixels (origin top-left). `click` and `press` are whole gestures (the agent's
 * shape); the rest are the live screen's stream: the pointer with the DOM `buttons` held, wheel
 * notches (positive scrolls down), one key down or up, and `release` of everything held.
 */
export type AppInput =
  | { kind: "click"; x: number; y: number; button: AppButton }
  | { kind: "press"; code: string; modifiers: AppModifier[] }
  | { kind: "pointer"; x: number; y: number; buttons: number }
  | { kind: "wheel"; x: number; y: number; dy: number }
  | { kind: "key"; code: string; down: boolean }
  | { kind: "release" };

export const appRunnerApi = {
  state: (workspaceRoot: string, guestTarget: AppRunTarget) =>
    invoke<AppRunnerState>("app_runner_state", { workspaceRoot, guestTarget }),
  importDisk: (workspaceRoot: string, guestTarget: AppGuestTarget) =>
    invoke<boolean>("app_runner_import", { workspaceRoot, guestTarget }),
  /** The backend's preference for this machine; explicit `default` always selects the unlabelled disk. */
  preferredTarget: () => invoke<string>("app_runner_preferred_target"),
  /**
   * Boot the project's guest, or connect to the chosen VM (`guestTarget` `vm`, with its `vmId`).
   * `shareProject` sends a read-only snapshot of committed HEAD only; it is never sent for a VM.
   */
  start: (workspaceRoot: string, shareProject: boolean, guestTarget: AppRunTarget, vmId?: string) =>
    invoke<AppRunInfo>(
      "app_runner_start",
      vmId === undefined ? { workspaceRoot, shareProject, guestTarget } : { workspaceRoot, shareProject, guestTarget, vmId },
    ),
  /** Machines the backend found registered with VirtualBox and VMware. Lists only; never starts one. */
  vms: (workspaceRoot: string) => invoke<unknown>("app_runner_vms", { workspaceRoot }).then(parseVmList),
  /** Allow the agent to launch this VM once, or clear the allowance with `null`. UI-only; the agent cannot call it. */
  vmGrant: (workspaceRoot: string, vmId: string | null) => invoke<void>("app_runner_vm_grant", { workspaceRoot, vmId }),
  /** Link a listed VM to an SSH config host alias, or unlink it with `null`. UI-only; the agent cannot call it. */
  vmSshLink: (workspaceRoot: string, vmId: string, host: string | null) =>
    invoke<void>("app_runner_vm_ssh_link", { workspaceRoot, vmId, host }),
  /** App → VM **Set up SSH** on the connected VM run: types one setup command in the guest and waits until SSH answers. */
  vmSshSetup: (workspaceRoot: string, runId: string, user: string, openTerminal: boolean) =>
    invoke<{ host: string; address: string; port: number; user: string }>("app_runner_vm_ssh_setup", {
      workspaceRoot,
      runId,
      user,
      openTerminal,
    }),
  /** The newest frame of a run, for a pane that reattaches. Null until the guest has a screen. */
  frame: (runId: string, workspaceRoot: string) =>
    invoke<AppFrameView | null>("app_runner_frame", { runId, workspaceRoot }),
  /** What changed since frame `since` (null: the whole frame); empty until the guest has a screen. */
  frameBytes: (runId: string, workspaceRoot: string, since: number | null) =>
    invoke<ArrayBuffer>("app_runner_frame_bytes", { runId, workspaceRoot, since }),
  stop: (runId: string, workspaceRoot: string) => invoke<void>("app_runner_stop", { runId, workspaceRoot }),
  /** Disconnect, then ask a connected VM's guest OS to shut down gracefully. UI-only; the agent cannot call it. */
  vmShutdown: (runId: string, workspaceRoot: string) => invoke<void>("app_runner_vm_shutdown", { runId, workspaceRoot }),
  /** The Agent access switch: on lets the agent view and act, off blocks both. UI-only; the agent cannot call it. */
  setAgentAccess: (runId: string, workspaceRoot: string, allowed: boolean) =>
    invoke<AppRunInfo>("app_runner_set_agent_access", { runId, workspaceRoot, allowed }),
  /** `frameSeq` is the frame the user aimed at; a frame the window has not been sent is refused. */
  input: (runId: string, workspaceRoot: string, frameSeq: number, event: AppInput) =>
    invoke<void>("app_runner_input", { runId, workspaceRoot, frameSeq, event }),
  onEvent: (handler: (event: AppRunnerEvent) => void) =>
    listen<AppRunnerEvent>("app_runner", (e) => handler(e.payload)),
  /** Only the agent's explicit start sends this; it never carries frames. */
  onReveal: (handler: (event: AppRevealEvent) => void) =>
    listen<AppRevealEvent>("app_runner_reveal", (e) => handler(e.payload)),
};

/** One part of the host's setup (`host.rs` `Step`). */
export interface AppHostStep {
  id: string;
  label: string;
  status: "ready" | "missing" | "blocked";
  detail: string;
  /** `install` runs `app_runner_host_install`; `import` opens the guest disk import. */
  action?: { label: string; kind: "install" | "import"; admin: boolean } | null;
}

/** `app_runner_host`: the hypervisor built into this computer and what it still needs. */
export interface AppHostSetup {
  platform: "macos" | "windows" | "linux" | "other";
  backend: string;
  guest: AppGuestTarget;
  ready: boolean;
  steps: AppHostStep[];
}

export interface AppHostProgress {
  step: string;
  line: string;
}

export const appHostApi = {
  setup: () => invoke<AppHostSetup>("app_runner_host"),
  /** Asks for native confirmation, runs the step's fixed install action, and returns the new setup. */
  install: (step: string) => invoke<AppHostSetup>("app_runner_host_install", { step }),
  onProgress: (handler: (event: AppHostProgress) => void) =>
    listen<AppHostProgress>("app_runner_host", (e) => handler(e.payload)),
};

/** The backend's coded error, when `error` is one; otherwise `null`. */
export function appRunnerError(error: unknown): AppRunnerError | null {
  if (error && typeof error === "object" && typeof (error as AppRunnerError).code === "string") {
    const { code, message } = error as AppRunnerError;
    return { code, message: typeof message === "string" && message ? message : code };
  }
  return null;
}

/** A user-facing line for a failed call, without a raw stack. */
export function appRunnerMessage(error: unknown): string {
  const coded = appRunnerError(error);
  if (coded) return coded.message;
  const text = error instanceof Error ? error.message : typeof error === "string" ? error : "";
  return text.trim() || "The app runner failed.";
}

/** A pointer button as the gesture names it, or `null` for buttons Dial does not send. */
export function appButton(button: number): AppButton | null {
  if (button === 0) return "left";
  if (button === 1) return "middle";
  if (button === 2) return "right";
  return null;
}

/** Frame pixel under the pointer; letterboxing outside the image must not send a click. */
export function framePoint(
  clientX: number,
  clientY: number,
  box: { left: number; top: number; width: number; height: number },
  frame: { width: number; height: number },
): { x: number; y: number } | null {
  if (box.width <= 0 || box.height <= 0 || frame.width <= 0 || frame.height <= 0) return null;
  const rx = (clientX - box.left) / box.width;
  const ry = (clientY - box.top) / box.height;
  if (!(rx >= 0 && rx < 1 && ry >= 0 && ry < 1)) return null;
  return { x: Math.floor(rx * frame.width), y: Math.floor(ry * frame.height) };
}

/** The modifiers held on a keyboard event, in the gesture's names. */
export function modifiersOf(event: { shiftKey: boolean; ctrlKey: boolean; altKey: boolean; metaKey: boolean }): AppModifier[] {
  const out: AppModifier[] = [];
  if (event.shiftKey) out.push("Shift");
  if (event.ctrlKey) out.push("Control");
  if (event.altKey) out.push("Alt");
  if (event.metaKey) out.push("Meta");
  return out;
}
