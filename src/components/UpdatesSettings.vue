<script setup lang="ts">
// Settings › Updates: the app's own version and the Dial engine's, each with check and install.
import { computed } from "vue";
import { engine, snapshot } from "../bridge";
import {
  checkApp,
  checkEngine,
  engineUpdateAvailable,
  installApp,
  installEngine,
  updateEngine,
  updates,
} from "../updates";
import Icon from "./Icon.vue";
import SGroup from "./settings/SGroup.vue";
import SRow from "./settings/SRow.vue";

const INSTALLATIONS: Record<string, string> = {
  installer: "the Dial installer",
  homebrew: "Homebrew",
  cargo: "Cargo",
  nix: "Nix",
};

const engineStatus = computed(() => (updates.engine?.state === "checked" ? updates.engine.status : null));

const engineLine = computed(() => {
  const e = updates.engine;
  if (updates.engineChecking && !e) return "Checking…";
  if (!e) return "Not checked yet";
  switch (e.state) {
    case "checked": {
      const s = e.status;
      const via = INSTALLATIONS[s.installation] ?? s.installation;
      if (s.update_available) return `Dial ${s.latest} is available (you have ${s.current}) · installed with ${via}`;
      if (s.latest) return `Dial ${s.current} is up to date · installed with ${via}`;
      return `Dial ${s.current} · could not reach GitHub${s.error ? `: ${s.error}` : ""}`;
    }
    case "unsupported":
      return e.detail;
    case "missing":
      return "No Dial engine was found on this computer.";
    case "failed":
      return `Could not check: ${e.error}`;
  }
  return "";
});

const canUpdateEngine = computed(
  () =>
    (engineUpdateAvailable() && engineStatus.value?.can_update) || updates.engine?.state === "unsupported",
);
const manualEngine = computed(() => engineStatus.value && !engineStatus.value.can_update && engineUpdateAvailable());

const appLine = computed(() => {
  if (!updates.appEnabled) return "This build does not update itself (development or local build).";
  if (updates.appChecking) return "Checking…";
  if (updates.appError) return `Could not check: ${updates.appError}`;
  if (updates.app) return `Dial Desktop ${updates.app.version} is available`;
  return "Dial Desktop is up to date";
});
</script>

<template>
  <SGroup title="Dial Desktop">
    <SRow :label="`Version ${updates.appVersion || 'unknown'}`" :description="updates.appStatus || appLine">
      <button v-if="updates.app" class="btn btn-primary" :disabled="updates.appPhase !== 'idle' || updates.appChecking || !!updates.engineBusy" @click="installApp">
        <Icon name="download" :size="14" /> {{ updates.appAction }}
      </button>
      <button v-else class="btn btn-secondary" :disabled="!updates.appEnabled || updates.appChecking" @click="checkApp">
        <Icon name="refresh" :size="14" /> Check
      </button>
      <template v-if="updates.app?.notes" #below-text>
        <div class="notes">{{ updates.app.notes }}</div>
      </template>
    </SRow>
  </SGroup>

  <SGroup title="Dial engine" description="Update the selected Dial engine, or install a separate signed release when its updates are managed elsewhere.">
    <SRow
      :label="engineStatus ? `Version ${engineStatus.current}` : 'Engine'"
      :description="updates.engineBusy === 'updating' ? 'Updating… (a source or Cargo install rebuilds Dial; this can take minutes)' : updates.engineBusy === 'installing' ? 'Installing the signed Dial release…' : engineLine"
    >
      <button v-if="updates.engine?.state === 'missing' || manualEngine" class="btn btn-primary" :disabled="!!updates.engineBusy || updates.appPhase !== 'idle'" @click="installEngine">
        <Icon name="download" :size="14" /> Install Desktop copy
      </button>
      <button v-else-if="canUpdateEngine" class="btn btn-primary" :disabled="!!updates.engineBusy || updates.appPhase !== 'idle'" @click="updateEngine">
        <Icon name="download" :size="14" /> Update engine
      </button>
      <button class="btn btn-secondary" :disabled="updates.engineChecking || !!updates.engineBusy" @click="checkEngine">
        <Icon name="refresh" :size="14" /> Check
      </button>
      <button v-if="updates.engineChanged && snapshot?.workspace" class="btn btn-secondary" :disabled="!!updates.engineBusy || snapshot.turnRunning" @click="engine.reconnect()">
        Reconnect this session
      </button>
      <template v-if="manualEngine" #below-text>
        <div class="s-desc">Installing a separate copy lets Desktop use the latest release without replacing your existing engine.</div>
      </template>
    </SRow>
    <template #after>
      <p v-if="updates.engineError" class="error">{{ updates.engineError }}</p>
      <details v-if="updates.engineLog" class="log">
        <summary>Output of the last install or update</summary>
        <pre>{{ updates.engineLog }}</pre>
      </details>
    </template>
  </SGroup>

  <p class="s-note">
    Nothing installs until you ask. To use Dial without this app, install the engine alone with
    <code>curl -fsSL https://dial.rootatlas.dev/install | sh</code> (PowerShell:
    <code>irm https://dial.rootatlas.dev/install | iex</code>) and keep it current with <code>dial update</code>.
  </p>
</template>

<style scoped>
code {
  font-family: var(--font-mono);
  font-size: 0.95em;
  color: var(--text-muted);
}
.notes {
  margin-top: 6px;
  font-size: 12px;
  color: var(--text-faint);
  white-space: pre-wrap;
}
.error {
  margin: 10px 0 0;
  font-size: var(--text-sm);
  color: var(--danger);
  white-space: pre-wrap;
}
.log {
  margin-top: 10px;
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.log pre {
  max-height: 240px;
  overflow: auto;
  padding: 10px;
  border-radius: var(--radius-sm);
  background: var(--surface);
  font: var(--text-xs) / 1.5 var(--font-mono);
  white-space: pre-wrap;
}
</style>
