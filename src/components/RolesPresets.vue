<script setup lang="ts">
// Model roles and presets: the terminal's Roles tab and /modelpreset.
import { computed, onMounted, ref } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import type { OpenSnapshot } from "../types";
import { ui } from "../ui";
import { engineCall, readable } from "../engineTools";
import {
  ROLES, applyPreset, assignRole, deletePreset, presetsIn, readSettings, renamePreset, rolesIn, savePreset, unassignRole, validPresetName,
  type Preset, type Selection,
} from "../roles";

const props = defineProps<{ snap: OpenSnapshot }>();
interface Listed { provider: string; model_id: string; label: string; thinking_levels: string[]; sources: string[] }
/** A `list_models` row for a non-chat role (`purpose: "embedding" | "image"`). */
interface ForRole { provider: string; provider_label?: string; model_id: string; label: string; base_url?: string | null; image_api?: unknown }

const live = computed(() => props.snap.connection.state === "live");
const loading = ref(false);
const error = ref("");
const roles = ref<Record<string, Selection>>({});
const presets = ref<Preset[]>([]);
const listed = ref<Listed[]>([]);
/** Models each non-chat role can take, from the engine's metadata; empty on an engine without
 *  these listings (older engines answer `purpose` with the chat list, recognised and dropped). */
const forRole = ref<Record<string, ForRole[]>>({});
const busy = ref("");

async function load() {
  if (!live.value) return;
  loading.value = true;
  error.value = "";
  try {
    const [flat, models, embedding, image] = await Promise.all([
      readSettings(),
      engineCall<{ models: Listed[] }>("list_models"),
      engineCall<{ models: ForRole[] }>("list_models", { purpose: "embedding" }).catch(() => ({ models: [] })),
      engineCall<{ models: ForRole[] }>("list_models", { purpose: "image" }).catch(() => ({ models: [] })),
    ]);
    roles.value = rolesIn(flat);
    presets.value = presetsIn(flat);
    listed.value = models.models ?? [];
    forRole.value = {
      embedding: (embedding.models ?? []).filter((m) => "provider_label" in m),
      image: (image.models ?? []).filter((m) => "image_api" in m),
    };
  } catch (e) {
    error.value = readable(e);
  } finally {
    loading.value = false;
  }
}
onMounted(load);

const key = (provider: string, model: string) => `${provider}\u0000${model}`;
const providers = computed(() => {
  const by = new Map<string, Listed[]>();
  for (const m of listed.value) by.set(m.provider, [...(by.get(m.provider) ?? []), m]);
  return [...by.entries()].sort(([a], [b]) => a.localeCompare(b));
});
const find = (s?: Selection | null) => (s ? listed.value.find((m) => m.provider === s.provider && m.model_id === s.model) : undefined);
const levels = (s?: Selection | null) => find(s)?.thinking_levels ?? [];
const shortName = (s?: Selection | null) => (s ? find(s)?.label ?? findFor(s)?.label ?? s.model : "");
/** A non-chat role's choices grouped by provider. */
function groupsFor(role: string) {
  const by = new Map<string, ForRole[]>();
  for (const m of forRole.value[role] ?? []) by.set(m.provider_label ?? m.provider, [...(by.get(m.provider_label ?? m.provider) ?? []), m]);
  return [...by.entries()].sort(([a], [b]) => a.localeCompare(b));
}
function findFor(s: Selection) {
  return Object.values(forRole.value).flat().find((m) => m.provider === s.provider && m.model_id === s.model);
}
/** Whether the assigned model is missing from the role's list, so the dropdown still shows it. */
const offList = (role: string) => {
  const s = roles.value[role];
  return !!s && !(forRole.value[role] ?? []).some((m) => m.provider === s.provider && m.model_id === s.model);
};

async function busyWith(id: string, work: () => Promise<unknown>) {
  if (busy.value) return;
  busy.value = id;
  try {
    await work();
    await load();
  } finally {
    busy.value = "";
  }
}

function pickModel(role: string, value: string) {
  if (!value) return busyWith(role, () => unassignRole(role));
  const [provider, model] = value.split("\u0000");
  const current = roles.value[role];
  // A level only carries over when the new model offers it.
  const keep = current?.thinking && listed.value.find((m) => m.provider === provider && m.model_id === model)?.thinking_levels.includes(String(current.thinking));
  return busyWith(role, () => assignRole(role, provider, model, keep ? String(current!.thinking) : null));
}
function pickFor(role: string, value: string) {
  if (!value) return busyWith(role, () => unassignRole(role));
  const [provider, model] = value.split("\u0000");
  const row = (forRole.value[role] ?? []).find((m) => m.provider === provider && m.model_id === model);
  return busyWith(role, () => assignRole(role, provider, model, null, row?.base_url));
}
function pickLevel(role: string, level: string) {
  const s = roles.value[role];
  if (s) return busyWith(role, () => assignRole(role, s.provider, s.model, level || null));
}

