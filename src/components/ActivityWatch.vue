<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, shallowRef } from 'vue';
import { activityWatchApi, showToast } from '../bridge';
import { canAddress, checkPeerReceipt, checkWorkerReceipt, MESSAGE_LIMIT, messageTooLong, resolveRecipient, type WatchPeer, type WatchTarget, type WatchWorker } from '../activityWatch';
import { paneError } from '../paneErrors';
import { openTaskTerminal } from '../commandRunner';
import Icon from './Icon.vue';

// Inspector mounts a fresh roster for each visible panel, project and live session.
const props = defineProps<{ kind: WatchTarget['kind']; root: string; sessionId: string }>();
const group = computed(() => props.kind === 'worker' ? 'workers' : 'peers');
const heading = computed(() => props.kind === 'worker' ? 'Session workers' : 'Project peers');
const workers = shallowRef<WatchWorker[]>([]), peers = shallowRef<WatchPeer[]>([]);
const count = computed(() => props.kind === 'worker' ? workers.value.length : peers.value.length);
const rosterError = ref('');
const selected = shallowRef<WatchTarget | null>(null), gone = ref(false);
const current = computed(() => selected.value && !gone.value ? resolveRecipient(selected.value, workers.value, peers.value) : null);
const detail = computed(() => current.value ?? selected.value);
const draft = ref(''), sending = ref(false), queued = ref(false), sendError = ref('');
const loading = ref(true), firstLoad = ref(true), backButton = ref<HTMLButtonElement>();
const tooLong = computed(() => messageTooLong(draft.value));
const canSend = computed(() => !!current.value && canAddress(current.value) && !rosterError.value && !sending.value && !!draft.value.trim() && !tooLong.value);
let alive = true, refreshing = false, timer: ReturnType<typeof setTimeout> | undefined;
let opener: HTMLButtonElement | null = null;
const unsupported = (reason: string) => reason.startsWith('This Dial engine does not support');
const label = (target: WatchTarget) => (target.kind === 'worker' ? target.row.name : target.row.summary) || target.row.id;

async function refresh() {
  if (!alive || refreshing || document.hidden) return;
  clearTimeout(timer);
  refreshing = true;
  loading.value = true;
  try {
    if (props.kind === 'worker') {
      const rows = await activityWatchApi.workers(props.root, props.sessionId);
      if (!alive) return;
      workers.value = rows;
    } else {
      const board = await activityWatchApi.peers(props.root, props.sessionId);
      if (!alive) return;
      if (board.session_id !== props.sessionId) throw new Error('The session changed. Refresh the peer list.');
      peers.value = board.rows.filter(p => p.id !== props.sessionId);
    }
    rosterError.value = '';
    // Once removed, require a new manual selection even if the id reappears.
    if (selected.value && !current.value) gone.value = true;
  } catch (cause) {
    if (!alive) return;
    rosterError.value = paneError(cause).detail;
  } finally {
    refreshing = false;
    if (alive) {
      firstLoad.value = false;
      loading.value = false;
      if (!document.hidden && !unsupported(rosterError.value))
        timer = setTimeout(refresh, rosterError.value ? 15000 : 5000);
    }
  }
}
function visibility() {
  clearTimeout(timer);
  if (!document.hidden && !unsupported(rosterError.value)) void refresh();
}
onMounted(() => { void refresh(); document.addEventListener('visibilitychange', visibility); });
onUnmounted(() => { alive = false; clearTimeout(timer); document.removeEventListener('visibilitychange', visibility); });

