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
  /** Locked by the parent (a start or import in flight). */
  locked: boolean;
}>();

const emit = defineEmits<{ install: [step: string]; import: []; refresh: [] }>();

const HOSTS: Record<string, string> = { macos: "This Mac", windows: "This PC", linux: "This computer", other: "This computer" };
const GUESTS: Record<string, string> = { macos: "macOS", windows: "Windows", linux: "Linux", default: "Guest" };
</script>

<template>
  <section class="host" aria-label="Virtualization on this computer">
    <div class="head">
      <h3 v-if="setup">
        {{ HOSTS[setup.platform] }} runs {{ GUESTS[setup.guest] ?? setup.guest }} guests with <strong>{{ setup.backend }}</strong>
      </h3>
      <h3 v-else>Virtualization on this computer</h3>
      <span class="spacer" />
      <button type="button" class="btn" :disabled="loading || !!busy" @click="emit('refresh')">Check again</button>
    </div>
    <div v-if="loading && !setup" class="hint" role="status">Checking this computer…</div>
    <div v-if="error" class="error" role="alert">{{ error }}</div>
    <ul v-if="setup" class="steps">
      <li v-for="step in setup.steps" :key="step.id" class="step" :class="step.status">
        <span class="mark" aria-hidden="true">{{ step.status === 'ready' ? '✓' : step.status === 'blocked' ? '!' : '•' }}</span>
        <div class="body">
          <div class="line">
            <span class="label">{{ step.label }}</span>
            <span class="state">{{ step.status === 'ready' ? 'Ready' : step.status === 'blocked' ? 'Needs you' : 'Missing' }}</span>
          </div>
          <p class="detail">{{ step.detail }}</p>
          <p v-if="progress[step.id]" class="progress" role="status">{{ progress[step.id] }}</p>
          <button
            v-if="step.status === 'missing' && step.action"
            type="button"
            class="btn primary"
            :disabled="!!busy || locked"
            :title="step.action.admin ? 'Your system will ask for administrator approval' : undefined"
            @click="step.action.kind === 'import' ? emit('import') : emit('install', step.id)"
          >
            {{ busy === step.id ? 'Working…' : step.action.label }}<span v-if="step.action.admin" class="admin"> (admin)</span>
          </button>
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
  font-size: 12px;
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
  font-size: 12px;
}
.mark {
  flex: none;
  width: 16px;
  text-align: center;
  color: var(--text-faint);
}
.step.ready .mark {
  color: var(--accent);
}
.step.blocked .mark {
  color: var(--warning, #b7791f);
}
.body {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
  flex: 1;
}
.line {
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
  font-size: 12px;
}
.progress {
  font-family: var(--font-mono, monospace);
  overflow-wrap: anywhere;
}
.error {
  color: var(--danger, #c0392b);
  font-size: 12px;
}
.admin {
  color: var(--text-faint);
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