// ── Presets ──
const main = computed<Selection | null>(() => {
  const m = props.snap.model;
  return m ? { provider: m.provider, model: m.modelId, ...(m.thinking ? { thinking: m.thinking } : {}) } : null;
});
const newName = ref("");
const nameProblem = computed(() => {
  const n = newName.value.trim();
  if (!n) return "";
  if (!validPresetName(n)) return "Start with a letter; use letters, digits, - or _ (no spaces)";
  if (presets.value.some((p) => p.name === n)) return "A preset with this name exists; use Update on it instead";
  return "";
});
function saveNew() {
  const n = newName.value.trim();
  if (!n || nameProblem.value || !main.value) return;
  return busyWith("save", async () => {
    if (await savePreset(n, main.value!, roles.value)) newName.value = "";
  });
}
async function update(p: Preset) {
  if (!main.value) return;
  if (!(await ask(`Replace ${p.name} with the current conversation model and roles?`, { title: "Update preset", okLabel: "Update" }))) return;
  return busyWith(p.name, () => savePreset(p.name, main.value!, roles.value));
}
async function remove(p: Preset) {
  if (!(await ask(`Delete the preset ${p.name}?`, { title: "Delete preset", kind: "warning", okLabel: "Delete" }))) return;
  return busyWith(p.name, () => deletePreset(p.name));
}
const renaming = ref("");
const renameTo = ref("");
function startRename(p: Preset) {
  renaming.value = p.name;
  renameTo.value = p.name;
}
function finishRename(p: Preset) {
  const to = renameTo.value.trim();
  renaming.value = "";
  if (!to || to === p.name) return;
  if (!validPresetName(to) || presets.value.some((x) => x.name === to)) return void ask(`“${to}” is not a free preset name.`, { title: "Rename preset" });
  return busyWith(p.name, () => renamePreset(p, to));
}
function use(p: Preset) {
  const { workspace, sessionId } = props.snap;
  if (sessionId) return busyWith(p.name, () => applyPreset(p, workspace.root, sessionId));
}
/** Whether the preset matches what is in use now. */
function inUse(p: Preset) {
  const same = (a?: Selection | null, b?: Selection | null) => !!a && !!b && a.provider === b.provider && a.model === b.model;
  const ids = new Set([...Object.keys(p.roles), ...Object.keys(roles.value)]);
  return same(p.main, main.value) && [...ids].every((id) => same(p.roles[id], roles.value[id]));
}
</script>