function resetCompose() { draft.value = ''; sendError.value = ''; queued.value = false; }
async function inspect(target: WatchTarget, event: MouseEvent) {
  opener = event.currentTarget as HTMLButtonElement;
  selected.value = target; gone.value = false; resetCompose();
  await nextTick(); backButton.value?.focus();
}
async function back() {
  if (sending.value) return;
  selected.value = null; resetCompose();
  await nextTick(); opener?.focus(); opener = null;
}
async function send() {
  const target = current.value;
  if (!target || !canSend.value) return;
  const { root, sessionId } = props, id = target.row.id, text = draft.value;
  sending.value = true; queued.value = false; sendError.value = '';
  try {
    if (target.kind === 'worker') checkWorkerReceipt(id, await activityWatchApi.message(root, sessionId, id, text, target.row.watchToken));
    else checkPeerReceipt(sessionId, id, await activityWatchApi.peerMessage(root, sessionId, id, text));
    if (!alive) { showToast(`Message queued for ${id}.`); return; }
    draft.value = ''; queued.value = true;
    if (target.kind === 'peer') void refresh(); // Re-read the thread; don't fabricate an outgoing message.
  } catch (cause) {
    const why = paneError(cause).detail;
    if (alive) sendError.value = why;
    else showToast(`Message to ${id}: ${why}`);
  } finally { sending.value = false; }
}
</script>
<template>
  <section class="watch" :aria-label="heading">
    <header>
      <strong>{{ heading }} <small v-if="count">{{ count }}</small></strong>
      <button class="icon-btn" :aria-label="`Refresh ${group}`" :title="`Refresh ${group}`" :disabled="loading" @click="refresh"><Icon name="refresh" :size="14" /></button>
    </header>
    <div v-show="!detail">
      <p v-if="rosterError" role="status">{{ rosterError }}</p>
      <template v-if="kind === 'worker'">
        <p v-if="!rosterError && !workers.length">{{ firstLoad ? 'Loading workers…' : 'No workers in this session.' }}</p>
        <button v-for="w in workers" :key="w.id" class="row" :disabled="!!rosterError" @click="inspect({ kind: 'worker', row: w }, $event)">
          <span class="row-label"><b>{{ w.name || w.id }}</b><small>{{ w.id }}<template v-if="w.phase && w.phase !== w.status"> · {{ w.phase }}</template></small></span>
          <span class="pill">{{ w.status }}</span><Icon name="chevronRight" :size="12" />
        </button>
      </template>
      <template v-else>
        <template v-if="unsupported(rosterError)">
          <button class="btn btn-secondary cli-peers" @click="openTaskTerminal({ kind: 'slash', command: '/peers' }, root)"><Icon name="terminal" :size="13" /> Open CLI peers</button>
          <p><small>This opens a separate Terminal session. Messages sent there come from that session.</small></p>
        </template>
        <p v-if="!rosterError && !peers.length">{{ firstLoad ? 'Looking for peer sessions…' : 'No other sessions on this project’s peer board.' }}</p>
        <button v-for="p in peers" :key="p.id" class="row" :disabled="!!rosterError" @click="inspect({ kind: 'peer', row: p }, $event)">
          <span class="row-label"><b>{{ p.summary || 'Peer session' }}</b><small class="mono">{{ p.id }}</small></span>
          <span v-if="p.phase" class="pill">{{ p.phase }}</span><Icon name="chevronRight" :size="12" />
        </button>
      </template>
    </div>
    <div v-if="detail" class="detail">
      <button ref="backButton" class="back" :disabled="sending" @click="back"><Icon name="chevronLeft" :size="12" /> All {{ group }}</button>
      <b>{{ label(detail) }}</b><small class="mono">{{ detail.row.id }}</small>
      <template v-if="detail.kind === 'worker'">
        <p>{{ [detail.row.status, detail.row.phase !== detail.row.status && detail.row.phase, detail.row.role || detail.row.agent, detail.row.model].filter(Boolean).join(' · ') }}<template v-if="detail.row.steps != null"> · {{ detail.row.steps }} steps</template></p>
        <div v-if="detail.row.task"><h3>Assignment</h3><p class="block">{{ detail.row.task }}</p></div>
        <details v-if="detail.row.result"><summary>Worker result</summary><p class="block">{{ detail.row.result }}</p></details>
      </template>
      <template v-else>
        <p>{{ [detail.row.phase, detail.row.model, ...detail.row.scopes].filter(Boolean).join(' · ') }}</p>
        <p v-if="detail.row.overlapping.length">Overlapping files: {{ detail.row.overlapping.join(', ') }}</p>
      </template>
      <p v-if="rosterError" role="status">{{ rosterError }} Refresh before sending.</p>
      <p v-else-if="!current" role="status">This recipient is no longer available. Go back to select a recipient.</p>
      <p v-else-if="!canAddress(current)">This {{ detail.kind }} is read only.</p>
      <form v-if="(current && canAddress(current)) || draft" @submit.prevent="send">
        <label for="watch-draft">Message {{ detail.kind === 'worker' ? 'worker' : 'peer session' }}</label>
        <textarea id="watch-draft" v-model="draft" rows="2" placeholder="Write a message…" :disabled="sending" @input="queued = false; sendError = ''" />
        <p v-if="tooLong" class="error">Keep the message to {{ MESSAGE_LIMIT }} UTF-8 bytes or fewer.</p>
        <button class="btn btn-secondary" :disabled="!canSend"><Icon name="send" :size="13" />{{ sending ? 'Sending…' : 'Send message' }}</button>
      </form>
      <p v-if="sendError" class="error" role="alert">{{ sendError }}</p>
      <p v-if="queued" role="status">Message queued.</p>
      <template v-if="detail.kind === 'peer'">
        <h3>Recent messages</h3>
        <p v-if="!detail.row.messages.length">No messages with this session yet.</p>
        <div v-else class="thread">
          <div v-for="(m, i) in detail.row.messages.slice().reverse()" :key="i" class="letter">
            <small>{{ m.outgoing ? 'To peer' : 'From peer' }}<template v-if="m.receipt_available && m.unread"> · Queued</template></small>
            <p class="block">{{ m.text }}</p>
          </div>
        </div>
      </template>
    </div>
  </section>
