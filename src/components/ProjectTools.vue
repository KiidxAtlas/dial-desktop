<script lang="ts">
// Project tools the terminal reaches with /usage, /skills, /extensions, /checks, /dirs,
// /memory and /schedule, drawn as Settings pages.
export type ProjectTab = "usage" | "skills" | "extensions" | "checks" | "folders" | "memory" | "schedules";
export const PROJECT_TABS: { id: ProjectTab; label: string }[] = [
  { id: "usage", label: "Usage & stats" },
  { id: "memory", label: "Memory" },
  { id: "skills", label: "Skills" },
  { id: "extensions", label: "Extensions" },
  { id: "checks", label: "Checks" },
  { id: "folders", label: "Extra folders" },
  { id: "schedules", label: "Schedules" },
];
</script>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { ask, open as openDialog } from "@tauri-apps/plugin-dialog";
import type { OpenSnapshot } from "../types";
import { showToast } from "../bridge";
import { openTaskTerminal } from "../commandRunner";
import { dueLabel, nextRun, saveSchedule, schedulerHeadline, type SchedulerStatus } from "../schedules";
import { applySaved, count, engineCall, loadEngineCommands, readable, tryEngine, usd } from "../engineTools";
import { installedPackages, listedSkills, type SkillPackage } from "../skills";
import SPage from "./settings/SPage.vue";
import SGroup from "./settings/SGroup.vue";
import SRow from "./settings/SRow.vue";
import SSwitch from "./settings/SSwitch.vue";
import SSegmented from "./settings/SSegmented.vue";

const props = defineProps<{ which: ProjectTab; snap: OpenSnapshot }>();
const live = computed(() => props.snap.connection.state === "live");
const loading = ref(false);
const error = ref("");
const data = ref<any>(null);
const extra = ref<any>(null);
const range = ref("7d");

const TITLES: Record<ProjectTab, [string, string]> = {
  usage: ["Usage & stats", "Plan limits for signed-in providers, this session's tokens, and your recorded usage."],
  memory: ["Memory", "Facts Dial remembers for this project and for you everywhere. Dial recalls them in later sessions."],
  skills: ["Skills", "Reusable procedures Dial can load for bigger tasks. Turn them on or off for the model."],
  extensions: ["Extensions", "MCP servers and other add-ons the engine knows about."],
  checks: ["Checks", "Commands Dial runs after it edits files, so mistakes surface right away."],
  folders: ["Extra folders", "Folders outside this project that Dial may read and write."],
  schedules: ["Schedules", "Prompts that run on a timer or when files change, for this project."],
};

async function load() {
  if (!live.value) return;
  loading.value = true;
  error.value = "";
  try {
    switch (props.which) {
      case "usage": {
        const [limits, session, stats] = await Promise.all([
          engineCall("usage").catch(() => null),
          engineCall("get_session_stats").catch(() => null),
          engineCall("stats", { range: range.value }),
        ]);
        data.value = { limits, session, stats };
        break;
      }
      case "memory":
        data.value = await engineCall("memory", { command: "view" });
        extra.value = await engineCall("memory", { command: "status" }).catch(() => null);
        break;
      case "skills":
        [data.value, extra.value] = await Promise.all([engineCall("list_skills"), engineCall("list_skills", { action: "installed" })]);
        break;
      case "extensions":
        data.value = await engineCall("list_extensions");
        break;
      case "checks":
        data.value = await engineCall("list_checks");
        break;
      case "folders":
        data.value = await engineCall("list_dirs");
        break;
      case "schedules":
        [data.value, extra.value] = await Promise.all([engineCall("list_schedules"), engineCall("schedule_status").catch(() => null)]);
        break;
    }
  } catch (e) {
    error.value = readable(e);
  } finally {
    loading.value = false;
  }
}
onMounted(load);
// The engine can connect after the page opened (or reconnect); load what it has then.
watch(live, (now) => {
  if (now) void load();
});
/** A change was saved, but the running session did not pick it up. */
function reloadFailed(e: unknown) {
  showToast(`Saved, but could not reload the running session: ${readable(e)}. Restart the engine to use it.`);
}

