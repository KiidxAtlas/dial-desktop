<script setup lang="ts">
// The VM list in the App panel's VM mode: the VirtualBox and VMware machines the backend registered, grouped
// by provider. Presentation only. The parent (AppPane) owns discovery, the selection, the agent's one-time
// allowance and the launch; this component only reports the user's choice.
import { computed } from "vue";
import { VM_PROVIDERS, type AppVmMachine } from "../../appRunner";

const props = defineProps<{
  machines: AppVmMachine[];
  loading: boolean;
  error: string | null;
  notices: string[];
  /** The machine the user picked. Exact id; nothing is chosen for the user. */
  selected: string | null;
  /** The machine the agent may launch once, as the backend's state reports it. */
  granted: string | null;
  /** Selection and the allowance are fixed while a launch or an allowance change is in flight. */
  locked: boolean;
  canStart: boolean;
  starting: boolean;
  /** Changes after each allowance or pick answer, so the native inputs re-read the true state. */
  syncKey: number;
}>();

const emit = defineEmits<{
  pick: [id: string];
  grant: [allow: boolean];
  refresh: [];
  start: [];
}>();

const groups = computed(() =>
  VM_PROVIDERS.map((provider) => ({
    ...provider,
    machines: props.machines.filter((machine) => machine.provider === provider.value),
  })).filter((group) => group.machines.length > 0),
);

const chosen = computed(() => props.machines.find((machine) => machine.id === props.selected) ?? null);

function onGrant(e: Event) {
  emit("grant", (e.target as HTMLInputElement).checked);
}
</script>

<template>
  <section class="vm-picker" aria-label="Virtual machines">
    <div class="head">
      <h3>Virtual machines</h3>
      <span class="spacer" />
      <button type="button" class="btn" :disabled="loading || locked" @click="emit('refresh')">Refresh</button>
    </div>

    <div v-if="loading" class="waiting" role="status">Looking for VirtualBox and VMware machines…</div>
    <div v-else-if="error" class="error" role="alert">
      <span>{{ error }}</span>
      <button type="button" class="btn" @click="emit('refresh')">Try again</button>
    </div>
    <template v-else>
      <p v-if="!machines.length" class="hint" role="status">
        No VirtualBox or VMware machines were found. Register one with VirtualBox or VMware, then refresh.
      </p>
      <fieldset v-for="group in groups" :key="`${group.value}:${syncKey}`" class="group" :disabled="locked">
        <legend>{{ group.label }}</legend>
        <label v-for="machine in group.machines" :key="machine.id" class="machine" :title="machine.id">
          <input
            type="radio"
            name="vm-choice"
            :value="machine.id"
            :checked="selected === machine.id"
            @change="emit('pick', machine.id)"
          />
          <span class="name">{{ machine.name }}</span>
          <span v-if="machine.running" class="badge running">Running</span>
          <span v-if="granted === machine.id" class="badge">Agent may launch once</span>
        </label>
      </fieldset>
    </template>

    <ul v-if="notices.length" class="notices" role="status">
      <li v-for="(line, index) in notices" :key="index">{{ line }}</li>
    </ul>

    <template v-if="chosen">
      <label class="grant" :key="`grant:${syncKey}`">
        <input type="checkbox" :checked="granted === chosen.id" :disabled="locked" @change="onGrant" />
        <span>Allow agent to launch this VM once</span>
      </label>
      <p class="hint">
        <template v-if="granted === chosen.id">
          The agent may launch <strong>{{ chosen.name }}</strong> one time. Unchecking revokes it.
        </template>
        <template v-else>The agent cannot launch a VM until you allow it. Dial never starts a VM on its own.</template>
      </p>
    </template>

    <button type="button" class="btn primary" :disabled="!canStart" @click="emit('start')">
      {{ starting ? (chosen?.running ? "Connecting…" : "Starting…") : chosen ? `${chosen.running ? "Connect to" : "Start"} ${chosen.name}` : "Start VM" }}
    </button>
    <p v-if="chosen?.running" class="hint">
      {{ chosen.name }} is already running. Connecting attaches to it as it is; nothing is restarted.
    </p>
  </section>
</template>

<style scoped>
.vm-picker {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
}
.head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.head h3 {
  margin: 0;
  font-size: 12px;
  font-weight: 600;
}
.spacer {
  flex: 1;
}
.group {
  border: none;
  margin: 0;
  padding: 0;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.group legend {
  font-size: 11px;
  color: var(--text-faint);
  padding: 0;
  margin-bottom: 2px;
}
.group:disabled {
  opacity: 0.6;
}
.machine {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  min-width: 0;
  padding: 3px 4px;
  border-radius: var(--radius-sm);
  cursor: pointer;
}
.machine:hover {
  background: var(--surface);
}
.name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  min-width: 0;
}
.badge {
  flex: none;
  margin-left: auto;
  font-size: 11px;
  color: var(--accent);
}
.badge.running {
  color: var(--text-muted);
}
.badge.running + .badge {
  margin-left: 6px;
}
.grant {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  font-size: 12px;
}
.hint,
.waiting {
  font-size: 12px;
  color: var(--text-faint);
  margin: 0;
}
.waiting {
  padding: 12px;
}
.notices {
  margin: 0;
  padding-left: 16px;
  font-size: 12px;
  color: var(--text-faint);
}
.error {
  font-size: 12px;
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  color: var(--danger, #c0392b);
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.btn {
  display: inline-flex;
  align-items: center;
  align-self: flex-start;
  height: 26px;
  padding: 0 10px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text);
  font-size: 12px;
  cursor: pointer;
}
.btn:disabled {
  opacity: 0.4;
  cursor: default;
}
.btn.primary {
  border-color: var(--accent);
}
</style>
