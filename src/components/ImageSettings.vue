<script setup lang="ts">
// Image generation: which model makes images, whether that route works, and the generation
// settings for that model (the terminal's `/image settings`). Reads `image_settings`; the model is
// the `image` role, and per-model values save as one `image.models.<key>` entry.
import { computed, onMounted, ref } from "vue";
import type { OpenSnapshot } from "../types";
import { engine, showToast } from "../bridge";
import { applySaved, engineCall, readable } from "../engineTools";
import { assignRole, unassignRole } from "../roles";
import { imageChoices, imageModelLabel, matchesImageRoute, type ImageModelsChecked, type RoleModel } from "../imageModels";
import { ui } from "../ui";
import Icon from "./Icon.vue";
import SPage from "./settings/SPage.vue";
import SGroup from "./settings/SGroup.vue";
import SRow from "./settings/SRow.vue";
import SNumber from "./settings/SNumber.vue";
import SSegmented from "./settings/SSegmented.vue";

const props = defineProps<{ snap: OpenSnapshot }>();

interface Lora { path: string; scale?: number }
interface Entry { model: string; size?: string; quality?: string; background?: string; moderation?: string; steps?: number; seed?: number; strength?: number; loras?: Lora[]; [key: string]: unknown }
interface Page {
  ok: boolean;
  error?: string;
  model?: string;
  api?: string;
  settings?: string[];
  edits?: boolean;
  key?: string;
  entry?: Entry;
  role: { provider: string; model: string; base_url?: string | null } | null;
  options: { sizes: string[]; qualities: string[]; backgrounds: string[]; moderations: string[]; max_steps: number; lora_scale_limit: number; loras: string[] };
}

const live = computed(() => props.snap.connection.state === "live");
const page = ref<Page | null>(null);
const models = ref<RoleModel[]>([]);
const loading = ref(false);
const checking = ref(false);
const busy = ref(false);
const error = ref("");
const checkedSummary = ref<ImageModelsChecked | null>(null);
let autoChecked = false;

async function load(refresh = false) {
  if (!live.value) return;
  loading.value = true;
  if (refresh) checking.value = true;
  error.value = "";
  try {
    let image: { models: RoleModel[]; checked?: ImageModelsChecked };
    if (refresh) {
      // Probe all configured providers first so the engine updates its persistent caches.
      image = await engineCall<{ models: RoleModel[]; checked?: ImageModelsChecked }>("list_models", {
        purpose: "image",
        refresh: true,
      }).catch(() => engineCall<{ models: RoleModel[] }>("list_models", { purpose: "image" }).catch(() => ({ models: [] })));
      if (image.checked) checkedSummary.value = image.checked;
    } else {
      image = await engineCall<{ models: RoleModel[] }>("list_models", { purpose: "image" }).catch(() => ({ models: [] }));
    }
    const [p, catalog] = await Promise.all([
      engineCall<Page>("image_settings"),
      engineCall<{ models: RoleModel[] }>("list_models").catch(() => ({ models: [] })),
    ]);
    page.value = p;
    models.value = imageChoices(image.models ?? [], catalog.models ?? [], p);
    if (!refresh && !autoChecked && models.value.length === 0) {
      autoChecked = true;
      await load(true);
      return;
    }
  } catch (e) {
    error.value = /does not call|unknown method|method_not_found/i.test(readable(e))
      ? "This Dial engine is too old for image settings. Update the engine, then reopen this page."
      : readable(e);
  } finally {
    loading.value = false;
    if (refresh) checking.value = false;
  }
}
onMounted(() => load(false));
const has = (field: string) => !!page.value?.settings?.includes(field);
const entry = computed<Entry>(() => page.value?.entry ?? { model: page.value?.model ?? "" });

