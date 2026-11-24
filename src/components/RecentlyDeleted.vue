<script setup lang="ts">
import { computed, ref } from 'vue';
import { hiddenSessions, purgeSession, restoreSessions, snapshot } from '../bridge';
import type { Deletable } from '../deferredDelete';
const props = defineProps<{ root: string }>();
const items = computed(() => {
  const byId = new Map(snapshot.value?.sessions.map(s => [s.id, s.title]));
  return Array.from(hiddenSessions, id => ({ id, title: byId.get(id) || `Session ${id}` })).reverse();
});
const busy = ref(false);
async function purge(item: Deletable) {
  if (busy.value) return;
  busy.value = true;
  try { await purgeSession(props.root, item); } finally { busy.value = false; }
}
</script>
<template>
  <details v-if="items.length" class="trash" :key="root">
    <summary>Recently Deleted ({{ items.length }})</summary>
    <p>Kept on this computer until you restore or permanently delete them.</p>
    <div v-for="item in items" :key="item.id" class="entry">
      <span class="truncate" :title="item.title">{{ item.title }}</span>
      <div class="actions"><button :aria-label="`Restore ${item.title}`" :disabled="busy" @click="restoreSessions(root, [item.id])">Restore</button>
      <button :aria-label="`Permanently delete ${item.title}`" :disabled="busy" @click="purge(item)">Delete permanently…</button></div>
    </div>
  </details>
</template>
<style scoped>
.trash { flex: none; max-height: 35%; overflow-y: auto; padding: 8px 12px; color: var(--text-muted); font-size: 12px; }
summary { cursor: pointer; font-weight: 600; }
p { margin: 8px 0; }
.entry { display: grid; gap: 4px; padding: 6px 0; border-top: 1px solid var(--border); }
.actions { display: flex; flex-wrap: wrap; gap: 8px; }
.actions button { color: var(--accent); }
</style>
