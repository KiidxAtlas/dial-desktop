<script setup lang="ts">
import { computed } from "vue";
import { phaseOf, runActivity, runFiles, runImages, runSummary, type RunItem, type ToolRun } from "../toolFolds";
import { entryKey, ui } from "../ui";
import ReceivedRow from "./ReceivedRow.vue";
import FindText from "./FindText.vue";
import { showPanel } from "../workspace";
import Icon from "./Icon.vue";
import ToolRow from "./ToolRow.vue";
import GeneratedImage from "./GeneratedImage.vue";
import type { TextMatch } from '../chatFind';

/** A burst of tool calls on one line, with the coordination messages and status lines that
 * arrived among them. A single call or message shows as its own row; several show a summary
 * that updates in place while they run, with the files they changed and the images they made
 * beneath it. Opening the line lists every row. */
const props = defineProps<{ run: ToolRun; open: boolean; session: string | null; match: number; occurrence?: TextMatch }>();
const emit = defineEmits<{ toggle: [event: MouseEvent] }>();

const activity = computed(() => runActivity(props.run));
const summary = computed(() => runSummary(props.run));
const images = computed(() => runImages(props.run));
/** Changed files, less the images already shown as thumbnails. */
const files = computed(() =>
  runFiles(props.run).filter((f) => !images.value.some((img) => img === f.path || img.endsWith(`/${f.path}`))),
);
/** Failed calls stay in view under a closed run, so a failure is never two clicks away. */
const FAILED_SHOWN = 3;
const failedTools = computed(() => props.run.tools.filter((t) => t.state === "failed"));
/** While calls run, the line names the phase ("Reading…", "Editing…"); once done, what was done. */
const headline = computed(() => (activity.value.running ? `${phaseOf(activity.value.current)}…` : summary.value));
/** At most this many pips; the rest show as a count. */
const PIPS = 14;
const tone = computed(() => (activity.value.running ? "running" : activity.value.failed ? "mixed" : activity.value.unknown ? "unknown" : "succeeded"));
const name = (path: string) => path.split("/").pop() || path;
const only = computed(() => (props.run.items.length === 1 ? props.run.items[0] : null));
const messages = computed(() => props.run.items.filter((item) => item.entry.kind === "received").length);
const count = (n: number, one: string) => `${n} ${one}${n === 1 ? "" : "s"}`;
const contents = computed(() =>
  [props.run.tools.length && count(props.run.tools.length, "tool call"), messages.value && count(messages.value, "message")].filter(Boolean).join(", "),
);
const hit = (item: RunItem) => (props.occurrence?.entry === item.index ? props.occurrence : undefined);

function openDiff(path: string) {
  ui.diffPath = path;
  showPanel("changes");
}
</script>