// ── Usage ──
interface Limit { id: string; label: string; amount: { used_fraction: number; remaining: number; unit: string }; window?: { resets_at_ms?: number | null; label?: string } }
const providers = computed<{ provider: string; plan?: string; error?: string | null; limits: Limit[] }[]>(() =>
  (data.value?.limits?.providers ?? []).map((p: any) => ({ provider: p.provider, plan: p.plan, error: p.error, limits: p.report?.limits ?? [] })),
);
function resets(ms?: number | null) {
  if (!ms) return "";
  const mins = Math.max(0, Math.round((ms - Date.now()) / 60000));
  return mins < 60 ? `resets in ${mins}m` : mins < 48 * 60 ? `resets in ${Math.round(mins / 60)}h` : `resets ${new Date(ms).toLocaleDateString()}`;
}
const session = computed(() => {
  const s = data.value?.session;
  if (!s) return null;
  const m = s.metrics ?? {};
  return [
    ["Messages", count(s.messages)],
    ["Input", count(m.prompt_tokens)],
    ["Output", count(m.output_tokens)],
    ["Cached", count(m.cached_tokens)],
    ["Cost", usd(s.observed_cost ?? s.cost)],
  ];
});
const report = computed(() => data.value?.stats?.report ?? null);
const overall = computed(() => {
  const r = report.value;
  if (!r) return [];
  const o = r.overall ?? {};
  const s = r.sessions ?? {};
  return [
    ["Requests", count(o.requests)],
    ["Input tokens", count(o.input)],
    ["Output tokens", count(o.output)],
    ["Cache hit", o.cache_rate != null ? `${Math.round(o.cache_rate)}%` : "—"],
    ["Sessions", count(s.sessions)],
    ["Tool calls", count(s.tool_calls)],
    ["Cost", usd(o.cost_usd)],
    ["Errors", o.error_rate != null ? `${o.error_rate.toFixed(1)}%` : "—"],
  ];
});
const daily = computed<{ day: string; requests: number }[]>(() => report.value?.daily ?? []);
const dayMax = computed(() => Math.max(1, ...daily.value.map((d) => d.requests)));
const models = computed<any[]>(() => (report.value?.by_model ?? []).slice(0, 6));
const tools = computed<any[]>(() => (report.value?.by_tool ?? []).slice(0, 8));
const RANGES = [{ value: "1d", label: "1d" }, { value: "7d", label: "7d" }, { value: "30d", label: "30d" }, { value: "", label: "All" }];
function setRange(r: string) {
  range.value = r;
  void load();
}

// ── Memory ──
interface Fact { ref: string; id: number; content: string; revision: number; source: string; updated: number }
const banks = computed<{ scope: string; facts: Fact[]; total: number }[]>(() => data.value?.view?.banks ?? []);
const newFact = ref("");
const newGlobal = ref(false);
const editing = ref("");
const editText = ref("");
const memoryQuery = ref("");
const hits = ref<Fact[] | null>(null);
async function memory(command: string, what: string) {
  const done = await tryEngine("memory", { command }, what);
  if (done !== null) await load();
  return done !== null;
}
async function saveFact() {
  const text = newFact.value.trim().replace(/\s+/g, " ");
  if (!text) return;
  if (await memory(`save ${newGlobal.value ? "--global " : ""}${text}`, "Could not save")) newFact.value = "";
}
async function updateFact(f: Fact) {
  const text = editText.value.trim().replace(/\s+/g, " ");
  editing.value = "";
  if (text && text !== f.content) await memory(`update ${f.ref} ${f.revision} ${text}`, "Could not update");
}
async function forgetFact(f: Fact) {
  if (await ask(`Forget “${f.content.slice(0, 80)}”?`, { title: "Forget fact", kind: "warning", okLabel: "Forget" }))
    await memory(`forget ${f.ref} ${f.revision}`, "Could not forget");
}
async function searchMemory() {
  const q = memoryQuery.value.trim();
  if (!q) return (hits.value = null);
  const r = await tryEngine<{ items: Fact[] }>("memory", { command: `search ${q}` }, "Search failed");
  hits.value = r?.items ?? [];
}

// ── Skills ──
const skills = computed(() => listedSkills(data.value?.text ?? ""));
const packages = computed(() => installedPackages(extra.value?.text ?? ""));
const skillFilter = ref("");
const shownSkills = computed(() => {
  const q = skillFilter.value.trim().toLowerCase();
  return q ? skills.value.filter((s) => `${s.name} ${s.description}`.toLowerCase().includes(q)) : skills.value;
});
const busy = ref("");
const installSpec = ref("");
type SkillAction = "install" | "import" | "update" | "remove" | "enable" | "disable";
async function changeSkill(action: SkillAction, target = "", pkg?: SkillPackage) {
  if (busy.value || loading.value) return;
  const root = props.snap.workspace.root;
  busy.value = action;
  try {
    if (action === "import") {
      const path = await openDialog({ directory: true, multiple: false, title: "Choose a folder of skills to import" });
      if (typeof path !== "string") return;
      target = path;
    }
    if (!target) return;
    const label = action[0].toUpperCase() + action.slice(1);
    const summary = action === "import" ? `Import skills from ${target} into ${root}?`
      : action === "install" ? `Install the skill package ${target} in ${root}?`
      : action === "remove" ? `Remove the ${pkg?.scopes.has("project") ? "project" : "user"}-scope package “${target}”? Its bundled skills will no longer be loaded from this copy.`
      : action === "update" ? `Download updates for “${target}” in ${[...(pkg?.scopes ?? [])].join(" and ")} scope?`
      : "";
    if (summary && !(await ask(summary, { title: `${label} skills`, kind: "warning", okLabel: label }))) return;
    const params = action === "import" ? { path: target, scope: "project" } : action === "install" ? { spec: target, scope: "project" } : { name: target };
    const done = await engineCall<{ text: string }>("skill_action", { ...params, action, confirm: true }, root);
    if (action === "install") installSpec.value = "";
    showToast(done.text);
    if (props.snap.workspace.root === root) {
      await applySaved().catch(e => showToast(`Skill change saved, but could not reload the running session: ${readable(e)}`));
      await Promise.all([load(), loadEngineCommands(true)]);
    }
  } catch (e) { showToast(`Could not ${action} skills: ${readable(e)}`); }
  finally { busy.value = ""; }
}

