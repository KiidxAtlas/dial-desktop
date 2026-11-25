<script setup lang="ts">
// Storage & processes: the terminal's `dial gc`, `dial images`, `dial ps` and `dial rules`.
// Works without a project. Cleanups show what they would remove before anything is deleted.
import { computed, onMounted, ref } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import { setupApi, showToast, snapshot } from "../bridge";
import Icon from "./Icon.vue";
import SGroup from "./settings/SGroup.vue";
import SRow from "./settings/SRow.vue";

const preview = computed(() => snapshot.value?.connection.state === "preview");
function refreshStorage() {
  void fill(gc, "gc");
  void fill(images, "images");
}
function refreshProcs() {
  void fill(procs, "ps");
}

function size(bytes: unknown) {
  const n = Number(bytes) || 0;
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = n / 1024, i = 0;
  while (value >= 1024 && i < units.length - 1) { value /= 1024; i++; }
  return `${value >= 100 ? Math.round(value) : value.toFixed(1)} ${units[i]}`;
}

interface Part<T> { data: T | null; error: string; busy: boolean }
const part = <T>() => ref<Part<T>>({ data: null, error: "", busy: false });
async function fill<T>(p: { value: Part<T> }, kind: string) {
  if (preview.value || p.value.busy) return;
  p.value = { ...p.value, busy: true, error: "" };
  try {
    p.value = { data: await setupApi.report<T>(kind), error: "", busy: false };
  } catch (e) {
    p.value = { data: p.value.data, error: String(e), busy: false };
  }
}

// ── Storage ──────────────────────────────────────────────────────────────────
const gc = part<any>();
const gcLine = computed(() => {
  const d = gc.value.data;
  if (!d) return gc.value.busy ? "Checking…" : "";
  const blobs = d.blobs ?? {}, archive = d.archive ?? {}, wal = d.wal ?? {};
  const parts = [
    `${blobs.wouldDelete ?? 0} unused stored ${blobs.wouldDelete === 1 ? "file" : "files"} (${size(blobs.bytes)})`,
    `${archive.wouldArchive ?? 0} old ${archive.wouldArchive === 1 ? "session" : "sessions"} to archive`,
  ];
  if (wal.walBytes) parts.push(`${size(wal.walBytes)} of database log to fold in`);
  return parts.join(" · ");
});
const gcWork = computed(() => {
  const d = gc.value.data;
  return !!d && ((d.blobs?.wouldDelete ?? 0) > 0 || (d.archive?.wouldArchive ?? 0) > 0 || !!d.wal?.wouldCheckpoint);
});
async function cleanStorage() {
  if (gc.value.busy) return;
  if (!(await ask(`Clean up now? ${gcLine.value}. Sessions you can still open are kept.`, { title: "Clean up storage", kind: "warning", okLabel: "Clean up" }))) return;
  gc.value = { ...gc.value, busy: true, error: "" };
  try {
    const done = await setupApi.report<any>("gc", { apply: true });
    const errors = [...(done.blobs?.errors ?? []), ...(done.archive?.errors ?? []), ...(done.wal?.errors ?? [])];
    showToast(errors.length ? `Cleaned up with ${errors.length} problem${errors.length === 1 ? "" : "s"}` : `Removed ${done.blobs?.deleted ?? 0} files, archived ${done.archive?.archived ?? 0} sessions`);
    gc.value = { data: null, error: errors.map(String).join("\n"), busy: false };
  } catch (e) {
    gc.value = { ...gc.value, error: String(e), busy: false };
    return;
  }
  const error = gc.value.error;
  await fill(gc, "gc");
  if (error) gc.value = { ...gc.value, error };
}

// ── Image cache ──────────────────────────────────────────────────────────────
const images = part<any>();
const imagesLine = computed(() => {
  const d = images.value.data;
  if (!d) return images.value.busy ? "Checking…" : "";
  return `${d.count ?? 0} ${d.count === 1 ? "image" : "images"} (${size(d.bytes)}) · ${d.expired ?? 0} past the ${d.ttlHours ?? "?"}-hour limit`;
});
async function purgeImages() {
  if (images.value.busy) return;
  images.value = { ...images.value, busy: true, error: "" };
  try {
    const plan = await setupApi.report<any>("images_purge");
    if (!plan.selected) {
      showToast("No expired images to remove");
    } else if (await ask(`Remove ${plan.selected} expired ${plan.selected === 1 ? "image" : "images"} (${size(plan.bytes)})? Images still shown in conversations are kept.`, { title: "Clear expired images", kind: "warning", okLabel: "Remove" })) {
      const done = await setupApi.report<any>("images_purge", { apply: true });
      showToast(`Removed ${done.deleted ?? 0} ${done.deleted === 1 ? "image" : "images"}`);
    }
    images.value = { ...images.value, busy: false };
  } catch (e) {
    images.value = { ...images.value, error: String(e), busy: false };
    return;
  }
  await fill(images, "images");
}

