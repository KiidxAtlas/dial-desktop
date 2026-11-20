<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { Decision, OpenSnapshot } from "../types";
import { engine, files, showToast, workspaceFiles } from "../bridge";
import * as attachments from "../attachments";
import { ui } from "../ui";
import { draftFor } from "../drafts";
import { canCompact, promptHistory } from "../sessionActions";
import { canUseTaskRecipes, taskRecipes } from "../taskRecipes";
import { engineCommands, loadEngineCommands } from "../engineTools";
import { matchSlash, slashCommands, type SlashCommand } from "../slashCommands";
import { runTyped, typedCommand } from "../commandRunner";
import CommandPanel from "./CommandPanel.vue";
import { editQueued, enqueuePrompt, markTurnAborted, queueFor, removeQueued, sendQueuedNext } from "../queue";
import { mention } from "../workspace";
import Icon from "./Icon.vue";
import ModelPicker from "./ModelPicker.vue";
import ApprovalMenu from "./ApprovalMenu.vue";
import QuestionCard from "./QuestionCard.vue";
import ContextMeter from "./ContextMeter.vue";
import VoicePanel from "./VoicePanel.vue";
import ModesMenu from "./ModesMenu.vue";

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
const modesMenu = ref<InstanceType<typeof ModesMenu> | null>(null);
const allCommands = computed<SlashCommand[]>(() => [
  ...slashCommands(
    { snap: props.snap, showApprovals: () => approvalMenu.value?.show(), showModes: () => modesMenu.value?.show() },
    engineCommands.value,
  ),
  ...(canUseTaskRecipes(props.snap)
    ? taskRecipes.map((r) => ({ name: `/${r.id}`, detail: r.title, kind: "desktop" as const, run: () => { draft.value.text = r.prompt; } }))
    : []),
]);
watch(() => [props.snap.workspace.root, props.snap.connection.state] as const, () => void loadEngineCommands(), { immediate: true });

const approval = computed(() => props.snap.approvals[0] ?? null);
/** The built-in command a typed `/name args` will run (shown in the key hint), else `""`. */
const typedName = computed(() => {
  const text = draft.value.text;
  if (draft.value.attachments.length || !text.startsWith("/") || /^\/\S*$/.test(text)) return "";
  return typedCommand(text, engineCommands.value)?.name ?? "";
});
// Only a lone `/word` opens the menu; `/word ` (with a space) is text to send, so a custom
// command or skill with its task goes to the engine, which expands it.
const slash = computed(() => {
  const text = draft.value.text;
  if (draft.value.attachments.length || !/^\/\S*$/.test(text)) return [];
  return matchSlash(allCommands.value, text).slice(0, 60);
});
/** Voice misheard: the request was stopped, so put what was heard in the box to fix and send. */
function editHeard(text: string) {
  draft.value.text = text;
  focus();
}
/** `false` only when the model is known not to take images; unknown models may try. */
const imagesAllowed = computed(() => props.snap.model?.acceptsImages !== false);
const hasContent = computed(() => !!draft.value.text.trim() || draft.value.attachments.length > 0);
const canQueue = computed(() => props.snap.connection.state === "live" && !!props.snap.sessionId && props.snap.turnRunning && !draft.value.sending && !queue.value.loading && !queue.value.saving && !queue.value.sending && hasContent.value && (!draft.value.attachments.length || imagesAllowed.value));
async function queueDraft() {
  if (!canQueue.value) return;
  const target = draft.value;
  const text = target.text;
  const images = [...target.attachments];
  target.sending = true;
  try {
    if (!(await enqueuePrompt(props.snap, text, images))) return;
    if (target.text === text) target.text = "";
    const keys = new Set(images.map((image) => image.key));
    target.attachments = target.attachments.filter((image) => !keys.has(image.key));
  } finally {
    target.sending = false;
    if (target === draft.value) focus();
  }
}
/** While a turn runs, typed text goes to it as guidance (images cannot). */
const steering = computed(
  () => props.snap.turnRunning && props.snap.connection.state === "live" && !!draft.value.text.trim() && !draft.value.attachments.length,
);
const canSend = computed(() => !draft.value.sending && (slash.value.length > 0 || steering.value || (
  props.snap.canSend && hasContent.value && (imagesAllowed.value || !draft.value.attachments.length)
)));
/** Files are being dragged over the window. */
const dropping = ref(false);
const caption = computed(() => {
  const c = props.snap.connection;
  switch (c.state) {
    case "live":
      return props.snap.workspace.display;
    case "starting":
      return `Starting Dial in ${props.snap.workspace.display}…`;
    case "preview":
      return "Design preview · prompts are not sent";
    case "failed":
      if (c.problem === "missing") return "No Dial engine found; install it from the sidebar";
      if (c.problem === "incompatible") return "This Dial engine is too old or too new for the app; update from the sidebar";
      return "The Dial engine could not start; see the status in the sidebar";
    case "ended":
      return "The Dial engine stopped; reconnect from the sidebar";
  }
  return "";
});

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
  const images = [...target.attachments];
  target.sending = true;
  try {
    const scope = { workspaceRoot: props.snap.workspace.root, sessionId: props.snap.sessionId };
    const accepted = steering.value ? await engine.steer(text, scope) : await engine.send(text, images, scope);
    if (!accepted) return;
    if (target.text === original) target.text = "";
    const sentKeys = new Set(images.map((a) => a.key));
    target.attachments = target.attachments.filter((a) => !sentKeys.has(a.key));
  } finally {
    target.sending = false;
    if (target === draft.value) focus();
  }
}

