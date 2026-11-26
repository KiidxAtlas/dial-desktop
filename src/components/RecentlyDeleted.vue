<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { hiddenSessions, purgeSessions, restoreSessions, snapshot } from '../bridge';
const props = defineProps<{ root: string }>();
const items = computed(() => {
  const byId = new Map(snapshot.value?.sessions.map(s => [s.id, s.title]));
  return Array.from(hiddenSessions, id => ({ id, title: byId.get(id) || `Session ${id}` })).reverse();
});
const picked = ref<string[]>([]);
const selected = computed(() => items.value.filter(item => picked.value.includes(item.id)));
watch(() => props.root, () => { picked.value = []; });
watch(items, () => { picked.value = selected.value.map(item => item.id); });
const busy = ref(false);
async function purge() {
  if (busy.value) return;
  busy.value = true;
  try { await purgeSessions(props.root, selected.value); } finally { busy.value = false; }
}
</script>
<template>
  <details v-if="items.length" :key="root" class="trash">
    <summary>Recently Deleted <span>{{ items.length }}</span></summary>
    <p>Kept on this computer until you restore or delete them.</p>
    <fieldset :disabled="busy" aria-label="Deleted sessions">
      <label class="all"><input type="checkbox" :checked="selected.length === items.length" :indeterminate="selected.length > 0 && selected.length < items.length" @change="picked = selected.length === items.length ? [] : items.map(i => i.id)">Select all <small v-if="selected.length">{{ selected.length }} selected</small></label>
      <div class="entries"><label v-for="item in items" :key="item.id"><input v-model="picked" type="checkbox" :value="item.id"><span class="truncate" :title="item.title">{{ item.title }}</span></label></div>
      <div class="actions"><button :disabled="!selected.length" @click="restoreSessions(root, selected.map(i => i.id))">Restore</button><button :disabled="!selected.length" class="danger" @click="purge">Delete permanently…</button></div>
    </fieldset>
  </details>
</template>
<style scoped>
.trash { flex: none; max-height: 38%; overflow: auto; border-top: 1px solid var(--border); padding: 9px 12px; font-size: 12px; }
summary { cursor: pointer; font-weight: 600; }
summary span { float: right; border-radius: 8px; padding: 0 6px; background: var(--surface); }
p, small { color: var(--text-muted); }
fieldset { margin: 0; padding: 0; border: 0; min-width: 0; }
label { display: flex; align-items: center; gap: 8px; padding: 5px 3px; cursor: pointer; }
small { margin-left: auto; }
.all { border-bottom: 1px solid var(--border); }
.entries { max-height: 14vh; overflow-y: auto; }
.entries label:hover { background: var(--surface); }
.actions { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 5px; }
button { padding: 4px 7px; border: 1px solid var(--border); border-radius: 5px; }
.danger { color: var(--danger); }
button:disabled { opacity: .45; }
</style>
