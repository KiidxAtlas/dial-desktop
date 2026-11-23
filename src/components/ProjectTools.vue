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
import { computed, onMounted, ref } from "vue";
import { ask, open as openDialog } from "@tauri-apps/plugin-dialog";
import type { OpenSnapshot } from "../types";
import { showToast } from "../bridge";
import { openTaskTerminal } from "../commandRunner";
import { saveSchedule } from "../schedules";
import { applySaved, count, engineCall, readable, tryEngine, usd } from "../engineTools";

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
        data.value = await engineCall("list_skills");
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
interface Skill { name: string; on: boolean; origin: string; path: string; description: string }
const skills = computed<Skill[]>(() =>
  String(data.value?.text ?? "")
    .split("\n")
    .filter(Boolean)
    .map((line) => {
      const m = /^(\S+) \[(on|off)(?:; ([^\]]*))?\] (\S+)(?: — (.*))?$/.exec(line);
      return m
        ? { name: m[1], on: m[2] === "on", origin: m[3] ?? "", path: m[4], description: m[5] ?? "" }
        : { name: line, on: true, origin: "", path: "", description: "" };
    }),
);
const skillFilter = ref("");
const shownSkills = computed(() => {
  const q = skillFilter.value.trim().toLowerCase();
  return q ? skills.value.filter((s) => `${s.name} ${s.description}`.toLowerCase().includes(q)) : skills.value;
});
const busy = ref("");
async function toggleSkill(s: Skill) {
  busy.value = s.name;
  // The engine asks for confirmation on disable; the switch itself is the user's choice.
  const done = await tryEngine("skill_action", { action: s.on ? "disable" : "enable", name: s.name, confirm: true }, "Could not change the skill");
  if (done !== null) await applySaved().catch(() => {});
  busy.value = "";
  if (done !== null) await load();
}
const installSpec = ref("");
async function installSkill() {
  const spec = installSpec.value.trim();
  if (!spec) return;
  busy.value = "install";
  const done = await tryEngine("skill_action", { action: "install", spec, scope: "project", confirm: true }, "Could not install");
  busy.value = "";
  if (done !== null) {
    installSpec.value = "";
    showToast("Skill installed");
    await applySaved().catch(() => {});
    await load();
  }
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
  if (reply.apply_settings) await applySaved().catch(() => {});
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
    await applySaved().catch(() => {});
    await load();
  }
}
async function removeCheck(name: string) {
  if (!(await ask(`Stop running “${name}” after edits?`, { title: "Remove check", kind: "warning", okLabel: "Remove" }))) return;
  if ((await tryEngine("remove_check", { name, confirm: true }, "Could not remove the check")) !== null) {
    await applySaved().catch(() => {});
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
    }
  } catch (e) { showToast(readable(e)); }
  finally { saving.value = false; }
}
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
</script>

