<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import { ask } from '@tauri-apps/plugin-dialog';
import { engineCall, readable } from '../engineTools';
import { showToast } from '../bridge';
import { ui } from '../ui';
import { showPanel } from '../workspace';
import CodeBlock from './CodeBlock.vue';

const props = defineProps<{ root: string; session: string }>();
interface Turn { id: string; label: string; outcome: string; step_limit: boolean; turn: number | null; files: string[]; details: string[] }
interface Diff { path: string; turn: number; token: string; diff: string; safe_restore?: boolean; conflict?: string }
const turns = ref<Turn[]>([]), selected = ref(''), error = ref(''), pending = ref(false), loading = ref(false);
const panel = ref<HTMLDetailsElement>();
const turn = computed(() => turns.value.find(t => t.id === selected.value));
const reviewPath = ref('');
const choices = computed(() => reviewPath.value ? turns.value.filter(t => t.turn && t.files.includes(reviewPath.value)) : turns.value);
const diff = ref<Diff | null>(null);
let revision = 0;
function reset() { revision++; diff.value = null; error.value = ''; }
function chooseCheckpoint(path: string) {
  if (panel.value) { panel.value.open = true; panel.value.scrollIntoView({ block: 'center' }); }
  reviewPath.value = path.startsWith(props.root + '/') ? path.slice(props.root.length + 1) : path;
  selected.value = ''; diff.value = null;
  if (!loading.value) reset();
}
function selectTurn() { reset(); if (reviewPath.value && turn.value) void review(reviewPath.value); }
function showAll() { reset(); reviewPath.value = ''; selected.value = turns.value[0]?.id ?? ''; }
defineExpose({ chooseCheckpoint });
onBeforeUnmount(reset);
async function load() {
  reset(); const request = revision; loading.value = true; turns.value = [];
  try {
    const data = await engineCall<{session_id: string; turns: Turn[]}>('evidence', {action: 'turns'}, props.root);
    if (request !== revision) return;
    if (data.session_id !== props.session) throw new Error('Session changed. Reopen review.');
    turns.value = data.turns; selected.value = reviewPath.value ? '' : data.turns[0]?.id ?? '';
  } catch (e) { if (request === revision) error.value = readable(e); }
  finally { loading.value = false; }
}
onMounted(load);
async function review(path: string) {
  reset(); const request = revision;
  try {
    const result = await engineCall<Diff>('checkpoint_diff', {session_id: props.session, turn: turn.value?.turn, path}, props.root);
    if (request === revision) diff.value = result;
  } catch (e) { if (request === revision) error.value = readable(e); }
}
async function restore() {
  const file = diff.value, request = revision;
  if (!file?.safe_restore || pending.value) return;
  pending.value = true;
  try {
    if (!await ask(`Restore ${file.path} to its state before this turn? Only this file will change.`, {title: 'Restore checkpointed file', kind: 'warning', okLabel: 'Restore file'}) || request !== revision) return;
    await engineCall('restore_checkpoint_files', {session_id: props.session, turn: file.turn, files: [{path: file.path, token: file.token}]}, props.root);
    if (request === revision) { reset(); showToast(`Restored ${file.path}`); }
  } catch (e) { if (request === revision) { diff.value = null; error.value = readable(e); } }
  finally { pending.value = false; }
}
function workingTree(path: string) { ui.diffPath = path; showPanel('changes'); }
</script>
<template>
  <details ref="panel" class="recorded">
    <summary>Turn results<span v-if="turns[0]"> · {{ turns[0].outcome.replaceAll('_', ' ') }}{{ turns[0].step_limit ? ' (step limit)' : '' }}</span></summary>
    <p v-if="error" role="alert">{{ error }} <button class="btn btn-secondary" @click="load">Reload</button></p>
    <p v-if="loading">Loading recorded results…</p>
    <p v-else-if="!turns.length && !error">No recorded turn result available.</p>
    <p v-if="reviewPath && !loading && !error">{{ choices.length ? 'Choose a retained turn for' : 'No retained checkpoint for' }} <code>{{ reviewPath }}</code>. <button class="btn btn-secondary" @click="showAll">Show all turns</button></p>
    <select v-if="choices.length" v-model="selected" aria-label="Recorded turns (newest first)" @change="selectTurn"><option v-if="reviewPath" disabled value="">Choose a turn…</option><option v-for="t in choices" :key="t.id" :value="t.id">{{ t.label }} · {{ t.outcome.replaceAll('_', ' ') }}</option></select>
    <template v-if="turn">
      <p><b>{{ turn.outcome.replaceAll('_', ' ') }}</b>{{ turn.step_limit ? ' · Step limit reached' : '' }} · Recorded at turn end, not rechecked now.</p>
      <p v-for="(line, index) in turn.details" :key="index">{{ line }}</p>
      <div v-if="turn.turn && turn.files.length"><b>Checkpoint review</b><p>Compare the file before this turn with its contents now. Later edits may appear.</p>
        <button v-for="path in turn.files" :key="path" class="btn btn-secondary" @click="review(path)">{{ path }}</button>
      </div>
      <template v-if="diff">
        <CodeBlock v-if="diff.diff" :caption="`Checkpoint → current · ${diff.path}`" :text="diff.diff" lang="diff" :max-height="280" />
        <p v-else>No text differences.</p>
        <p v-if="!diff.safe_restore">Restore unavailable: {{ diff.conflict || 'Final edit could not be verified.' }}</p>
        <button v-if="diff.safe_restore && diff.diff" class="btn btn-secondary" :disabled="pending" @click="restore">Restore this file to before the turn…</button>
        <button class="btn btn-secondary" @click="workingTree(diff.path)">Open Git working-tree diff</button>
      </template>
    </template>
  </details>
</template>
<style scoped>
.recorded { margin: 10px 0; border: 1px solid var(--border); border-radius: var(--radius-md); padding: 8px 12px; font-size: var(--text-sm); color: var(--text-muted); overflow-wrap: anywhere; }
summary { cursor: pointer; color: var(--text); } select { max-width: 100%; margin-top: 8px; } p { margin: 6px 0; } .btn { margin: 3px; max-width: 100%; white-space: normal; height: auto; min-height: 28px; }
</style>
