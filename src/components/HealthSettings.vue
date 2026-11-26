<script setup lang="ts">
// Health & setup: the terminal's `dial doctor`, plus putting `dial` on the terminal's PATH.
// Works without a project. Nothing installs until the user has seen what would run.
import { computed, onMounted, ref } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import { setupApi, showToast, snapshot, type Capability, type CliLink, type DoctorReport } from "../bridge";
import { needsEngineInstall, updates } from "../updates";
import { ui } from "../ui";
import Icon from "./Icon.vue";
import SGroup from "./settings/SGroup.vue";
import SRow from "./settings/SRow.vue";

const preview = computed(() => snapshot.value?.connection.state === "preview");

// ── Capabilities ─────────────────────────────────────────────────────────────

const report = ref<DoctorReport | null>(null);
const loading = ref(false), error = ref(""), query = ref("");
async function load() {
  if (preview.value || loading.value) return;
  loading.value = true;
  error.value = "";
  try {
    report.value = await setupApi.doctor();
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}
const shown = (c: Capability) => !query.value.trim() || `${c.name} ${c.id}`.toLowerCase().includes(query.value.trim().toLowerCase());
const installed = computed(() => (report.value?.capabilities ?? []).filter((c) => c.installed && shown(c)));
const available = computed(() => (report.value?.capabilities ?? []).filter((c) => !c.installed && shown(c)));
const total = computed(() => report.value?.capabilities.length ?? 0);

/** The capability whose setup is previewed or running, and what the engine printed. */
const open = ref(""), plan = ref(""), planError = ref(""), busy = ref<"" | "preview" | "install">("");
const log = ref("");
async function previewFix(c: Capability) {
  if (busy.value) return;
  if (open.value === c.id) { open.value = ""; return; }
  open.value = c.id;
  plan.value = planError.value = log.value = "";
  busy.value = "preview";
  try {
    plan.value = await setupApi.fix(c.id, false);
  } catch (e) {
    planError.value = String(e);
  } finally {
    busy.value = "";
  }
}
async function install(c: Capability) {
  if (busy.value) return;
  busy.value = "install";
  planError.value = "";
  try {
    log.value = await setupApi.fix(c.id, true);
    showToast(`${c.name} is set up`);
    await load();
    if (report.value?.capabilities.some((x) => x.id === c.id && !x.installed)) planError.value = "The setup finished, but Dial still does not find it. See the output below.";
    else open.value = "";
  } catch (e) {
    planError.value = String(e);
  } finally {
    busy.value = "";
  }
}

// ── `dial` in the terminal ───────────────────────────────────────────────────

const link = ref<CliLink | null>(null), linkError = ref(""), linking = ref(false);
async function loadLink() {
  if (preview.value) return;
  try {
    link.value = await setupApi.cliLink();
    linkError.value = "";
  } catch (e) {
    linkError.value = String(e);
  }
}
async function setLink(remove: boolean) {
  if (linking.value) return;
  linking.value = true;
  linkError.value = "";
  try {
    link.value = await setupApi.setCliLink(remove);
    showToast(remove ? "Removed the dial command" : "dial now runs Desktop’s engine in new terminals");
  } catch (e) {
    linkError.value = String(e);
  } finally {
    linking.value = false;
  }
}
/** The link exists but new terminals do not look in its folder, and Dial knows the profile to fix. */
const pathFix = computed(() => {
  const l = link.value;
  return l?.linked && !l.onPath && l.shell && !l.shell.present ? l.shell : null;
});
/** Append the PATH line to the shell profile, after showing exactly what is added and where. */
async function addPath() {
  const shell = pathFix.value;
  if (!shell || linking.value) return;
  const ok = await ask(
    `Add this line to the end of ${shell.profile}?\n\n${shell.line}\n\nNew terminals then find the dial command. Nothing else in the file changes, and you can delete the line at any time.`,
    { title: "Add dial to PATH", kind: "info", okLabel: "Add line", cancelLabel: "Cancel" },
  ).catch(() => false);
  if (!ok) return;
  linking.value = true;
  linkError.value = "";
  try {
    link.value = await setupApi.addCliPath();
    showToast("Added. Open a new terminal to use dial.");
  } catch (e) {
    linkError.value = String(e);
  } finally {
    linking.value = false;
  }
}
const linkLine = computed(() => {
  const l = link.value;
  if (!l) return linkError.value ? "" : "Checking…";
  if (!l.linked && l.managed) return `Desktop’s terminal command points at a previous engine. Update it to use ${l.engine}, or remove it.`;
  if (l.linked && l.other) return `Added, but your terminal finds ${l.other} first. Remove that one, or choose it as Desktop’s engine in Updates.`;
  if (l.linked && !l.onPath && l.shell?.present) return `Added. ${l.shell.profile} puts its folder on PATH, so open a new terminal to use it.`;
  if (l.linked && !l.onPath && l.shell) return `Added at ${l.link}, but new terminals do not look in that folder yet. Dial can add one line to ${l.shell.profile} to fix that.`;
  if (l.linked && !l.onPath) return `Added at ${l.link}, but that folder is not on your PATH yet. Add it in your shell profile, then open a new terminal.`;
  if (l.linked) return `Typing dial in a new terminal runs the same engine as Desktop.`;
  if (l.other) return `Your terminal already has its own dial at ${l.other}. Desktop uses ${l.engine}.`;
  if (l.blocked) return `${l.link} exists and was not made by Desktop, so it is left alone.`;
  return "Desktop’s engine is private to this app. Add a dial command to use the same engine from a terminal.";
});
const linkStatus = computed(() => {
  const l = link.value;
  if (!l) return null;
  if (l.linked && !l.other && l.onPath) return { text: "On PATH", kind: "ok" as const };
  if (l.linked && !l.other && l.shell?.present) return { text: "Ready in new terminals", kind: "ok" as const };
  if (l.linked) return { text: "Needs attention", kind: "warn" as const };
  return l.other ? { text: "Separate install", kind: "warn" as const } : { text: "Not added" };
});
/** One line at the top: all good, or how many things need a look. Optional capabilities that
 *  are not set up are not problems; a missing engine or a broken terminal command is. */
const summary = computed(() => {
  const issues = [
    needsEngineInstall(updates.engine?.state) && "the engine",
    linkStatus.value?.kind === "warn" && "the terminal command",
    !!error.value && "the capability check",
  ].filter(Boolean) as string[];
  if (issues.length) return { kind: "warn", text: `${issues.length === 1 ? "1 thing needs" : `${issues.length} things need`} a look: ${issues.join(", ")}.` };
  if (!report.value || !link.value) return { kind: "", text: "Checking your setup…" };
  const ready = report.value.capabilities.filter((c) => c.installed).length;
  return { kind: "ok", text: `All good. ${ready} of ${total.value} optional capabilities are set up.` };
});

onMounted(() => { void load(); void loadLink(); });
</script>

<template>
  <p v-if="preview" class="s-note">The design preview has no engine, so there is nothing to check.</p>
  <template v-else>
    <p class="health-summary" :class="summary.kind" role="status"><span class="dot" aria-hidden="true" />{{ summary.text }}</p>
    <div v-if="needsEngineInstall(updates.engine?.state)" class="s-banner" role="status">
      <span>Desktop needs a working Dial engine. Install or choose one in Updates.</span>
      <button class="btn btn-primary" @click="ui.settingsTab = 'updates'">Open Updates</button>
    </div>

    <SGroup title="Terminal command" description="Use the engine Desktop runs from your own terminal.">
      <SRow label="dial command" :description="linkLine" :status="linkStatus">
        <button v-if="pathFix" class="btn btn-primary" :disabled="linking" @click="addPath">
          <Icon name="terminal" :size="14" /> {{ linking ? "Adding…" : "Add to PATH…" }}
        </button>
        <button v-if="link && !link.linked && !link.blocked" class="btn btn-primary" :disabled="linking" @click="setLink(false)">
          <Icon name="terminal" :size="14" /> {{ linking ? "Saving…" : link.managed ? "Update command" : "Add to PATH" }}
        </button>
        <button v-if="link?.managed" class="btn btn-secondary" :disabled="linking" @click="setLink(true)">Remove</button>
        <template v-if="link || linkError" #below-text>
          <code v-if="link" class="path" :title="link.link">{{ link.link }} → {{ link.engine }}</code>
          <p v-if="linkError" role="alert" class="s-error">{{ linkError }}</p>
        </template>
      </SRow>
    </SGroup>

    <SGroup title="Optional capabilities" :description="report ? `${report.capabilities.filter((c) => c.installed).length} of ${total} set up. Language servers, voice, local runtimes and helpers Dial uses when they exist.` : 'Language servers, voice, local runtimes and helpers Dial uses when they exist.'">
      <template #aside>
        <input v-if="total > 8" v-model="query" class="s-input find" placeholder="Filter" aria-label="Filter capabilities" spellcheck="false" />
        <button class="btn btn-secondary" :disabled="loading" @click="load"><Icon name="refresh" :size="14" /> {{ loading ? "Checking…" : "Check again" }}</button>
      </template>
      <SRow v-if="error" label="Could not run the check" :description="error">
        <button class="btn btn-secondary" @click="load">Try again</button>
      </SRow>
      <SRow v-else-if="!report" label="Checking what is installed…" />
      <template v-else>
        <template v-for="c in available" :key="c.id">
          <SRow :label="c.name" :hint="c.id" :status="{ text: 'Not set up' }">
            <button class="btn btn-secondary" :disabled="!!busy && open !== c.id" :aria-expanded="open === c.id" @click="previewFix(c)">{{ open === c.id ? "Close" : "Set up…" }}</button>
            <template v-if="open === c.id" #below-text>
              <div class="plan">
                <p v-if="busy === 'preview'" class="s-desc">Asking the engine what it would run…</p>
                <pre v-else-if="plan">{{ plan }}</pre>
                <p v-if="planError" role="alert" class="s-error pre">{{ planError }}</p>
                <pre v-if="log">{{ log }}</pre>
                <div v-if="plan" class="plan-actions">
                  <span class="s-desc">Nothing has run yet. Installing runs the command above on this computer.</span>
                  <button class="btn btn-primary" :disabled="!!busy" @click="install(c)"><Icon name="download" :size="14" /> {{ busy === "install" ? "Installing…" : "Install" }}</button>
                </div>
              </div>
            </template>
          </SRow>
        </template>
        <SRow v-for="c in installed" :key="c.name" :label="c.name" :status="{ text: 'Ready', kind: 'ok' }">
          <template v-if="c.path" #below-text><code class="path" :title="c.path">{{ c.path }}</code></template>
        </SRow>
        <SRow v-if="!available.length && !installed.length" label="Nothing matches" description="Try a shorter word." />
      </template>
    </SGroup>

    <SGroup v-if="report?.notes.length" title="Memory and services" description="What the engine reports about its optional services.">
      <pre class="notes">{{ report.notes.join("\n") }}</pre>
    </SGroup>
  </template>
</template>

<style scoped>
.path {
  display: block;
  margin-top: 4px;
  overflow: hidden;
  font: var(--text-xs) / 1.5 var(--font-mono);
  color: var(--text-faint);
  text-overflow: ellipsis;
  white-space: nowrap;
}
.find {
  width: 140px;
}
.plan {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 8px;
}
pre {
  margin: 0;
  padding: 10px 12px;
  max-height: 220px;
  overflow: auto;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--bg);
  font: var(--text-xs) / 1.55 var(--font-mono);
  color: var(--text-muted);
  white-space: pre-wrap;
  user-select: text;
  -webkit-user-select: text;
}
.notes {
  border: none;
  max-height: none;
  background: none;
}
.plan-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.pre {
  white-space: pre-wrap;
}
.health-summary {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0 0 var(--space-md);
  font-size: 13.5px;
  color: var(--text-muted);
}
.health-summary .dot { width: 8px; height: 8px; flex: none; border-radius: 50%; background: var(--text-faint); }
.health-summary.ok .dot { background: var(--success); }
.health-summary.warn { color: var(--text); }
.health-summary.warn .dot { background: var(--warning); }
</style>
