<script setup lang="ts">
// One engine setting, drawn with the control that fits it: a switch, a choice, a stepper, a
// model or file picker. Text boxes are left for values no list can offer (URLs, variable names).
import { computed, onMounted, ref, watch } from "vue";
import type { Choice, Setting, SettingScope } from "../types";
import { resetTitle, sourceBadge } from "../settingScope";
import { models } from "../bridge";
import { usableModels } from "../modelPicker";
import { ROW_TEXT, settingFeedback } from "../settingsFeedback";
import { ui } from "../ui";
import SettingValue from "./SettingValue.vue";
import SRow from "./settings/SRow.vue";
import SSwitch from "./settings/SSwitch.vue";
import SSegmented from "./settings/SSegmented.vue";
import SCombo, { type ComboOption } from "./settings/SCombo.vue";
import SNumber from "./settings/SNumber.vue";
import SContextWindow from "./settings/SContextWindow.vue";
import SChips from "./settings/SChips.vue";
import SPath from "./settings/SPath.vue";

/** `pageScope`: where the page saves edits, so a project override can say it shadows them. */
const props = defineProps<{ setting: Setting; pageScope?: SettingScope }>();
const emit = defineEmits<{ save: [value: unknown, linked?: Record<string, unknown>]; reset: [] }>();

const badge = computed(() => sourceBadge(props.setting.source, props.pageScope));

const s = computed(() => props.setting);
const c = computed(() => props.setting.control);

function save(value: unknown) {
  if (JSON.stringify(value) !== JSON.stringify(props.setting.value)) emit("save", value);
}

/** What happened to the last change of this row (see `settingsFeedback`). */
const status = computed(() => {
  const state = settingFeedback.rows[props.setting.key];
  if (!state) return null;
  const kind = state === "failed" ? "bad" : state === "restart" || state === "turn" ? "warn" : state === "saved" ? "ok" : undefined;
  return { text: ROW_TEXT[state], kind } as const;
});

/** Credentials are set on the Providers page, not typed into a row. */
function manageProviders() {
  ui.settingsTab = "providers";
}

// ── Choices ──────────────────────────────────────────────────────────────────

/** `auto | on | off` selects store `"auto" | true | false`. */
const autoBool = computed(
  () => c.value.type === "select" && c.value.options.length === 3 && c.value.options.every((o) => ["auto", "on", "off"].includes(o.value)),
);
const selected = computed(() => {
  const v = s.value.value;
  return v === true ? "on" : v === false ? "off" : String(v ?? "");
});
function choose(v: string) {
  save(autoBool.value && v === "on" ? true : autoBool.value && v === "off" ? false : v);
}
const options = computed<Choice[]>(() => {
  if (c.value.type !== "select") return [];
  const known = c.value.options;
  return known.some((o) => o.value === selected.value) || !selected.value ? known : [{ value: selected.value, label: selected.value }, ...known];
});

const imageChoices = computed(() => c.value.type === "text" ? c.value.suggestions : []);
const imageOptions = computed(() => imageChoices.value.map((o) => ({ value: o.id ?? o.value, label: o.label, detail: o.value })));
function chooseImage(id: string) {
  const choice = imageChoices.value.find((o) => o.id === id);
  if (choice) emit("save", choice.value, choice.with);
  else save(id);
}
/** Few short choices read best side by side. */
const segmented = computed(() => options.value.length <= 4 && options.value.reduce((n, o) => n + o.label.length, 0) <= 30);

// ── Numbers ──────────────────────────────────────────────────────────────────

const isAuto = computed(() => s.value.value === "auto");
const numeric = computed(() => {
  const v = Number(s.value.value);
  if (Number.isFinite(v) && s.value.value !== null && s.value.value !== "") return v;
  const d = Number(s.value.default);
  return Number.isFinite(d) && s.value.default !== "auto" ? d : c.value.type === "number" ? (c.value.min ?? 0) : 0;
});
function autoMode(mode: string) {
  if (mode === "auto") save("auto");
  else save(numeric.value);
}

// ── Text ─────────────────────────────────────────────────────────────────────

const hint = computed(() => (c.value.type === "text" ? c.value.hint : "plain"));
const text = computed(() => (s.value.value == null ? "" : String(s.value.value)));
const defaultText = computed(() => (s.value.default == null ? "" : String(s.value.default)));
const emptyLabel = computed(() => (defaultText.value ? `Default (${defaultText.value})` : "Not set"));