// ── Extensions ──
interface Form { submit: string; target: string; title: string; note?: string; error?: string; fields: { key: string; label: string; hint?: string; required: boolean; secret: boolean }[] }
const catalogQuery = ref("");
const catalog = ref<{ rows: any[]; message?: string; error?: string | null } | null>(null);
const form = ref<Form | null>(null);
const formValues = ref<Record<string, string>>({});
async function act(intent: Record<string, unknown>, what: string) {
  const reply = await tryEngine<{ ok: boolean; message?: string; apply_settings?: boolean; form?: Form | null; catalog?: any }>(
    "extension_action", { intent, confirm: true }, `${what} failed`,
  );
  if (!reply) return null;
  if (reply.form) {
    form.value = reply.form;
    formValues.value = {};
  } else if (reply.message) showToast(reply.message);
  if (reply.apply_settings) await applySaved().catch(reloadFailed);
  return reply;
}
async function extension(x: any, intent: Record<string, unknown>, verb?: string) {
  if (verb === "Remove" && !(await ask(`Remove the MCP server ${x.title ?? x.id}?`, { title: "Remove server", kind: "warning", okLabel: "Remove" }))) return;
  busy.value = x.id;
  const reply = await act(intent, verb ?? "Change");
  busy.value = "";
  if (reply) await load();
}
async function browseCatalog() {
  busy.value = "catalog";
  const reply = await tryEngine<{ catalog: any }>("extension_action", { intent: { op: "catalog_browse", tab: "Mcp", query: catalogQuery.value.trim(), page: 0 } }, "Search failed");
  busy.value = "";
  if (reply) catalog.value = reply.catalog;
}
async function installCatalog(row: any) {
  const command = (row.details ?? []).find((d: string) => d.startsWith("install:")) ?? "";
  if (!(await ask(`Install ${row.title ?? row.id}?\n\n${command}`, { title: "Install MCP server", okLabel: "Install" }))) return;
  busy.value = row.id;
  const reply = await act(row.origin === "curated" ? { op: "install_curated_mcp", id: row.id } : { op: "install_mcp", id: row.id }, "Install");
  busy.value = "";
  if (reply && !reply.form) await load();
}
async function submitForm() {
  const f = form.value;
  if (!f) return;
  const values = Object.fromEntries(Object.entries(formValues.value).filter(([, v]) => v.trim()));
  busy.value = "form";
  const reply = await act({ op: f.submit, id: f.target, values }, "Install");
  busy.value = "";
  if (reply && !reply.form) {
    form.value = null;
    await load();
  }
}

// ── Checks ──
const checkName = ref("");
const checkCommand = ref("");
const checkExt = ref("");
async function addCheck() {
  const name = checkName.value.trim();
  const command = checkCommand.value.trim();
  if (!name || !command) return;
  if (!(await ask(`Dial will run this after every edit:\n\n${command}`, { title: `Add check “${name}”`, okLabel: "Add check" }))) return;
  const file_extensions = checkExt.value.split(/[\s,]+/).map((x) => x.replace(/^\./, "")).filter(Boolean);
  const done = await tryEngine("add_check", { name, command, confirm: true, ...(file_extensions.length ? { file_extensions } : {}) }, "Could not add the check");
  if (done !== null) {
    checkName.value = checkCommand.value = checkExt.value = "";
    await applySaved().catch(reloadFailed);
    await load();
  }
}
async function removeCheck(name: string) {
  if (!(await ask(`Stop running “${name}” after edits?`, { title: "Remove check", kind: "warning", okLabel: "Remove" }))) return;
  if ((await tryEngine("remove_check", { name, confirm: true }, "Could not remove the check")) !== null) {
    await applySaved().catch(reloadFailed);
    await load();
  }
}

// ── Folders ──
async function addFolder() {
  const path = await openDialog({ directory: true, multiple: false, title: "Let Dial use another folder" });
  if (typeof path !== "string") return;
  if ((await tryEngine("add_dir", { path }, "Could not add the folder")) !== null) await load();
}
async function removeFolder(path: string) {
  if (!(await ask(`Stop letting Dial read and write ${path}?`, { title: "Remove folder", kind: "warning", okLabel: "Remove" }))) return;
  if ((await tryEngine("remove_dir", { path }, "Could not remove the folder")) !== null) await load();
}
const dirs = computed<string[]>(() => (data.value?.dirs ?? []).map((d: any) => (typeof d === "string" ? d : d.path ?? JSON.stringify(d))));

