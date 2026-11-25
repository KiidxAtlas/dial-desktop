<script setup lang="ts">
// The App panel's host setup: the hypervisor built into this computer (Apple Virtualization on a Mac,
// Hyper-V on Windows, KVM on Linux), each part it needs, and an install button for what is missing.
// Presentation only. The parent (AppPane) calls the backend; the backend owns every command and asks for
// native confirmation before it changes anything.
import type { AppHostSetup } from "../../appRunner";

defineProps<{
  setup: AppHostSetup | null;
  loading: boolean;
  error: string | null;
  /** The step whose install is running, if any. Every button is locked while one runs. */
  busy: string | null;
  /** The newest progress line per step. */
  progress: Record<string, string>;
  /** Every progress line per step, for **Show log**. */
  log: Record<string, string[]>;
  /** A cancel was asked for the running step. */
  cancelling: boolean;
  /** Locked by the parent (a start or import in flight). */
  locked: boolean;
}>();

const emit = defineEmits<{ install: [step: string]; import: []; refresh: []; cancel: [] }>();

const HOSTS: Record<string, string> = { macos: "This Mac", windows: "This PC", linux: "This computer", other: "This computer" };
const GUESTS: Record<string, string> = { macos: "macOS", windows: "Windows", linux: "Linux", default: "Guest" };
/** One vocabulary for every readiness list: Ready, Needs setup (the user must act), Missing (Dial can install it). */
const MARKS: Record<string, string> = { ready: "✓", blocked: "!", missing: "✕" };
const STATES: Record<string, string> = { ready: "Ready", blocked: "Needs setup", missing: "Missing" };
</script>

<template>
  <section class="host" aria-label="Virtualization on this computer">
    <div class="head">
      <h3 v-if="setup">
        {{ HOSTS[setup.platform] }} runs {{ GUESTS[setup.guest] ?? setup.guest }} guests with <strong>{{ setup.backend }}</strong>
      </h3>
      <h3 v-else>Virtualization on this computer</h3>
      <span class="spacer" />
      <button type="button" class="btn btn-secondary" :disabled="loading || !!busy" @click="emit('refresh')">Check again</button>
    </div>
    <div v-if="loading && !setup" class="hint" role="status">Checking this computer…</div>
    <div v-if="error" class="error" role="alert">{{ error }}</div>
    <ul v-if="setup" class="steps">
      <li v-for="step in setup.steps" :key="step.id" class="step" :class="step.status">
        <span class="mark" aria-hidden="true">{{ MARKS[step.status] ?? '•' }}</span>
        <div class="body">
          <div class="line">
            <span class="label">{{ step.label }}</span>
            <span class="state">{{ STATES[step.status] ?? step.status }}</span>
          </div>
          <p class="detail">{{ step.detail }}</p>
          <p v-if="progress[step.id]" class="progress" role="status">{{ progress[step.id] }}</p>
          <button
            v-if="step.status === 'missing' && step.action"
            type="button"
            class="btn btn-primary"
            :disabled="!!busy || locked"
            :title="step.action.admin ? 'Your system will ask for administrator approval' : undefined"
            @click="step.action.kind === 'import' ? emit('import') : emit('install', step.id)"
          >
            {{ busy === step.id ? 'Working…' : step.action.label }}<span v-if="step.action.admin" class="admin"> (admin)</span>
          </button>
          <template v-if="busy === step.id && step.action?.kind === 'install'">
            <button v-if="step.action.cancellable" type="button" class="btn btn-secondary" :disabled="cancelling" @click="emit('cancel')">
              {{ cancelling ? 'Cancelling…' : 'Cancel' }}
            </button>
            <p v-else class="hint">
              This step runs with administrator approval, which Dial cannot stop once it has started. It ends when the
              system installer finishes or you decline its prompt.
            </p>
          </template>
          <details v-if="log[step.id]?.length" class="log">
            <summary>Show log ({{ log[step.id].length }} line{{ log[step.id].length === 1 ? '' : 's' }})</summary>
            <pre>{{ log[step.id].join('\n') }}</pre>
          </details>
        </div>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.host {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
}
.head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.head h3 {
  margin: 0;
  font-size: var(--text-sm);
  font-weight: 400;
}
.spacer {
  flex: 1;
}
.steps {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.step {
  display: flex;
  gap: 8px;
  font-size: var(--text-sm);
}
.mark {
  flex: none;
  width: 16px;
  text-align: center;
  color: var(--text-faint);
}
.step.ready .mark {
  color: var(--success);
}
.step.blocked .mark {
  color: var(--warning);
}
.step.missing .mark {
  color: var(--danger);
}
.body {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
  flex: 1;
  align-items: flex-start;
}
.line {
  align-self: stretch;
  display: flex;
  gap: 8px;
}
.label {
  font-weight: 600;
}
.state {
  margin-left: auto;
  color: var(--text-faint);
}
.detail,
.progress,
.hint {
  margin: 0;
  color: var(--text-faint);
  font-size: var(--text-sm);
}
.progress {
  font-family: var(--font-mono, monospace);
  overflow-wrap: anywhere;
}
.error {
  color: var(--danger);
  font-size: var(--text-sm);
}
.log summary {
  cursor: pointer;
  font-size: 12px;
  font-size: var(--text-sm);
}
.log,
.log pre {
  align-self: stretch;
}
.log pre {
  margin: 4px 0 0;
  max-height: 220px;
  overflow: auto;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  background: var(--surface);
  font-family: var(--font-mono, monospace);
  font-size: var(--text-xs);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.admin {
  color: var(--text-faint);
}
</style>