const microphones = ref<ComboOption[]>([]);
onMounted(async () => {
  if (hint.value !== "microphone") return;
  try {
    const devices = await navigator.mediaDevices?.enumerateDevices();
    microphones.value = (devices ?? [])
      .filter((d) => d.kind === "audioinput" && d.label)
      .map((d) => ({ value: d.label, label: d.label }));
  } catch {
    /* The list stays empty; a typed name still works. */
  }
});

/** The list a text setting picks from, or `null` when it can only be typed. */
const textOptions = computed<ComboOption[] | null>(() => {
  if (c.value.type !== "text") return null;
  const given: ComboOption[] = c.value.suggestions.map((o) => ({ value: o.value, label: o.label, detail: o.label !== o.value ? o.value : undefined }));
  if (hint.value === "model") {
    const seen = new Set(given.map((o) => o.value));
    // The composer picker's list: catalog rows for providers that are not set up are left out.
    for (const m of usableModels(models.value, (m) => m.modelId === text.value)) {
      if (seen.has(m.modelId)) continue;
      seen.add(m.modelId);
      given.push({ value: m.modelId, label: m.label, group: m.configured ? "Yours" : m.provider, detail: m.label !== m.modelId ? m.modelId : undefined });
    }
    given.sort((a, b) => Number(b.group === "Yours") - Number(a.group === "Yours") || (a.group ?? "").localeCompare(b.group ?? ""));
    return given;
  }
  if (hint.value === "provider") {
    const names = [...new Set(models.value.map((m) => m.provider))].sort();
    return [...given, ...names.filter((n) => !given.some((g) => g.value === n)).map((n) => ({ value: n, label: n }))];
  }
  if (hint.value === "microphone") return [...microphones.value];
  return given.length ? given : null;
});

