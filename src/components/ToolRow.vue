<script setup lang="ts">
import { computed } from "vue";
import type { ToolEntry } from "../types";
import { toggle, ui } from "../ui";
import { addToPrompt, showPanel } from "../workspace";
import Icon from "./Icon.vue";
import CodeBlock from "./CodeBlock.vue";
import GeneratedImage from "./GeneratedImage.vue";

const props = defineProps<{ tool: ToolEntry }>();
const open = computed(() => ui.expanded.has(props.tool.key));
function openDiff(path: string) {
  ui.diffPath = path;
  showPanel("changes");
}
function askAboutFailure() {
  const t = props.tool;
  const detail = t.output?.text || t.progress || t.facts.join("\n");
  addToPrompt(`Help diagnose the failed ${t.label} call (${t.target}).${detail ? `\n\nReported output:\n${detail.slice(-4000)}` : ""}`);
}

/** The one-line result under the call, as a terminal agent prints it. */
const result = computed(() => {
  const t = props.tool;
  if (t.progress) return t.progress;
  if (t.state === "failed") return t.facts.join(" · ") || "Failed";
  if (t.changedPaths.length === 1) return `Updated ${t.changedPaths[0]}`;
  if (t.changedPaths.length > 1) return `Updated ${t.changedPaths.length} files`;
  if (t.state === "unknown") return "Outcome unknown";
  return null;
});
</script>

<template>
  <div class="tool" :class="[tool.state, { open }]">
    <button class="head" :aria-expanded="open" @click="toggle(tool.key)">
      <span class="dot" :title="tool.state" />
      <span class="label">{{ tool.label }}</span>
      <span class="target mono truncate">{{ tool.target }}</span>
      <Icon class="chevron" :name="open ? 'chevronDown' : 'chevronRight'" :size="12" />
    </button>
    <div v-if="result" class="result">
      <span class="elbow">⎿</span>
      <span class="truncate">{{ result }}</span>
    </div>
    <div v-if="tool.images.length" class="images">
      <GeneratedImage v-for="path in tool.images" :key="path" :path="path" />
    </div>
    <div v-if="open" class="details">
      <CodeBlock
        v-if="tool.input"
        :caption="tool.input.caption"
        :text="tool.input.text"
        :lang="tool.input.caption === 'arguments' ? 'json' : ''"
        :max-height="200"
      />
      <CodeBlock
        v-if="tool.output"
        :caption="tool.output.caption"
        :text="tool.output.text"
        :lang="tool.output.caption === 'diff' ? 'diff' : ''"
        :max-height="280"
      />
      <button v-if="tool.state === 'failed'" class="btn btn-secondary" @click="askAboutFailure">Ask about failure</button>
      <div v-if="tool.changedPaths.length" class="paths">
        <button v-for="path in tool.changedPaths" :key="path" class="mono truncate" :title="`Show diff: ${path}`" @click="openDiff(path)">{{ path }}</button>
      </div>
      <div v-if="tool.facts.length && tool.state !== 'failed'" class="facts">{{ tool.facts.join(" · ") }}</div>
    </div>
  </div>
</template>

<style scoped>
.tool {
  --tone: var(--text-faint);
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
  gap: 10px;
  max-width: 100%;
  padding: 2px 0;
  text-align: left;
}
.dot {
  width: 8px;
  height: 8px;
  flex: none;
  border-radius: 50%;
  background: var(--tone);
}
.running .dot {
  background: var(--text-muted);
  animation: pulse 1s ease-in-out infinite;
}
.unknown .dot {
  background: none;
  box-shadow: inset 0 0 0 1.5px var(--text-faint);
}
.label {
  font-size: 14px;
  font-weight: 600;
  white-space: nowrap;
}
.target {
  min-width: 0;
  font-size: 13px;
  color: var(--text-muted);
}
.chevron {
  color: var(--text-faint);
  opacity: 0;
  transition: opacity 120ms var(--ease);
}
.head:hover .chevron,
.open .chevron {
  opacity: 1;
}
.result {
  display: flex;
  gap: 8px;
  padding-left: 18px;
  font-size: 13px;
  color: var(--text-faint);
}
.failed .result {
  color: var(--danger);
}
.elbow {
  flex: none;
  color: var(--text-faint);
}
.details {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin: 8px 0 4px 18px;
}
.images {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin: 8px 0 4px 18px;
}
.paths {
  font-size: var(--text-sm);
  color: var(--text-muted);
}
.facts {
  font-size: var(--text-sm);
  color: var(--text-faint);
}
</style>
