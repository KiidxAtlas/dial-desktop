<script setup lang="ts">
import { computed } from "vue";
import type { ToolEntry } from "../types";
import { toggle, ui } from "../ui";
import { snapshot } from "../bridge";
import { addToPrompt, showFile, showPanel } from "../workspace";
import { relativeTo } from "../paths";
import Icon from "./Icon.vue";
import CodeBlock from "./CodeBlock.vue";
import GeneratedImage from "./GeneratedImage.vue";
import FindText from './FindText.vue';
import type { TextMatch } from '../chatFind';

const props = defineProps<{ tool: ToolEntry; occurrence?: TextMatch }>();
const open = computed(() => !!props.occurrence || ui.expanded.has(props.tool.key));
function openDiff(path: string) {
  ui.diffPath = path;
  showPanel("changes");
}
function askAboutFailure() {
  const t = props.tool;
  const detail = t.output?.text || t.progress || t.facts.join("\n");
  addToPrompt(`Help diagnose the failed ${t.label} call (${t.target}).${detail ? `\n\nReported output:\n${detail.slice(-4000)}` : ""}`);
}
/** The project file a read or edit call names, to open in the Files panel. */
const file = computed(() => {
  const t = props.tool;
  if (!["read", "edit", "write", "view"].includes(t.label.toLowerCase())) return null;
  const root = snapshot.value?.workspace?.root ?? "";
  // `relativeTo` accepts either separator under a Windows root and returns a `/`-separated path.
  const path = root ? relativeTo(root, t.target.trim()) : t.target.trim();
  return path && !/\s/.test(path) && !/^(?:[\\/]|[a-z]:[\\/])/i.test(path) && /[./\\]/.test(path) ? path : null;
});

/** The one-line result under the call, as a terminal agent prints it. */
const result = computed(() => {
  const t = props.tool;
  if (t.progress) return t.progress;
  if (t.state === "failed") return t.facts.join(" · ") || "Failed";
  if (t.changedPaths.length === 1) return t.changedPaths[0] === t.target ? null : `Updated ${t.changedPaths[0]}`;
  if (t.changedPaths.length > 1) return `Updated ${t.changedPaths.length} files`;
  if (t.state === "unknown") return "Outcome unknown";
  return null;
});
</script>

<template>
  <div class="tool" :class="[tool.state, { open }]" :data-tool="tool.key">
    <button class="head" :aria-expanded="open" @click="toggle(tool.key)">
      <span class="status" :title="tool.state">
        <span v-if="tool.state === 'running'" class="spinner run-spin" />
        <span v-else class="dot" />
      </span>
      <span class="label"><FindText v-if="occurrence?.field === 'label'" :text="tool.label" :match="occurrence" /><template v-else>{{ tool.label }}</template></span>
      <span class="target mono" :class="{ truncate: occurrence?.field !== 'target' }"><FindText v-if="occurrence?.field === 'target'" :text="tool.target" :match="occurrence" /><template v-else>{{ tool.target }}</template></span>
      <Icon class="chevron" name="chevronRight" :size="12" />
    </button>
    <div v-if="result" class="result">
      <span class="elbow">⎿</span>
      <span :class="tool.state === 'failed' ? 'clamp' : 'truncate'" :title="tool.state === 'failed' ? result : undefined">{{ result }}</span>
      <button v-if="tool.state === 'failed' && !open" class="ask" @click="askAboutFailure">Ask about failure</button>
    </div>
    <div v-if="tool.images.length" class="images">
      <GeneratedImage v-for="path in tool.images" :key="path" :path="path" />
    </div>
    <div v-if="open" class="details">
      <div v-if="tool.input && occurrence?.field === 'input'" class="search-block"><small>{{ tool.input.caption }}</small><pre><FindText :text="tool.input.text" :match="occurrence" /></pre></div>
      <CodeBlock
        v-else-if="tool.input"
        :caption="tool.input.caption"
        :text="tool.input.text"
        :lang="tool.input.caption === 'arguments' ? 'json' : ''"
        :max-height="200"
      />
      <div v-if="tool.output && occurrence?.field === 'output'" class="search-block"><small>{{ tool.output.caption }}</small><pre><FindText :text="tool.output.text" :match="occurrence" /></pre></div>
      <CodeBlock
        v-else-if="tool.output"
        :caption="tool.output.caption"
        :text="tool.output.text"
        :lang="tool.output.caption === 'diff' ? 'diff' : ''"
        :max-height="280"
      />
      <div v-if="tool.state === 'failed' || file || tool.changedPaths.length" class="tool-actions">
        <button v-if="tool.state === 'failed'" class="btn btn-secondary" @click="askAboutFailure">Ask about failure</button>
        <button v-if="file" class="btn btn-secondary" :title="`Open ${file} in the Files panel`" @click="showFile(file)"><Icon name="file" :size="13" /> Open file</button>
        <button v-for="path in tool.changedPaths" :key="path" class="btn btn-secondary mono" :title="`Show diff: ${path}`" @click="openDiff(path)"><Icon name="diff" :size="13" /> <span class="truncate">{{ path }}</span></button>
      </div>
      <div v-if="tool.facts.length && tool.state !== 'failed'" class="facts">{{ tool.facts.join(" · ") }}</div>
    </div>
  </div>
