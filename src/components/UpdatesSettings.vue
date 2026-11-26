<script setup lang="ts">
// Settings › Updates: the app's own version and the Dial engine's, each with check and install.
import { computed, onMounted, ref, watch } from "vue";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { diagnosticsApi, engine, showToast, snapshot, type EngineChoice, type EngineOrigin, type StartupReport } from "../bridge";
import {
  checkApp,
  checkEngine,
  engineUpdateAvailable,
  installApp,
  installEngine,
  needsEngineInstall,
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
      const source = s.method === "checkout" ? "built from a source checkout" : `installed with ${INSTALLATIONS[s.installation] ?? s.installation}`;
      if (s.update_available) return `Dial ${s.latest} is available (you have ${s.current}) · ${source}`;
      if (s.latest) return `Dial ${s.current} is up to date · ${source}`;
      return `Dial ${s.current} · could not reach GitHub${s.error ? `: ${s.error}` : ""}`;
    }
    case "unsupported":
      return e.detail;
    case "missing":
      return "No Dial engine was found on this computer.";
    case "unlaunchable":
      return `The selected Dial engine could not start: ${e.error}`;
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

/** How Desktop picked its engine, in the words of `dial-desktop --help`. */
const ORIGINS: Record<EngineOrigin, string> = {
  flag: "Chosen with --dial when Desktop was started",
  env: "Chosen by the DIAL_BIN environment variable",
  choice: "Chosen here with “Use a different engine…”",
  desktop: "The copy Desktop installed (Install Desktop copy)",
  path: "Found on your PATH",
  fallback: "Found in a usual installation folder (not on PATH)",
  missing: "Not found: Desktop looked for DIAL_BIN, the engine chosen here, its own copy, PATH and the usual folders",
};
const startup = ref<StartupReport | null>(null);
const startupError = ref("");
let startupRequest = 0;
async function loadStartup() {
  const run = ++startupRequest;
  startup.value = null;
  startupError.value = "";
  try {
    const result = await diagnosticsApi.startup();
    if (run === startupRequest) startup.value = result;
  } catch (error) {
    if (run === startupRequest) startupError.value = String(error) || "The startup report was unavailable.";
  }
}
onMounted(loadStartup);
// An install or update can switch the engine in use.
watch(() => updates.engineChanged, loadStartup);

// ── Which engine to use (saved in the app's folder; below --dial and DIAL_BIN) ─────────────

const choice = ref<EngineChoice | null>(null);
const overriddenBy = ref<EngineOrigin | null>(null);
const choosing = ref(false), choiceError = ref(""), choiceNotice = ref("");
const choiceLoaded = ref(false), choiceLoadError = ref("");
/** The engine in use changed since this session's engine started: offer a restart. */
const switched = ref(false);
async function loadChoice() {
  choiceLoadError.value = "";
  try {
    const saved = await diagnosticsApi.engineChoice();
    choice.value = saved.choice;
    overriddenBy.value = saved.overriddenBy;
    choiceLoaded.value = true;
  } catch (error) {
    choiceLoadError.value = `Could not read the engine preference: ${error}`;
  }
}
onMounted(loadChoice);
const choiceLine = computed(() => {
  if (!choiceLoaded.value) return choiceLoadError.value || "Reading the engine preference…";
  const c = choice.value;
  const base = !c ? "Automatic: DIAL_BIN, then Desktop’s copy, then PATH and the usual folders."
    : c.kind === "custom" ? `This engine: ${c.path}`
    : c.kind === "path" ? "The engine on your PATH, ahead of Desktop’s copy."
    : "Dial Desktop’s own copy.";
  return overriddenBy.value ? `${base} Not in effect: ${ORIGINS[overriddenBy.value].toLowerCase()}, which wins.` : base;
});
const before = computed(() => startup.value?.engine ?? null);
async function pick(next: EngineChoice | null, done: string) {
  if (choosing.value) return;
  choosing.value = true;
  choiceError.value = choiceNotice.value = "";
  const previous = before.value;
  try {
    startup.value = await diagnosticsApi.setEngineChoice(next);
    ++startupRequest;
    startupError.value = "";
    choice.value = next;
    if (overriddenBy.value) {
      choiceNotice.value = `Saved. ${ORIGINS[overriddenBy.value]}, so this applies once Desktop starts without it.`;
    } else {
      switched.value = switched.value || startup.value.engine !== previous;
      choiceNotice.value = `${done}. New sessions use it${switched.value && snapshot.value?.workspace ? "; restart the engine to switch this one" : ""}. Desktop keeps it on the next start.`;
      void checkEngine();
    }
  } catch (error) {
    choiceError.value = `Could not use that engine: ${error}`;
  } finally {
    choosing.value = false;
  }
}
async function chooseEngineFile() {
  choiceError.value = "";
  let path: string | null = null;
  try {
    const picked = await openDialog({ title: "Choose a Dial engine", multiple: false, directory: false });
    path = typeof picked === "string" ? picked : null;
  } catch (error) {
    choiceError.value = String(error);
  }
  if (!path) return;
  choosing.value = true;
  let version = "";
  try {
    version = await diagnosticsApi.validateEngine(path);
  } catch (error) {
    choiceError.value = `That file did not run as a Dial engine (${path} --version): ${error}`;
    return;
  } finally {
    choosing.value = false;
  }
  await pick({ kind: "custom", path }, `Using ${version}`);
}
function selectEngine(event: Event) {
  const select = event.currentTarget as HTMLSelectElement;
  const next = select.value;
  // Only show the new preference once it is saved; a file pick can also be cancelled.
  select.value = choice.value?.kind ?? "automatic";
  switch (next) {
    case "file": return chooseEngineFile();
    case "path": return pick({ kind: "path" }, "Using the engine on PATH");
    case "desktop": return pick({ kind: "desktop" }, "Using Dial Desktop’s copy");
    case "automatic": return pick(null, "Back to the usual order");
  }
}
const restarting = ref(false);
async function restartNow() {
  restarting.value = true;
  try {
    if (await engine.reconnect()) {
      switched.value = false;
      choiceNotice.value = "This session restarted with the chosen engine.";
      void checkEngine();
    }
  } finally {
    restarting.value = false;
  }
}