<template>
  <div class="roles">
    <div class="top">
      <div>
        <h2>Roles &amp; presets</h2>
        <p class="note">Dial hands background work to helper models. Pick one per role, then save the whole set as a preset to switch in one click.</p>
      </div>
      <button v-if="live" class="btn btn-secondary" :disabled="loading" @click="load">Refresh</button>
    </div>

    <div v-if="!live" class="empty">Connect to the Dial engine to change roles.</div>
    <div v-else-if="error" class="empty bad">{{ error }}</div>
    <template v-else>
      <section class="group">
        <h3>Conversation</h3>
        <div class="row">
          <div class="who"><b>Main model</b><span class="muted">Chats and writes code with you</span></div>
          <div class="pick">
            <span class="current">{{ snap.model?.label ?? "—" }}<span v-if="snap.model?.thinking" class="muted"> · {{ snap.model.thinking }}</span></span>
            <button class="btn btn-secondary" @click="ui.modelPickerOpen = true; ui.settingsOpen = false">Change…</button>
          </div>
        </div>
      </section>

      <section class="group">
        <h3>Helper roles</h3>
        <div v-for="r in ROLES" :key="r.id" class="row" :class="{ busy: busy === r.id }">
          <div class="who"><b>{{ r.label }}</b><span class="muted">{{ r.detail }}</span></div>
          <div class="pick">
            <template v-if="r.chat">
              <select
                :value="roles[r.id] ? key(roles[r.id].provider, roles[r.id].model) : ''"
                :disabled="!!busy || loading"
                :aria-label="`${r.label} model`"
                @change="pickModel(r.id, ($event.target as HTMLSelectElement).value)"
              >
                <option value="">Unassigned</option>
                <option v-if="roles[r.id] && !find(roles[r.id])" :value="key(roles[r.id].provider, roles[r.id].model)">{{ roles[r.id].model }} ({{ roles[r.id].provider }})</option>
                <optgroup v-for="[p, list] in providers" :key="p" :label="p">
                  <option v-for="m in list" :key="m.model_id" :value="key(p, m.model_id)">{{ m.label }}</option>
                </optgroup>
              </select>
              <select
                v-if="levels(roles[r.id]).length"
                class="level"
                :value="roles[r.id]?.thinking ?? ''"
                :disabled="!!busy"
                :aria-label="`${r.label} thinking`"
                @change="pickLevel(r.id, ($event.target as HTMLSelectElement).value)"
              >
                <option value="">Default thinking</option>
                <option v-for="l in levels(roles[r.id])" :key="l" :value="l">{{ l }}</option>
              </select>
            </template>
            <template v-else-if="forRole[r.id]?.length">
              <select
                :value="roles[r.id] ? key(roles[r.id].provider, roles[r.id].model) : ''"
                :disabled="!!busy || loading"
                :aria-label="`${r.label} model`"
                @change="pickFor(r.id, ($event.target as HTMLSelectElement).value)"
              >
                <option value="">{{ r.id === "image" ? "Unassigned (main model, when it makes images)" : "Unassigned (keyword recall only)" }}</option>
                <option v-if="offList(r.id)" :value="key(roles[r.id].provider, roles[r.id].model)">{{ roles[r.id].model }} · {{ roles[r.id].provider }}</option>
                <optgroup v-for="[p, list] in groupsFor(r.id)" :key="p" :label="p">
                  <option v-for="m in list" :key="m.provider + m.model_id" :value="key(m.provider, m.model_id)">{{ m.label || m.model_id }}</option>
                </optgroup>
              </select>
            </template>
            <template v-else>
              <span class="current">{{ roles[r.id] ? `${shortName(roles[r.id])} · ${roles[r.id].provider}` : "Unassigned" }}</span>
              <button v-if="roles[r.id]" class="link bad" :disabled="!!busy" @click="busyWith(r.id, () => unassignRole(r.id))">Clear</button>
              <span class="muted small">No {{ r.id }} models found. Refresh a provider in /models to list them.</span>
            </template>
          </div>
        </div>
        <p class="muted small">Roles save for every project and apply to this session right away.</p>
      </section>

      <section class="group">
        <h3>Presets</h3>
        <p v-if="!presets.length" class="muted">No presets yet. Save the current models below.</p>
        <div v-for="p in presets" :key="p.name" class="preset" :class="{ busy: busy === p.name }">
          <div class="who">
            <template v-if="renaming === p.name">
              <input v-model="renameTo" class="field" @keydown.enter="finishRename(p)" @keydown.esc.stop="renaming = ''" @blur="finishRename(p)" />
            </template>
            <b v-else>{{ p.name }} <span v-if="inUse(p)" class="tag ok">In use</span></b>
            <span class="muted small">
              <template v-if="p.main">Main {{ shortName(p.main) }}<template v-if="p.main.thinking"> ({{ p.main.thinking }})</template></template>
              <template v-for="(s, id) in p.roles" :key="id"> · {{ id }} {{ shortName(s) }}</template>
            </span>
          </div>
          <div class="actions">
            <button class="btn btn-primary" :disabled="!!busy || !snap.sessionId || snap.turnRunning" :title="snap.turnRunning ? 'Wait for the turn to end' : 'Use these models'" @click="use(p)">Apply</button>
            <button class="link" :disabled="!!busy" title="Replace with the current models" @click="update(p)">Update</button>
            <button class="link" :disabled="!!busy" @click="startRename(p)">Rename</button>
            <button class="link bad" :disabled="!!busy" @click="remove(p)">Delete</button>
          </div>
        </div>
        <form class="save" @submit.prevent="saveNew">
          <input v-model="newName" class="field" placeholder="Name, e.g. cheap-and-fast" :disabled="!!busy" />
          <button class="btn btn-secondary" :disabled="!newName.trim() || !!nameProblem || !!busy || !main">Save current as preset</button>
        </form>
        <p v-if="nameProblem" class="bad small">{{ nameProblem }}</p>
        <p class="muted small">Applying sets the roles for every project and switches this session to the preset's main model.</p>
      </section>
    </template>
  </div>
</template>

<style scoped>
.top { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; }
h2 { margin: 0 0 4px; font-size: 18px; font-weight: 600; }
h3 { margin: 0 0 6px; font-size: 13px; font-weight: 600; }
.note { margin: 0 0 18px; color: var(--text-muted); font-size: var(--text-sm); }
.group { margin-bottom: 24px; }
.muted { color: var(--text-faint); font-weight: 400; }
.small { font-size: var(--text-sm); }
.bad { color: var(--danger); }
.empty { padding: 40px 0; text-align: center; color: var(--text-faint); }
.row, .preset { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 10px 0; border-bottom: 1px solid var(--border); }
.busy { opacity: 0.6; }
.who { display: flex; flex-direction: column; gap: 2px; min-width: 0; flex: 1; }
.who .muted { font-size: var(--text-sm); }
.pick, .actions { display: flex; align-items: center; gap: 8px; flex: none; }
.current { font-size: var(--text-sm); }
select, .field { height: 30px; max-width: 260px; padding: 0 8px; border: 1px solid var(--border-strong); border-radius: var(--radius-sm); background: var(--surface); color: var(--text); font: inherit; font-size: var(--text-sm); }
select.level { max-width: 150px; }
.field:focus, select:focus { outline: none; border-color: var(--accent); }
.save { display: flex; gap: 8px; margin-top: 12px; }
.save .field { flex: 1; max-width: none; }
.link { font-size: var(--text-sm); color: var(--text-muted); }
.link:hover:not(:disabled) { color: var(--text); text-decoration: underline; }
.link.bad:hover:not(:disabled) { color: var(--danger); }
.tag { margin-left: 6px; padding: 0 6px; border: 1px solid var(--border); border-radius: 999px; font-size: 10.5px; font-weight: 400; color: var(--text-faint); }
.tag.ok { color: var(--success); }
</style>