// ── Processes ────────────────────────────────────────────────────────────────
interface Proc { pid: number; kind: string; model: string; phase: string; memory_bytes: number; orphaned: boolean; project: string }
const procs = part<Proc[]>();
const rows = computed<Proc[]>(() => (Array.isArray(procs.value.data) ? procs.value.data : []));
const stopping = ref(0);
async function stopProc(p: Proc) {
  if (stopping.value) return;
  if (!(await ask(`Stop ${p.model || p.kind} (process ${p.pid})? Anything using it will lose it until it starts again.`, { title: "Stop process", kind: "warning", okLabel: "Stop" }))) return;
  stopping.value = p.pid;
  try {
    await setupApi.report("ps_stop", { target: String(p.pid) });
    showToast(`Stopped process ${p.pid}`);
  } catch (e) {
    procs.value = { ...procs.value, error: String(e) };
  } finally {
    stopping.value = 0;
  }
  const error = procs.value.error;
  await fill(procs, "ps");
  if (error) procs.value = { ...procs.value, error };
}

// Recent captured output and restart, so a stuck server needs no `dial ps` in a terminal.
const logsFor = ref(0);
const logs = ref({ busy: false, text: "", error: "" });
async function showLogs(p: Proc) {
  logsFor.value = p.pid;
  logs.value = { busy: true, text: "", error: "" };
  try {
    const out = await setupApi.report<unknown>("ps_logs", { target: String(p.pid) });
    if (logsFor.value === p.pid) logs.value = { busy: false, text: typeof out === "string" ? out : JSON.stringify(out, null, 2), error: "" };
  } catch (e) {
    if (logsFor.value === p.pid) logs.value = { busy: false, text: "", error: String(e) };
  }
}
async function restartProc(p: Proc) {
  if (stopping.value) return;
  if (!(await ask(`Restart ${p.model || p.kind} (process ${p.pid})? It stops, then starts again with the same command.`, { title: "Restart process", kind: "warning", okLabel: "Restart" }))) return;
  stopping.value = -p.pid;
  try {
    const out = await setupApi.report<unknown>("ps_restart", { target: String(p.pid) });
    showToast(typeof out === "string" && out.trim() ? out.trim().split("\n")[0] : `Restarted process ${p.pid}`);
    if (logsFor.value === p.pid) logsFor.value = 0;
  } catch (e) {
    procs.value = { ...procs.value, error: String(e) };
  } finally {
    stopping.value = 0;
  }
  const error = procs.value.error;
  await fill(procs, "ps");
  if (error) procs.value = { ...procs.value, error };
}

// ── Stream rules ─────────────────────────────────────────────────────────────
const rules = part<any>();
const ruleRows = computed<any[]>(() => rules.value.data?.rules ?? []);
const ruleName = (r: any) => (typeof r === "string" ? r : r.name ?? r.id ?? r.path ?? JSON.stringify(r));
const ruleAbout = (r: any) => (typeof r === "string" ? undefined : r.description ?? r.path ?? undefined);

function loadAll() {
  void fill(gc, "gc");
  void fill(images, "images");
  void fill(procs, "ps");
  void fill(rules, "rules");
}
onMounted(loadAll);
</script>

