<script setup lang="ts">
import { computed } from "vue";
import type { Entry } from "../types";
import { toggle, ui } from "../ui";
import { copyText } from "../sessionActions";
import Icon from "./Icon.vue";
import FindText from "./FindText.vue";
import type { TextMatch } from "../chatFind";

/** A coordination message from a worker or a peer session, drawn as a call row: closed it names
 * the sender, open it shows the text. `openKey` is where its open state is stored. */
const props = defineProps<{
  entry: Extract<Entry, { kind: "received" }>;
  openKey: string;
  /** A search or jump landed on this row. */
  highlighted?: boolean;
  occurrence?: TextMatch;
}>();
const open = computed(() => props.highlighted || !!props.occurrence || ui.expanded.has(props.openKey));
</script>

<template>
  <div class="received" :class="{ open }">
    <button class="head" :aria-expanded="open" @click="toggle(openKey)">
      <span class="status"><Icon name="chat" :size="14" /></span>
      <span class="label">Message received from</span>
      <span class="sender mono" :class="{ truncate: occurrence?.field !== 'sender' }"><FindText v-if="occurrence?.field === 'sender'" :text="entry.sender" :match="occurrence" /><template v-else>{{ entry.sender }}</template></span>
      <Icon class="chevron" name="chevronRight" :size="12" />
    </button>
    <div v-if="open" class="received-detail">
      <small v-if="entry.source === 'peerNotice'">Arrival notice · Dial did not include the message body.</small>
      <small v-else-if="entry.source === 'workerNotice'">Worker message preview</small>
      <div class="text"><FindText v-if="occurrence?.field === 'text'" :text="entry.text" :match="occurrence" /><template v-else>{{ entry.text }}</template></div>
      <button class="act" title="Copy message" aria-label="Copy message" @click="copyText(entry.text)"><Icon name="copy" :size="14" /></button>
    </div>
  </div>
</template>

<style scoped>
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
.head:hover { background: var(--hover); }
.head:focus-visible { outline: 1px solid var(--focus-border); outline-offset: 1px; }
.status {
  display: grid;
  flex: none;
  place-items: center;
  width: 16px;
  height: 16px;
  color: var(--text-faint);
}
.label {
  font-size: 13.5px;
  font-weight: 560;
  white-space: nowrap;
}
.sender {
  min-width: 0;
  overflow-wrap: anywhere;
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
.received.open > .head .chevron { opacity: 1; }
.received.open > .head .chevron { transform: rotate(90deg); }
.received-detail {
  margin: 6px 0 6px 16px;
  padding-left: 12px;
  border-left: 1px solid var(--border);
  font-size: var(--text-sm);
  animation: rise 200ms var(--ease) both;
}
.received-detail small { display: block; margin-bottom: 6px; color: var(--text-muted); }
.text { white-space: pre-wrap; overflow-wrap: anywhere; user-select: text; -webkit-user-select: text; }
.act {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  margin: 4px 0 0 -6px;
  border-radius: var(--radius-sm);
  color: var(--text-faint);
  transition: background 120ms var(--ease), color 120ms var(--ease);
}
.act:hover { background: var(--hover); color: var(--text); }
@media (prefers-reduced-motion: reduce) {
  .received-detail { animation: none; }
}
</style>
