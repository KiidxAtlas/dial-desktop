<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref } from "vue";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { engine, terminalApi } from "../bridge";
import type { OpenSnapshot } from "../types";
import SGroup from "./settings/SGroup.vue";
import SRow from "./settings/SRow.vue";

const props = defineProps<{ snap: OpenSnapshot }>();
const unavailable = computed(() => {
  const c = props.snap.connection;
  if (c.state === "preview") return "Sign-in is unavailable in the design preview.";
  if (c.state === "failed" && c.problem === "missing") return "Install the Dial engine in Settings → Updates first.";
  return "";
});
const host = ref<HTMLElement | null>(null);
const open = ref(false);
const ended = ref(false);
const error = ref("");
let serial = 0;
let id: number | null = null;
let term: Terminal | null = null;
let observer: ResizeObserver | null = null;

function close() {
  serial++;
  observer?.disconnect();
  observer = null;
  if (id !== null) void terminalApi.close(id).catch(e => { error.value = String(e); });
  id = null;
  term?.dispose();
  term = null;
  open.value = false;
}
onBeforeUnmount(close);

async function start() {
  if (open.value || unavailable.value) return;
  const run = ++serial;
  error.value = "";
  ended.value = false;
  open.value = true;
  await nextTick();
  if (serial !== run || !host.value) return;
  try {
    const screen = term = new Terminal({ fontSize: 13, scrollback: 0, cursorBlink: true });
    const fit = new FitAddon();
    screen.loadAddon(fit);
    screen.open(host.value);
    fit.fit();
    const writeError = (e: unknown) => { if (serial === run) error.value = String(e); };
    screen.onData(data => { if (id !== null) void terminalApi.write(id, data).catch(writeError); });
    const session = await terminalApi.openProviderSetup(props.snap.workspace.root, screen.cols, screen.rows, event => {
      if (serial !== run) return;
      if (event.kind === "data") screen.write(event.data);
      else {
        id = null;
        ended.value = true;
      }
    });
    if (serial !== run) { await terminalApi.close(session); return; }
    if (ended.value) return;
    id = session;
    observer = new ResizeObserver(() => {
      if (serial !== run) return;
      fit.fit();
      void terminalApi.resize(session, screen.cols, screen.rows).catch(writeError);
    });
    observer.observe(host.value);
    screen.focus();
  } catch (e) {
    if (serial !== run) return;
    close();
    error.value = String(e);
  }
}
</script>

<template>
  <SGroup title="Provider accounts">
    <SRow label="Sign in or add an API key" :description="unavailable || 'Choose a provider in Dial’s setup panel below. On first launch, choose Hosted provider.'">
      <button v-if="!open" class="btn btn-primary" :disabled="!!unavailable" @click="start">Open provider setup</button>
      <button v-else class="btn btn-secondary" @click="close">Close setup</button>
    </SRow>
  </SGroup>
  <p v-if="error" role="alert" class="s-note">{{ error }}</p>
  <div v-if="open" ref="host" class="setup-terminal" aria-label="Interactive provider setup" @keydown.stop @keyup.stop />
  <p v-if="open && ended" class="s-note" role="status">Setup has closed. Review any messages above, then close the panel.</p>
  <SGroup v-if="!unavailable" title="Use the provider">
    <SRow label="Reconnect after saving" description="Choose and save a model in setup, then reconnect this session to load your changes.">
      <button class="btn btn-secondary" :disabled="open || snap.turnRunning || snap.connection.state === 'starting'" @click="engine.reconnect()">Reconnect</button>
    </SRow>
  </SGroup>
  <p v-if="snap.connection.state === 'failed'" class="s-note" role="status">Connection error: {{ snap.connection.error }}</p>
</template>

<style scoped>
.setup-terminal { height: clamp(320px, 50vh, 600px); padding: 8px; overflow: hidden; border-radius: var(--radius-sm); background: #000; }
.setup-terminal :deep(.xterm) { height: 100%; }
</style>