// ── Model ──
const key = (provider: string, model: string) => `${provider}\u0000${model}`;
const groups = computed(() => {
  const by = new Map<string, RoleModel[]>();
  for (const m of models.value) {
    const name = m.provider_label ?? m.provider;
    by.set(name, [...(by.get(name) ?? []), m]);
  }
  return [...by.entries()].sort(([a], [b]) => a.localeCompare(b));
});
const chosen = computed(() => (page.value?.role ? key(page.value.role.provider, page.value.role.model) : ""));
const offList = computed(() => !!page.value?.role && !models.value.some((m) => key(m.provider, m.model_id) === chosen.value));
const shortModel = (id: string) => (id.startsWith("/") ? id.split("/").filter(Boolean).slice(-3).join("/") : id);
/** The listing's name for the route's model, else the tail of its id. */
const routeName = computed(() => {
  const id = page.value?.model ?? "";
  return models.value.find((m) => matchesImageRoute(m, page.value ?? undefined))?.label || shortModel(id);
});
const cacheHint = computed(() => {
  if (checking.value) return "Checking configured providers and caching capable image models…";
  if (!models.value.length) return "No image generators cached yet. Click Check models to probe configured providers.";
  const verified = models.value.filter((m) => m.image_capability_source === "verified").length;
  const candidates = models.value.filter((m) => m.image_capability === "candidate").length;
  const capable = models.value.length - candidates;
  const parts = [`${capable} capable`];
  if (verified) parts.push(`${verified} verified`);
  if (candidates) parts.push(`${candidates} unverified`);
  const base = `Cached across ${groups.value.length} provider${groups.value.length === 1 ? "" : "s"} (${parts.join(", ")}). Unassigned follows the conversation model when it can generate images.`;
  return checkedSummary.value
    ? `${base} Last checked ${checkedSummary.value.providers} providers.`
    : base;
});

async function pickModel(value: string) {
  busy.value = true;
  try {
    if (!value) await unassignRole("image");
    else {
      const [provider, model] = value.split("\u0000");
      const row = models.value.find((m) => m.provider === provider && m.model_id === model);
      await assignRole("image", provider, model, null, row?.base_url);
    }
    await load();
  } finally {
    busy.value = false;
  }
}

// ── Per-model settings ──
async function saveEntry(next: Entry) {
  const p = page.value;
  if (!p?.key) return;
  busy.value = true;
  try {
    const keys = Object.keys(next).filter((k) => k !== "model" && next[k] !== undefined);
    if (keys.length) await engineCall("set_setting", { key: `image.models.${p.key}`, value: next, scope: "global" });
    else await engineCall("reset_setting", { key: `image.models.${p.key}`, scope: "global" });
    await applySaved();
    await load();
  } catch (e) {
    showToast(`Could not save the image setting: ${readable(e)}`);
  } finally {
    busy.value = false;
  }
}
function set(field: keyof Entry, value: unknown) {
  const next: Entry = { ...entry.value };
  if (value === undefined || value === "") delete next[field];
  else next[field] = value;
  return saveEntry(next);
}

const lora = computed(() => entry.value.loras?.[0]);
function setLora(path: string) {
  const rest = entry.value.loras?.slice(1) ?? [];
  return set("loras", path ? [{ path, scale: lora.value?.scale ?? 1 }, ...rest] : rest.length ? rest : undefined);
}
function setLoraScale(scale: number) {
  if (!lora.value) return;
  return set("loras", [{ ...lora.value, scale }, ...(entry.value.loras?.slice(1) ?? [])]);
}

const sizeLabel = (s: string) => (s === "auto" ? "Automatic" : s.replace("x", " × "));
const word = (s: string) => (s === "auto" ? "Automatic" : s[0].toUpperCase() + s.slice(1));

// ── Test ──
const canTest = computed(() => live.value && !!page.value?.ok && !props.snap.turnRunning);
async function test() {
  const accepted = await engine.send(
    "Generate one test image with image_gen: a red lighthouse on a cliff at sunset. Then say where it was saved.",
    [],
    { workspaceRoot: props.snap.workspace.root, sessionId: props.snap.sessionId },
  );
  if (accepted) ui.settingsOpen = false;
}
</script>

