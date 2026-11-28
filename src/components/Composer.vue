<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { Decision, OpenSnapshot } from "../types";
import { engine, files, showToast, snapshot, workspaceFiles } from "../bridge";
import * as attachments from "../attachments";
import { clipboardAction, MAX_ATTACHMENTS, MAX_FILE_BYTES } from '../attachmentFormats';
import { viewAttachment } from "../imageViewer";
import { openMenu, ui } from "../ui";
import { beginSend, draftFor } from "../drafts";
import { draftReady, readingFiles, restoreInput } from '../draftInput';
import { startupDestination } from '../startupPrompt';
import { canCompact, promptHistory } from "../sessionActions";
import { sendInType, typeIdFor, typeSessionOnScreen, types } from "../savedTypes";
import { canUseTaskRecipes, taskRecipes } from "../taskRecipes";
import { engineCall, engineCommands, loadEngineCommands } from "../engineTools";
import { matchSlash, slashCommands, type SlashCommand } from "../slashCommands";
import { runTyped, typedCommand } from "../commandRunner";
import CommandPanel from "./CommandPanel.vue";
import { editQueued, enqueuePrompt, markTurnAborted, queueFor, removeQueued, sendQueuedNext } from "../queue";
import { mention } from "../workspace";
import { isLegacyFunctionKey, normalizeArrowKey, sanitizeEditable } from "../macKeys";
import { inTerminal } from "../keysGuard";
import { denyKeyLabel, isDenyKey, mayAnswerFrom } from "../approvalKeys";
import Icon from "./Icon.vue";
import ModelPicker from "./ModelPicker.vue";
import ApprovalMenu from "./ApprovalMenu.vue";
import QuestionCard from "./QuestionCard.vue";
import ContextMeter from "./ContextMeter.vue";
import VoicePanel from "./VoicePanel.vue";