// ── Schedules ──
const schedules = computed<any[]>(() => data.value?.schedules ?? []);
const schedulePrompt = ref("");
const trigger = ref<"every" | "watch">("every");
const interval = ref("1h");
const pattern = ref("");
const saving = ref(false);
async function createSchedule() {
  if (saving.value) return;
  const root = props.snap.workspace.root;
  saving.value = true;
  try {
    const draft = { prompt: schedulePrompt.value.trim(), ...(trigger.value === "every" ? { every: interval.value.trim() } : { watch: [pattern.value.trim()] }) };
    if (await saveSchedule(draft, root, summary => ask(summary, { title: "Save schedule", kind: "warning", okLabel: "Save" }), request => engineCall("add_schedule", request, root))) {
      schedulePrompt.value = "";
      showToast("Schedule saved");
      await load();
      await offerBackground(root);
    }
  } catch (e) { showToast(readable(e)); }
  finally { saving.value = false; }
}
/** The old way: a scheduler in a Terminal tab, which ends with the tab. */
async function startRunner() {
  const root = props.snap.workspace.root;
  if (await ask(`Run this project's saved schedules in ${root}? Due tasks may start immediately, with each schedule's saved tool approvals. The runner stays open in the Terminal panel until ended or the project closes.`, { title: "Start scheduler", kind: "warning", okLabel: "Start" })) {
    openTaskTerminal({ kind: "scheduler" }, root);
  }
}
async function removeSchedule(entry: { id: string; prompt: string }) {
  const root = props.snap.workspace.root;
  if (!(await ask(`Remove the schedule “${entry.prompt}”?`, { title: "Remove schedule", kind: "warning", okLabel: "Remove" }))) return;
  try { await engineCall("remove_schedule", { id: entry.id, confirm: true }, root); await load(); }
  catch (e) { showToast(readable(e)); }
}

// The app-owned background scheduler: live through the `scheduler` event plus light polling.
const runner = ref<SchedulerStatus | null>(null);
const runnerBusy = ref(false);
const showLog = ref(false);
const now = ref(Date.now());
const runnerActive = computed(() => runner.value?.phase === "running" || runner.value?.phase === "restarting");
const headline = computed(() => (void now.value, schedulerHeadline(runner.value, !!extra.value?.running)));
const runnerExit = computed(() => {
  const exit = runner.value?.lastExit;
  if (!exit || runner.value?.phase === "running") return "";
  return `Last exit ${exit.code == null ? "without a code" : `code ${exit.code}`} at ${new Date(exit.atMs).toLocaleTimeString()}${runner.value?.restarts ? ` · ${runner.value.restarts} restarts` : ""}`;
});
function nextLabel(s: { entry: { every_secs?: number; created?: number }; last_run?: number | null }) {
  const due = nextRun(s.entry, s.last_run);
  return due == null ? "" : `Next ${dueLabel(due, now.value)}`;
}
async function refreshRunner(withEngine = false) {
  const root = props.snap.workspace.root;
  try {
    const status = await invoke<SchedulerStatus>("scheduler_status", { workspaceRoot: root });
    if (props.snap.workspace.root === root) runner.value = status;
  } catch { /* an older app build: the card shows the engine's view only */ }
  // The engine's lease probe also sees schedulers this app does not own (a Terminal tab).
  if (withEngine && live.value && props.which === "schedules") {
    const [status, list] = await Promise.all([engineCall("schedule_status", {}, root).catch(() => undefined), engineCall("list_schedules", {}, root).catch(() => undefined)]);
    if (props.snap.workspace.root !== root || props.which !== "schedules") return;
    if (status !== undefined) extra.value = status;
    if (list !== undefined && data.value) data.value = list;
  }
}
async function callRunner(action: "scheduler_start" | "scheduler_stop", background?: boolean) {
  const root = props.snap.workspace.root;
  runnerBusy.value = true;
  try {
    runner.value = await invoke<SchedulerStatus>(action, { workspaceRoot: root, background: background ?? null });
    await refreshRunner(true);
  } catch (e) { showToast(readable(e)); }
  finally { runnerBusy.value = false; }
}
function consent(root: string, title: string) {
  return ask(`Run this project's saved schedules in the background while ${root} is open? Due tasks may start immediately, with each schedule's saved tool approvals. No terminal is needed; Dial stops them when the project closes or the app quits.`, { title, kind: "warning", okLabel: "Run in background", cancelLabel: "Not now" });
}
async function startBackground() {
  if (await consent(props.snap.workspace.root, "Start scheduler")) await callRunner("scheduler_start");
}
/** The option: on runs now and whenever the project opens; off stops and stays off. */
async function setBackground(on: boolean) {
  if (!on) return callRunner("scheduler_stop", false);
  if (runnerActive.value || (await consent(props.snap.workspace.root, "Run schedules in the background"))) await callRunner("scheduler_start", true);
}
/** The first saved schedule asks once whether to keep schedules running with the project. */
async function offerBackground(root: string) {
  await refreshRunner(true);
  if (props.snap.workspace.root !== root || runner.value?.background != null || runnerActive.value || extra.value?.running) return;
  const yes = await consent(root, "Run schedules in the background");
  if (props.snap.workspace.root === root) await callRunner(yes ? "scheduler_start" : "scheduler_stop", yes);
}
let timer: ReturnType<typeof setInterval> | undefined;
let ticks = 0;
function follow(on: boolean) {
  clearInterval(timer);
  timer = undefined;
  if (!on) return;
  void refreshRunner();
  timer = setInterval(() => {
    now.value = Date.now();
    // Local status every 5s; the engine's lease and last runs every 15s.
    void refreshRunner(++ticks % 3 === 0);
  }, 5000);
}
let unlisten: UnlistenFn | null = null;
let unmounted = false;
void listen<SchedulerStatus>("scheduler", (e) => {
  if (e.payload.root !== props.snap.workspace.root) return;
  runner.value = e.payload;
  if (props.which === "schedules") void refreshRunner(true);
}).then((stop) => (unmounted ? stop() : (unlisten = stop)), () => {});
watch(() => props.which === "schedules", follow, { immediate: true });
onBeforeUnmount(() => {
  unmounted = true;
  follow(false);
  unlisten?.();
});
</script>