</template>

<style scoped>
.search-block pre { white-space: pre-wrap; overflow-wrap: anywhere; max-height: 280px; overflow: auto; user-select: text; cursor: text; }
.tool {
  --tone: var(--text-faint);
}
.tool.flash {
  border-radius: var(--radius-sm);
  animation: flash 1.4s var(--ease);
}
@keyframes flash {
  0%, 30% { background: color-mix(in srgb, var(--accent) 16%, transparent); box-shadow: 0 0 0 6px color-mix(in srgb, var(--accent) 16%, transparent); }
  100% { background: transparent; box-shadow: 0 0 0 6px transparent; }
}
.tool.succeeded {
  --tone: var(--success);
}
.tool.failed {
  --tone: var(--danger);
}
.head {
  display: flex;
  align-items: center;
  gap: 8px;
  max-width: 100%;
  margin-left: -8px;
  padding: 4px 10px 4px 8px;
  border-radius: var(--radius-sm);
  text-align: left;
  transition: background 140ms var(--ease);
}
.head:hover {
  background: var(--hover);
}
.head:focus-visible {
  outline: 1px solid var(--focus-border);
  outline-offset: 1px;
}
.status {
  display: grid;
  flex: none;
  place-items: center;
  width: 16px;
  height: 16px;
}
.run-spin {
  width: 10px;
  height: 10px;
  color: var(--text-muted);
}
.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--tone);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--tone) 16%, transparent);
}
.unknown .dot {
  background: none;
  box-shadow: inset 0 0 0 1.5px var(--text-faint);
}
.label {
  font-size: 13.5px;
  font-weight: 560;
  white-space: nowrap;
}
.target {
  min-width: 0;
  font-size: 12.5px;
  color: var(--text-muted);
}
.chevron {
  flex: none;
  color: var(--text-faint);
  opacity: 0.45;
  transition: opacity 140ms var(--ease), transform 200ms var(--ease);
}
.head:hover .chevron,
.head:focus-visible .chevron,
.tool.open > .head .chevron {
  opacity: 1;
}
.tool.open > .head .chevron {
  transform: rotate(90deg);
}
.result {
  display: flex;
  gap: 8px;
  padding-left: 16px;
  font-size: 12.5px;
  color: var(--text-faint);
}
.failed .result {
  color: var(--danger);
}
.result .clamp {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 3;
  line-clamp: 3;
  overflow: hidden;
  min-width: 0;
  overflow-wrap: anywhere;
}
.result .ask {
  flex: none;
  align-self: flex-start;
  color: var(--text-muted);
  text-decoration: underline;
  text-underline-offset: 2px;
}
.result .ask:hover {
  color: var(--text);
}
.elbow {
  flex: none;
  color: var(--text-faint);
}
.details {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin: 6px 0 6px 16px;
  animation: rise 200ms var(--ease) both;
}
.tool-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.tool-actions .btn {
  gap: 6px;
  max-width: 100%;
  font-size: 12px;
}
.images {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  margin: 8px 0 4px 16px;
}
.facts {
  font-size: var(--text-sm);
  color: var(--text-faint);
}
</style>
