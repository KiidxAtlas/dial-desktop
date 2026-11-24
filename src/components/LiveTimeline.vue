<script setup lang="ts">
import { onBeforeUnmount, shallowRef, watch } from 'vue';
import type { OpenSnapshot } from '../types';
import Timeline from './Timeline.vue';

// The RPC snapshot can advance every frame. Coalesce only successive updates to the same
// growing reply: the composer and other panes continue to see every engine update immediately.
const props = defineProps<{ snap: OpenSnapshot }>();
const shown = shallowRef(props.snap);
let timer: ReturnType<typeof setTimeout> | undefined;
function flush() {
  clearTimeout(timer);
  timer = undefined;
  shown.value = props.snap;
}
watch(() => props.snap, next => {
  const old = shown.value;
  const last = next.entries.at(-1), previous = old.entries.at(-1);
  if (next.workspace.root !== old.workspace.root || next.sessionId !== old.sessionId ||
      !next.turnRunning || next.connection !== old.connection || next.historyLoading !== old.historyLoading ||
      next.approvals !== old.approvals || next.question !== old.question || next.entries.length !== old.entries.length ||
      last?.kind !== 'assistant' || !last.streaming || previous?.kind !== 'assistant' || !previous.streaming) return flush();
  // Leading structural changes and completion render at once. A non-resetting timer lets long
  // streams progress even when deltas arrive continuously; only the newest state is needed.
  timer ??= setTimeout(flush, 50);
});
onBeforeUnmount(() => clearTimeout(timer));
</script>

<template><Timeline :snap="shown" /></template>