<template>
  <div class="tools">
    <div class="top">
      <div>
        <h2>{{ TITLES[which][0] }}</h2>
        <p class="note">{{ TITLES[which][1] }}</p>
      </div>
      <button v-if="live" class="btn btn-secondary" :disabled="loading" @click="load">Refresh</button>
    </div>

    <div v-if="!live" class="empty">Connect to the Dial engine to see this.</div>
    <div v-else-if="error" class="empty bad">{{ error }}</div>
    <div v-else-if="loading && !data" class="empty">Loading…</div>

    <template v-else-if="data">
      <!-- Usage -->
      <template v-if="which === 'usage'">
        <section v-for="p in providers" :key="p.provider" class="group">
          <h3>{{ p.provider }}<span v-if="p.plan" class="muted"> · {{ p.plan }}</span></h3>
          <p v-if="p.error" class="bad">{{ p.error }}</p>
          <div v-for="l in p.limits" :key="l.id" class="limit">
            <div class="limit-head">
              <span>{{ l.label }}</span>
              <span class="muted">{{ Math.round((l.amount.used_fraction ?? 0) * 100) }}% used <template v-if="resets(l.window?.resets_at_ms)">· {{ resets(l.window?.resets_at_ms) }}</template></span>
            </div>
            <div class="meter"><i :style="{ width: `${Math.min(100, (l.amount.used_fraction ?? 0) * 100)}%` }" :class="{ hot: l.amount.used_fraction > 0.8 }" /></div>
          </div>
        </section>
        <section v-if="session" class="group">
          <h3>This session</h3>
          <div class="cards">
            <div v-for="[k, v] in session" :key="k" class="card"><span class="muted">{{ k }}</span><b>{{ v }}</b></div>
          </div>
        </section>
        <section class="group">
          <div class="row-head">
            <h3>All projects</h3>
            <div class="segmented">
              <button v-for="r in ['1d', '7d', '30d', '']" :key="r" :class="{ on: range === r }" @click="setRange(r)">{{ r || "All" }}</button>
            </div>
          </div>
          <div class="cards">
            <div v-for="[k, v] in overall" :key="k" class="card"><span class="muted">{{ k }}</span><b>{{ v }}</b></div>
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
        </section>
      </template>

      <!-- Memory -->
      <template v-else-if="which === 'memory'">
        <section class="group">
          <h3>Remember something</h3>
          <form class="inline" @submit.prevent="saveFact">
            <input v-model="newFact" class="field" placeholder="e.g. Use pnpm, not npm, in this repo" />
            <label class="check"><input v-model="newGlobal" type="checkbox" /> Everywhere</label>
            <button class="btn btn-primary" :disabled="!newFact.trim()">Save</button>
          </form>
          <form class="inline" @submit.prevent="searchMemory">
            <input v-model="memoryQuery" class="field" placeholder="Search what Dial remembers" @input="!memoryQuery && (hits = null)" />
            <button class="btn btn-secondary">Search</button>
          </form>
          <p v-if="extra" class="muted small">
            {{ extra.facts ?? 0 }} project facts · {{ extra.userFacts ?? 0 }} everywhere · auto-remember {{ extra.autoRetain }}
          </p>
        </section>
        <section v-if="hits" class="group">
          <h3>Matches</h3>
          <p v-if="!hits.length" class="muted">Nothing matches.</p>
          <div v-for="f in hits" :key="`h${f.ref}`" class="item"><span class="grow">{{ f.content }}</span></div>
        </section>
        <section v-for="b in banks" :key="b.scope" class="group">
          <h3>{{ b.scope === "global" ? "Everywhere" : "This project" }} <span class="muted">{{ b.total }}</span></h3>
          <p v-if="!b.facts.length" class="muted">Nothing yet.</p>
          <div v-for="f in b.facts" :key="f.ref" class="item">
            <template v-if="editing === f.ref">
              <input v-model="editText" class="field grow" @keydown.enter="updateFact(f)" @keydown.esc.stop="editing = ''" />
              <button class="btn btn-primary" @click="updateFact(f)">Save</button>
            </template>
            <template v-else>
              <span class="grow" @dblclick="editing = f.ref; editText = f.content">{{ f.content }}</span>
              <button class="link" @click="editing = f.ref; editText = f.content">Edit</button>
              <button class="link bad" @click="forgetFact(f)">Forget</button>
            </template>
          </div>
        </section>
      </template>

      <!-- Skills -->
      <template v-else-if="which === 'skills'">
        <section class="group">
          <form class="inline" @submit.prevent="installSkill">
            <input v-model="installSpec" class="field" placeholder="Install from a git URL or owner/repo" />
            <button class="btn btn-secondary" :disabled="!installSpec.trim() || busy === 'install'">{{ busy === "install" ? "Installing…" : "Install" }}</button>
          </form>
          <input v-model="skillFilter" class="field" placeholder="Filter skills" />
        </section>
        <section class="group">
          <p v-if="!shownSkills.length" class="muted">No skills.</p>
          <div v-for="s in shownSkills" :key="s.name" class="item">
            <div class="grow">
              <div><b class="mono">{{ s.name }}</b> <span v-if="s.origin" class="tag">{{ s.origin }}</span></div>
              <div class="muted small">{{ s.description }}</div>
            </div>
            <button class="switch" role="switch" :aria-checked="s.on" :class="{ on: s.on }" :disabled="busy === s.name" :title="s.on ? 'Turn off' : 'Turn on'" @click="toggleSkill(s)"><i /></button>
          </div>
        </section>
      </template>

      <!-- Extensions -->
      <template v-else-if="which === 'extensions'">
        <section v-for="(list, kind) in data" :key="kind" class="group">
          <template v-if="Array.isArray(list)">
            <h3>{{ kind === "mcp" ? "MCP servers" : String(kind) }} <span class="muted">{{ list.length }}</span></h3>
            <p v-if="!list.length" class="muted">None installed.</p>
            <div v-for="x in list" :key="x.id" class="item" :class="{ dim: busy === x.id }">
              <div class="grow">
                <div><b>{{ x.title ?? x.id }}</b> <span class="tag" :class="{ ok: x.state === 'enabled' }">{{ x.state }}</span></div>
                <div class="muted small">{{ (x.details ?? []).join(" · ") || x.summary }}</div>
              </div>
              <template v-if="kind === 'mcp'">
                <button class="link" :disabled="!!busy" @click="extension(x, { op: 'test_mcp', id: x.id }, 'Test')">Test</button>
                <button class="link bad" :disabled="!!busy" @click="extension(x, { op: 'remove_mcp', id: x.id }, 'Remove')">Remove</button>
                <button class="switch" role="switch" :aria-checked="x.state === 'enabled'" :class="{ on: x.state === 'enabled' }" :disabled="!!busy" :title="x.state === 'enabled' ? 'Turn off' : 'Turn on'" @click="extension(x, { op: 'toggle_mcp', id: x.id, enable: x.state !== 'enabled' })"><i /></button>
              </template>
            </div>
          </template>
        </section>
        <section class="group">
          <h3>Add an MCP server</h3>
          <form class="inline" @submit.prevent="browseCatalog">
            <input v-model="catalogQuery" class="field" placeholder="Search the catalog, e.g. github, postgres, browser" />
            <button class="btn btn-secondary" :disabled="busy === 'catalog'">{{ busy === "catalog" ? "Searching…" : "Search" }}</button>
          </form>
          <p v-if="catalog?.message" class="muted small">{{ catalog.message }}</p>
          <p v-if="catalog?.error" class="bad small">{{ catalog.error }}</p>
          <div v-for="row in catalog?.rows ?? []" :key="row.id ?? row.name" class="item">
            <div class="grow"><b>{{ row.title ?? row.name ?? row.id }}</b><div class="muted small">{{ row.summary ?? row.description ?? "" }}</div></div>
            <button class="btn btn-secondary" :disabled="!!busy || !row.id" @click="installCatalog(row)">Install</button>
          </div>
          <form v-if="form" class="form" @submit.prevent="submitForm">
            <b>{{ form.title }}</b>
            <p v-if="form.note" class="muted small">{{ form.note }}</p>
            <p v-if="form.error" class="bad small">{{ form.error }}</p>
            <label v-for="f in form.fields" :key="f.key" class="form-field">
              <span>{{ f.label }}<template v-if="f.required"> *</template></span>
              <input v-model="formValues[f.key]" class="field" :type="f.secret ? 'password' : 'text'" :placeholder="f.hint ?? ''" :required="f.required" autocomplete="off" />
            </label>
            <div class="inline">
              <button class="btn btn-primary" :disabled="busy === 'form'">Continue</button>
              <button type="button" class="link" @click="form = null">Cancel</button>
            </div>
          </form>
        </section>
      </template>

      <!-- Checks -->
      <template v-else-if="which === 'checks'">
        <section class="group">
          <h3>Your checks</h3>
          <p v-if="!(data.configured ?? []).length" class="muted">None yet. Add one, such as your test or lint command.</p>
          <div v-for="c in data.configured ?? []" :key="c.name" class="item">
            <div class="grow"><b>{{ c.name }}</b><div class="mono small muted">{{ c.command }}</div></div>
            <button class="link bad" @click="removeCheck(c.name)">Remove</button>
          </div>
          <form class="stack" @submit.prevent="addCheck">
            <div class="inline">
              <input v-model="checkName" class="field" placeholder="Name, e.g. tests" style="max-width: 180px" />
              <input v-model="checkCommand" class="field mono" placeholder="Command, e.g. npm test" />
            </div>
            <div class="inline">
              <input v-model="checkExt" class="field" placeholder="Only for file types (optional): ts, vue" />
              <button class="btn btn-primary" :disabled="!checkName.trim() || !checkCommand.trim()">Add check</button>
            </div>
          </form>
        </section>
        <section v-if="(data.automatic ?? []).length" class="group">
          <h3>Automatic <span class="muted">for files changed now</span></h3>
          <div v-for="c in data.automatic" :key="c.name" class="item"><span class="grow">{{ c.name }}</span></div>
        </section>
      </template>

      <!-- Folders -->
      <template v-else-if="which === 'folders'">
        <section class="group">
          <div class="item"><span class="grow mono">{{ data.root }}</span><span class="tag">Project</span></div>
          <div v-for="d in dirs" :key="d" class="item">
            <span class="grow mono">{{ d }}</span>
            <button class="link bad" @click="removeFolder(d)">Remove</button>
          </div>
          <div class="actions"><button class="btn btn-secondary" @click="addFolder">Add folder…</button></div>
        </section>
      </template>

      <!-- Schedules -->
      <template v-else-if="which === 'schedules'">
        <section class="group">
          <h3>Create a read-only schedule</h3>
          <form class="form" @submit.prevent="createSchedule">
            <label class="form-field">Prompt <textarea v-model="schedulePrompt" class="field schedule-prompt" rows="3" required placeholder="Summarize recent changes" /></label>
            <div class="inline">
              <select v-model="trigger" class="field" aria-label="Trigger"><option value="every">Repeat every</option><option value="watch">When files change</option></select>
              <input v-if="trigger === 'every'" v-model="interval" class="field" aria-label="Interval" required placeholder="30s, 5m, 2h or 1d" />
              <input v-else v-model="pattern" class="field" aria-label="File pattern" required placeholder="src/**/*.ts" />
              <button class="btn btn-primary" :disabled="saving || !schedulePrompt.trim() || !(trigger === 'every' ? interval : pattern).trim()">Save schedule</button>
            </div>
          </form>
        </section>
        <section class="group">
          <div class="item"><div class="grow"><b>{{ extra?.running ? "Scheduler running" : extra ? "Scheduler stopped" : "Scheduler status unavailable" }}</b><div class="muted small">Saved schedules need a running scheduler. Refresh to check its status.</div></div>
            <button v-if="!extra?.running" class="btn btn-secondary" :disabled="!schedules.length" @click="startRunner">Start scheduler</button></div>
          <p v-if="!schedules.length" class="muted">No schedules yet.</p>
          <div v-for="s in schedules" :key="s.entry.id" class="item">
            <div class="grow"><b>{{ s.entry.prompt }}</b><div class="muted small">{{ [s.entry.every_secs && `Every ${s.entry.every_secs}s`, ...(s.entry.watch ?? []), s.access].filter(Boolean).join(' · ') }}</div></div>
            <button class="link bad" @click="removeSchedule(s.entry)">Remove</button>
          </div>
        </section>
      </template>
    </template>
  </div>
