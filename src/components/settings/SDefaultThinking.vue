<script setup lang="ts">
import { onMounted, ref } from "vue";
import { applySaved, engineCall, readable } from "../../engineTools";
import { snapshot } from "../../bridge";
import type { SettingScope } from "../../types";
import SRow from "./SRow.vue";

// The parent keys this editor by workspace, saved model/provider and destination layer.
const props = defineProps<{ root: string; scope: SettingScope }>();
interface Thinking { provider: string; model_id: string; levels: string[]; level: string | null }
const info = ref<Thinking | null>(null);
const error = ref("");
const saving = ref(false);
async function load() {
  try {
    info.value = await engineCall<Thinking>("get_default_thinking", { scope: props.scope }, props.root);
  } catch (e) { error.value = readable(e); }
}
onMounted(load);
async function choose(event: Event) {
  const select = event.target as HTMLSelectElement;
  const model = info.value;
  if (!model || saving.value) return;
  const level = select.value;
  select.value = model.level ?? "default"; // Only display a change once the engine confirms it.
  saving.value = true;
  error.value = "";
  try {
    info.value = await engineCall<Thinking>("set_default_thinking", {
      scope: props.scope, provider: model.provider, model_id: model.model_id, level,
    }, props.root);
    // A new session can reuse this engine: refresh its base settings between turns.
    if (snapshot.value?.workspace?.root === props.root) await applySaved();
  } catch (e) { error.value = readable(e); await load(); }
  finally { saving.value = false; }
}
</script>

<template>
  <SRow label="Default thinking level" :description="info?.model_id ? `For new sessions using ${info.model_id}.` : 'For new sessions.'">
    <span v-if="error" class="s-note" role="alert">{{ error }}</span>
    <select v-if="info && (info.levels.length || info.level)" class="s-select" aria-label="Default thinking level" :value="info.level ?? 'default'" :disabled="saving" @change="choose">
      <option value="default">Use inherited default</option>
      <option v-if="info.level && !info.levels.includes(info.level)" :value="info.level" disabled>{{ info.level }}</option>
      <option v-for="level in info.levels" :key="level" :value="level">{{ level }}</option>
    </select>
    <span v-else-if="!error" class="s-note">{{ info ? 'No adjustable levels reported' : 'Loading levels…' }}</span>
  </SRow>
</template>
