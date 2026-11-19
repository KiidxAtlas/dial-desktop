<script setup lang="ts">
// A changed file's uncommitted diff, line by line, for the Changes panel.
import { computed, ref, watch } from "vue";
import { showToast, workspaceFiles } from "../../bridge";
import { ui } from "../../ui";
import { addToPrompt, mention, showPanel } from "../../workspace";
import type { DiffView } from "../../types";
import Icon from "../Icon.vue";

const props = defineProps<{ path: string; root: string }>();
const emit = defineEmits<{ back: [] }>();

const view = ref<DiffView | null>(null);
const error = ref<string | null>(null);

async function load() {
  error.value = null;
  try {
    view.value = await workspaceFiles.diff(props.path);
  } catch (e) {
    view.value = null;
    error.value = String(e);
  }
}

/** Diff lines with their kind; Git's file headers are left out (the panel names the file). */
const lines = computed(() => {
  const text = view.value?.diff ?? "";
  const out: { kind: string; text: string }[] = [];
  let body = false;
  for (const line of text.split("\n")) {
    if (line.startsWith("@@")) body = true;
    if (!body) continue;
    const kind = line.startsWith("@@") ? "hunk" : line.startsWith("+") ? "add" : line.startsWith("-") ? "del" : "ctx";
    out.push({ kind, text: line });
  }
  while (out.length && out[out.length - 1].text === "") out.pop();
  return out;
});
const added = computed(() => lines.value.filter((l) => l.kind === "add").length);
const removed = computed(() => lines.value.filter((l) => l.kind === "del").length);
const relative = computed(() => view.value?.path ?? props.path.replace(`${props.root}/`, ""));

function openInFiles() {
  ui.filePath = relative.value;
  showPanel("files");
}

watch(() => props.path, load, { immediate: true });
defineExpose({ load });

function copy() {
  navigator.clipboard.writeText(view.value?.diff ?? "").catch(() => showToast("Could not copy to the clipboard"));
}
</script>

<template>
  <div class="diff">
    <div class="head">
      <button class="icon-btn" title="All changed files" @click="emit('back')">
        <Icon name="arrowLeft" :size="14" />
      </button>
      <span class="mono truncate" :title="relative">{{ relative }}</span>
      <span v-if="view" class="counts"><b class="add">+{{ added }}</b> <b class="del">−{{ removed }}</b></span>
      <button class="icon-btn" title="Refresh" @click="load"><Icon name="refresh" :size="14" /></button>
      <button class="icon-btn" title="Open in Files" @click="openInFiles"><Icon name="file" :size="14" /></button>
      <button class="icon-btn" title="Copy diff" :disabled="!view?.diff" @click="copy"><Icon name="copy" :size="14" /></button>
      <button
        class="icon-btn"
        title="Ask Dial about this change"
        @click="addToPrompt(`About your change to ${mention(relative)}: `)"
      >
        <Icon name="chat" :size="14" />
      </button>
    </div>
    <div v-if="error" class="empty">
      <strong>No diff</strong>
      <span>{{ error }}</span>
    </div>
    <div v-else-if="view && !lines.length" class="empty">
      <strong>No uncommitted changes</strong>
      <span>The file matches the last commit.</span>
    </div>
    <div v-else-if="view" class="lines mono">
      <div v-if="view.untracked" class="note">New file, not yet tracked by Git</div>
      <div v-for="(line, i) in lines" :key="i" class="line" :class="line.kind">{{ line.text || " " }}</div>
    </div>
  </div>
</template>

<style scoped>
.diff {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}
.head {
  display: flex;
  align-items: center;
  gap: 4px;
  padding-bottom: 8px;
  font-size: var(--text-sm);
}
.head .mono {
  flex: 1;
  min-width: 0;
}
.counts {
  font-size: var(--text-xs);
  white-space: nowrap;
}
.add {
  color: var(--success);
}
.del {
  color: var(--danger);
}
.icon-btn:disabled {
  opacity: 0.35;
}
.lines {
  flex: 1;
  overflow: auto;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--bg);
  font-size: 12px;
  line-height: 1.55;
  user-select: text;
  -webkit-user-select: text;
}
.line {
  white-space: pre;
  padding: 0 10px;
  min-width: max-content;
}
.line.add {
  background: color-mix(in srgb, var(--success) 14%, transparent);
}
.line.del {
  background: color-mix(in srgb, var(--danger) 14%, transparent);
}
.line.hunk {
  color: var(--info);
  background: var(--hover);
  margin-top: 4px;
}
.note {
  padding: 6px 10px;
  font-family: var(--font-ui);
  font-size: var(--text-xs);
  color: var(--text-faint);
}
.empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  margin-top: var(--space-xl);
  text-align: center;
}
.empty strong {
  font-weight: 500;
  color: var(--text-muted);
}
.empty span {
  font-size: var(--text-sm);
  color: var(--text-faint);
}
</style>