<template>
  <SPage :title="TITLES[which][0]" :description="TITLES[which][1]">
    <template v-if="live" #actions>
      <button class="btn btn-secondary" :disabled="loading || (which === 'skills' && !!busy)" @click="load">Refresh</button>
    </template>

    <SGroup v-if="!live"><div class="s-empty">{{ snap.connection.state === "preview" ? "The design preview has no engine." : "This page loads once the Dial engine is connected." }}</div></SGroup>
    <SGroup v-else-if="error"><div class="s-empty"><span class="bad">{{ error }}</span> <button class="btn btn-secondary" :disabled="loading" @click="load">Try again</button></div></SGroup>
    <SGroup v-else-if="loading && !data"><div class="s-empty">Loading…</div></SGroup>
    <SGroup v-else-if="!data"><div class="s-empty">Not loaded yet. <button class="btn btn-secondary" @click="load">Load</button></div></SGroup>

    <template v-else-if="data">
      <!-- Usage -->
      <template v-if="which === 'usage'">
        <SGroup v-for="p in providers" :key="p.provider" class="cap" :title="p.provider" :description="p.plan">
          <div v-if="p.error" class="s-card-pad bad small">{{ p.error }}</div>
          <div v-else-if="!p.limits.length" class="s-empty">No limits reported.</div>
          <SRow v-for="l in p.limits" :key="l.id" :label="l.label">
            <template #below-text>
              <div class="meter"><i :style="{ width: `${Math.min(100, (l.amount.used_fraction ?? 0) * 100)}%` }" :class="{ hot: l.amount.used_fraction > 0.8 }" /></div>
            </template>
            <span class="s-muted small">{{ Math.round((l.amount.used_fraction ?? 0) * 100) }}% used <template v-if="resets(l.window?.resets_at_ms)">· {{ resets(l.window?.resets_at_ms) }}</template></span>
          </SRow>
        </SGroup>
        <SGroup v-if="session" title="This session" plain>
          <div class="cards">
            <div v-for="[k, v] in session" :key="k" class="card"><span class="s-muted">{{ k }}</span><b>{{ v }}</b></div>
          </div>
        </SGroup>
        <SGroup title="All projects" plain>
          <template #aside><SSegmented :model-value="range" :options="RANGES" label="Usage range" @update:model-value="setRange" /></template>
          <div class="cards">
            <div v-for="[k, v] in overall" :key="k" class="card"><span class="s-muted">{{ k }}</span><b>{{ v }}</b></div>
          </div>
          <div v-if="daily.length > 1" class="chart" :title="'Requests per day'">
            <div v-for="d in daily" :key="d.day" class="bar" :style="{ height: `${(d.requests / dayMax) * 100}%` }" :title="`${d.day}: ${d.requests} requests`" />
          </div>
          <table v-if="models.length" class="table">
            <thead><tr><th>Model</th><th>Requests</th><th>Input</th><th>Output</th><th>Speed</th></tr></thead>
            <tbody>
              <tr v-for="m in models" :key="m.key"><td class="mono">{{ m.label }}</td><td>{{ count(m.requests) }}</td><td>{{ count(m.input) }}</td><td>{{ count(m.output) }}</td><td>{{ Math.round(m.tokens_per_second ?? 0) }} tok/s</td></tr>
            </tbody>
          </table>
          <table v-if="tools.length" class="table">
            <thead><tr><th>Tool</th><th>Calls</th><th>Errors</th><th>Avg time</th></tr></thead>
            <tbody>
              <tr v-for="t in tools" :key="t.key ?? t.label"><td class="mono">{{ t.label ?? t.key }}</td><td>{{ count(t.calls) }}</td><td>{{ (t.error_rate ?? 0).toFixed(1) }}%</td><td>{{ ((t.avg_ms ?? 0) / 1000).toFixed(1) }}s</td></tr>
            </tbody>
          </table>
        </SGroup>
      </template>

      <!-- Memory -->
      <template v-else-if="which === 'memory'">
        <SGroup title="Remember something">
          <form class="s-row" @submit.prevent="saveFact">
            <input v-model="newFact" class="s-input grow" aria-label="Fact to remember" placeholder="e.g. Use pnpm, not npm, in this repo" />
            <label class="opt"><SSwitch v-model="newGlobal" label="Remember everywhere" /> Everywhere</label>
            <button class="btn btn-primary" :disabled="!newFact.trim()">Save</button>
          </form>
          <form class="s-row" @submit.prevent="searchMemory">
            <input v-model="memoryQuery" class="s-input grow" aria-label="Search memory" placeholder="Search what Dial remembers" @input="!memoryQuery && (hits = null)" />
            <button class="btn btn-secondary">Search</button>
          </form>
          <template v-if="extra" #after>
            <p class="s-note">{{ extra.facts ?? 0 }} project facts · {{ extra.userFacts ?? 0 }} everywhere · auto-remember {{ extra.autoRetain }}</p>
          </template>
        </SGroup>
        <SGroup v-if="hits" title="Matches">
          <div v-if="!hits.length" class="s-empty">Nothing matches.</div>
          <div v-for="f in hits" :key="`h${f.ref}`" class="s-row"><span class="grow text">{{ f.content }}</span></div>
        </SGroup>
        <SGroup v-for="b in banks" :key="b.scope" :title="b.scope === 'global' ? 'Everywhere' : 'This project'">
          <template #aside><span class="s-muted small">{{ b.total }}</span></template>
          <div v-if="!b.facts.length" class="s-empty">Nothing yet.</div>
          <div v-for="f in b.facts" :key="f.ref" class="s-row">
            <template v-if="editing === f.ref">
              <input v-model="editText" class="s-input grow" aria-label="Edit fact" @keydown.enter="updateFact(f)" @keydown.esc.stop="editing = ''" />
              <button class="btn btn-primary" @click="updateFact(f)">Save</button>
            </template>
            <template v-else>
              <span class="grow text" @dblclick="editing = f.ref; editText = f.content">{{ f.content }}</span>
              <button class="s-link" @click="editing = f.ref; editText = f.content">Edit</button>
              <button class="s-link danger" @click="forgetFact(f)">Forget</button>
            </template>
          </div>
        </SGroup>
      </template>

      <!-- Skills -->
      <template v-else-if="which === 'skills'">
        <SGroup title="Add skills">
          <form class="s-row" @submit.prevent="changeSkill('install', installSpec.trim())">
            <input v-model="installSpec" class="s-input grow" :disabled="!!busy" aria-label="Skill package to install" placeholder="Install from a git URL or registry/skill" />
            <button class="btn btn-secondary" :disabled="!installSpec.trim() || !!busy || loading">{{ busy === "install" ? "Installing…" : "Install" }}</button>
            <button type="button" class="btn btn-secondary" :disabled="!!busy || loading" @click="changeSkill('import')">{{ busy === "import" ? "Importing…" : "Import folder…" }}</button>
          </form>
        </SGroup>
        <SGroup title="Installed packages">
          <div v-if="!packages.length" class="s-empty">No skill packages installed.</div>
          <div v-for="pkg in packages" :key="pkg.name" class="s-row">
            <div class="s-row-text">
              <div class="s-label"><b class="mono">{{ pkg.name }}</b><span v-for="scope in pkg.scopes" :key="scope" class="s-tag">{{ scope }}</span></div>
              <p class="s-desc">{{ [...pkg.skills].join(', ') }}</p>
            </div>
            <div class="s-control">
              <button class="s-link" :disabled="!!busy || loading" @click="changeSkill('update', pkg.name, pkg)">Update</button>
              <button class="s-link danger" :disabled="!!busy || loading" @click="changeSkill('remove', pkg.name, pkg)">Remove</button>
            </div>
          </div>
        </SGroup>
        <SGroup title="Available skills">
          <template #aside><input v-model="skillFilter" class="s-input filter" aria-label="Filter skills" placeholder="Filter skills" /></template>
          <div v-if="!shownSkills.length" class="s-empty">No skills.</div>
          <div v-for="s in shownSkills" :key="s.name" class="s-row">
            <div class="s-row-text">
              <div class="s-label"><b class="mono">{{ s.name }}</b><span v-if="s.origin" class="s-tag">{{ s.origin }}</span></div>
              <p class="s-desc">{{ s.description }}</p>
            </div>
            <SSwitch :model-value="s.on" :label="s.name" :disabled="!!busy || loading" :title="s.on ? 'Turn off' : 'Turn on'" @update:model-value="changeSkill(s.on ? 'disable' : 'enable', s.name)" />
          </div>
        </SGroup>
      </template>

      <!-- Extensions -->
      <template v-else-if="which === 'extensions'">
        <template v-for="(list, kind) in data" :key="kind">
          <SGroup v-if="Array.isArray(list)" :class="{ cap: kind !== 'mcp' }" :title="kind === 'mcp' ? 'MCP servers' : String(kind)">
            <template #aside><span class="s-muted small">{{ list.length }}</span></template>
            <div v-if="!list.length" class="s-empty">None installed.</div>
            <div v-for="x in list" :key="x.id" class="s-row" :class="{ dim: busy === x.id }">
              <div class="s-row-text">
                <div class="s-label"><b>{{ x.title ?? x.id }}</b><span class="s-tag" :class="{ ok: x.state === 'enabled' }">{{ x.state }}</span></div>
                <p class="s-desc">{{ (x.details ?? []).join(" · ") || x.summary }}</p>
              </div>
              <div v-if="kind === 'mcp'" class="s-control">
                <button class="s-link" :disabled="!!busy" @click="extension(x, { op: 'test_mcp', id: x.id }, 'Test')">Test</button>
                <button class="s-link danger" :disabled="!!busy" @click="extension(x, { op: 'remove_mcp', id: x.id }, 'Remove')">Remove</button>
                <SSwitch :model-value="x.state === 'enabled'" :label="x.title ?? x.id" :disabled="!!busy" :title="x.state === 'enabled' ? 'Turn off' : 'Turn on'" @update:model-value="extension(x, { op: 'toggle_mcp', id: x.id, enable: x.state !== 'enabled' })" />
              </div>
            </div>
          </SGroup>
        </template>
        <SGroup title="Add an MCP server">
          <form class="s-row" @submit.prevent="browseCatalog">
            <input v-model="catalogQuery" class="s-input grow" aria-label="Search the MCP catalog" placeholder="Search the catalog, e.g. github, postgres, browser" />
            <button class="btn btn-secondary" :disabled="busy === 'catalog'">{{ busy === "catalog" ? "Searching…" : "Search" }}</button>
          </form>
          <p v-if="catalog?.message" class="s-card-pad s-desc">{{ catalog.message }}</p>
          <p v-if="catalog?.error" class="s-card-pad bad small">{{ catalog.error }}</p>
          <div v-for="row in catalog?.rows ?? []" :key="row.id ?? row.name" class="s-row">
            <div class="s-row-text">
              <div class="s-label"><b>{{ row.title ?? row.name ?? row.id }}</b></div>
              <p class="s-desc">{{ row.summary ?? row.description ?? "" }}</p>
            </div>
            <button class="btn btn-secondary" :disabled="!!busy || !row.id" @click="installCatalog(row)">Install</button>
          </div>
          <form v-if="form" class="s-row stacked" @submit.prevent="submitForm">
            <b class="s-label">{{ form.title }}</b>
            <p v-if="form.note" class="s-desc">{{ form.note }}</p>
            <p v-if="form.error" class="bad small">{{ form.error }}</p>
            <label v-for="f in form.fields" :key="f.key" class="form-field">
              <span>{{ f.label }}<template v-if="f.required"> *</template></span>
              <input v-model="formValues[f.key]" class="s-input" :type="f.secret ? 'password' : 'text'" :placeholder="f.hint ?? ''" :required="f.required" autocomplete="off" />
            </label>
            <div class="inline">
              <button class="btn btn-primary" :disabled="busy === 'form'">Continue</button>
              <button type="button" class="btn btn-secondary" @click="form = null">Cancel</button>
            </div>
          </form>
        </SGroup>
      </template>

      <!-- Checks -->
      <template v-else-if="which === 'checks'">
        <SGroup title="Your checks">
          <div v-if="!(data.configured ?? []).length" class="s-empty">None yet. Add one, such as your test or lint command.</div>
          <div v-for="c in data.configured ?? []" :key="c.name" class="s-row">
            <div class="s-row-text">
              <div class="s-label"><b>{{ c.name }}</b></div>
              <p class="s-desc mono">{{ c.command }}</p>
            </div>
            <button class="s-link danger" @click="removeCheck(c.name)">Remove</button>
          </div>
          <form class="s-row stacked" @submit.prevent="addCheck">
            <div class="inline">
              <input v-model="checkName" class="s-input narrow" aria-label="Check name" placeholder="Name, e.g. tests" />
              <input v-model="checkCommand" class="s-input grow mono" aria-label="Check command" placeholder="Command, e.g. npm test" />
            </div>
            <div class="inline">
              <input v-model="checkExt" class="s-input grow" aria-label="Only for file types (optional)" placeholder="Only for file types (optional): ts, vue" />
              <button class="btn btn-primary" :disabled="!checkName.trim() || !checkCommand.trim()">Add check</button>
            </div>
          </form>
        </SGroup>
        <SGroup v-if="(data.automatic ?? []).length" title="Automatic" description="For files changed now">
          <div v-for="c in data.automatic" :key="c.name" class="s-row"><span class="grow text">{{ c.name }}</span></div>
        </SGroup>
      </template>

      <!-- Folders -->
      <template v-else-if="which === 'folders'">
        <SGroup title="Folders">
          <template #aside><button class="btn btn-secondary" @click="addFolder">Add folder…</button></template>
          <div class="s-row"><span class="grow text mono">{{ data.root }}</span><span class="s-tag">Project</span></div>
          <div v-if="!dirs.length" class="s-empty">No extra folders. Dial works only inside this project.</div>
          <div v-for="d in dirs" :key="d" class="s-row">
            <span class="grow text mono">{{ d }}</span>
            <button class="s-link danger" @click="removeFolder(d)">Remove</button>
          </div>
        </SGroup>
      </template>

      <!-- Schedules -->
      <template v-else-if="which === 'schedules'">
        <SGroup title="Create a read-only schedule">
          <form class="s-row stacked" @submit.prevent="createSchedule">
            <label class="form-field">Prompt <textarea v-model="schedulePrompt" class="s-input prompt" rows="3" required placeholder="Summarize recent changes" /></label>
            <div class="inline">
              <select v-model="trigger" class="s-select" aria-label="Trigger"><option value="every">Repeat every</option><option value="watch">When files change</option></select>
              <input v-if="trigger === 'every'" v-model="interval" class="s-input grow" aria-label="Interval" required placeholder="30s, 5m, 2h or 1d" />
              <input v-else v-model="pattern" class="s-input grow" aria-label="File pattern" required placeholder="src/**/*.ts" />
              <button class="btn btn-primary" :disabled="saving || !schedulePrompt.trim() || !(trigger === 'every' ? interval : pattern).trim()">Save schedule</button>
            </div>
          </form>
        </SGroup>
        <SGroup title="Scheduler">
          <div class="s-row">
            <div class="s-row-text">
              <div class="s-label"><span class="dot" :class="{ on: headline.ours || !!extra?.running, warn: runner?.phase === 'restarting' || runner?.phase === 'failed' }" /><b>{{ headline.title }}</b></div>
              <p class="s-desc">{{ headline.detail }}</p>
              <p v-if="runnerExit" class="s-desc">{{ runnerExit }}</p>
            </div>
            <button v-if="headline.ours" class="btn btn-secondary" :disabled="runnerBusy" @click="callRunner('scheduler_stop')">Stop</button>
            <button v-else class="btn btn-secondary" :disabled="runnerBusy || !schedules.length || !!extra?.running" @click="startBackground">Start</button>
          </div>
          <SRow label="Run schedules in the background while this project is open">
            <SSwitch :model-value="runner?.background === true" label="Run schedules in the background" :disabled="runnerBusy" @update:model-value="setBackground" />
          </SRow>
          <div class="s-row stacked">
            <div class="inline">
              <button class="s-link" :disabled="!runner?.log.length" @click="showLog = !showLog">{{ showLog ? "Hide log" : "Show log" }}</button>
              <button class="s-link" :disabled="runnerActive || !!extra?.running || !schedules.length" title="A scheduler in a Terminal tab; it ends when the tab closes" @click="startRunner">Open in Terminal instead</button>
            </div>
            <pre v-if="showLog" class="log">{{ runner?.log.join("\n") || "No output yet." }}</pre>
          </div>
        </SGroup>
        <SGroup title="Saved schedules">
          <div v-if="!schedules.length" class="s-empty">No schedules yet.</div>
          <div v-for="s in schedules" :key="s.entry.id" class="s-row">
            <div class="s-row-text">
              <div class="s-label"><b>{{ s.entry.prompt }}</b></div>
              <p class="s-desc">{{ [s.entry.every_secs && `Every ${s.entry.every_secs}s`, ...(s.entry.watch ?? []), s.access, (runnerActive || extra?.running) && nextLabel(s)].filter(Boolean).join(' · ') }}</p>
            </div>
            <button class="s-link danger" @click="removeSchedule(s.entry)">Remove</button>
          </div>
        </SGroup>
      </template>
    </template>
  </SPage>
