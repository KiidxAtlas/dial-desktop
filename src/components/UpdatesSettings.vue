<script setup lang="ts">
// Settings › Updates: the app's own version and the Dial engine's, each with check and install.
import { computed } from "vue";
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
const manualEngine = computed(() =>
  engineUpdateAvailable() && engineStatus.value && !engineStatus.value.can_update ? engineStatus.value.update_command : "",
);

const appLine = computed(() => {
  if (!updates.appEnabled) return "This build does not update itself (development or local build).";
  if (updates.appChecking) return "Checking…";
  if (updates.app) return `Dial Desktop ${updates.app.version} is available`;
  if (updates.appError) return `Could not check: ${updates.appError}`;
  return "Dial Desktop is up to date";
});

const appProgressText = computed(() => {
  const p = updates.appProgress;
  if (p === undefined) return "";
  return p === null ? "Downloading…" : `Downloading… ${Math.round(p * 100)}%`;
});
</script>

<template>
  <h2>Updates</h2>
  <p class="note">
    Dial Desktop and the Dial engine update separately. The engine is the same <code>dial</code> your terminal
    runs, so updating it here updates it there too. Nothing installs until you ask.
  </p>

  <section class="group">
    <h3>Dial Desktop</h3>
    <div class="row">
      <div class="text">
        <div class="title">Version {{ updates.appVersion || "unknown" }}</div>
        <div class="detail">{{ appProgressText || appLine }}</div>
        <div v-if="updates.app?.notes" class="notes">{{ updates.app.notes }}</div>
      </div>
      <div class="actions">
        <button
          v-if="updates.app"
          class="btn btn-primary"
          :disabled="updates.appProgress !== undefined"
          @click="installApp"
        >
          <Icon name="download" :size="14" /> Install and restart
        </button>
        <button
          v-else
          class="btn btn-secondary"
          :disabled="!updates.appEnabled || updates.appChecking"
          @click="checkApp"
        >
          <Icon name="refresh" :size="14" /> Check
        </button>
      </div>
    </div>
  </section>

  <section class="group">
    <h3>Dial engine</h3>
    <div class="row">
      <div class="text">
        <div class="title">{{ engineStatus ? `Version ${engineStatus.current}` : "Engine" }}</div>
        <div class="detail">
          {{ updates.engineBusy === "updating" ? "Updating… (a source or Cargo install rebuilds Dial; this can take minutes)" : updates.engineBusy === "installing" ? "Installing the signed Dial release…" : engineLine }}
        </div>
        <div v-if="manualEngine" class="detail">To update: <code>{{ manualEngine }}</code></div>
      </div>
      <div class="actions">
        <button
          v-if="updates.engine?.state === 'missing'"
          class="btn btn-primary"
          :disabled="!!updates.engineBusy"
          @click="installEngine"
        >
          <Icon name="download" :size="14" /> Install engine
        </button>
        <button
          v-else-if="canUpdateEngine"
          class="btn btn-primary"
          :disabled="!!updates.engineBusy"
          @click="updateEngine"
        >
          <Icon name="download" :size="14" /> Update engine
        </button>
        <button
          class="btn btn-secondary"
          :disabled="updates.engineChecking || !!updates.engineBusy"
          @click="checkEngine"
        >
          <Icon name="refresh" :size="14" /> Check
        </button>
      </div>
    </div>
    <p v-if="updates.engineError" class="error">{{ updates.engineError }}</p>
    <details v-if="updates.engineLog" class="log">
      <summary>Output of the last install or update</summary>
      <pre>{{ updates.engineLog }}</pre>
    </details>
  </section>

  <section class="group">
    <h3>Terminal only</h3>
    <p class="note">
      To use Dial without this app, install the engine alone with
      <code>curl -fsSL https://dial.rootatlas.dev/install | sh</code> (PowerShell:
      <code>irm https://dial.rootatlas.dev/install | iex</code>) and keep it current with <code>dial update</code>.
    </p>
  </section>
</template>

<style scoped>
h2 {
  margin: 0;
  font-family: ui-serif, Georgia, "Times New Roman", serif;
  font-size: 24px;
  font-weight: 400;
}
.note {
  margin: 10px 0 8px;
  font-size: var(--text-sm);
  line-height: 1.55;
  color: var(--text-faint);
}
code {
  font-family: var(--font-mono);
  font-size: 0.95em;
  color: var(--text-muted);
}
.group h3 {
  margin: 26px 0 0;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border-strong);
  font-size: var(--text-xs);
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--text-faint);
}
.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 14px 0;
  border-bottom: 1px solid var(--border);
}
.text {
  min-width: 0;
}
.title {
  font-size: 14px;
}
.detail {
  margin-top: 3px;
  font-size: var(--text-sm);
  color: var(--text-muted);
  overflow-wrap: anywhere;
}
.notes {
  margin-top: 6px;
  font-size: var(--text-sm);
  color: var(--text-faint);
  white-space: pre-wrap;
}
.actions {
  flex: none;
  display: flex;
  gap: 8px;
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