async function chooseImages() {
  const target = draft.value;
  try {
    await attachments.addPaths(target, await files.chooseImages());
  } catch (error) {
    showToast(`Could not open the file picker: ${error}`);
  }
  if (target === draft.value) focus();
}

/** Attach clipboard images without interfering with normal text paste. */
function onPaste(event: ClipboardEvent) {
  if (!imagesAllowed.value) return;
  const clipboard = event.clipboardData;
  const images = Array.from(clipboard?.files ?? []).filter((f) => f.type.startsWith("image/"));
  if (!images.length) {
    for (const item of Array.from(clipboard?.items ?? [])) {
      if (item.kind !== "file") continue;
      const file = item.getAsFile();
      if (file?.type.startsWith("image/")) images.push(file);
    }
  }
  const target = draft.value;
  if (images.length) {
    event.preventDefault();
    void attachments.addBlobs(target, images);
  } else if (event.isTrusted && !clipboard?.getData("text/plain")) {
    // WKWebView can omit native clipboard images from both DOM lists.
    void attachments.addNativeClipboardImage(target);
  }
}

// Files dropped anywhere on the window attach to the draft.
let unlistenDrop: (() => void) | null = null;
let unmounted = false;
onMounted(async () => {
  const unlisten = await getCurrentWebview()
    .onDragDropEvent(({ payload }) => {
      dropping.value = imagesAllowed.value && !ui.settingsOpen && (payload.type === "enter" || payload.type === "over");
      if (payload.type === "drop" && payload.paths.length && imagesAllowed.value && !ui.settingsOpen) {
        const target = draft.value;
        void attachments.addPaths(target, payload.paths).then(() => {
          if (target === draft.value) focus();
        });
      }
    })
    .catch(() => null);
  if (unmounted) unlisten?.();
  else unlistenDrop = unlisten;
});
onUnmounted(() => {
  unmounted = true;
  unlistenDrop?.();
});

function onKey(event: KeyboardEvent) {
  if (event.isComposing) return;
  const count = paths.value.length || slash.value.length;
  if (event.key === "Enter" && (event.metaKey || event.ctrlKey) && props.snap.turnRunning) {
    event.preventDefault(); queueDraft(); return;
  }
  if (count && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
    event.preventDefault(); selected.value = (selected.value + (event.key === "ArrowDown" ? 1 : -1) + count) % count;
    nextTick(() => input.value?.parentElement?.querySelector('.slash-row.active')?.scrollIntoView({ block: 'nearest' }));
    return;
  }
  if (paths.value.length && !event.shiftKey && (event.key === "Tab" || event.key === "Enter")) {
    event.preventDefault(); pickPath(paths.value[selected.value] ?? paths.value[0]); return;
  }
  if (token.value !== null && event.key === "Escape") {
    event.stopPropagation(); event.preventDefault(); caret.value = -1; return;
  }
  if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
    event.preventDefault();
    send();
  } else if (event.key === "Tab" && !event.shiftKey && slash.value.length) {
    event.preventDefault();
    const pick = slash.value[selected.value] ?? slash.value[0];
    draft.value.text = `${pick.name}${pick.insert ? " " : ""}`;
  } else if ((event.key === "ArrowUp" || event.key === "ArrowDown") && !event.altKey && !event.metaKey && !draft.value.attachments.length && browsable()) {
    // Like a shell: ↑ in an empty box walks back through this session's prompts, ↓ forward.
    recall(event.key === "ArrowUp" ? 1 : -1, event);
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
  // Never send a reusable answer the engine would read as "no".
  engine.approve(approval.value.approvalId, oneTime.value && decision !== "deny" ? "allow" : decision);
}

