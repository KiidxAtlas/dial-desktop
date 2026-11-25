<script setup lang="ts">
// Model roles and presets: the terminal's Roles tab and /modelpreset.
import { computed, onMounted, ref } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import type { OpenSnapshot } from "../types";
import { ui } from "../ui";
import { engineCall, readable } from "../engineTools";
import { imageChoices, imageModelLabel, type ImageRoute, type RoleModel } from "../imageModels";
import {
  ROLES, applyPreset, assignRole, deletePreset, presetsIn, readSettings, renamePreset, rolesIn, savePreset, unassignRole, validPresetName,
  type Preset, type Selection,
} from "../roles";
import SPage from "./settings/SPage.vue";
import SGroup from "./settings/SGroup.vue";
import SRow from "./settings/SRow.vue";

const props = defineProps<{ snap: OpenSnapshot }>();
interface Listed extends RoleModel { thinking_levels: string[]; sources: string[] }

const live = computed(() => props.snap.connection.state === "live");
const loading = ref(false);
const error = ref("");
const roles = ref<Record<string, Selection>>({});
const presets = ref<Preset[]>([]);
const listed = ref<Listed[]>([]);
const forRole = ref<Record<string, RoleModel[]>>({});
const busy = ref("");

async function load(refresh = false) {
  if (!live.value) return;
  loading.value = true;
  error.value = "";
  try {
    const image = refresh
      ? await engineCall<{ models: RoleModel[] }>("list_models", { purpose: "image", refresh: true })
          .catch(() => engineCall<{ models: RoleModel[] }>("list_models", { purpose: "image" }).catch(() => ({ models: [] })))
      : await engineCall<{ models: RoleModel[] }>("list_models", { purpose: "image" }).catch(() => ({ models: [] }));
    const [flat, models, embedding, route] = await Promise.all([
      readSettings(),
      engineCall<{ models: Listed[] }>("list_models"),
      engineCall<{ models: RoleModel[] }>("list_models", { purpose: "embedding" }).catch(() => ({ models: [] })),
      engineCall<ImageRoute>("image_settings").catch(() => undefined),
    ]);
    roles.value = rolesIn(flat);
    presets.value = presetsIn(flat);
    listed.value = models.models ?? [];
    forRole.value = {
      embedding: (embedding.models ?? []).filter((m) => "provider_label" in m),
      image: imageChoices(image.models ?? [], listed.value, route),
    };
  } catch (e) {
    error.value = readable(e);
  } finally {
    loading.value = false;
  }
}
onMounted(() => load(false));

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
  const by = new Map<string, RoleModel[]>();
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
  <SPage title="Roles & presets" description="Dial hands background work to helper models. Pick one per role, then save the whole set as a preset to switch in one click.">
    <template v-if="live" #actions>
      <button class="btn btn-secondary" :disabled="loading" @click="load(true)">{{ loading ? "Checking…" : "Check & refresh" }}</button>
    </template>

    <SGroup v-if="!live"><div class="s-empty">Connect to the Dial engine to change roles.</div></SGroup>
    <SGroup v-else-if="error"><div class="s-empty bad">{{ error }}</div></SGroup>
    <template v-else>
      <SGroup title="Conversation">
        <SRow label="Main model" description="Chats and writes code with you">
          <span class="current">{{ snap.model?.label ?? "—" }}<span v-if="snap.model?.thinking" class="s-muted"> · {{ snap.model.thinking }}</span></span>
          <button class="btn btn-secondary" @click="ui.modelPickerOpen = true; ui.settingsOpen = false">Change…</button>
        </SRow>
      </SGroup>

      <SGroup title="Helper roles">
        <SRow v-for="r in ROLES" :key="r.id" :label="r.label" :description="r.detail" :class="{ busy: busy === r.id }">
          <template v-if="r.chat">
            <select
              class="s-select"
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
              class="s-select level"
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
              class="s-select"
              :value="roles[r.id] ? key(roles[r.id].provider, roles[r.id].model) : ''"
              :disabled="!!busy || loading"
              :aria-label="`${r.label} model`"
              @change="pickFor(r.id, ($event.target as HTMLSelectElement).value)"
            >
              <option value="">{{ r.id === "image" ? "Unassigned (main model, when it makes images)" : "Unassigned (keyword recall only)" }}</option>
              <option v-if="offList(r.id)" :disabled="r.id === 'image'" :value="key(roles[r.id].provider, roles[r.id].model)">{{ roles[r.id].model }} · {{ roles[r.id].provider }}{{ r.id === 'image' ? ' (not listed)' : '' }}</option>
              <optgroup v-for="[p, list] in groupsFor(r.id)" :key="p" :label="p">
                <option v-for="m in list" :key="m.provider + m.model_id" :value="key(m.provider, m.model_id)">{{ r.id === "image" ? imageModelLabel(m) : (m.label || m.model_id) }}</option>
              </optgroup>
            </select>
          </template>
          <template v-else>
            <span class="current">{{ roles[r.id] ? `${shortName(roles[r.id])} · ${roles[r.id].provider}` : "Unassigned" }}</span>
            <button v-if="roles[r.id]" class="s-link danger" :disabled="!!busy" @click="busyWith(r.id, () => unassignRole(r.id))">Clear</button>
          </template>
          <template v-if="!r.chat && !forRole[r.id]?.length" #below-text>
            <p class="s-desc">No {{ r.id }} models found. Refresh a provider in /models to list them.</p>
          </template>
        </SRow>
        <template #after><p class="s-note">Roles save for every project and apply to this session right away.</p></template>
      </SGroup>

      <SGroup title="Presets">
        <div v-if="!presets.length" class="s-empty">No presets yet. Save the current models below.</div>
        <div v-for="p in presets" :key="p.name" class="s-row" :class="{ busy: busy === p.name }">
          <div class="s-row-text">
            <input v-if="renaming === p.name" v-model="renameTo" class="s-input" :aria-label="`Rename ${p.name}`" @keydown.enter="finishRename(p)" @keydown.esc.stop="renaming = ''" @blur="finishRename(p)" />
            <div v-else class="s-label"><b>{{ p.name }}</b><span v-if="inUse(p)" class="s-tag ok">In use</span></div>
            <p class="s-desc">
              <template v-if="p.main">Main {{ shortName(p.main) }}<template v-if="p.main.thinking"> ({{ p.main.thinking }})</template></template>
              <template v-for="(s, id) in p.roles" :key="id"> · {{ id }} {{ shortName(s) }}</template>
            </p>
          </div>
          <div class="s-control">
            <button class="btn btn-primary" :disabled="!!busy || !snap.sessionId || snap.turnRunning" :title="snap.turnRunning ? 'Wait for the turn to end' : 'Use these models'" @click="use(p)">Apply</button>
            <button class="s-link" :disabled="!!busy" title="Replace with the current models" @click="update(p)">Update</button>
            <button class="s-link" :disabled="!!busy" @click="startRename(p)">Rename</button>
            <button class="s-link danger" :disabled="!!busy" @click="remove(p)">Delete</button>
          </div>
        </div>
        <form class="s-row" @submit.prevent="saveNew">
          <input v-model="newName" class="s-input grow" aria-label="New preset name" placeholder="Name, e.g. cheap-and-fast" :disabled="!!busy" :aria-invalid="!!nameProblem" />
          <button class="btn btn-secondary" :disabled="!newName.trim() || !!nameProblem || !!busy || !main">Save current as preset</button>
        </form>
        <template #after>
          <p v-if="nameProblem" class="s-note bad">{{ nameProblem }}</p>
          <p class="s-note">Applying sets the roles for every project and switches this session to the preset's main model.</p>
        </template>
      </SGroup>
    </template>
  </SPage>
</template>

<style scoped>
/* Cards, rows and controls come from settings.css; only row state and sizes stay here. */
.bad { color: var(--danger); }
.busy { opacity: 0.6; }
.grow { flex: 1; min-width: 0; }
.current { font-size: var(--text-sm); }
.s-select.level { max-width: 150px; }
</style>