const props = defineProps<{ snap: OpenSnapshot }>();
const input = ref<HTMLTextAreaElement | null>(null);
const draft = computed(() => draftFor(props.snap));
const approvalMenu = ref<InstanceType<typeof ApprovalMenu> | null>(null);
const queue = computed(() => queueFor(props.snap));
const selected = ref(0);
const caret = ref(-1);
const paths = ref<string[]>([]);
function trackCaret() {
  const el = input.value;
  if (el && sanitizeEditable(el)) draft.value.text = el.value;
  caret.value = el && el.selectionStart === el.selectionEnd ? el.selectionStart : -1;
}
const token = computed(() => {
  const text = draft.value.text;
  if (caret.value < 0 || (text[caret.value] && !/\s/.test(text[caret.value]))) return null;
  return /(?:^|\s)@([^\s@`]*)$/.exec(text.slice(0, caret.value))?.[1] ?? null;
});
watch([token, draft], ([query], _, cleanup) => {
  paths.value = [];
  selected.value = 0;
  if (query === null) return;
  let current = true;
  const timer = setTimeout(async () => {
    try {
      const found = await workspaceFiles.search(query, 8);
      if (current) paths.value = found;
    } catch (e) { if (current) showToast(`File search failed: ${e}`); }
  }, 150);
  cleanup(() => { current = false; clearTimeout(timer); });
}, { flush: "sync" });
function pickPath(path: string) {
  if (token.value === null || !paths.value.includes(path)) return;
  const start = caret.value - token.value.length - 1;
  const insert = mention(path) + " ";
  draft.value.text = draft.value.text.slice(0, start) + insert + draft.value.text.slice(caret.value);
  caret.value = -1;
  nextTick(() => { input.value?.focus(); input.value?.setSelectionRange(start + insert.length, start + insert.length); });
}

/** Slash commands: the terminal's, run the desktop way (see `slashCommands.ts`), plus task recipes. */
const modelPicker = ref<InstanceType<typeof ModelPicker> | null>(null);
const allCommands = computed<SlashCommand[]>(() => [
  ...slashCommands(
    { snap: props.snap, showApprovals: () => approvalMenu.value?.show(), showModes: () => modelPicker.value?.showModes() },
    engineCommands.value,
  ),
  ...(canUseTaskRecipes(props.snap)
    ? taskRecipes().map((r) => ({ name: `/${r.id}`, detail: r.title, kind: "desktop" as const, run: () => { draft.value.text = r.prompt; } }))
    : []),
]);
watch(() => [props.snap.workspace.root, props.snap.connection.state] as const, () => void loadEngineCommands(), { immediate: true });

/** Silently pre-warm an idle Dial-managed local server as the user drafts their first prompt. */
const prewarmedKey = ref("");
watch(
  () => [
    draft.value.text.trim().length >= 3,
    props.snap.workspace.root,
    props.snap.sessionId,
    props.snap.connection.state,
    props.snap.turnRunning,
    props.snap.entries.length,
    props.snap.model?.managedLocal,
    props.snap.model?.resident,
    props.snap.model?.modelId,
  ] as const,
  ([typing, root, sessionId, conn, running, entries, managedLocal, resident, modelId], _, cleanup) => {
    if (!typing || conn !== "live" || running || entries > 0 || !managedLocal || resident || !modelId) return;
    const key = `${root}\0${sessionId ?? ""}\0${modelId}`;
    if (prewarmedKey.value === key) return;
    const timer = setTimeout(() => {
      prewarmedKey.value = key;
      void engineCall("warm_local_model", {}, root).catch(() => {});
    }, 600);
    cleanup(() => clearTimeout(timer));
  },
);
const approval = computed(() => props.snap.approvals[0] ?? null);
/** The built-in command a typed `/name args` will run (shown in the key hint), else `""`. */
const typedName = computed(() => {
  const text = draft.value.text;
  if (draft.value.attachments.length || !text.startsWith("/") || /^\/\S*$/.test(text)) return "";
  return typedCommand(text, engineCommands.value)?.name ?? "";
});
/** Esc closed the menu for this text; typing again opens it. */
const slashDismissed = ref(false);
// Only a lone `/word` opens the menu; `/word ` (with a space) is text to send, so a custom
// command or skill with its task goes to the engine, which expands it.
const slash = computed(() => {
  const text = draft.value.text;
  if (slashDismissed.value || !draftReady(draft.value) || draft.value.attachments.length || !/^\/\S*$/.test(text)) return [];
  return matchSlash(allCommands.value, text).slice(0, 60);
});
/** Voice misheard: the request was stopped, so put what was heard in the box to fix and send. */
function editHeard(text: string) {
  restoreInput(draft.value, { text, attachments: [] });
  focus();
}
const attachmentHelp = `Choose, paste or drop up to ${MAX_ATTACHMENTS} files (${MAX_FILE_BYTES / (1024 * 1024)} MiB each). Copies may be saved in this project when you send.`;
const hasContent = computed(() => !!draft.value.text.trim() || draft.value.attachments.length > 0);
const ready = computed(() => draftReady(draft.value) && draft.value.attachments.length <= MAX_ATTACHMENTS);
const canQueue = computed(() => props.snap.connection.state === "live" && !!props.snap.sessionId && props.snap.turnRunning && ready.value && !queue.value.loading && !queue.value.saving && !queue.value.sending && hasContent.value);
async function queueDraft() {
  if (!canQueue.value) return;
  const target = draft.value;
  const text = target.text;
  const attached = [...target.attachments];
  const finish = beginSend(target);
  focus();
  let queued = false;
  try {
    queued = await enqueuePrompt(props.snap, text, attached);
  } finally {
    finish(queued);
  }
}
const canSendToWorker = computed(() => props.snap.connection.state === "live" && ready.value && hasContent.value);
async function sendToWorker() {
  if (!canSendToWorker.value) return;
  const target = draft.value;
  const text = target.text;
  const attached = [...target.attachments];
  const finish = beginSend(target);
  focus();
  let sent = false;
  try {
    sent = await engine.sendToWorker(text, attached, { workspaceRoot: props.snap.workspace.root });
    if (sent) showToast("Sent to an independent worker");
  } finally {
    finish(sent);
  }
}
/** While a turn runs, typed text goes to it as guidance (files cannot). */
const steering = computed(
  () => props.snap.turnRunning && props.snap.connection.state === "live" && !!draft.value.text.trim() && !draft.value.attachments.length,
);
const queueKey = computed(() => (props.snap.platform === "macos" ? "⌘ Enter" : "Ctrl+Enter"));
/** What Enter does while a turn runs: a note now, or (with files, which a note cannot carry) a
 *  queued prompt. Kept short: the bar shows one hint; the rest is in its tooltip and the ⌘/ sheet. */
const runningHint = computed(() => {
  if (!props.snap.turnRunning || draft.value.sending || !hasContent.value || slash.value.length || paths.value.length) return "";
  if (draft.value.attachments.length) return canQueue.value ? "queue" : "";
  return steering.value ? "add note" : "";
});
const runningTitle = computed(() => draft.value.attachments.length
  ? "Enter queues this for the next turn: files can't go in a note"
  : `Enter adds a note to this turn; ${queueKey.value} queues it for the next turn instead`);
const starting = computed(() => props.snap.connection.state === 'starting' && !!props.snap.startupDraftKey && !ui.typeOpen);
const canSend = computed(() => ready.value && (slash.value.length > 0 || steering.value || (
  (props.snap.canSend || starting.value) && hasContent.value
)));
/** Files are being dragged over the window. */
const dropping = ref(false);
/** A note under the composer while the engine starts or in the design preview. When the engine is
 *  down, the banner above the conversation says so once, with Reconnect; this stays quiet. */
const caption = computed(() => {
  const c = props.snap.connection;
  if (c.state === 'starting') return draft.value.pending ? 'Queued · sends when Dial is ready' : `Starting Dial in ${props.snap.workspace.display}…`;
  if (c.state === "preview") return "Design preview · prompts are not sent";
  return "";
});
/** The engine is down: the composer looks unavailable. */
const offline = computed(() => props.snap.connection.state === "failed" || props.snap.connection.state === "ended");

function resize() {
  const el = input.value;
  if (!el) return;
  el.style.height = "auto";
  el.style.height = `${Math.min(el.scrollHeight, Math.max(220, Math.round(window.innerHeight * 0.4)))}px`;
}

function focus() {
  nextTick(() => {
    input.value?.focus();
    resize();
  });
}

function runCommand(command: SlashCommand) {
  if (command.insert) {
    draft.value.text = `${command.name} `;
    focus();
    return;
  }
  if (!command.run) {
    showToast(`${command.name} is only in the terminal for now: ${command.reason ?? "no desktop control yet"}`);
    return;
  }
  draft.value.text = "";
  command.run();
  if (!["/model", "/think", "/mode", "/approval", "/switch", "/effort"].includes(command.name)) focus();
}

async function send() {
  const target = draft.value;
  if (!canSend.value) return;
  const original = target.text;
  const text = original.trim();
  if (slash.value.length) return runCommand(slash.value[selected.value] ?? slash.value[0]);
  // `/goal set …`, `/effort high`, `/doctor`: the desktop runs built-in commands itself, as the
  // terminal does; custom commands and skills still go to the engine, which expands them.
  const typed = !target.attachments.length && typedCommand(text, engineCommands.value);
  if (typed) {
    const bare = allCommands.value.find((c) => c.name === `/${typed.name}` && c.run);
    runTyped(props.snap, typed.name, typed.rest, engineCommands.value, bare?.run);
    if (target.text === original) target.text = "";
    return;
  }
  const attached = [...target.attachments];
  const steer = steering.value;
  const finish = beginSend(target);
  focus();
  let accepted = false;
  try {
    const scope = { workspaceRoot: props.snap.workspace.root, sessionId: props.snap.sessionId, typeId: typeIdFor(props.snap.sessionId) };
    if (starting.value) {
      const sessionId = await startupDestination(props.snap, () => ui.typeOpen ? null : snapshot.value, lifetime.signal);
      if (!sessionId) { showToast('Startup prompt was not sent. It is still in its draft.'); return; }
      accepted = await engine.send(text, attached, { ...scope, sessionId, typeId: typeIdFor(sessionId) });
      return;
    }
    // In an open type, a prompt or steer goes to the type's own conversation.
    accepted = steer
      ? await engine.steer(text, scope)
      : ui.typeOpen && !typeSessionOnScreen()
        ? await sendInType(ui.typeOpen, text, attached)
        : await engine.send(text, attached, scope);
  } finally {
    finish(accepted);
  }
}

async function chooseFiles() {
  const target = draft.value;
  try {
    await readingFiles(target, async () => attachments.addPaths(target, await files.chooseFiles()));
  } catch (error) {
    showToast(`Could not open the file picker: ${error}`);
  }
  if (target === draft.value) focus();
}

async function paste(action: ReturnType<typeof clipboardAction>) {
  const target = draft.value;
  const before = target.attachments.length;
  const platform = props.snap.platform;
  await readingFiles(target, async () => {
    if (action.kind === 'files') {
      if (action.hasDirectory) showToast('Folders cannot be attached. Choose the files inside instead.');
      await attachments.addBlobs(target, action.files);
    } else if (action.kind === 'macImage') await attachments.addNativeClipboardImage(target);
    else if (action.kind === 'nativeFiles') await attachments.addNativeClipboardFiles(target, platform);
  });
  if (!unmounted && target === draft.value && target.attachments.length > before) focus();
}

function attachmentMenu(event: MouseEvent) {
  if (props.snap.platform !== 'macos') return void chooseFiles();
  openMenu(event, [
    { label: 'Choose files…', run: chooseFiles },
    { label: 'Paste files or image from clipboard', run: () => paste({ kind: 'nativeFiles' }) },
  ], event.currentTarget as HTMLElement);
}

/** Only intercept file pastes in the composer or transcript. Text keeps its normal behavior. */
function onPaste(event: ClipboardEvent) {
  const el = event.target;
  const inTranscript = el instanceof HTMLElement && !!el.closest('.main .scroller') && !el.closest('input, textarea, [contenteditable=true]');
  if (el !== input.value && !inTranscript || ui.settingsOpen || ui.paletteOpen || ui.shortcutsOpen || draft.value.sending) return;
  const action = clipboardAction(event, props.snap.platform);
  if (action.kind === 'text') return;
  event.preventDefault();
  void paste(action);
}

// Files dropped anywhere on the window attach to the draft.
let unlistenDrop: (() => void) | null = null;
const lifetime = new AbortController();
let unmounted = false;
onMounted(async () => {
  document.addEventListener('paste', onPaste);
  const unlisten = await getCurrentWebview()
    .onDragDropEvent(({ payload }) => {
      dropping.value = !ui.settingsOpen && (payload.type === "enter" || payload.type === "over");
      if (payload.type === "drop" && payload.paths.length && !ui.settingsOpen) {
        const target = draft.value;
        const paths = payload.paths;
        void readingFiles(target, () => attachments.addPaths(target, paths)).then(() => {
          if (target === draft.value) focus();
        });
      }
    })
    .catch(() => null);
  if (unmounted) unlisten?.();
  else unlistenDrop = unlisten;
});
onUnmounted(() => {
  document.removeEventListener('paste', onPaste);
  unmounted = true;
  lifetime.abort();
  unlistenDrop?.();
});

function onKey(event: KeyboardEvent) {
  if (event.isComposing) return;
  const key = normalizeArrowKey(event.key);
  const legacy = isLegacyFunctionKey(event.key);
  const count = paths.value.length || slash.value.length;
  if (key === "Enter" && (event.metaKey || event.ctrlKey) && props.snap.turnRunning) {
    event.preventDefault(); queueDraft(); return;
  }
  if (count && (key === "ArrowDown" || key === "ArrowUp")) {
    event.preventDefault(); selected.value = (selected.value + (key === "ArrowDown" ? 1 : -1) + count) % count;
    nextTick(() => input.value?.parentElement?.querySelector('.slash-row.active')?.scrollIntoView({ block: 'nearest' }));
    return;
  }
  if (paths.value.length && !event.shiftKey && (key === "Tab" || key === "Enter")) {
    event.preventDefault(); pickPath(paths.value[selected.value] ?? paths.value[0]); return;
  }
  if (token.value !== null && key === "Escape") {
    event.stopPropagation(); event.preventDefault(); caret.value = -1; return;
  }
  // Esc closes the command menu first; it does not also stop the running turn.
  if (slash.value.length && key === "Escape") {
    event.stopPropagation(); event.preventDefault(); slashDismissed.value = true; return;
  }
  if (key === "Enter" && !event.shiftKey && !event.isComposing) {
    event.preventDefault();
    if (props.snap.turnRunning && draft.value.attachments.length) queueDraft();
    else send();
  } else if (key === "Tab" && !event.shiftKey && slash.value.length) {
    event.preventDefault();
    const pick = slash.value[selected.value] ?? slash.value[0];
    draft.value.text = `${pick.name}${pick.insert ? " " : ""}`;
  } else if ((key === "ArrowUp" || key === "ArrowDown") && !event.altKey && !event.metaKey && !draft.value.attachments.length && browsable()) {
    // Like a shell: ↑ in an empty box walks back through this session's prompts, ↓ forward.
    recall(key === "ArrowUp" ? 1 : -1, event);
    if (!event.defaultPrevented && !draft.value.text) event.preventDefault();
  } else if (
    !draft.value.text &&
    !event.metaKey &&
    !event.ctrlKey &&
    !event.altKey &&
    (key === "ArrowLeft" || key === "ArrowRight" || key === "ArrowUp" || key === "ArrowDown")
  ) {
    // In an empty composer an arrow key has nowhere to move; preventing default stops macOS
    // WKWebView from advancing the caret or painting/inserting a placeholder box.
    event.preventDefault();
  } else if (legacy) {
    event.preventDefault();
  }
}

/** Position in the session's earlier prompts (0 = newest) while ↑/↓ browses them, else -1. */
const historyAt = ref(-1);
const history = computed(() => promptHistory(props.snap));
/** Browsing starts from an empty box and continues while the recalled text is untouched. */
function browsable() {
  const text = draft.value.text;
  return historyAt.value < 0 ? !text : text === history.value[historyAt.value];
}
function recall(step: 1 | -1, event: KeyboardEvent) {
  const next = historyAt.value + step;
  if (next < -1 || next >= history.value.length || (step < 0 && historyAt.value < 0)) return;
  event.preventDefault();
  historyAt.value = next;
  const text = next < 0 ? "" : history.value[next];
  draft.value.text = text;
  nextTick(() => input.value?.setSelectionRange(text.length, text.length));
}
watch(() => draft.value.text, (text) => {
  if (historyAt.value >= 0 && text !== history.value[historyAt.value]) historyAt.value = -1;
});
watch(draft, () => (historyAt.value = -1));

/** Requests the engine grants for this one request only: "always"/"project" would count as a denial. */
const ONE_TIME_TOOLS = new Set(["request_directory", "request_sensitive_directory", "batch"]);
const oneTime = computed(() => !!approval.value && ONE_TIME_TOOLS.has(approval.value.tool));
/** The folder a directory request names, from its "Canonical directory:" line. */
const requestedFolder = computed(() => {
  if (!approval.value?.tool.includes("directory")) return null;
  return /^Canonical directory: (.+)$/m.exec(approval.value.summary)?.[1]?.trim() ?? null;
});
function decide(decision: Decision) {
  if (!approval.value) return;
  const answeredByKey = !!allowButton.value?.closest(".approval")?.contains(document.activeElement);
  // Never send a reusable answer the engine would read as "no".
  engine.approve(approval.value.approvalId, oneTime.value && decision !== "deny" ? "allow" : decision);
  // The card goes away with its buttons; keep the keyboard in the composer.
  if (answeredByKey && props.snap.approvals.length <= 1) focus();
}
/** Allow every waiting request once (none "always"): the list above names them. */
function allowAll() {
  const answeredByKey = !!allowButton.value?.closest(".approval")?.contains(document.activeElement);
  for (const each of props.snap.approvals) engine.approve(each.approvalId, "allow");
  if (answeredByKey) focus();
}
/** Plain words for what a request lets Dial do; tools not listed keep their own name. */
const TOOL_ACTIONS: Record<string, string> = {
  bash: "run a command", run: "run a command", shell: "run a command", bg: "start a background job",
  edit: "edit files", write: "write a file", ast_edit: "edit code", delete: "delete files", move: "move files",
  web: "use the web", fetch: "fetch a web page", web_search: "search the web", batch: "run several tools at once",
};
const approvalAction = computed(() => (approval.value ? TOOL_ACTIONS[approval.value.tool.toLowerCase()] ?? null : null));
/** The lasting answers sit in a menu, so the card shows one clear yes and one clear no. Each one
 *  names what it keeps allowing, so the rule is visible before it is made. */
function scopeMenu(event: MouseEvent) {
  const what = approval.value?.tool ?? "this tool";
  openMenu(event, [
    { label: `Always allow ${what} in this project`, run: () => decide("project") },
    { label: `Always allow ${what} everywhere`, run: () => decide("always") },
  ], event.currentTarget as HTMLElement);
}
/** While a turn runs: the note goes now by default; queueing and independent workers wait in this menu. */
function noteMenu(event: MouseEvent) {
  openMenu(event, [
    { label: "Send note now", hint: "Enter", disabled: !canSend.value, run: () => void send() },
    { label: "Queue for next turn", hint: queueKey.value, disabled: !canQueue.value, run: () => void queueDraft() },
    { label: "Send to independent worker", disabled: !canSendToWorker.value, run: () => void sendToWorker() },
  ], event.currentTarget as HTMLElement);
}
const mac = computed(() => props.snap.platform === "macos");
const denyKey = computed(() => denyKeyLabel(mac.value));
/** ⌘⌫ / Ctrl+Backspace denies the request on screen, unless text is being edited or a terminal has the keys. */
function onApprovalKey(event: KeyboardEvent) {
  if (!approval.value || event.defaultPrevented || !isDenyKey(event, mac.value)) return;
  const active = document.activeElement;
  if (inTerminal(active) || !mayAnswerFrom(active instanceof HTMLElement ? active as HTMLElement & { value?: string } : null)) return;
  if (ui.settingsOpen || ui.paletteOpen || ui.menu) return;
  event.preventDefault();
  event.stopPropagation();
  decide("deny");
}
// Capture, so the sidebar's ⌫ (delete the picked sessions) never sees it.
onMounted(() => window.addEventListener("keydown", onApprovalKey, true));
onUnmounted(() => window.removeEventListener("keydown", onApprovalKey, true));

/** With a type open whose own conversation is not the one on screen, where a prompt goes. */
const sendingTo = computed(() => {
  if (!ui.typeOpen) return null;
  const open = types.list.find((t) => t.id === ui.typeOpen);
  if (!open || (ui.typeMode === "chat" && typeSessionOnScreen())) return null;
  return `${open.icon ?? ""} ${open.name}`.trim();
});
/** First line of an approval's summary, for the list of requests still waiting. */
function firstLine(text: string) {
  return text.split("\n").find((line) => line.trim())?.trim() ?? "";
}
/**
 * A new request takes keyboard focus to its Allow button, so it can be answered without the mouse;
 * not from a terminal, or from a field with text being typed (Enter there must not approve).
 */
const allowButton = ref<HTMLButtonElement | null>(null);
watch(() => approval.value?.approvalId, (id, was) => {
  if (!id || id === was) return;
  const active = document.activeElement;
  if (inTerminal(active)) return;
  if ((active instanceof HTMLInputElement || active instanceof HTMLTextAreaElement) && active.value.trim()) return;
  if (active instanceof HTMLElement && active.isContentEditable) return;
  void nextTick(() => allowButton.value?.focus({ preventScroll: true }));
});
// While a request waits the empty prompt box shrinks to one line, so the request and the
// conversation keep the room; it grows back as soon as there's text or the request is answered.
watch(() => !!approval.value, () => nextTick(resize));

watch(() => draft.value.text, () => { selected.value = 0; slashDismissed.value = false; nextTick(resize); });
watch(() => ui.focusComposer, focus);
onMounted(focus);
</script>

<template>
  <footer class="dock">
    <div class="column">
      <Transition name="pop">
        <section v-if="approval" :key="approval.approvalId" class="card approval" aria-label="Permission request">
          <div class="card-head" role="alert" aria-live="assertive">
            <span class="card-icon"><Icon name="shield" :size="14" /></span>
            <span v-if="requestedFolder">{{ approval.tool === 'request_sensitive_directory' ? 'Allow protected-file access in' : 'Give Dial access to' }} <strong class="mono">{{ requestedFolder }}</strong>?</span>
            <span v-else-if="approvalAction">Allow Dial to <strong>{{ approvalAction }}</strong>? <span class="tool-id mono">{{ approval.tool }}</span></span>
            <span v-else>Allow <strong>{{ approval.tool }}</strong>?</span>
          </div>
          <pre v-if="approval.summary" class="summary">{{ approval.summary }}</pre>
          <!-- One counter for the rest of the queue; opening it lists them. -->
          <details v-if="snap.approvals.length > 1" class="waiting">
            <summary>{{ snap.approvals.length - 1 }} more waiting</summary>
            <ol>
              <li v-for="other in snap.approvals.slice(1)" :key="other.approvalId" :title="other.summary">
                <strong>{{ other.tool }}</strong> <span class="truncate">{{ firstLine(other.summary) }}</span>
              </li>
            </ol>
          </details>
          <div class="actions">
            <button ref="allowButton" class="btn btn-primary" title="Allow this request (Enter while the button has focus)" @click="decide('allow')">{{ oneTime ? "Allow" : "Allow once" }}<kbd class="key" aria-hidden="true">↩</kbd></button>
            <button v-if="snap.approvals.length > 1" class="btn btn-secondary" :title="`Allow each of the ${snap.approvals.length} waiting requests once`" @click="allowAll">
              Allow all {{ snap.approvals.length }} once
            </button>
            <button
              v-if="!oneTime"
              class="btn btn-secondary"
              title="Keep allowing this kind of call, in this project or everywhere"
              aria-haspopup="menu"
              @click="scopeMenu"
            >
              Always allow…
            </button>
            <span v-else class="once-note">Applies to this request only</span>
            <button class="btn btn-danger deny" :title="`Deny this request (${denyKey})`" :aria-keyshortcuts="mac ? 'Meta+Backspace' : 'Control+Backspace'" @click="decide('deny')">Deny<kbd class="key" aria-hidden="true">{{ denyKey }}</kbd></button>
          </div>
        </section>
      </Transition>

      <Transition name="pop">
        <QuestionCard v-if="snap.question" :key="snap.question.id" :question="snap.question" />
      </Transition>
      <section v-if="queue.items.length" class="queue card" aria-label="Follow-up queue">
        <strong>{{ queue.paused ? 'Queue paused' : 'Queued follow-ups' }}</strong>
        <p v-if="queue.recovered" class="faint queue-recovery">Restored after restart. Check the transcript before sending a follow-up that may already have been sent.</p>
        <p v-else class="faint queue-recovery">{{ queue.paused ? 'Check the conversation before continuing this queue.' : 'Follow-ups send after this turn while this conversation is open. Switching conversations pauses the queue.' }}</p>
        <div v-for="item in queue.items" :key="item.id" class="queue-row">
          <span class="truncate" :title="item.text">{{ item.text || 'File prompt' }}{{ item.attachments.length ? ` · ${item.attachments.length} file${item.attachments.length === 1 ? '' : 's'}` : '' }}</span>
          <button class="icon-btn" title="Edit queued prompt" aria-label="Edit queued prompt" :disabled="queue.saving || queue.sending || !ready" @click="editQueued(snap, item.id)"><Icon name="pencil" :size="14" /></button>
          <button class="icon-btn" title="Remove queued prompt" aria-label="Remove queued prompt" :disabled="queue.saving || queue.sending" @click="removeQueued(snap, item.id)"><Icon name="cross" :size="14" /></button>
        </div>
        <button v-if="!snap.turnRunning" class="btn btn-secondary" :disabled="queue.loading || queue.saving || queue.sending || !snap.canSend" @click="sendQueuedNext(snap)">Send next</button>
        <button v-else-if="queue.paused" class="btn btn-secondary" :disabled="queue.loading || queue.saving || queue.sending || snap.connection.state !== 'live'" @click="queue.paused = false; queue.recovered = false">Resume after this turn</button>
      </section>
      <Transition name="pop">
        <VoicePanel v-if="snap.voice.phase !== 'off'" :voice="snap.voice" :turn-running="snap.turnRunning" @edit="editHeard" />
      </Transition>
      <section v-if="snap.localLoadFailure && snap.connection.state === 'live'" class="card" aria-label="Model load failed">
        <div class="card-head" role="alert"><Icon name="chip" :size="16" /><strong class="truncate" :title="snap.localLoadFailure.modelId">Couldn’t load {{ snap.localLoadFailure.modelId.split(/[\\/]/).pop() }}</strong></div>
        <pre class="summary">{{ snap.localLoadFailure.error }}</pre>
        <div class="actions">
          <button class="btn btn-secondary" :disabled="!snap.canSend || snap.turnRunning" @click="engine.setModel(snap, snap.localLoadFailure.provider, snap.localLoadFailure.modelId)">Retry load</button>
          <button class="btn btn-secondary" @click="ui.modelPickerOpen = true">Choose another model</button>
        </div>
      </section>
      <div class="composer" :class="{ dropping, offline }" @click="input?.focus()">
        <div v-if="dropping" class="drop-hint"><Icon name="file" :size="16" /> Drop files to attach</div>
        <Transition name="pop">
          <div v-if="slash.length" class="slash" @click.stop>
            <div class="slash-head">Commands</div>
            <button v-for="(c, i) in slash" :key="c.name" class="slash-row" :class="{ active: i === selected }" :title="c.reason ?? (c.usage ? `${c.name} ${c.usage}` : '')" @click="runCommand(c)" @mouseenter="selected = i">
              <span class="mono">{{ c.name }}<span v-if="c.usage" class="usage"> {{ c.usage }}</span></span>
              <span class="slash-detail truncate">{{ c.detail }}</span>
              <span v-if="c.kind !== 'desktop'" class="kind" :class="c.kind">{{ c.kind === 'terminal' ? 'Terminal' : c.kind === 'skill' ? 'Skill' : 'Custom' }}</span>
            </button>
            <div class="slash-foot"><kbd>↑</kbd><kbd>↓</kbd> choose <kbd>Tab</kbd> complete <kbd>Enter</kbd> run</div>
          </div>
        </Transition>
        <div v-if="paths.length" class="slash" aria-label="Matching files" @mousedown.prevent @click.stop>
          <div class="slash-head">Files</div>
          <button v-for="(path, i) in paths" :key="path" class="slash-row file-row" :class="{active: i === selected}" @click="pickPath(path)" @mouseenter="selected = i">
            <Icon name="file" :size="13" />
            <span class="mono truncate">{{ path }}</span>
          </button>
          <div class="slash-foot"><kbd>↑</kbd><kbd>↓</kbd> choose <kbd>Tab</kbd> insert <kbd>Esc</kbd> dismiss</div>
        </div>
        <div v-if="draft.attachments.length" class="thumbs" @click.stop>
          <div v-for="a in draft.attachments" :key="a.key" class="thumb" :class="{ 'file-chip': !a.url }" :title="`${a.name} · ${attachmentHelp}`">
            <button v-if="a.url" class="preview-thumb" :aria-label="`Preview ${a.name}`" :title="`Preview ${a.name}`" @click="viewAttachment({ name: a.name, url: a.url! })">
              <img :src="a.url" alt="" />
            </button>
            <template v-else><Icon name="file" :size="15" /><span class="truncate">{{ a.name }}</span></template>
            <button class="remove" :aria-label="`Remove ${a.name}`" :title="`Remove ${a.name}`" @click="draft.attachments = draft.attachments.filter((file) => file.key !== a.key)">
              <Icon name="cross" :size="11" />
            </button>
          </div>
        </div>
        <textarea
          ref="input"
          v-model="draft.text"
          aria-label="Prompt"
          data-prompt
          :rows="approval ? 1 : 2"
          spellcheck="true"
          autocorrect="off"
          autocapitalize="off"
          :placeholder="draft.pending ? 'Write your next message…' : snap.turnRunning ? 'Add a note for Dial while it works…' : ui.typeOpen ? `Ask Dial to change ${types.list.find((t) => t.id === ui.typeOpen)?.name ?? 'this type'}…` : snap.entries.length ? 'Message Dial…' : 'What would you like to do in this project?'"
          :readonly="draft.sending && !draft.pending"
          @keydown="onKey"
          @input="trackCaret" @click="trackCaret" @select="trackCaret" @keyup.left="trackCaret" @keyup.right="trackCaret"
        />
        <div class="bar" @click.stop>
          <button
            class="attach"
            aria-label="Attach files"
            :title="attachmentHelp"
            @click="attachmentMenu"
          >
            <Icon name="file" :size="15" />
          </button>
          <ModelPicker ref="modelPicker" :snap="snap" />
          <ApprovalMenu ref="approvalMenu" :snap="snap" />
          <span v-if="sendingTo" class="sending-to" :title="`Prompts go to ${sendingTo}'s own conversation, not the chat that was on screen`">
            Sending to <strong class="truncate">{{ sendingTo }}</strong>
          </span>
          <!-- One short hint in leftover room (the rest is in its tooltip and the ⌘/ sheet); when it doesn't fit it wraps out of sight instead of pushing the send button onto its own row. -->
          <span class="hint-slot">
            <span v-if="draft.loading" class="keyhint status" role="status">Restoring draft…</span>
            <span v-else-if="draft.reading" class="keyhint status" role="status">Preparing attachments…</span>
            <span v-else-if="draft.attachments.length > MAX_ATTACHMENTS" class="keyhint status" role="status">Remove files to send {{ MAX_ATTACHMENTS }} or fewer.</span>
            <span v-else-if="typedName && !slash.length && !paths.length" class="keyhint"><kbd>Enter</kbd> run /{{ typedName }}</span>
            <span v-else-if="!draft.sending && !snap.turnRunning && hasContent && !slash.length && !paths.length" class="keyhint" title="Enter sends; Shift+Enter starts a new line"><kbd>⇧ Enter</kbd> new line</span>
            <span v-else-if="runningHint" class="keyhint always" role="status" :title="runningTitle"><kbd>Enter</kbd> {{ runningHint }}</span>
          </span>
          <ContextMeter
            v-if="snap.context || snap.connection.state === 'live'"
            :key="JSON.stringify([snap.workspace.root, snap.sessionId, snap.model?.modelId, snap.model?.provider])"
            :context="snap.context"
            :live="snap.connection.state === 'live'"
            :turn-running="snap.turnRunning"
            :can-compact="canCompact(snap)"
            :local="snap.model?.local ?? false"
            @compact="engine.compact()"
          />
          <button
            v-if="snap.voice.phase === 'off' && snap.connection.state === 'live' && !(snap.turnRunning && steering)"
            class="attach mic"
            :disabled="snap.turnRunning"
            aria-label="Talk to Dial"
            :title="snap.turnRunning ? 'Voice mode starts once Dial finishes' : 'Talk to Dial: speak, and hear the answer'"
            @click="engine.voiceStart()"
          >
            <Icon name="mic" :size="16" />
          </button>
          <button v-if="snap.turnRunning" class="send stop" title="Stop the running turn (Esc)" aria-label="Stop the running turn" @click.stop="markTurnAborted(snap); engine.abort()">
            <Icon name="stop" :size="14" />
          </button>
          <span v-if="snap.turnRunning && steering" class="split">
            <button class="send labeled" :disabled="!canSend" title="Send note now (Enter)" @click.stop="send">Send note</button>
            <button
              class="send caret"
              :disabled="!canQueue && !canSendToWorker"
              :title="`More ways to send (queue: ${queueKey})`"
              aria-label="More ways to send"
              aria-haspopup="menu"
              @click.stop="noteMenu"
            >
              <Icon name="chevronDown" :size="12" />
            </button>
          </span>
          <span
            v-else-if="hasContent && snap.turnRunning && snap.sessionId && snap.connection.state === 'live'"
            class="split"
          >
            <button
              class="send labeled"
              :disabled="!canQueue"
              title="Queue for the next turn (Enter): files can't go in a note"
              @click.stop="queueDraft"
            >
              Queue for next turn
            </button>
            <button
              class="send caret"
              :disabled="!canQueue && !canSendToWorker"
              title="More ways to send"
              aria-label="More ways to send"
              aria-haspopup="menu"
              @click.stop="noteMenu"
            >
              <Icon name="chevronDown" :size="12" />
            </button>
          </span>
          <button
            v-else-if="!snap.turnRunning"
            class="send"
            :disabled="!canSend"
            title="Send (Enter)"
            aria-label="Send"
            @click.stop="send"
          >
            <Icon name="arrowUp" :size="16" />
          </button>
        </div>
      </div>
      <div v-if="draft.sending && !snap.turnRunning && snap.connection.state !== 'starting'" class="caption" role="status">Submitting prompt…</div>
      <div v-if="caption" class="caption" role="status">
        <span class="truncate">{{ caption }}</span>
      </div>
    </div>
    <CommandPanel />
  </footer>
</template>

<style scoped>
.dock {
  flex: none;
  padding: 0 var(--space-xl) 16px;
  scrollbar-gutter: stable both-edges;
}
.column {
  max-width: var(--content);
  margin: 0 auto;
  display: flex;
  flex-direction: column;
  gap: var(--space-sm);
}
.card {
  padding: 12px 14px;
  border-radius: var(--radius-lg);
  background: var(--glass);
  -webkit-backdrop-filter: var(--glass-blur);
  backdrop-filter: var(--glass-blur);
  border: 1px solid var(--border-strong);
}
.card.approval {
  border-color: color-mix(in srgb, var(--warning) 45%, var(--border-strong));
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--warning) 10%, transparent), 0 8px 28px rgb(0 0 0 / 22%);
}
/* A request right above the prompt box joins it as one panel: no gap, one shared edge, so the
   two read as "answer this, or type instead" and take less height than two floating cards. */
.card.approval:has(+ .composer) {
  margin-bottom: calc(-1 * var(--space-sm));
  border-bottom: none;
  border-bottom-left-radius: 0;
  border-bottom-right-radius: 0;
  box-shadow: none;
}
.card.approval + .composer {
  border-color: color-mix(in srgb, var(--warning) 45%, var(--border-strong));
  border-top-color: var(--border);
  border-top-left-radius: 0;
  border-top-right-radius: 0;
  padding-top: 10px;
}
.card.approval + .composer:focus-within {
  border-color: var(--focus-border);
  border-top-color: var(--border);
}
.card-icon {
  display: grid;
  place-items: center;
  width: 24px;
  height: 24px;
  border-radius: 50%;
  background: color-mix(in srgb, var(--warning) 16%, transparent);
  color: var(--warning);
}
.card-head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 14px;
  color: var(--text-muted);
}
.card-head strong {
  font-weight: 600;
  color: var(--text);
}
.once-note { align-self: center; font-size: var(--text-sm); color: var(--text-faint); }
.actions .key {
  margin-left: 6px;
  padding: 0 4px;
  border: 0;
  background: color-mix(in srgb, currentColor 14%, transparent);
  color: inherit;
  font-size: 10.5px;
  opacity: 0.7;
}
.sending-to {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  max-width: 220px;
  margin-left: 4px;
  padding: 2px 8px;
  border-radius: 999px;
  border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
  color: var(--text-muted);
  font-size: var(--text-xs);
  white-space: nowrap;
}
.sending-to strong { min-width: 0; color: var(--accent); font-weight: 600; }
.waiting { margin-top: 8px; font-size: var(--text-sm); color: var(--text-muted); }
.waiting summary { cursor: pointer; color: var(--text-faint); }
.waiting summary:hover { color: var(--text); }
.waiting ol { margin: 6px 0 0; padding: 0; list-style: none; max-height: 120px; overflow-y: auto; }
.waiting li { display: flex; gap: 6px; min-width: 0; }
.waiting li strong { flex: none; font-weight: 600; color: var(--text); }
.card-head .mono { font-family: var(--font-mono); font-weight: 500; overflow-wrap: anywhere; }
.card-head .tool-id { margin-left: 4px; font-size: var(--text-xs); color: var(--text-faint); }
.summary {
  margin: 8px 0 0;
  padding: 8px 12px;
  border-radius: var(--radius-sm);
  background: var(--bubble);
  font: var(--text-mono) / 1.55 var(--font-mono);
  color: var(--text-muted);
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 160px;
  overflow: auto;
  user-select: text;
  -webkit-user-select: text;
}
.actions {
  display: flex;
  gap: 6px;
  margin-top: 10px;
  flex-wrap: wrap;
}
/* Right-aligned on whichever line it lands, so a wrapped Deny doesn't sit under Allow. */
.actions .deny {
  margin-left: auto;
}
.composer {
  position: relative;
  padding: 14px 10px 10px 18px;
  border-radius: var(--radius-xl);
  background: var(--glass);
  -webkit-backdrop-filter: var(--glass-blur);
  backdrop-filter: var(--glass-blur);
  border: 1px solid var(--border-strong);
  box-shadow: 0 4px 20px rgb(0 0 0 / 18%);
  cursor: text;
  transition:
    border-color 140ms var(--ease),
    opacity 160ms var(--ease);
}
.composer.offline {
  opacity: 0.6;
  border-style: dashed;
}
.composer.dropping {
  border-color: var(--accent);
  border-style: dashed;
}
.drop-hint {
  position: absolute;
  inset: 0;
  z-index: 10;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  border-radius: inherit;
  background: color-mix(in srgb, var(--surface) 88%, transparent);
  color: var(--accent);
  font-weight: 500;
  pointer-events: none;
}
.thumbs {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-bottom: 10px;
  cursor: default;
}
.thumb {
  position: relative;
  width: 56px;
  height: 56px;
  border-radius: var(--radius-md);
  border: 1px solid var(--border);
  background: var(--bg);
}
.file-chip { display: inline-flex; align-items: center; align-self: center; gap: 6px; width: auto; max-width: min(100%, 220px); height: auto; padding: 7px 12px; font-size: var(--text-sm); }
.file-chip svg { flex: none; }
.preview-thumb { display: block; width: 100%; height: 100%; padding: 0; border-radius: inherit; cursor: zoom-in; }
.thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  border-radius: inherit;
}
.remove {
  position: absolute;
  top: -6px;
  right: -6px;
  display: grid;
  place-items: center;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: var(--raised);
  border: 1px solid var(--border-strong);
  color: var(--text-muted);
  opacity: 0.75;
  transition: opacity 120ms var(--ease);
}
.thumb:hover .remove,
.remove:focus-visible {
  opacity: 1;
}
.remove:hover {
  color: var(--text);
}
.attach {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  border-radius: var(--radius-sm);
  color: var(--text-muted);
  transition:
    background 120ms var(--ease),
    color 120ms var(--ease);
}
.attach:hover:not(:disabled) {
  background: var(--hover);
  color: var(--text);
}
.attach:disabled {
  opacity: 0.4;
}
/* The mic shows while the composer is in use, so the idle bar stays quiet. */
.mic {
  width: 32px;
  height: 32px;
  margin-right: 4px;
  border-radius: 50%;
  transition: opacity 140ms var(--ease);
}
.composer:not(:hover):not(:focus-within) .mic {
  opacity: 0;
}
.slash {
  position: absolute;
  left: 8px;
  right: 8px;
  bottom: calc(100% + var(--pop-gap));
  z-index: 20;
  padding: 6px;
  border-radius: var(--radius-md);
  background: var(--raised);
  box-shadow: var(--shadow-pop);
}
.slash-head {
  padding: 4px 10px 6px;
  font-size: var(--text-xs);
  font-weight: 500;
  color: var(--text-faint);
}
.slash-foot {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-top: 4px;
  padding: 6px 10px 2px;
  border-top: 1px solid var(--border);
  font-size: var(--text-xs);
  color: var(--text-faint);
}
.slash-foot kbd { margin-left: 4px; }
.slash-foot kbd:first-child { margin-left: 0; }
.file-row {
  align-items: center;
  gap: 8px;
  color: var(--text-muted);
}
.file-row .mono { color: var(--text); font-size: var(--text-mono); }
.hint-slot {
  flex: 1 1 0;
  min-width: 0;
  height: 32px;
  display: flex;
  flex-wrap: wrap;
  align-content: flex-start;
  overflow: hidden;
}
/* Shares the first line with the hint; a hint wider than what's left wraps to the clipped second line. */
.hint-slot::before {
  content: "";
  flex: 1 0 0;
  height: 32px;
}
.keyhint {
  display: none;
  align-items: center;
  gap: 4px;
  height: 32px;
  margin-right: 8px;
  font-size: var(--text-xs);
  color: var(--text-faint);
  white-space: nowrap;
}
/* Status lines stay visible however narrow the bar: they come first and end in an ellipsis. */
.keyhint.status {
  display: block;
  order: -1;
  min-width: 0;
  max-width: 100%;
  margin-left: auto;
  line-height: 32px;
  overflow: hidden;
  text-overflow: ellipsis;
}
.composer:focus-within .keyhint:not(.status), .keyhint.always { display: inline-flex; }
.slash, .queue { max-height: 30vh; overflow-y: auto; }
.queue-recovery { margin: 3px 0 6px; }
.queue-row { display: flex; align-items: center; gap: 6px; }
.queue-row span { flex: 1; }
.slash-row {
  display: flex;
  align-items: baseline;
  gap: 12px;
  width: 100%;
  padding: 7px 10px;
  border-radius: var(--radius-sm);
  text-align: left;
}
.slash-row.active,
.slash-row:hover {
  background: var(--hover);
}
.slash-row .mono {
  color: var(--text);
}
.slash-detail {
  flex: 1;
  min-width: 0;
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.slash-row .mono {
  flex: none;
}
.slash-row .usage {
  color: var(--text-faint);
}
.slash-row.off .mono,
.slash-row.off .slash-detail {
  opacity: 0.55;
}
.slash-row .kind {
  flex: none;
  align-self: center;
  padding: 0 6px;
  border-radius: 999px;
  font-size: 10.5px;
  line-height: 17px;
  color: var(--text-faint);
  border: 1px solid var(--border);
}
.slash-row .kind.skill {
  color: var(--accent);
  border-color: color-mix(in srgb, var(--accent) 40%, transparent);
}
.composer:focus-within {
  border-color: var(--focus-border);
}
textarea {
  display: block;
  width: 100%;
  max-height: max(220px, 40vh);
  padding: 0 6px 0 0;
  border: none;
  outline: none;
  resize: none;
  background: none;
  color: var(--text);
  font: 14.5px/1.55 var(--font-ui);
}
textarea::placeholder {
  color: var(--text-faint);
}
.bar {
  display: flex;
  align-items: center;
  gap: 2px;
  /* Pulls the first 28px button out so its icon, not its hover box, lines up with the text. */
  margin: 8px 0 0 -6px;
  cursor: default;
  /* ModelPicker drops its effort label when this gets narrow. */
  container: composer-bar / inline-size;
}
.bar > :not(.hint-slot):not(.picker):not(.sending-to) {
  flex: none;
}
.send {
  display: grid;
  place-items: center;
  width: 32px;
  height: 32px;
  border-radius: var(--radius-md);
  background: var(--accent);
  color: var(--on-accent);
  transition:
    background 120ms var(--ease),
    opacity 120ms var(--ease),
    transform 90ms var(--ease);
}
.send.labeled { width: auto; padding: 0 10px; font-size: var(--text-sm); }
.split { display: inline-flex; gap: 1px; }
.split .send.labeled { border-radius: var(--radius-md) 0 0 var(--radius-md); }
.split .send.caret { width: 22px; border-radius: 0 var(--radius-md) var(--radius-md) 0; }
.send:active:not(:disabled) {
  transform: scale(0.92);
}
.send:hover:not(:disabled) {
  background: var(--accent-hover);
}
.send:disabled {
  opacity: 0.35;
}
.send.stop {
  background: var(--text);
  color: var(--bg);
}
.send.stop:hover {
  background: var(--text-strong);
}
/* Status under the composer. Over a background it sits on a soft glass pill so it stays legible
 * on a busy picture; on a plain window the pill is invisible. */
.caption {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 10px;
  min-width: 0;
  max-width: 100%;
  width: fit-content;
  margin-inline: auto;
  padding: 2px 10px;
  border-radius: 999px;
  background: var(--glass-soft);
  -webkit-backdrop-filter: var(--glass-blur);
  backdrop-filter: var(--glass-blur);
  font-size: var(--text-sm);
  color: var(--text-muted);
}

.pop-enter-active,
.pop-leave-active {
  transition:
    opacity 180ms var(--ease),
    transform 180ms var(--ease);
}
.pop-enter-from,
.pop-leave-to {
  opacity: 0;
  transform: translateY(6px);
}
</style>