/** Everything above as plain text, for a bug report. */
function report() {
  const s = startup.value;
  return [
    `Dial Desktop ${updates.appVersion || "unknown"}`,
    `Engine: ${engineLine.value}`,
    `Engine binary: ${s ? s.engine ?? "none" : "unknown"}${s?.origin ? ` (${ORIGINS[s.origin]})` : ""}`,
    `Connection: ${snapshot.value?.connection.state ?? "unknown"}`,
    `Platform: ${navigator.userAgent}`,
    ...(!s ? [startupError.value ? `Startup diagnostics unavailable: ${startupError.value}` : "Startup diagnostics: loading"]
      : s.warnings.length ? ["Warnings:", ...s.warnings.map((w) => `- ${w}`)] : ["Warnings: none"]),
    ...(updates.engineError ? [`Last engine error: ${updates.engineError}`] : []),
  ].join("\n");
}
async function copyReport() {
  try {
    await navigator.clipboard.writeText(report());
    showToast("Diagnostics copied");
  } catch (error) {
    showToast(`Could not copy: ${error}`);
  }
}
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
      class="engine-row"
      :label="engineStatus ? `Version ${engineStatus.current}` : 'Engine'"
      :description="updates.engineBusy === 'updating' ? 'Updating… (a source or Cargo install rebuilds Dial; this can take minutes)' : updates.engineBusy === 'installing' ? 'Installing the signed Dial release…' : engineLine"
    >
      <button v-if="needsEngineInstall(updates.engine?.state) || manualEngine" class="btn btn-primary" :disabled="!!updates.engineBusy || updates.appPhase !== 'idle'" @click="installEngine">
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
      <template v-if="manualEngine || startup?.engine" #below-text>
        <div v-if="manualEngine" class="s-desc">Installing a separate copy lets Desktop use the latest release without replacing your existing engine.</div>
        <!-- Which binary is running, in the same card as its version: one place for "what engine is this". -->
        <code v-if="startup?.engine" class="path" :title="startup.origin ? `${ORIGINS[startup.origin]}\n${startup.engine}` : startup.engine">{{ startup.engine }}</code>
      </template>
    </SRow>
    <SRow v-if="snapshot?.connection.state !== 'preview'" stacked class="engine-row" label="Engine to use" :description="choiceLine">
      <button v-if="choiceLoadError" class="btn btn-secondary" @click="loadChoice">Try again</button>
      <select v-if="choiceLoaded" class="s-select" aria-label="Engine to use" :value="choice?.kind ?? 'automatic'" :disabled="choosing" @change="selectEngine">
        <option value="automatic">Automatic</option>
        <option value="desktop">Dial Desktop’s copy</option>
        <option value="path">Engine on PATH</option>
        <option v-if="choice?.kind === 'custom'" value="custom">Custom engine</option>
        <option value="file">Use a different engine…</option>
      </select>
      <template v-if="choiceError || choiceNotice || switched" #below-text>
        <p v-if="choiceError" role="alert" class="error">{{ choiceError }}</p>
        <p v-else-if="choiceNotice" role="status" class="s-desc">{{ choiceNotice }}</p>
        <button
          v-if="switched && snapshot?.workspace"
          class="btn btn-primary restart"
          :disabled="restarting || snapshot.turnRunning || !!updates.engineBusy"
          :title="snapshot.turnRunning ? 'Wait for the running turn to finish' : undefined"
          @click="restartNow"
        >{{ restarting ? "Restarting…" : "Restart engine now" }}</button>
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

  <SGroup title="Diagnostics" description="Problems Desktop met while starting, which otherwise only reach a terminal.">
    <template #aside>
      <button class="btn btn-secondary" @click="copyReport">Copy diagnostics</button>
    </template>
    <SRow v-if="startupError" label="Diagnostics unavailable" :description="startupError">
      <button class="btn btn-secondary" @click="loadStartup">Try again</button>
    </SRow>
    <SRow v-else-if="!startup" label="Loading startup diagnostics…" />
    <SRow v-else-if="!startup.warnings.length" label="No problems found" description="No startup warnings were reported." />
    <template v-else>
      <SRow v-for="w in startup?.warnings ?? []" :key="w" label="Warning" :description="w" />
    </template>
  </SGroup>

  <p class="s-note">
    Nothing installs until you ask. To use Dial without this app, install the engine alone with
    <code>curl -fsSL https://dial.rootatlas.dev/install | sh</code> (PowerShell:
    <code>irm https://dial.rootatlas.dev/install | iex</code>) and keep it current with <code>dial update</code>.
  </p>
</template>

<style scoped>
.engine-row :deep(.s-control) {
  flex-wrap: wrap;
}
.engine-row :deep(.s-desc) {
  overflow-wrap: anywhere;
}
code {
  font-family: var(--font-mono);
  font-size: 0.95em;
  color: var(--text-muted);
}
.path {
  display: block;
  margin-top: 4px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
  user-select: text;
  -webkit-user-select: text;
}
.restart {
  margin-top: 8px;
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
