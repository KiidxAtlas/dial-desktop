<script setup lang="ts">
// The VM list in the App panel's VM mode: the VirtualBox and VMware machines the backend registered, grouped
// by provider. Presentation only. The parent (AppPane) owns discovery, the selection, the agent's one-time
// allowance and the launch; this component only reports the user's choice.
import { computed } from "vue";
import { VM_PROVIDERS, type AppVmMachine, type AppVmReadiness } from "../../appRunner";
import { openExternal } from "../../bridge";

const VM_DOCS_URL =
  "https://github.com/KiidxAtlas/dial-desktop/blob/main/docs/app-runtime-packaging.md#registered-virtual-machines";

const props = defineProps<{
  machines: AppVmMachine[];
  loading: boolean;
  error: string | null;
  notices: string[];
  /** What each provider still needs before Dial can open its console. */
  readiness: AppVmReadiness[];
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

const readinessGroups = computed(() =>
  VM_PROVIDERS.map((provider) => ({
    ...provider,
    checks: props.readiness.filter((check) => check.provider === provider.value),
  })).filter((group) => group.checks.length > 0),
);
const MARKS: Record<AppVmReadiness["status"], string> = { ready: "✓", missing: "!", unknown: "•" };
const STATES: Record<AppVmReadiness["status"], string> = { ready: "Ready", missing: "Needs setup", unknown: "Not checked yet" };

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
      <button type="button" class="btn btn-secondary" :disabled="loading || locked" @click="emit('refresh')">Refresh</button>
    </div>

    <div v-if="loading" class="waiting" role="status">Looking for VirtualBox and VMware machines…</div>
    <div v-else-if="error" class="error" role="alert">
      <span>{{ error }}</span>
      <button type="button" class="btn btn-secondary" @click="emit('refresh')">Try again</button>
    </div>
    <template v-else>
      <div v-if="!machines.length" class="empty" role="status">
        <p class="hint">No VirtualBox or VMware machines were found. Register one with VirtualBox or VMware, then refresh.</p>
        <ul class="hint tips">
          <li><strong>VirtualBox:</strong> install VirtualBox so <code>VBoxManage</code> is available and its Python SDK works.</li>
          <li><strong>VMware:</strong> enable the VM's VNC console, listening on loopback (127.0.0.1).</li>
        </ul>
        <button type="button" class="link" @click="openExternal(VM_DOCS_URL)">How Dial finds virtual machines</button>
      </div>
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

    <div v-if="!loading && !error && machines.length" class="start">
      <button type="button" class="btn btn-primary" :disabled="!canStart" @click="emit('start')">
        {{ starting ? (chosen?.running ? "Connecting…" : "Starting…") : chosen ? `${chosen.running ? "Connect to" : "Start"} ${chosen.name}` : "Start VM" }}
      </button>
      <p v-if="!chosen" class="hint">Choose a machine above to start it.</p>
      <p v-else-if="chosen.running" class="hint">
        {{ chosen.name }} is already running. Connecting attaches to it as it is; nothing is restarted.
        Disconnect releases Dial only; the VM stays on.
      </p>
      <p v-else class="hint caution" role="note">
        This is your real VM: changes made inside it stay on its disk, and its own network, shared folders and clipboard
        settings apply. Disconnect releases Dial only; the VM stays on.
      </p>
    </div>

    <div v-if="chosen" class="grant-box">
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
    </div>

    <details v-if="!loading && readinessGroups.length" class="readiness" :open="!machines.length">
      <summary>Provider readiness</summary>
      <div v-for="group in readinessGroups" :key="group.value" class="ready-group">
        <span class="ready-provider">{{ group.label }}</span>
        <ul>
          <li v-for="check in group.checks" :key="check.id" class="check" :class="check.status">
            <span class="mark" aria-hidden="true">{{ MARKS[check.status] }}</span>
            <span class="check-body">
              <span class="check-line">
                <strong>{{ check.label }}</strong>
                <span class="check-state">{{ STATES[check.status] }}</span>
              </span>
              <span class="hint">{{ check.detail }}</span>
            </span>
          </li>
        </ul>
      </div>
      <button type="button" class="link" @click="openExternal(VM_DOCS_URL)">Setup guide for VirtualBox and VMware</button>
    </details>
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
  font-size: var(--text-sm);
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
  font-size: var(--text-xs);
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
  font-size: var(--text-sm);
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
  font-size: var(--text-xs);
  color: var(--accent);
}
.badge.running {
  color: var(--text-muted);
}
.badge.running + .badge {
  margin-left: 6px;
}
.start,
.grant-box {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 4px;
}
.grant {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  font-size: var(--text-sm);
}
.hint,
.waiting {
  font-size: var(--text-sm);
  color: var(--text-faint);
  margin: 0;
}
.hint.caution {
  color: var(--warning);
}
.waiting {
  padding: 12px;
}
.empty {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.tips {
  padding-left: 16px;
}
.link {
  align-self: flex-start;
  padding: 0;
  border: none;
  background: none;
  color: var(--accent);
  font-size: var(--text-sm);
  cursor: pointer;
  text-decoration: underline;
}
.notices {
  margin: 0;
  padding-left: 16px;
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.readiness summary {
  cursor: pointer;
  font-size: var(--text-sm);
  color: var(--text-muted);
}
.readiness {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.ready-group ul {
  list-style: none;
  margin: 2px 0 6px;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.ready-provider {
  font-size: var(--text-xs);
  color: var(--text-faint);
}
.check {
  display: flex;
  gap: 6px;
  font-size: var(--text-sm);
}
.check .mark {
  flex: none;
  width: 14px;
  text-align: center;
  color: var(--text-faint);
}
.check.ready .mark {
  color: var(--success);
}
.check.missing .mark {
  color: var(--warning);
}
.check-body {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}
.check-line {
  display: flex;
  gap: 8px;
}
.check-state {
  margin-left: auto;
  color: var(--text-faint);
}
.error {
  font-size: var(--text-sm);
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  color: var(--danger);
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
</style>