<template>
  <ToolRow
    v-if="only?.entry.kind === 'tool' && !activity.running"
    :tool="only.entry"
    :occurrence="hit(only)"
    :data-entry="run.start"
    :class="{ 'current-match': match === run.start }"
  />
  <ReceivedRow
    v-else-if="only?.entry.kind === 'received'"
    :entry="only.entry"
    :open-key="entryKey(session, 'received', only.index)"
    :highlighted="match === run.start"
    :occurrence="hit(only)"
    :data-entry="run.start"
    :class="{ 'current-match': match === run.start }"
  />
  <div v-else class="run" :class="[tone, { open }]">
    <button
      class="line"
      :aria-expanded="open"
      :title="`${open ? 'Hide' : 'Show'} ${contents} (⌥-click: all)`"
      @click="emit('toggle', $event)"
    >
      <span class="status" aria-hidden="true">
        <span v-if="activity.running" class="spinner" />
        <Icon v-else-if="tone === 'succeeded'" name="check" :size="12" />
        <span v-else class="dot" />
      </span>
      <span class="summary" aria-live="polite">{{ headline }}</span>
      <span class="pips" aria-hidden="true">
        <span
          v-for="t in run.tools.slice(-PIPS)"
          :key="t.key"
          class="pip"
          :class="t.state"
        />
        <span v-if="run.tools.length > PIPS" class="more">+{{ run.tools.length - PIPS }}</span>
      </span>
      <span v-if="activity.failed" class="badge failed">{{ activity.failed }} failed</span>
      <span v-if="activity.unknown" class="badge">{{ activity.unknown }} unknown</span>
      <span v-if="activity.running" class="current truncate">{{ activity.current.progress || activity.current.target }}</span>
      <Icon class="chevron" name="chevronRight" :size="12" />
    </button>

    <div v-if="!open && failedTools.length" class="failures">
      <ToolRow v-for="t in failedTools.slice(0, FAILED_SHOWN)" :key="t.key" :tool="t" />
      <button v-if="failedTools.length > FAILED_SHOWN" class="more-failed" @click="emit('toggle', $event)">
        Show all {{ failedTools.length }} failed calls
      </button>
    </div>

    <div v-if="!open && (files.length || images.length)" class="previews">
      <div v-if="files.length" class="files">
        <button
          v-for="file in files"
          :key="file.path"
          class="file"
          :title="`Show changes: ${file.path}`"
          @click="openDiff(file.path)"
        >
          <Icon name="file" :size="12" />
          <span class="mono truncate">{{ name(file.path) }}</span>
          <span v-if="file.added" class="added">+{{ file.added }}</span>
          <span v-if="file.removed" class="removed">−{{ file.removed }}</span>
        </button>
      </div>
      <div v-if="images.length" class="thumbs">
        <GeneratedImage v-for="path in images" :key="path" :path="path" compact />
      </div>
    </div>

    <Transition name="expand">
      <div v-if="open" class="body">
        <div class="calls">
          <div
            v-for="item in run.items"
            :key="item.entry.kind === 'tool' ? item.entry.key : item.index"
            class="call"
            :class="{ 'current-match': match === item.index }"
            :data-entry="item.index"
          >
            <ToolRow v-if="item.entry.kind === 'tool'" :tool="item.entry" :occurrence="hit(item)" />
            <ReceivedRow
              v-else-if="item.entry.kind === 'received'"
              :entry="item.entry"
              :open-key="entryKey(session, 'received', item.index)"
              :highlighted="match === item.index"
              :occurrence="hit(item)"
            />
            <p v-else class="note"><FindText v-if="hit(item)" :text="item.entry.message" :match="hit(item)!" /><template v-else>{{ item.entry.message }}</template></p>
          </div>
        </div>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.run {
  --tone: var(--success);
}
.run.mixed { --tone: var(--warning); }
.run.unknown { --tone: var(--text-faint); }
.run.running { --tone: var(--text-muted); }
.line {
  display: flex;
  align-items: center;
  gap: 8px;
  max-width: 100%;
  margin-left: -8px;
  padding: 4px 10px 4px 8px;
  border-radius: var(--radius-sm);
  font-size: 13.5px;
  color: var(--text-muted);
  text-align: left;
  transition: background 140ms var(--ease), color 140ms var(--ease);
}
.line:hover { background: var(--hover); color: var(--text); }
.line:focus-visible { outline: 1px solid var(--focus-border); outline-offset: 1px; }
.status {
  display: grid;
  flex: none;
  place-items: center;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  color: var(--tone);
  background: color-mix(in srgb, var(--tone) 14%, transparent);
  transition: background 200ms var(--ease), color 200ms var(--ease);
}
.status .spinner { width: 10px; height: 10px; }
.status .dot { width: 6px; height: 6px; border-radius: 50%; background: var(--tone); }
.summary {
  flex: none;
  color: var(--text);
  font-weight: 560;
  white-space: nowrap;
}
.pips {
  display: inline-flex;
  flex: none;
  align-items: center;
  gap: 3px;
}
.pip {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: color-mix(in srgb, var(--success) 75%, transparent);
  animation: pop 260ms var(--ease) both;
}
.pip.failed { background: var(--danger); }
.pip.unknown { background: none; box-shadow: inset 0 0 0 1px var(--text-faint); }
.pip.running { background: var(--text-muted); animation: pop 260ms var(--ease) both, pulse 1s ease-in-out infinite; }
.more { font-size: 11px; color: var(--text-faint); font-variant-numeric: tabular-nums; }
@keyframes pop { from { transform: scale(0); opacity: 0; } }
.badge {
  flex: none;
  padding: 0 6px;
  border-radius: 999px;
  font-size: 11px;
  line-height: 17px;
  color: var(--text-muted);
  background: var(--hover);
  font-variant-numeric: tabular-nums;
}
.badge.failed { color: var(--danger); background: color-mix(in srgb, var(--danger) 12%, transparent); }
.current {
  min-width: 0;
  font-size: 12.5px;
  color: var(--text-faint);
}
.current b { font-weight: 600; color: var(--text-muted); }
.chevron {
  flex: none;
  color: var(--text-faint);
  opacity: 0.45;
  transition: opacity 140ms var(--ease), transform 200ms var(--ease);
}
.line:hover .chevron, .line:focus-visible .chevron, .run.open > .line .chevron { opacity: 1; }
.run.open > .line .chevron { transform: rotate(90deg); }