<template>
  <p v-if="preview" class="s-note">The design preview has no engine, so there is nothing to show.</p>
  <template v-else>
    <SGroup title="Storage" description="What Dial keeps on this computer, and what is safe to clear.">
      <template #aside>
        <button class="btn btn-secondary" :disabled="gc.busy || images.busy" @click="refreshStorage"><Icon name="refresh" :size="14" /> Check again</button>
      </template>
      <SRow label="Sessions and stored files" :description="gcLine || gc.error" :status="gc.data ? (gcWork ? { text: 'Can be cleaned', kind: 'warn' } : { text: 'Tidy', kind: 'ok' }) : null">
        <button class="btn btn-secondary" :disabled="gc.busy || !gcWork" @click="cleanStorage">{{ gc.busy ? "Working…" : "Clean up…" }}</button>
        <template v-if="gc.data?.stateDir || (gc.error && gcLine)" #below-text>
          <code v-if="gc.data?.stateDir" class="path" :title="gc.data.stateDir">{{ gc.data.stateDir }}</code>
          <p v-if="gc.error && gcLine" role="alert" class="s-error">{{ gc.error }}</p>
        </template>
      </SRow>
      <SRow label="Image cache" :description="imagesLine || images.error" :status="images.data ? (images.data.expired ? { text: 'Has expired images', kind: 'warn' } : { text: 'Tidy', kind: 'ok' }) : null">
        <button class="btn btn-secondary" :disabled="images.busy || !images.data?.expired" @click="purgeImages">{{ images.busy ? "Working…" : "Clear expired…" }}</button>
        <template v-if="images.data?.store || (images.error && imagesLine)" #below-text>
          <code v-if="images.data?.store" class="path" :title="images.data.store">{{ images.data.store }}</code>
          <p v-if="images.error && imagesLine" role="alert" class="s-error">{{ images.error }}</p>
        </template>
      </SRow>
    </SGroup>

    <SGroup title="Running processes" description="Local models, tool servers and helpers the engine started, across every project.">
      <template #aside>
        <button class="btn btn-secondary" :disabled="procs.busy" @click="refreshProcs"><Icon name="refresh" :size="14" /> {{ procs.busy ? "Checking…" : "Refresh" }}</button>
      </template>
      <SRow v-if="procs.error" label="Could not list processes" :description="procs.error" />
      <SRow v-if="!procs.data && !procs.error" label="Checking…" />
      <SRow v-else-if="procs.data && !rows.length" label="Nothing is running" description="Dial starts these when a session needs them." />
      <SRow
        v-for="p in rows"
        :key="p.pid"
        :label="p.model || p.kind"
        :description="[p.kind, p.phase, size(p.memory_bytes), p.project].filter(Boolean).join(' · ')"
        :hint="`pid ${p.pid}`"
        :status="p.orphaned ? { text: 'Left behind', kind: 'warn' } : null"
      >
        <button class="btn btn-ghost" :disabled="!!stopping || logsFor === p.pid" @click="showLogs(p)">{{ logsFor === p.pid && logs.busy ? "Loading…" : "Output" }}</button>
        <button class="btn btn-secondary" :disabled="!!stopping" @click="restartProc(p)">{{ stopping === -p.pid ? "Restarting…" : "Restart" }}</button>
        <button class="btn btn-secondary" :disabled="!!stopping" @click="stopProc(p)">{{ stopping === p.pid ? "Stopping…" : "Stop" }}</button>
      </SRow>
      <template v-if="logsFor" #after>
        <div class="logs">
          <div class="logs-head">
            <span>Recent output · pid {{ logsFor }}</span>
            <button class="btn btn-ghost" @click="logsFor = 0">Close</button>
          </div>
          <p v-if="logs.error" class="s-note bad">{{ logs.error }}</p>
          <pre v-else>{{ logs.text || (logs.busy ? "Loading…" : "No output captured yet.") }}</pre>
        </div>
      </template>
    </SGroup>

    <SGroup title="Stream rules" description="Rules that watch what the model writes and step in, loaded from your rule files.">
      <SRow v-if="rules.error" label="Could not load rules" :description="rules.error" />
      <SRow v-else-if="!rules.data" label="Checking…" />
      <SRow v-else-if="!ruleRows.length" label="No rules loaded" :description="rules.data.enabled === false ? 'Stream rules are turned off in the engine settings.' : 'Add rule files to your project or home folder to have Dial enforce them.'" />
      <SRow v-for="r in ruleRows" :key="ruleName(r)" :label="ruleName(r)" :description="ruleAbout(r)" />
      <template v-if="rules.data?.warnings?.length" #after><p class="s-note bad">{{ rules.data.warnings.map(String).join("\n") }}</p></template>
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
.logs {
  margin-top: 8px;
  border: 1px solid var(--border);
  border-radius: 8px;
  overflow: hidden;
}
.logs-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 4px 4px 4px 12px;
  border-bottom: 1px solid var(--border);
  font-size: var(--text-xs);
  color: var(--text-muted);
}
.logs pre {
  margin: 0;
  max-height: 320px;
  overflow: auto;
  padding: 10px 12px;
  font: var(--text-xs) / 1.5 var(--font-mono);
  white-space: pre-wrap;
  word-break: break-word;
}
</style>