<template>
  <SPage title="Image generation" description="Which model makes images when Dial is asked for one, and how it renders them.">
    <template #actions>
      <button v-if="live" class="btn btn-secondary" :disabled="loading || busy" @click="load(true)">{{ checking ? "Checking…" : "Check models" }}</button>
    </template>

    <div v-if="!live" class="empty">Connect to the Dial engine to change image settings.</div>
    <div v-else-if="error" class="empty bad">{{ error }}</div>
    <template v-else-if="page">
      <SGroup title="Model">
        <div class="status" :class="page.ok ? 'ok' : 'bad'">
          <Icon :name="page.ok ? 'check' : 'cross'" :size="14" />
          <div>
            <b v-if="page.ok">Ready · {{ page.api }}</b>
            <b v-else>Images can't be made right now</b>
            <p v-if="page.ok" :title="page.model">{{ routeName }}</p>
            <p v-else>{{ page.error }}</p>
          </div>
          <button v-if="page.ok" class="btn btn-secondary" :disabled="!canTest" :title="snap.turnRunning ? 'Wait for the current turn to finish' : 'Ask Dial to make one image in this session'" @click="test">
            Generate a test image
          </button>
        </div>
        <SRow label="Image model" :description="cacheHint">
          <select class="s-select" :value="chosen" :disabled="busy || loading" aria-label="Image model" @change="pickModel(($event.target as HTMLSelectElement).value)">
            <option value="">Unassigned</option>
            <option v-if="offList && page.role" disabled :value="chosen">{{ shortModel(page.role.model) }} · {{ page.role.provider }} (not listed)</option>
            <optgroup v-for="[p, list] in groups" :key="p" :label="p">
              <option v-for="m in list" :key="m.provider + m.model_id" :value="key(m.provider, m.model_id)">{{ imageModelLabel(m, shortModel(m.model_id)) }}</option>
            </optgroup>
          </select>
        </SRow>
      </SGroup>

      <SGroup v-if="page.ok && page.settings?.length" title="Generation" :description="`Saved for this model only. ${page.api} uses the settings shown; others are kept for other models.`">
        <SRow v-if="has('size')" label="Default size" description="Used when a request doesn't choose a size." :modified="!!entry.size" @reset="set('size', undefined)">
          <select class="s-select" :value="entry.size ?? ''" :disabled="busy" aria-label="Default size" @change="set('size', ($event.target as HTMLSelectElement).value)">
            <option value="">Model default</option>
            <option v-for="s in page.options.sizes" :key="s" :value="s">{{ sizeLabel(s) }}</option>
          </select>
        </SRow>
        <SRow v-if="has('quality')" label="Quality" description="Higher quality costs more and takes longer. gpt-image models take low, medium or high; DALL·E 3 takes standard or HD." :modified="!!entry.quality" @reset="set('quality', undefined)">
          <select class="s-select" :value="entry.quality ?? ''" :disabled="busy" aria-label="Quality" @change="set('quality', ($event.target as HTMLSelectElement).value)">
            <option value="">Model default</option>
            <option v-for="q in page.options.qualities" :key="q" :value="q">{{ word(q) }}</option>
          </select>
        </SRow>
        <SRow v-if="has('background')" label="Background" description="Transparent backgrounds need PNG or WebP output." :modified="!!entry.background" @reset="set('background', undefined)">
          <SSegmented
            :model-value="entry.background ?? 'auto'"
            :options="page.options.backgrounds.map((b) => ({ value: b, label: word(b) }))"
            :label="'Background'"
            @update:model-value="(v: string) => set('background', v === 'auto' ? undefined : v)"
          />
        </SRow>
        <SRow v-if="has('moderation')" label="Moderation" description="How strictly the provider filters content. Its own policy still applies." :modified="!!entry.moderation" @reset="set('moderation', undefined)">
          <SSegmented
            :model-value="entry.moderation ?? 'auto'"
            :options="page.options.moderations.map((m) => ({ value: m, label: word(m) }))"
            :label="'Moderation'"
            @update:model-value="(v: string) => set('moderation', v === 'auto' ? undefined : v)"
          />
        </SRow>
        <SRow v-if="has('steps')" label="Steps" description="Denoising steps per image. Fast and distilled models need few (4–8); more is slower and rarely better for them." :modified="entry.steps != null" @reset="set('steps', undefined)">
          <SSegmented
            :model-value="entry.steps == null ? 'default' : 'set'"
            :options="[{ value: 'default', label: 'Default' }, { value: 'set', label: 'Set' }]"
            label="Steps"
            @update:model-value="(v: string) => set('steps', v === 'set' ? 8 : undefined)"
          />
          <SNumber v-if="entry.steps != null" :model-value="entry.steps" :min="1" :max="page.options.max_steps" integer label="Steps" @update:model-value="(v: number) => set('steps', v)" />
        </SRow>
        <SRow v-if="has('seed')" label="Seed" description="A fixed seed repeats the same image for the same prompt and settings." :modified="entry.seed != null" @reset="set('seed', undefined)">
          <SSegmented
            :model-value="entry.seed == null ? 'random' : 'fixed'"
            :options="[{ value: 'random', label: 'Random' }, { value: 'fixed', label: 'Fixed' }]"
            label="Seed"
            @update:model-value="(v: string) => set('seed', v === 'fixed' ? 42 : undefined)"
          />
          <SNumber v-if="entry.seed != null" :model-value="entry.seed" :min="0" :max="4294967295" integer label="Seed" @update:model-value="(v: number) => set('seed', v)" />
        </SRow>
        <SRow v-if="has('strength') && page.edits" label="Edit strength" description="How far an edit may move from the source image: low stays close, 1.0 repaints freely." :modified="entry.strength != null" @reset="set('strength', undefined)">
          <SNumber :model-value="entry.strength ?? 1" :min="0" :max="1" :step="0.05" label="Edit strength" @update:model-value="(v: number) => set('strength', v)" />
        </SRow>
        <template v-if="has('loras')">
          <SRow label="LoRA" description="An adapter applied to every image, from ~/.mlx-serve/models/loras." :modified="!!lora" @reset="setLora('')">
            <select class="s-select" :value="lora?.path ?? ''" :disabled="busy || (!page.options.loras.length && !lora)" aria-label="LoRA" @change="setLora(($event.target as HTMLSelectElement).value)">
              <option value="">{{ page.options.loras.length || lora ? "None" : "None installed" }}</option>
              <option v-if="lora && !page.options.loras.includes(lora.path)" :value="lora.path">{{ lora.path }}</option>
              <option v-for="l in page.options.loras" :key="l" :value="l">{{ l }}</option>
            </select>
          </SRow>
          <SRow v-if="lora" label="LoRA strength" description="1.0 is full strength.">
            <SNumber :model-value="lora.scale ?? 1" :min="-page.options.lora_scale_limit" :max="page.options.lora_scale_limit" :step="0.05" label="LoRA strength" @update:model-value="setLoraScale" />
          </SRow>
          <p v-if="(entry.loras?.length ?? 0) > 1" class="note">{{ entry.loras!.length - 1 }} more LoRA{{ entry.loras!.length > 2 ? "s are" : " is" }} stacked after this one; edit them in the terminal with /image settings.</p>
        </template>
      </SGroup>
    </template>
    <div v-else-if="loading" class="empty">Loading…</div>
  </SPage>
</template>

<style scoped>
.status {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 12px 14px;
  border-bottom: 1px solid var(--border);
}
.status > svg {
  flex: none;
  margin-top: 2px;
}
.status > div {
  flex: 1;
  min-width: 0;
}
.status b {
  font-size: 13px;
  font-weight: 600;
}
.status p {
  margin-top: 2px;
  font-size: 12px;
  color: var(--text-muted);
  overflow-wrap: anywhere;
}
.status.ok > svg {
  color: var(--success, #2f9e5b);
}
.status.bad > svg,
.status.bad b {
  color: var(--danger);
}
.note {
  padding: 8px 14px 12px;
  font-size: 12px;
  color: var(--text-faint);
}
.empty {
  padding: 24px 0;
  color: var(--text-muted);
}
.empty.bad {
  color: var(--danger);
}
</style>