const draft = ref("");
watch(text, (v) => (draft.value = v), { immediate: true });
function commitText() {
  const v = draft.value.trim();
  if (hint.value === "url" && v && !/^[a-z][a-z\d+.-]*:\/\//i.test(v)) return (urlError.value = "Start with http:// or https://");
  urlError.value = "";
  save(v);
}
const urlError = ref("");

// ── Lists and tables ─────────────────────────────────────────────────────────

const list = computed(() => (Array.isArray(s.value.value) ? s.value.value.map(String) : []));
const editing = ref(false);
const structured = ref<unknown>(null);
const submitted = ref(false);
const entries = computed(() => {
  const v = s.value.value;
  if (Array.isArray(v)) return v.length;
  return v && typeof v === "object" ? Object.keys(v).length : 0;
});
function startEditing() {
  structured.value = JSON.parse(JSON.stringify(s.value.value ?? (Array.isArray(s.value.default) ? [] : {})));
  editing.value = true;
}
function saveStructure() {
  if (submitted.value) return;
  if (JSON.stringify(structured.value) === JSON.stringify(s.value.value)) { editing.value = false; return; }
  submitted.value = true;
  save(structured.value);
}
// Refreshes (including unrelated saves) can settle row feedback. Only dismiss a submitted
// draft when the refreshed setting actually matches it; a rejected edit stays retryable.
watch(() => settingFeedback.rows[s.value.key], (state) => {
  if (!submitted.value || state === "saving") return;
  submitted.value = false;
  if (state && state !== "failed" && JSON.stringify(s.value.value) === JSON.stringify(structured.value)) editing.value = false;
}, { flush: "post" });

const keyHint = computed(() => `${s.value.key}${defaultText.value && typeof s.value.default !== "object" ? ` · default ${defaultText.value}` : ""}`);
const stacked = computed(() => c.value.type === "list" || (c.value.type === "table" && editing.value));
/** Table descriptions go on about the raw editor, so the first sentence says what it is; lists
 *  are edited as chips, not comma-separated text. */
const description = computed(() => {
  const d = s.value.description;
  if (c.value.type === "list") return d.replace(/,? comma[- ]separated/gi, "");
  if (c.value.type !== "table") return d;
  return d.match(/^.*?[.;](?=\s|$)/)?.[0].replace(/;$/, ".") ?? d;
});
/** Lists of paths get a folder picker beside the text box. */
const folderList = computed(() => /(write|paths|dirs|folders|roots)$/.test(s.value.key));
</script>

<template>
  <SRow
    :label="s.label"
    :description="description"
    :modified="s.modified"
    :restart="s.restart"
    :stacked="stacked"
    :hint="keyHint"
    :status="status"
    :badge="badge"
    :reset-title="resetTitle(s.source)"
    @reset="emit('reset')"
  >
    <button v-if="s.redacted" class="btn btn-secondary" title="Credentials are added, replaced and removed on the Providers page" @click="manageProviders">
      Manage in Providers
    </button>

    <SSwitch v-else-if="c.type === 'toggle'" :model-value="s.value === true" :label="s.label" @update:model-value="save" />

    <SCombo v-else-if="s.key === 'image.model' && c.type === 'text'"
      :model-value="selected" :options="imageOptions" :label="s.label" custom @update:model-value="chooseImage" />

    <template v-else-if="c.type === 'select'">
      <SSegmented v-if="segmented" :model-value="selected" :options="options" :label="s.label" @update:model-value="choose" />
      <SCombo v-else-if="options.length > 24" :model-value="selected" :options="options" :label="s.label" @update:model-value="choose" />
      <select v-else class="s-select" :value="selected" :aria-label="s.label" @change="choose(($event.target as HTMLSelectElement).value)">
        <option v-for="o in options" :key="o.value" :value="o.value">{{ o.label }}</option>
      </select>
    </template>

    <SContextWindow v-else-if="c.type === 'number' && c.auto && ['context_window', 'local_context_window', 'cloud_context_window'].includes(s.key)"
      :value="s.value" :min="c.min" :max="c.max" :label="s.label" @save="save" />

    <template v-else-if="c.type === 'number'">
      <SSegmented
        v-if="c.auto"
        :model-value="isAuto ? 'auto' : 'set'"
        :options="[{ value: 'auto', label: 'Auto' }, { value: 'set', label: 'Custom' }]"
        :label="`${s.label} mode`"
        @update:model-value="autoMode"
      />
      <SNumber
        v-if="!isAuto"
        :model-value="numeric"
        :min="c.min"
        :max="c.max"
        :step="c.step"
        :integer="c.integer"
        :unit="c.unit"
        :label="s.label"
        @update:model-value="save"
      />
    </template>

    <template v-else-if="c.type === 'text'">
      <SPath
        v-if="hint === 'program' || hint === 'file' || hint === 'folder'"
        :model-value="text"
        :kind="hint"
        :label="s.label"
        :empty-label="hint === 'program' ? 'Find automatically' : emptyLabel"
        @update:model-value="save"
      />
      <SCombo
        v-else-if="textOptions"
        :model-value="text"
        :options="textOptions"
        :label="s.label"
        :empty-label="emptyLabel"
        custom
        @update:model-value="save"
      />
      <div v-else class="typed">
        <input
          v-model="draft"
          class="s-input"
          :class="{ mono: hint !== 'plain' }"
          spellcheck="false"
          :placeholder="hint === 'url' ? 'https://…' : hint === 'envVar' ? 'VARIABLE_NAME' : defaultText || 'Not set'"
          :aria-label="s.label"
          @change="commitText"
          @keydown.enter="commitText"
        />
        <p v-if="urlError" class="s-error">{{ urlError }}</p>
      </div>
    </template>

    <SChips v-else-if="c.type === 'list'" :model-value="list" :label="s.label" :folders="folderList" @update:model-value="save" />

    <template v-else-if="c.type === 'table' && !editing">
      <span class="s-muted count">{{ entries ? `${entries} ${entries === 1 ? "entry" : "entries"}` : "None" }}</span>
      <button class="btn btn-secondary" @click="startEditing">Edit…</button>
    </template>
    <form v-else class="table-edit" @submit.prevent="saveStructure">
      <fieldset :disabled="submitted">
        <SettingValue :value="structured" :label="s.label" @change="structured = $event" />
        <div class="actions">
          <button class="btn btn-secondary" type="button" @click="editing = false">Cancel</button>
          <button class="btn btn-primary" type="submit">{{ submitted ? 'Saving…' : 'Save' }}</button>
        </div>
      </fieldset>
    </form>
    <template v-if="(s.key === 'model' && hint === 'model') || badge?.hint" #below-text>
      <p v-if="badge?.hint" class="s-desc override">{{ badge.hint }}</p>
      <p v-if="s.key === 'model' && hint === 'model'" class="s-desc">The default for new sessions. The model picker under the message box switches only the session you are in.</p>
    </template>
  </SRow>
</template>

<style scoped>
.typed {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 4px;
}
.typed .s-input {
  width: 240px;
}
.count {
  font-size: 12.5px;
  white-space: nowrap;
}
.override {
  color: var(--accent);
}
.table-edit { width: 100%; }
.table-edit fieldset {
  min-width: 0;
  margin: 0;
  border: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
  width: 100%;
}
.actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>