.failures {
  margin: 4px 0 2px 16px;
  padding-left: 8px;
  border-left: 2px solid color-mix(in srgb, var(--danger) 45%, transparent);
}
.more-failed {
  margin-top: 2px;
  font-size: 12px;
  color: var(--text-muted);
  text-decoration: underline;
  text-underline-offset: 2px;
}
.more-failed:hover { color: var(--text); }

.previews {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin: 6px 0 2px 16px;
  animation: rise 220ms var(--ease) both;
}
.files { display: flex; flex-wrap: wrap; gap: 6px; }
.file {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  max-width: 260px;
  height: 24px;
  padding: 0 9px 0 7px;
  border-radius: 999px;
  font-size: 12px;
  color: var(--text-muted);
  background: var(--glass-soft, transparent);
  box-shadow: inset 0 0 0 1px var(--border);
  transition: background 140ms var(--ease), color 140ms var(--ease), box-shadow 140ms var(--ease);
}
.file:hover { background: var(--hover); color: var(--text); box-shadow: inset 0 0 0 1px var(--border-strong); }
.file .mono { min-width: 0; font-size: 11.5px; }
.added, .removed { flex: none; font-family: var(--font-mono); font-size: 11px; font-variant-numeric: tabular-nums; }
.added { color: var(--success); }
.removed { color: var(--danger); }
.thumbs { display: flex; flex-wrap: wrap; gap: 8px; }

.body {
  display: grid;
  grid-template-rows: 1fr;
}
.calls {
  min-height: 0;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin: 4px 0 0 7px;
  padding: 4px 0 2px 14px;
  border-left: 1px solid var(--border-strong);
}
.note {
  margin: 0;
  padding: 2px 0;
  font-size: 12.5px;
  color: var(--text-faint);
  overflow-wrap: anywhere;
  user-select: text;
  -webkit-user-select: text;
}
.call.current-match,
.tool.current-match,
.received.current-match {
  border-radius: var(--radius-sm);
  box-shadow: inset 0 0 0 1px var(--accent);
  background: color-mix(in srgb, var(--accent) 7%, transparent);
}
.expand-enter-active, .expand-leave-active {
  transition: grid-template-rows 240ms var(--ease), opacity 200ms var(--ease);
}
.expand-enter-from, .expand-leave-to { grid-template-rows: 0fr; opacity: 0; }
@media (prefers-reduced-motion: reduce) {
  .pip, .previews { animation: none; }
  .expand-enter-active, .expand-leave-active { transition: none; }
}
</style>
