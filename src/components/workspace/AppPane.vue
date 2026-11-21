<script setup lang="ts">
// The App panel: the project's guest (Dial's own bundled runner, or a private QEMU/macOS guest) or a
// VirtualBox/VMware machine the user picks (VM mode). The screen is live, like a remote desktop viewer: frames
// arrive as raw bytes and are drawn on a canvas, newest first; moves, drags, the wheel and held keys stream to the
// guest. A blur, a panel switch or Resume lets go of anything held. Stop ends a private run; Disconnect releases
// Dial from a VM and leaves it on.
// Resume hands control back to the agent. Hiding the panel never stops or disconnects the run.
//
// The backend decides what each target can do (`unsupported`, `missing_runtime`, `failed`) and reports a run
// open under another target as `openGuestTarget`; this pane follows those fields and never parses message text.
import { computed, onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import {
  APP_GUEST_TARGETS,
  appButton,
  appHostApi,
  appRevealRequest,
  appRunnerApi,
  appRunnerMessage,
  framePoint,
  fetchPreferredTarget,
  isGuestTarget,
  missingOf,
  parseFramePacket,
  runTargetLabel,
  type AppController,
  type AppFrame,
  type FramePacket,
  type AppGuestTarget,
  type AppInput,
  type AppRunInfo,
  type AppRunTarget,
  type AppRunnerEvent,
  type AppRunnerState,
  type AppHostSetup,
  type AppVmMachine,
  type AppVmSshLink,
} from "../../appRunner";
import Icon from "../Icon.vue";
import HostSetup from "./HostSetup.vue";
import VmPicker from "./VmPicker.vue";
import VmSshLink from "./VmSshLink.vue";

const props = defineProps<{ root: string; active: boolean }>();

const state = shallowRef<AppRunnerState | null>(null);
const run = shallowRef<AppRunInfo | null>(null);
const frame = shallowRef<AppFrame | null>(null);
const controller = ref<AppController | null>(null);
const error = ref<string | null>(null);
const starting = ref(false);
/** Off by default: a snapshot of committed HEAD may include secrets. */
const shareProject = ref(false);
/**
 * What the pane asks about: a private guest the selector picks, or `vm` (VM mode). Null until the user or the
 * backend's preference picks one. Never defaulted silently.
 */
const target = ref<AppRunTarget | null>(null);
const mode = computed<"app" | "vm">(() => (target.value === "vm" ? "vm" : "app"));
/** The last private guest picked; leaving VM mode returns to it. */
const lastGuest = ref<AppGuestTarget | null>(null);
/** The backend's preferred private guest, once it has answered. */
const preferred = ref<AppGuestTarget | null>(null);
const screen = ref<HTMLElement | null>(null);
const canvas = ref<HTMLCanvasElement | null>(null);

// VM mode. The list and the selection belong to one scope; the allowance is the backend's (`state.approvedVmId`).
const vms = shallowRef<AppVmMachine[]>([]);
const vmNotices = ref<string[]>([]);
const vmLoading = ref(false);
const vmError = ref<string | null>(null);
/** True once the current scope's list answered; the allowance is only reconciled against a real list. */
const vmLoaded = ref(false);
/** The machine the user picked (exact id). Never chosen for the user. */
const vmSelected = ref<string | null>(null);
/** An allowance change is in flight; the picker is locked so consents cannot race. */
const grantBusy = ref(false);
/** Bumped after an allowance answer so the picker's native inputs re-read the true selection and allowance. */
const syncKey = ref(0);
/** Each listed VM's SSH host, and the SSH config aliases offered when linking one. */
const sshLinks = shallowRef<Record<string, AppVmSshLink>>({});
const sshHosts = shallowRef<string[]>([]);
const sshBusy = ref(false);

/** Bumped when the workspace or target changes or the pane unmounts; late answers from an older scope are dropped. */
let scope = 0;
// A status event or a newer state request supersedes an older in-flight state reply.
let revision = 0;
let vmRound = 0;
/** Set once the user or an agent reveal picks a target; the backend's preferred default then never overrides it. */
const chosen = ref(false);
/** Where the backend's preferred guest answer stands: pending, applied (ready), or unknown (needs a choice). */
const preference = ref<"pending" | "ready" | "unknown">("pending");
const preferenceDetail = ref("");
/** Live input waiting to be sent, oldest first. Moves (same buttons) and wheel turns collapse into the newest. */
let queue: { event: AppInput; seq: number }[] = [];
let sending = false;
/** What this pane holds down in the guest, so leaving the screen lets go of it. */
const heldKeys = new Set<string>();
let heldButtons = 0;
/** Wheel travel in CSS pixels not yet sent as a notch. */
let wheelTravel = 0;
/** The newest frame Dial has announced, and whether a pull of the bytes is running. */
let wanted = 0;
let pulling = false;
/** The screen's pixels as last drawn: changed rectangles are written into it, then onto the canvas. */
let picture: ImageData | null = null;

const running = computed(() => run.value?.status === "running");
const live = computed(() => run.value?.status === "starting" || run.value?.status === "running");
const vmRun = computed(() => run.value?.guestTarget === "vm");
const missing = computed(() => state.value?.status === "missing_runtime");
const need = computed(() => (state.value ? missingOf(state.value) : null));
const unsupported = computed(() => state.value?.status === "unsupported");
/** Another target holds this workspace's one run. Show it, or stop it there, before starting here. */
const openOther = computed<AppRunTarget | null>(() => {
  const other = state.value?.openGuestTarget;
  return other && other !== target.value ? other : null;
});
/** The VM the agent may launch once, as the backend reports it. */
const approvedVm = computed(() => state.value?.approvedVmId ?? null);
const vmLocked = computed(() => starting.value || grantBusy.value);
const canStartVm = computed(() => {
  const id = vmSelected.value;
  return (
    !!id &&
    vms.value.some((machine) => machine.id === id) &&
    !vmLoading.value &&
    !starting.value &&
    !live.value &&
    !grantBusy.value &&
    !openOther.value
  );
});
/** The VM the SSH link editor is for: the connected VM, else the one the user picked. */
const sshVm = computed<AppVmMachine | null>(() => {
  const id = (vmRun.value && run.value?.vmId) || vmSelected.value;
  return (id && vms.value.find((machine) => machine.id === id)) || null;
});

// The host's built-in hypervisor and its install steps. Host-wide, not tied to a workspace scope.
const host = shallowRef<AppHostSetup | null>(null);
const hostLoading = ref(false);
const hostError = ref<string | null>(null);
const hostBusy = ref<string | null>(null);
const hostProgress = ref<Record<string, string>>({});
/** The setup card shows for the guest this host runs, until a run is open. */
const showHost = computed(() => mode.value === "app" && !live.value && !!host.value && target.value === host.value.guest);

async function loadHost() {
  hostLoading.value = true;
  hostError.value = null;
  try {
    host.value = await appHostApi.setup();
  } catch (e) {
    hostError.value = appRunnerMessage(e);
  } finally {
    hostLoading.value = false;
  }
}

async function installStep(step: string) {
  if (hostBusy.value) return;
  hostBusy.value = step;
  hostError.value = null;
  hostProgress.value = { ...hostProgress.value, [step]: "" };
  try {
    host.value = await appHostApi.install(step);
  } catch (e) {
    hostError.value = appRunnerMessage(e);
  } finally {
    hostBusy.value = null;
  }
  await refresh();
}

async function importHostDisk() {
  await importDisk();
  await loadHost();
}

function checkHost() {
  void loadHost();
  void refresh();
}
/** Private runs end with Stop; a VM is only disconnected, so the label says what actually happens. */
const stopLabel = computed(() => (vmRun.value ? "Disconnect" : "Stop"));
const stopTitle = computed(() => (vmRun.value ? "Release Dial from the VM. The VM stays on." : "Stop the guest"));
const label = computed(() => {
  switch (run.value?.status) {
    case "starting":
      return "Starting";
    case "running":
      return "Running";
    case "stopped":
      return "Stopped";
    case "failed":
      return "Failed";
    default:
      return "Not running";
  }
});

function onEvent(event: AppRunnerEvent) {
  const current = run.value;
  if (event.workspaceRoot !== props.root) return;
  if (!current || event.runId !== current.runId) {
    // The agent can launch while this pane is already mounted. Ask the backend which run is
    // current, for the selected target and for any other target that holds the run, rather than
    // binding input to an id from a delayed event.
    if (event.kind === "status" && (event.status === "starting" || event.status === "running")) void refresh();
    return;
  }
  if (event.kind === "frame") {
    if (live.value) want(event.seq);
    return;
  }
  revision++;
  run.value = { ...current, status: event.status };
  controller.value = event.controller;
  if (event.status === "running" && !frame.value) want(1);
  if (event.status !== "running") {
    frame.value = null;
    forgetInput();
  }
  if (event.message && (event.status === "failed" || event.status === "stopped")) error.value = event.message;
}

/** A frame `seq` is ready: pull the newest bytes unless a pull is already on its way. */
function want(seq: number) {
  wanted = Math.max(wanted, seq);
  const r = run.value;
  if (!pulling && r) void pull(scope, r.runId, props.root);
}

/**
 * Fetch and draw until the newest announced frame is on screen. Each fetch asks for what changed
 * since the frame shown, so a busy screen sends only its changed tiles and frames that arrived
 * meanwhile are folded into one fetch, never queued.
 */
async function pull(mine: number, runId: string, root: string) {
  pulling = true;
  try {
    while (mine === scope && run.value?.runId === runId && live.value && (!frame.value || wanted > frame.value.seq)) {
      const since = picture && frame.value ? frame.value.seq : null;
      const packet = parseFramePacket(await appRunnerApi.frameBytes(runId, root, since));
      if (mine !== scope || run.value?.runId !== runId || !live.value) return;
      if (!packet || (frame.value && packet.seq < frame.value.seq)) break;
      if (!apply(packet)) {
        // A delta that does not fit what is drawn: start over from the whole frame.
        picture = null;
        continue;
      }
      frame.value = { kind: "frame", runId, workspaceRoot: root, seq: packet.seq, width: packet.width, height: packet.height };
      if (packet.seq >= wanted) break;
    }
  } catch (e) {
    if (mine === scope && run.value?.runId === runId && live.value) error.value = appRunnerMessage(e);
  } finally {
    pulling = false;
  }
}

/** Write a packet's rectangles into the picture and onto the canvas. False if it is a delta for another picture. */
function apply(packet: FramePacket): boolean {
  if (packet.full) picture = new ImageData(packet.width, packet.height);
  const pic = picture;
  if (!pic || pic.width !== packet.width || pic.height !== packet.height) return false;
  const stride = pic.width * 4;
  for (const r of packet.rects) {
    const row = r.w * 4;
    for (let y = 0; y < r.h; y++) {
      pic.data.set(r.rgba.subarray(y * row, (y + 1) * row), (r.y + y) * stride + r.x * 4);
    }
  }
  const el = canvas.value;
  const ctx = el?.getContext("2d");
  if (!el || !ctx) return true;
  if (packet.full || el.width !== pic.width || el.height !== pic.height) {
    paint();
  } else {
    for (const r of packet.rects) ctx.putImageData(pic, 0, 0, r.x, r.y, r.w, r.h);
  }
  return true;
}

/** Draw the whole picture, sizing the canvas to the guest's pixels (also for a canvas that mounts late). */
function paint() {
  const el = canvas.value;
  const pic = picture;
  if (!el || !pic) return;
  if (el.width !== pic.width || el.height !== pic.height) {
    el.width = pic.width;
    el.height = pic.height;
  }
  el.getContext("2d")?.putImageData(pic, 0, 0);
}
watch(canvas, paint);

async function refresh() {
  const mine = scope;
  const root = props.root;
  const guest = target.value;
  if (!guest) return;
  const requested = ++revision;
  try {
    const next = await appRunnerApi.state(root, guest);
    if (mine !== scope || requested !== revision) return;
    state.value = next;
    if (!next.run || next.run.runId !== run.value?.runId) frame.value = null;
    run.value = next.run && next.run.workspaceRoot === root ? next.run : null;
    controller.value = run.value?.controller ?? null;
    // A VM run shows the machine it is connected to, so the picker never offers a different one.
    if (guest === "vm" && run.value?.vmId) vmSelected.value = run.value.vmId;
    if (run.value && (run.value.status === "running" || run.value.status === "starting")) want(1);
    error.value = next.status === "failed" && next.message ? next.message : null;
    reconcileVm();
  } catch (e) {
    if (mine !== scope || requested !== revision) return;
    state.value = null;
    error.value = appRunnerMessage(e);
  }
}

/** Discover the registered VMs. Runs on entering VM mode and on Refresh; never starts or grants anything. */
async function discoverVms() {
  const mine = scope;
  const root = props.root;
  const round = ++vmRound;
  vmLoading.value = true;
  vmError.value = null;
  try {
    const list = await appRunnerApi.vms(root);
    if (mine !== scope || round !== vmRound) return;
    vms.value = list.machines;
    vmNotices.value = list.notices;
    sshLinks.value = list.sshLinks;
    sshHosts.value = list.sshHosts;
    vmLoaded.value = true;
    reconcileVm();
  } catch (e) {
    if (mine !== scope || round !== vmRound) return;
    vms.value = [];
    vmNotices.value = [];
    sshLinks.value = {};
    sshHosts.value = [];
    vmLoaded.value = false;
    vmError.value = appRunnerMessage(e);
  } finally {
    if (mine === scope && round === vmRound) vmLoading.value = false;
  }
}

/**
 * Keep the VM selection and the agent's allowance on machines the list still shows. A machine that vanished
 * loses its selection, and an allowance for it is revoked, so a stale consent cannot reach a different VM.
 * Does nothing until a real list has answered.
 */
function reconcileVm() {
  if (mode.value !== "vm" || !vmLoaded.value || live.value || grantBusy.value) return;
  const listed = (id: string | null | undefined) => !!id && vms.value.some((machine) => machine.id === id);
  if (vmSelected.value && !listed(vmSelected.value)) vmSelected.value = null;
  const granted = state.value?.approvedVmId;
  if (granted && !listed(granted)) void setGrant(null);
}

/** Every allowance change runs after the one before it, so a switch can wait for a change still in flight. */
let grantTask: Promise<unknown> = Promise.resolve();
let grantsInFlight = 0;

/**
 * Set or clear the agent's one-time allowance for this workspace, then re-read the backend's state. Resolves true
 * only when the backend accepted the change and the re-read agrees. A change queued for an old scope is never sent.
 */
function setGrant(vmId: string | null): Promise<boolean> {
  const mine = scope;
  const root = props.root;
  grantsInFlight++;
  grantBusy.value = true;
  const task = grantTask.then(async (): Promise<boolean> => {
    if (mine !== scope) return false;
    let failure: string | null = null;
    try {
      await appRunnerApi.vmGrant(root, vmId);
    } catch (e) {
      failure = appRunnerMessage(e);
    }
    if (mine !== scope) return false;
    // Re-read whether or not the change landed, so the UI shows what the backend holds.
    await refresh();
    if (mine !== scope) return false;
    if (failure) {
      error.value = failure;
      return false;
    }
    return !!state.value && (vmId === null ? !state.value.approvedVmId : state.value.approvedVmId === vmId);
  });
  grantTask = task.catch(() => false).finally(() => {
    grantsInFlight--;
    grantBusy.value = grantsInFlight > 0;
  });
  return task;
}

/**
 * Clear any allowance before the VM view it belongs to is left. Waits for a change in flight first, then reads the
 * backend's state, so a grant that landed during the switch is revoked and never left behind.
 * Resolves false when it could not be cleared; the caller then stays where it is.
 */
async function clearAllowance(): Promise<boolean> {
  const mine = scope;
  await grantTask;
  if (mine !== scope) return false;
  if (state.value && !state.value.approvedVmId) return true;
  return setGrant(null);
}

/** Forget everything tied to the old scope: the run's screen, pending press, sharing, VM choice and any in-flight reply. */
function newScope() {
  scope++;
  revision++;
  state.value = null;
  starting.value = false;
  shareProject.value = false;
  run.value = null;
  frame.value = null;
  controller.value = null;
  error.value = null;
  forgetInput();
  vms.value = [];
  vmNotices.value = [];
  sshLinks.value = {};
  sshHosts.value = [];
  sshBusy.value = false;
  vmLoading.value = false;
  vmError.value = null;
  vmLoaded.value = false;
  vmSelected.value = null;
}

/** Ask for the state of the current target, and for the VM list when VM mode is shown. */
function refreshScope() {
  if (target.value === "vm") void discoverVms();
  void refresh();
}

/** Workspace changed: a new scope. The run itself is left alone. */
watch(
  () => props.root,
  () => {
    newScope();
    refreshScope();
  },
);

/**
 * The target changed (selector, mode or an agent reveal): a new scope. The new target's state is asked for,
 * which reattaches any run the backend reports for it, or names the other target that holds the run.
 */
watch(target, (next) => {
  if (next && next !== "vm") lastGuest.value = next;
  newScope();
  refreshScope();
});

/** The agent started a guest: select the target it booted, so its run is the one shown. */
async function consumeReveal() {
  const request = appRevealRequest.value;
  if (!request || request.workspaceRoot !== props.root) return;
  appRevealRequest.value = null;
  const mine = scope;
  if (mode.value === "vm" && request.guestTarget !== "vm" && !(await clearAllowance())) return;
  if (scope !== mine) return;
  markChosen();
  if (request.guestTarget !== target.value) target.value = request.guestTarget;
}

/** The user picked a target (or accepted the open one): the backend's preferred default never overrides it. */
function markChosen() {
  chosen.value = true;
}
function chooseTarget(next: AppRunTarget) {
  markChosen();
  target.value = next;
}
function onPick(e: Event) {
  const value = (e.target as HTMLSelectElement).value;
  if (isGuestTarget(value)) chooseTarget(value);
}
/** Bumped by each mode switch; a switch that is overtaken by a newer one does nothing. */
let modeRound = 0;

/**
 * Switch between the private guests and the VM list. Leaving VM mode first clears the allowance (waiting for any
 * change in flight). If that cannot be confirmed, the switch does not happen.
 */
async function setMode(next: "app" | "vm") {
  if (next === mode.value) return;
  const round = ++modeRound;
  if (mode.value === "vm") {
    const cleared = await clearAllowance();
    if (!cleared || round !== modeRound) return;
  }
  if (next === "vm") {
    chooseTarget("vm");
    return;
  }
  const guest = lastGuest.value ?? preferred.value;
  if (guest) chooseTarget(guest);
  else {
    markChosen();
    target.value = null;
  }
}

/**
 * A machine the user picked. Changing it first revokes an allowance for the previous one. The selection moves to the
 * new machine only once that revoke is confirmed; otherwise the previous machine stays selected.
 */
async function pickVm(id: string) {
  if (vmLocked.value || vmSelected.value === id) return;
  if (!vms.value.some((machine) => machine.id === id)) return;
  const mine = scope;
  const granted = approvedVm.value;
  error.value = null;
  if (granted && granted !== id) {
    const cleared = await setGrant(null);
    if (mine !== scope) return;
    syncKey.value++;
    if (!cleared) return;
  }
  if (mine === scope) vmSelected.value = id;
}

/** The allowance checkbox: only for the machine that is selected and listed. */
async function grantVm(allow: boolean) {
  if (grantBusy.value) return;
  const id = vmSelected.value;
  if (!id || !vms.value.some((machine) => machine.id === id)) return;
  const mine = scope;
  await setGrant(allow ? id : null);
  if (mine === scope) syncKey.value++;
}

/** Save or clear the SSH host for the VM the editor shows, then re-read the list and the agent-facing state. */
async function linkSsh(host: string | null) {
  const vm = sshVm.value;
  if (!vm || sshBusy.value) return;
  const mine = scope;
  const root = props.root;
  sshBusy.value = true;
  error.value = null;
  try {
    await appRunnerApi.vmSshLink(root, vm.id, host);
  } catch (e) {
    if (mine === scope) error.value = appRunnerMessage(e);
  } finally {
    if (mine === scope) {
      await discoverVms();
      await refresh();
    }
    sshBusy.value = false;
  }
}

/** One-press SSH setup on the connected VM; the backend types the command in the guest and waits for SSH. */
async function setupSsh(user: string, openTerminal: boolean) {
  const runId = run.value?.runId;
  if (!runId || sshBusy.value) return;
  const mine = scope;
  const root = props.root;
  sshBusy.value = true;
  error.value = null;
  try {
    await appRunnerApi.vmSshSetup(root, runId, user, openTerminal);
  } catch (e) {
    if (mine === scope) error.value = appRunnerMessage(e);
  } finally {
    if (mine === scope) {
      await discoverVms();
      await refresh();
    }
    sshBusy.value = false;
  }
}
watch(appRevealRequest, consumeReveal);

// Leaving the panel lets go of anything held. It never clears the agent's one-time allowance or disconnects a VM.
watch(
  () => props.active,
  (active) => {
    if (!active) releaseAll();
  },
);

/** Queue one input at the newest frame on screen. Sent in order; a move only ever waits for the newest move. */
function send(event: AppInput) {
  const r = run.value;
  const f = frame.value;
  if (!props.active || !r || r.status !== "running" || r.workspaceRoot !== props.root || !f) return;
  const last = queue[queue.length - 1]?.event;
  if (event.kind === "pointer" && last?.kind === "pointer" && last.buttons === event.buttons) queue.pop();
  else if (event.kind === "wheel" && last?.kind === "wheel") {
    queue.pop();
    event = { ...event, dy: Math.max(-10, Math.min(10, last.dy + event.dy)) };
  }
  queue.push({ event, seq: f.seq });
  if (!sending) void drain(scope, r.runId, props.root);
}

async function drain(mine: number, runId: string, root: string) {
  sending = true;
  try {
    while (queue.length) {
      if (mine !== scope || run.value?.runId !== runId || !running.value) {
        queue = [];
        return;
      }
      const { event, seq } = queue.shift()!;
      try {
        await appRunnerApi.input(runId, root, seq, event);
      } catch (e) {
        if (mine !== scope || run.value?.runId !== runId || !running.value) return;
        error.value = appRunnerMessage(e);
      }
    }
  } finally {
    sending = false;
  }
}

/** Let go of every key and button this pane holds in the guest. */
function releaseAll() {
  wheelTravel = 0;
  if (!heldKeys.size && !heldButtons) return;
  heldKeys.clear();
  heldButtons = 0;
  send({ kind: "release" });
}

/** Drop queued input and held state for a run that is gone (its guest let go when it closed). */
function forgetInput() {
  queue = [];
  heldKeys.clear();
  heldButtons = 0;
  wheelTravel = 0;
}

/** Frame pixel under the pointer. Outside the picture, `null`, unless `pin` keeps a drag at the edge. */
function pointIn(e: { clientX: number; clientY: number }, pin = false): { x: number; y: number } | null {
  const f = frame.value;
  const el = canvas.value;
  if (!f || !el) return null;
  const box = el.getBoundingClientRect();
  const exact = framePoint(e.clientX, e.clientY, box, f);
  if (exact || !pin || box.width <= 0 || box.height <= 0) return exact;
  const clamp = (v: number, max: number) => Math.max(0, Math.min(max - 1, Math.floor(v)));
  return {
    x: clamp(((e.clientX - box.left) / box.width) * f.width, f.width),
    y: clamp(((e.clientY - box.top) / box.height) * f.height, f.height),
  };
}

/** The DOM `buttons` Dial passes on: left, right and middle. */
function buttonsOf(e: PointerEvent): number {
  return e.buttons & 7;
}

function onPointerDown(e: PointerEvent) {
  if (!props.active || !frame.value || !appButton(e.button)) return;
  const p = pointIn(e);
  if (!p) return;
  screen.value?.focus();
  (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  heldButtons = buttonsOf(e);
  send({ kind: "pointer", x: p.x, y: p.y, buttons: heldButtons });
}

function onPointerMove(e: PointerEvent) {
  const p = pointIn(e, heldButtons !== 0);
  if (!p) return;
  heldButtons = buttonsOf(e);
  send({ kind: "pointer", x: p.x, y: p.y, buttons: heldButtons });
}

function onPointerUp(e: PointerEvent) {
  const p = pointIn(e, true);
  const before = heldButtons;
  heldButtons = e.type === "pointercancel" ? 0 : buttonsOf(e);
  if (p && before !== heldButtons) send({ kind: "pointer", x: p.x, y: p.y, buttons: heldButtons });
  else if (!p && before) releaseAll();
}

/** Wheel travel to notches: a mouse wheel's click is about 100px; a trackpad's small deltas add up. */
function onWheel(e: WheelEvent) {
  const p = pointIn(e);
  if (!p) return;
  const scale = e.deltaMode === 1 ? 40 : e.deltaMode === 2 ? 800 : 1;
  wheelTravel += e.deltaY * scale;
  const notches = Math.trunc(wheelTravel / 50);
  if (!notches) return;
  wheelTravel -= notches * 50;
  send({ kind: "wheel", x: p.x, y: p.y, dy: Math.max(-10, Math.min(10, notches)) });
}

function onKeyDown(e: KeyboardEvent) {
  // Keys aimed at the guest never reach Dial's own shortcuts (the template stops propagation).
  e.preventDefault();
  if (e.isComposing || !e.code || !props.active || !frame.value) return;
  // The guest repeats a held key itself.
  if (heldKeys.has(e.code)) return;
  heldKeys.add(e.code);
  send({ kind: "key", code: e.code, down: true });
}

function onKeyUp(e: KeyboardEvent) {
  e.preventDefault();
  if (!e.code) return;
  if (heldKeys.delete(e.code)) send({ kind: "key", code: e.code, down: false });
  // While ⌘ is down, WebKit sends no keyup for the other keys: let go of them with ⌘.
  if (e.code === "MetaLeft" || e.code === "MetaRight") {
    for (const code of [...heldKeys]) {
      heldKeys.delete(code);
      send({ kind: "key", code, down: false });
    }
  }
}

function onWindowBlur() {
  releaseAll();
}

/**
 * Boot the guest for `guest`, or connect to the VM `vmId`. Only one run per workspace; a VM never shares the
 * project snapshot.
 */
async function launch(guest: AppRunTarget, vmId?: string) {
  if (starting.value || live.value || openOther.value) return;
  const mine = scope;
  const root = props.root;
  starting.value = true;
  error.value = null;
  try {
    const share = guest === "vm" ? false : shareProject.value;
    const info = await appRunnerApi.start(root, share, guest, vmId);
    if (mine !== scope) return;
    run.value = info;
    controller.value = info.controller;
    state.value = null;
    // A fast handshake can emit running and the first frame before start's reply arrives.
    if (info.status === "starting") await refresh();
  } catch (e) {
    if (mine === scope) error.value = appRunnerMessage(e);
  } finally {
    if (mine === scope) starting.value = false;
  }
}

function start() {
  const guest = target.value;
  if (!guest || guest === "vm") return;
  void launch(guest);
}

/** Start or connect to the machine the user picked, exactly. Nothing starts without this click. */
function startVm() {
  const id = vmSelected.value;
  if (!id || !canStartVm.value) return;
  void launch("vm", id);
}

async function importDisk() {
  const guest = target.value;
  if (!guest || guest === "vm" || openOther.value) return;
  const mine = scope;
  starting.value = true;
  error.value = null;
  try {
    if ((await appRunnerApi.importDisk(props.root, guest)) && mine === scope) await refresh();
  } catch (e) {
    if (mine === scope) error.value = appRunnerMessage(e);
  } finally {
    if (mine === scope) starting.value = false;
  }
}

async function stop() {
  const r = run.value;
  if (!r) return;
  forgetInput();
  try {
    await appRunnerApi.stop(r.runId, r.workspaceRoot);
    if (run.value?.runId === r.runId) run.value = { ...r, status: "stopped" };
  } catch (e) {
    if (run.value?.runId === r.runId) error.value = appRunnerMessage(e);
  }
}

/** Disconnect and ask the VM's own OS to shut down (ACPI power button / soft stop), after the user confirms. */
async function shutDownVm() {
  const r = run.value;
  if (!r || !vmRun.value) return;
  const confirmed = await ask(
    `Shut down ${r.title}? Dial disconnects and asks the guest OS to shut down, as if its power button was pressed. Unsaved work inside the VM may be lost.`,
    { title: "Shut down the VM?", kind: "warning", okLabel: "Shut down", cancelLabel: "Cancel" },
  );
  if (!confirmed || run.value?.runId !== r.runId) return;
  forgetInput();
  try {
    await appRunnerApi.vmShutdown(r.runId, r.workspaceRoot);
    if (run.value?.runId === r.runId) run.value = { ...r, status: "stopped" };
  } catch (e) {
    if (run.value?.runId === r.runId) error.value = appRunnerMessage(e);
  }
}

/** The Agent access switch. Only the user changes it; typing in the guest does not. */
const accessBusy = ref(false);
async function setAgentAccess(allowed: boolean) {
  const r = run.value;
  if (!r || accessBusy.value) return;
  if (allowed) releaseAll();
  accessBusy.value = true;
  try {
    const info = await appRunnerApi.setAgentAccess(r.runId, r.workspaceRoot, allowed);
    if (run.value?.runId === r.runId) {
      run.value = info;
      controller.value = info.controller;
    }
  } catch (e) {
    if (run.value?.runId === r.runId) error.value = appRunnerMessage(e);
  } finally {
    accessBusy.value = false;
  }
}

let disposed = false;
let stopListening: (() => void) | null = null;
let stopHostListening: (() => void) | null = null;
/** Each attempt to resolve the preference gets a round; only the newest attempt may apply its answer. */
let prefRound = 0;

/**
 * Ask the backend for this machine's preferred guest and apply it, unless the user or an agent reveal has
 * already chosen. An unknown answer selects nothing: the pane waits for an explicit choice or a retry.
 */
async function resolvePreference(): Promise<void> {
  const round = ++prefRound;
  preference.value = "pending";
  const answer = await fetchPreferredTarget();
  if (disposed || round !== prefRound) return;
  if (!answer.ok) {
    preference.value = "unknown";
    preferenceDetail.value = answer.detail;
    return;
  }
  preference.value = "ready";
  preferenceDetail.value = "";
  preferred.value = answer.target;
  // A chosen target is already in place and its refresh was requested when it was chosen.
  if (chosen.value) {
    refreshScope();
    return;
  }
  target.value = answer.target; // the target watch refreshes
}

function retryPreference() {
  void resolvePreference();
}

onMounted(async () => {
  window.addEventListener("blur", onWindowBlur);
  consumeReveal();
  try {
    const off = await appRunnerApi.onEvent(onEvent);
    if (disposed) off();
    else stopListening = off;
    const offHost = await appHostApi.onProgress((event) => {
      hostProgress.value = { ...hostProgress.value, [event.step]: event.line };
    });
    if (disposed) offHost();
    else stopHostListening = offHost;
  } catch (e) {
    error.value = appRunnerMessage(e);
  }
  void loadHost();
  await resolvePreference();
});
onBeforeUnmount(() => {
  disposed = true;
  scope++;
  forgetInput();
  picture = null;
  window.removeEventListener("blur", onWindowBlur);
  stopListening?.();
  stopHostListening?.();
});
</script>

<template>
  <div class="app-pane">
    <div class="bar">
      <span class="status" :class="run?.status ?? 'idle'">{{ label }}</span>
      <span v-if="run?.title" class="title">{{ run.title }}</span>
      <span class="spacer" />
      <select v-if="mode === 'app'" :value="target ?? ''" class="target" aria-label="Guest target" title="Which guest to run" @change="onPick">
        <option v-if="!target" value="" disabled>Choose a guest</option>
        <option v-for="choice in APP_GUEST_TARGETS" :key="choice.value" :value="choice.value">{{ choice.label }}</option>
      </select>
      <label v-if="running && controller" class="access" :title="controller === 'agent' ? 'The agent can see and use this screen. Turn off to block it.' : 'The agent cannot see or use this screen. Turn on to allow it.'">
        <input
          type="checkbox"
          role="switch"
          :checked="controller === 'agent'"
          :aria-checked="controller === 'agent'"
          :disabled="accessBusy"
          @change="setAgentAccess(($event.target as HTMLInputElement).checked)"
        />
        <span>Agent access</span>
      </label>
      <button v-if="live" class="btn danger" :title="stopTitle" @click="stop">
        <Icon name="stop" :size="12" /> {{ stopLabel }}
      </button>
      <button v-if="live && vmRun" class="btn danger" title="Disconnect and ask the VM's OS to shut down" @click="shutDownVm">
        Shut down
      </button>
      <div class="mode" role="group" aria-label="App or virtual machine">
        <button type="button" class="seg" :class="{ on: mode === 'app' }" :aria-pressed="mode === 'app'" @click="setMode('app')">App</button>
        <button type="button" class="seg" :class="{ on: mode === 'vm' }" :aria-pressed="mode === 'vm'" @click="setMode('vm')">VM</button>
      </div>
    </div>
    <div v-if="!target && preference === 'pending'" class="waiting" role="status">Checking which guest this machine uses…</div>
    <div v-if="!target && preference === 'unknown'" class="notice" role="alert">
      <strong>Dial could not tell which guest to use on this machine.</strong>
      <p>Nothing has started, and no guest was chosen for you. Pick a guest in the selector above, or retry the check.</p>
      <p v-if="preferenceDetail" class="hint">{{ preferenceDetail }}</p>
      <button class="btn" @click="retryPreference">Retry check</button>
    </div>

    <div v-if="mode === 'vm'" class="vm-warning" role="note">
      <strong>This is a real VM with a real disk.</strong>
      <p>
        Disk changes you or the agent make in the VM are kept on its disk. The VM's own network, shared folders,
        clipboard and other settings apply. <strong>Disconnect releases Dial only; the VM stays on.</strong>
      </p>
    </div>

    <VmSshLink
      v-if="mode === 'vm' && sshVm"
      :vm-id="sshVm.id"
      :vm-name="sshVm.name"
      :provider="sshVm.provider"
      :link="sshLinks[sshVm.id] ?? null"
      :hosts="sshHosts"
      :busy="sshBusy"
      :connected="!!(running && vmRun && run?.vmId === sshVm.id)"
      @save="linkSsh"
      @setup="setupSsh"
    />

    <div v-if="running && controller" class="control">
      <template v-if="controller === 'user'">Agent access is off: the agent cannot see or use the {{ vmRun ? 'VM' : 'guest' }}.</template>
      <template v-else>Agent access is on: the agent can see and use the {{ vmRun ? 'VM' : 'guest' }}. You can still type and click.</template>
    </div>

    <HostSetup
      v-if="showHost"
      :setup="host"
      :loading="hostLoading"
      :error="hostError"
      :busy="hostBusy"
      :progress="hostProgress"
      :locked="starting || !!openOther"
      @install="installStep"
      @import="importHostDisk"
      @refresh="checkHost"
    />

    <div v-if="unsupported && !showHost" class="error" role="alert">{{ state?.message ?? 'This guest is not supported in this build.' }}</div>

    <div v-if="missing && mode === 'app' && !showHost" class="notice" role="alert">
      <template v-if="need === 'helper'">
        <strong>This build has no macOS guest helper.</strong>
        <p>A virtual Mac needs Dial’s macOS guest helper, and this build does not include it. Importing a disk cannot fix that; use a build that packages the helper. Dial won’t switch to QEMU or run the app on your desktop instead.</p>
      </template>
      <template v-else-if="need === 'image' && target === 'macos'">
        <strong>No virtual Mac guest is imported.</strong>
        <p>Choose the prepared virtual Mac folder. Dial copies its guest image into its local app data and leaves the original unchanged. Nothing is downloaded. The guest also needs the companion files from that folder, and macOS will ask for Screen Recording and Accessibility (privacy/TCC) permission for the guest helper inside the guest.</p>
      </template>
      <template v-else-if="need === 'image'">
        <strong>The app runner is not available.</strong>
        <p>Choose a bootable raw disk image for this machine’s architecture. Dial copies it locally and discards guest writes after each run.</p>
      </template>
      <template v-else>
        <strong>The app runner is not available.</strong>
        <p>This build does not include a usable guest runtime (<code>{{ state?.runtime ?? 'unknown' }}</code>). Use a build packaged with QEMU and firmware. Dial won’t run the app on your desktop instead.</p>
      </template>
      <button v-if="need === 'image'" class="btn" :disabled="starting || !!openOther" @click="importDisk">{{ starting ? 'Importing…' : target === 'macos' ? 'Import virtual Mac…' : 'Import guest disk…' }}</button>
      <p v-if="state?.message" class="hint">{{ state.message }}</p>
      <button class="btn" @click="refresh">Check again</button>
    </div>
    <div v-else-if="missing && mode !== 'app'" class="error" role="alert">{{ state?.message ?? 'The VM connector is not available in this build.' }}</div>

    <VmPicker
      v-else-if="mode === 'vm' && !live"
      :machines="vms"
      :loading="vmLoading"
      :error="vmError"
      :notices="vmNotices"
      :selected="vmSelected"
      :granted="approvedVm"
      :locked="vmLocked"
      :can-start="canStartVm"
      :starting="starting"
      :sync-key="syncKey"
      @pick="pickVm"
      @grant="grantVm"
      @refresh="discoverVms"
      @start="startVm"
    />
    <div v-else-if="run?.status === 'starting'" class="waiting">
      {{ vmRun ? 'Connecting to the VM… Disconnect releases Dial; the VM stays on.' : 'Starting the guest… Stop cancels the start.' }}
    </div>
    <div v-else-if="running" ref="screen" class="screen" tabindex="0" aria-label="Guest screen"
      @pointerdown="onPointerDown" @pointermove="onPointerMove" @pointerup="onPointerUp"
      @pointercancel="onPointerUp" @wheel.prevent="onWheel" @contextmenu.prevent
      @keydown.stop="onKeyDown" @keyup.stop="onKeyUp" @blur="releaseAll">
      <canvas v-show="frame" ref="canvas" />
      <div v-if="!frame" class="waiting">Waiting for the guest's first frame…</div>
    </div>

    <div v-if="openOther && !running" class="notice" role="status">
      <strong>A run is already open for this project: {{ runTargetLabel(openOther) }}.</strong>
      <p>Show it to see and control it, or stop it there before starting this one.</p>
      <button class="btn" @click="chooseTarget(openOther)">Show {{ runTargetLabel(openOther) }}</button>
    </div>

    <div v-if="mode === 'app' && ((state?.status === 'idle' && !error) || run?.status === 'stopped')" class="start">
      <p class="hint">
        Ask Dial to run an app here, or start the guest yourself. To include your project, select the snapshot before starting.
      </p>
      <label class="share">
        <input v-model="shareProject" type="checkbox" :disabled="starting" />
        <span>Share a read-only snapshot of committed HEAD (may include secrets); local edits are excluded.</span>
      </label>
      <p v-if="shareProject" class="hint">
        If the project has a committed <code>.dial/app.json</code>, run its launcher from the guest disc root:
        <code>python3 -I DIAL-LAUNCH.py .</code> (Windows: <code>py -3 -I DIAL-LAUNCH.py .</code>).
      </p>
      <button class="btn" :disabled="starting || !!openOther" @click="start">{{ starting ? "Starting…" : "Start guest" }}</button>
    </div>

    <div v-if="error" class="error" role="alert">{{ error }}</div>
    <button v-if="error && !running && !missing" class="btn" @click="refresh">Check again</button>
  </div>
</template>

<style scoped>
.app-pane {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  gap: 8px;
}
.bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  row-gap: 6px;
  flex: none;
}
.spacer {
  flex: 1;
}
.status {
  font-size: 12px;
  color: var(--text-faint);
}
.status.running {
  color: var(--accent);
}
.title {
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  align-self: flex-start;
  height: 26px;
  padding: 0 10px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text);
  font-size: 12px;
  cursor: pointer;
}
.btn:disabled {
  opacity: 0.4;
  cursor: default;
}
.btn.danger {
  color: var(--danger, #c0392b);
}
/* Agent access: a small switch drawn from a checkbox, so it stays keyboard- and screen-reader-operable. */
.access {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 26px;
  font-size: 12px;
  color: var(--text);
  cursor: pointer;
  user-select: none;
}
.access input {
  appearance: none;
  position: relative;
  width: 28px;
  height: 16px;
  margin: 0;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--surface);
  cursor: pointer;
  transition: background 120ms;
}
.access input::after {
  content: "";
  position: absolute;
  top: 2px;
  left: 2px;
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: var(--text-muted, #999);
  transition: transform 120ms;
}
.access input:checked {
  background: var(--accent, #d97757);
  border-color: var(--accent, #d97757);
}
.access input:checked::after {
  background: #fff;
  transform: translateX(12px);
}
.access input:focus-visible {
  outline: 2px solid var(--accent, #d97757);
  outline-offset: 2px;
}
.access input:disabled {
  opacity: 0.5;
}
.mode {
  flex: none;
  margin-left: auto;
  display: inline-flex;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  overflow: hidden;
}
.seg {
  height: 26px;
  padding: 0 10px;
  border: none;
  background: var(--surface);
  color: var(--text-faint);
  font-size: 12px;
  cursor: pointer;
}
.seg + .seg {
  border-left: 1px solid var(--border);
}
.seg.on {
  color: var(--text);
  font-weight: 600;
}
.seg:focus-visible {
  outline: 2px solid var(--focus-border);
  outline-offset: -2px;
}
.control,
.hint {
  font-size: 12px;
  color: var(--text-faint);
}
.notice,
.error,
.vm-warning {
  font-size: 12px;
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
}
.notice p,
.vm-warning p {
  margin: 4px 0 8px;
}
.vm-warning {
  border-color: var(--warning, #b7791f);
}
.error {
  color: var(--danger, #c0392b);
}
.screen {
  flex: 0 1 auto;
  min-height: 0;
  overflow: auto;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--surface);
  outline: none;
  touch-action: none;
}
.screen:focus-visible {
  border-color: var(--focus-border);
}
.screen canvas {
  display: block;
  width: 100%;
  height: auto;
  user-select: none;
}
.waiting {
  font-size: 12px;
  color: var(--text-faint);
  padding: 12px;
}
.start {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.share {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  font-size: 12px;
}
.target {
  flex: none;
  max-width: 180px;
  height: 26px;
  padding: 0 6px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text);
  font-size: 12px;
}
</style>