watch(() => draft.value.text, () => { selected.value = 0; nextTick(resize); });
watch(() => ui.focusComposer, focus);
onMounted(focus);
</script>

<template>
  <footer class="dock">
    <div class="column">
      <Transition name="pop">
        <section v-if="approval" :key="approval.approvalId" class="card approval">
          <div class="card-head">
            <span class="card-icon"><Icon name="shield" :size="14" /></span>
            <span v-if="requestedFolder">{{ approval.tool === 'request_sensitive_directory' ? 'Allow protected-file access in' : 'Give Dial access to' }} <strong class="mono">{{ requestedFolder }}</strong>?</span>
            <span v-else>Allow <strong>{{ approval.tool }}</strong>?</span>
            <span v-if="snap.approvals.length > 1" class="more">+{{ snap.approvals.length - 1 }} more waiting</span>
          </div>
          <pre v-if="approval.summary" class="summary">{{ approval.summary }}</pre>
          <div class="actions">
            <button class="btn btn-primary" @click="decide('allow')">{{ oneTime ? "Allow" : "Allow once" }}</button>
            <template v-if="!oneTime">
              <button class="btn btn-secondary" title="Allow this kind of call from now on" @click="decide('always')">
                Always allow
              </button>
              <button class="btn btn-secondary" title="Allow this kind of call in this project" @click="decide('project')">
                Allow for project
              </button>
            </template>
            <span v-else class="once-note">Applies to this request only</span>
            <span class="grow" />
            <button class="btn btn-danger" @click="decide('deny')">Deny</button>
          </div>
        </section>
      </Transition>

      <Transition name="pop">
        <QuestionCard v-if="snap.question" :key="snap.question.id" :question="snap.question" />
      </Transition>
      <section v-if="queue.items.length" class="queue card" aria-label="Follow-up queue">
        <strong>{{ queue.paused ? 'Queue paused' : 'Queued follow-ups' }}</strong>
        <p v-if="queue.recovered" class="faint queue-recovery">Restored after restart. Check the transcript before sending a follow-up that may already have been sent.</p>
        <div v-for="item in queue.items" :key="item.id" class="queue-row">
          <span class="truncate" :title="item.text">{{ item.text || 'Image prompt' }}{{ item.attachments.length ? ` · ${item.attachments.length} image(s)` : '' }}</span>
          <button class="icon-btn" title="Edit queued prompt" aria-label="Edit queued prompt" :disabled="queue.saving || queue.sending || draft.sending" @click="editQueued(snap, item.id)"><Icon name="pencil" :size="14" /></button>
          <button class="icon-btn" title="Remove queued prompt" aria-label="Remove queued prompt" :disabled="queue.saving || queue.sending" @click="removeQueued(snap, item.id)"><Icon name="cross" :size="14" /></button>
        </div>
        <button v-if="!snap.turnRunning" class="btn btn-secondary" :disabled="queue.loading || queue.saving || queue.sending || !snap.canSend || (!imagesAllowed && !!queue.items[0]?.attachments.length)" @click="sendQueuedNext(snap)">Send next</button>
      </section>
      <Transition name="pop">
        <VoicePanel v-if="snap.voice.phase !== 'off'" :voice="snap.voice" :turn-running="snap.turnRunning" @edit="editHeard" />
      </Transition>
      <div class="composer" :class="{ dropping }" @click="input?.focus()">
        <div v-if="dropping" class="drop-hint"><Icon name="image" :size="16" /> Drop images to attach</div>
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
          <div v-for="a in draft.attachments" :key="a.key" class="thumb" :title="a.name">
            <img :src="a.url" :alt="a.name" />
            <button class="remove" :aria-label="`Remove ${a.name}`" :title="`Remove ${a.name}`" @click="draft.attachments = draft.attachments.filter((image) => image.key !== a.key)">
              <Icon name="cross" :size="11" />
            </button>
          </div>
        </div>
        <textarea
          ref="input"
          v-model="draft.text"
          aria-label="Prompt"
          rows="2"
          spellcheck="true"
          :placeholder="snap.turnRunning ? 'Add a note for Dial while it works…' : 'How can I help you today?'"
          :readonly="draft.sending"
          @keydown="onKey"
          @input="trackCaret" @click="trackCaret" @select="trackCaret" @keyup.left="trackCaret" @keyup.right="trackCaret"
          @paste="onPaste"
        />
        <div class="bar" @click.stop>
          <button
            class="attach"
            :disabled="!imagesAllowed"
            aria-label="Attach images"
            :title="imagesAllowed ? 'Attach images (or paste / drop them)' : `${snap.model?.label ?? 'This model'} does not take images`"
            @click="chooseImages"
          >
            <Icon name="image" :size="15" />
          </button>
          <ModelPicker :snap="snap" />
          <ApprovalMenu ref="approvalMenu" :snap="snap" />
          <ModesMenu ref="modesMenu" :snap="snap" />
          <span class="grow" />
          <span v-if="typedName && !slash.length && !paths.length" class="keyhint"><kbd>Enter</kbd> run /{{ typedName }}</span>
          <span v-else-if="hasContent && !slash.length && !paths.length" class="keyhint"><kbd>Enter</kbd> {{ snap.turnRunning ? 'note' : 'send' }} <kbd>⇧ Enter</kbd> new line</span>
          <ContextMeter
            v-if="snap.context"
            :context="snap.context"
            :can-compact="canCompact(snap)"
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
          <button v-if="snap.turnRunning && snap.sessionId && snap.connection.state === 'live'" class="btn btn-secondary" :disabled="!canQueue" title="Queue for the next turn (⌘/Ctrl+Enter)" @click="queueDraft">Queue</button>
          <button v-if="snap.turnRunning" class="send stop" title="Stop the running turn (Esc)" @click.stop="markTurnAborted(snap); engine.abort()">
            <Icon name="stop" :size="14" />
          </button>
          <button
            v-if="!snap.turnRunning || steering"
            class="send"
            :disabled="!canSend"
            :title="snap.turnRunning ? 'Send note (Enter)' : 'Send (Enter)'"
            :aria-label="snap.turnRunning ? 'Send note' : 'Send'"
            @click.stop="send"
          >
            <Icon name="arrowUp" :size="16" />
          </button>
        </div>
      </div>
      <div v-if="snap.connection.state !== 'live'" class="caption truncate">{{ caption }}</div>
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
  padding: 14px 16px;
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
.more {
  margin-left: auto;
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.once-note { align-self: center; font-size: var(--text-sm); color: var(--text-faint); }
.card-head .mono { font-family: var(--font-mono); font-weight: 500; overflow-wrap: anywhere; }
.summary {
  margin: 10px 0 0;
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
  margin-top: 12px;
  flex-wrap: wrap;
}
.grow {
  flex: 1;
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
  transition: border-color 140ms var(--ease);
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
  opacity: 0;
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
.mic {
  width: 32px;
  height: 32px;
  margin-right: 4px;
  border-radius: 50%;
}
.slash {
  position: absolute;
  left: 8px;
  right: 8px;
  bottom: calc(100% + 8px);
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
.keyhint {
  display: none;
  align-items: center;
  gap: 4px;
  margin-right: 8px;
  font-size: var(--text-xs);
  color: var(--text-faint);
  white-space: nowrap;
}
.keyhint kbd:last-of-type { margin-left: 6px; }
.composer:focus-within .keyhint { display: inline-flex; }
@media (max-width: 880px) { .composer:focus-within .keyhint { display: none; } }
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
  margin: 8px 0 0 -10px;
  cursor: default;
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
.caption {
  text-align: center;
  font-size: var(--text-sm);
  color: var(--text-faint);
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