</template>

<style scoped>
.top { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; }
h2 { margin: 0 0 4px; font-size: 18px; font-weight: 600; }
h3 { margin: 0 0 10px; font-size: 13px; font-weight: 600; text-transform: capitalize; }
.note { margin: 0 0 18px; color: var(--text-muted); font-size: var(--text-sm); }
.group { margin-bottom: 22px; }
.muted { color: var(--text-faint); font-weight: 400; }
.small { font-size: var(--text-sm); }
.bad { color: var(--danger); }
.empty { padding: 40px 0; text-align: center; color: var(--text-faint); }
.limit { margin-bottom: 10px; }
.limit-head { display: flex; justify-content: space-between; font-size: var(--text-sm); margin-bottom: 4px; }
.meter { height: 6px; border-radius: 999px; background: var(--hover); overflow: hidden; }
.meter i { display: block; height: 100%; background: var(--accent); border-radius: inherit; }
.meter i.hot { background: var(--warning); }
.cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(120px, 1fr)); gap: 8px; margin-bottom: 14px; }
.card { display: flex; flex-direction: column; gap: 2px; padding: 10px 12px; border: 1px solid var(--border); border-radius: var(--radius-sm); font-size: var(--text-sm); }
.card b { font-size: 16px; font-weight: 600; }
.row-head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 10px; }
.row-head h3 { margin: 0; }
.segmented { display: inline-flex; border: 1px solid var(--border); border-radius: var(--radius-sm); overflow: hidden; }
.segmented button { padding: 3px 10px; font-size: var(--text-sm); color: var(--text-muted); }
.segmented button.on { background: var(--active); color: var(--text); }
.chart { display: flex; align-items: flex-end; gap: 2px; height: 64px; margin-bottom: 14px; }
.chart .bar { flex: 1; min-height: 2px; background: color-mix(in srgb, var(--accent) 70%, transparent); border-radius: 2px 2px 0 0; }
.table { width: 100%; border-collapse: collapse; margin-bottom: 14px; font-size: var(--text-sm); }
.table th { text-align: left; font-weight: 500; color: var(--text-faint); padding: 4px 8px 6px 0; border-bottom: 1px solid var(--border); }
.table td { padding: 5px 8px 5px 0; border-bottom: 1px solid var(--border); }
.item { display: flex; align-items: center; gap: 10px; padding: 8px 0; border-bottom: 1px solid var(--border); }
.item.dim { opacity: 0.55; }
.form { display: flex; flex-direction: column; gap: 8px; margin-top: 12px; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-sm); }
.form-field { display: flex; flex-direction: column; gap: 4px; font-size: var(--text-sm); }
.grow { flex: 1; min-width: 0; overflow-wrap: anywhere; }
.inline { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; }
.stack { margin-top: 12px; }
.field { flex: 1; min-width: 0; height: 32px; padding: 0 10px; border: 1px solid var(--border-strong); border-radius: var(--radius-sm); background: var(--surface); color: var(--text); font: inherit; font-size: var(--text-sm); }
.field.schedule-prompt { flex: none; height: auto; padding-block: 7px; resize: vertical; }
.field:focus { outline: none; border-color: var(--accent); }
.check { display: inline-flex; align-items: center; gap: 4px; font-size: var(--text-sm); color: var(--text-muted); white-space: nowrap; }
.link { font-size: var(--text-sm); color: var(--text-muted); }
.link:hover { color: var(--text); text-decoration: underline; }
.link.bad:hover { color: var(--danger); }
.tag { padding: 0 6px; border: 1px solid var(--border); border-radius: 999px; font-size: 10.5px; color: var(--text-faint); }
.tag.ok { color: var(--success); }
.actions { margin-top: 12px; }
.switch { position: relative; flex: none; width: 28px; height: 16px; border-radius: 999px; background: var(--border-strong); }
.switch i { position: absolute; top: 2px; left: 2px; width: 12px; height: 12px; border-radius: 50%; background: var(--text); transition: transform 140ms var(--ease); }
.switch.on { background: var(--accent); }
.switch.on i { transform: translateX(12px); }
</style>