</template>
<style scoped>
.watch { font-size: var(--text-sm); overflow-wrap: anywhere; }
header, .row, .back { display: flex; align-items: center; gap: 6px; }
header { justify-content: space-between; margin-bottom: 8px; }
h3, label, small { color: var(--text-faint); font-size: var(--text-xs); font-weight: 500; }
h3 { margin: 9px 0 3px; }
p { margin: 4px 0; color: var(--text-muted); }
.row { width: 100%; padding: 6px; border-radius: var(--radius-sm); text-align: left; color: var(--text-muted); }
.row:hover:not(:disabled), .back:hover:not(:disabled) { background: var(--hover); color: var(--text); }
.row-label { flex: 1; min-width: 0; } .row-label b, .row-label small { display: block; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.row-label b, .detail > b { color: var(--text); font-weight: 500; } .pill { max-width: 40%; overflow: hidden; text-overflow: ellipsis; }
.back { color: var(--text-muted); font-size: var(--text-xs); border-radius: var(--radius-sm); padding: 4px; margin-bottom: 6px; }
.detail > b, .detail > small { display: block; } .detail form { display: grid; gap: 5px; margin: 10px 0; }
textarea { width: 100%; min-width: 0; resize: vertical; min-height: 44px; max-height: 220px; border: 1px solid var(--border-strong); border-radius: var(--radius-sm); background: var(--bg); color: var(--text); padding: 6px 8px; font: inherit; }
form .btn { justify-self: start; } form .btn, .cli-peers { font-size: var(--text-sm); } .error { color: var(--danger); }
.block { white-space: pre-wrap; } details { margin: 5px 0; } summary { cursor: pointer; color: var(--text-muted); }
.detail > div > .block, details .block { max-height: 10rem; overflow: auto; }
.thread { max-height: 240px; overflow-y: auto; } .letter { padding: 4px 0; border-top: 1px solid var(--border); }
</style>