</template>

<style scoped>
/* Layout inside the shared s-* cards and rows (settings.css); controls come from there too. */
.cap :deep(.s-group-head h3) { text-transform: capitalize; }
.small { font-size: var(--text-sm); }
.bad { color: var(--danger); }
.grow { flex: 1; min-width: 0; overflow-wrap: anywhere; }
.text { font-size: var(--text-ui); color: var(--text); user-select: text; -webkit-user-select: text; }
.mono { font-family: var(--font-mono); }
.dim { opacity: 0.55; }
.inline { display: flex; align-items: center; gap: 8px; }
.narrow { flex: 0 1 180px; }
.filter { width: 200px; }
.opt { display: inline-flex; flex: none; align-items: center; gap: 7px; font-size: var(--text-sm); color: var(--text-muted); white-space: nowrap; cursor: pointer; }
.form-field { display: flex; flex-direction: column; gap: 4px; font-size: var(--text-sm); color: var(--text-muted); }
.prompt { height: auto; padding-block: 7px; resize: vertical; }
.meter { height: 6px; margin-top: 6px; border-radius: 999px; background: var(--hover); overflow: hidden; }
.meter i { display: block; height: 100%; background: var(--accent); border-radius: inherit; }
.meter i.hot { background: var(--warning); }
.cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(120px, 1fr)); gap: 8px; margin-bottom: 14px; }
.card { display: flex; flex-direction: column; gap: 2px; padding: 10px 12px; border: 1px solid var(--border); border-radius: var(--radius-md); background: var(--surface); font-size: var(--text-sm); }
.card b { font-size: var(--text-lg); font-weight: 600; }
.chart { display: flex; align-items: flex-end; gap: 2px; height: 64px; margin-bottom: 14px; }
.chart .bar { flex: 1; min-height: 2px; background: color-mix(in srgb, var(--accent) 70%, transparent); border-radius: 2px 2px 0 0; }
.table { width: 100%; border-collapse: collapse; margin-bottom: 14px; font-size: var(--text-sm); }
.table th { text-align: left; font-weight: 500; color: var(--text-faint); padding: 4px 8px 6px 0; border-bottom: 1px solid var(--border); }
.table td { padding: 5px 8px 5px 0; border-bottom: 1px solid var(--border); }
.dot { flex: none; display: inline-block; width: 7px; height: 7px; border-radius: 50%; background: var(--text-faint); }
.dot.on { background: var(--success); }
.dot.warn { background: var(--warning); }
.log { max-height: 220px; overflow: auto; margin: 0; padding: 8px 10px; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--bg); font-family: var(--font-mono); font-size: 11.5px; white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
