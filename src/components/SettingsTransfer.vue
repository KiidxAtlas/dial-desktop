<script setup lang="ts">
// Settings › Overview › Back up and move settings: export the saved Dial settings (no
// credentials) to a file, and import one after showing what changes (`settingsTransfer`).
import { computed, ref } from "vue";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { settings, settingsFiles, showToast } from "../bridge";
import { applySaved, engineCall, readable } from "../engineTools";
import { buildExport, importPlan, parseImport, preview, type ImportItem } from "../settingsTransfer";
import type { OpenSnapshot, Setting, SettingScope } from "../types";
import SGroup from "./settings/SGroup.vue";
import SRow from "./settings/SRow.vue";
import SSegmented from "./settings/SSegmented.vue";
import SSwitch from "./settings/SSwitch.vue";

const props = defineProps<{ snap: OpenSnapshot; rows: Setting[] }>();
const scopeOptions = [{ value: "global", label: "Everywhere" }, { value: "project", label: "This project" }];

const withProject = ref(false);
const busy = ref(false), error = ref(""), notice = ref("");

async function exportSettings() {
  error.value = notice.value = "";
  if (!props.rows.length) { error.value = "Engine settings load with the engine; try again once Dial is connected."; return; }
  const file = buildExport(props.rows, withProject.value);
  const count = Object.keys(file.settings).length + Object.keys(file.project ?? {}).length;
  try {
    const path = await saveDialog({ title: "Export Dial settings", defaultPath: "dial-settings.json", filters: [{ name: "Dial settings", extensions: ["json"] }] });
    if (!path) return;
    busy.value = true;
    await settingsFiles.write(path, `${JSON.stringify(file, null, 2)}\n`);
    notice.value = `Exported ${count} setting${count === 1 ? "" : "s"} to ${path}. Credentials and hidden values are left out.`;
  } catch (e) {
    error.value = `Could not export: ${readable(e)}`;
  } finally {
    busy.value = false;
  }
}

/** The file being imported, until applied or cancelled. */
const pending = ref<{ path: string; settings: Record<string, unknown>; project: Record<string, unknown> | null } | null>(null);
const part = ref<"settings" | "project">("settings");
const target = ref<SettingScope>("global");
const plan = computed<ImportItem[]>(() => (pending.value ? importPlan((part.value === "project" ? pending.value.project : pending.value.settings) ?? {}, props.rows) : []));
const applicable = computed(() => plan.value.filter((p) => p.kind === "change" || p.kind === "new"));
const KIND_TEXT: Record<ImportItem["kind"], string> = { change: "Changes", same: "Already set", new: "New key", secret: "Skipped: credential" };

async function chooseImport() {
  error.value = notice.value = "";
  try {
    const path = await openDialog({ title: "Import Dial settings", multiple: false, directory: false, filters: [{ name: "Dial settings", extensions: ["json"] }] });
    if (typeof path !== "string") return;
    const parsed = parseImport(await settingsFiles.read(path));
    pending.value = { path, ...parsed };
    part.value = "settings";
    target.value = "global";
    if (!Object.keys(parsed.settings).length && parsed.project && Object.keys(parsed.project).length) {
      part.value = "project";
      target.value = "project";
    }
  } catch (e) {
    error.value = `Could not import: ${readable(e)}`;
  }
}
function choosePart(next: "settings" | "project") {
  part.value = next;
  target.value = next === "project" ? "project" : "global";
}

async function applyImport() {
  const items = applicable.value, where = target.value;
  if (!items.length || busy.value) return;
  busy.value = true;
  error.value = notice.value = "";
  const failed: string[] = [];
  for (const item of items) {
    try {
      await engineCall("set_setting", { key: item.key, value: item.value, scope: where });
    } catch (e) {
      failed.push(`${item.label}: ${readable(e)}`);
    }
  }
  const done = items.length - failed.length;
  try {
    if (done) {
      await applySaved();
      await settings.load();
    }
  } catch (e) {
    failed.push(`Applying: ${readable(e)}`);
  }
  busy.value = false;
  notice.value = `Imported ${done} of ${items.length} setting${items.length === 1 ? "" : "s"} ${where === "project" ? `for ${props.snap.workspace.name}` : "for every project"}.${props.snap.turnRunning ? " They apply when the running turn ends." : ""}`;
  if (failed.length) error.value = `Not imported:\n${failed.join("\n")}`;
  else { pending.value = null; showToast("Settings imported"); }
}
</script>

<template>
  <SGroup title="Back up and move settings" description="Export your saved Dial settings to a file, or import one on another computer or project. API keys, tokens and other hidden values are never exported or imported.">
    <SRow label="Export" description="Values saved for every project, as a JSON file.">
      <span class="also"><SSwitch v-model="withProject" label="Include this project's settings" /> Include {{ snap.workspace.name }}</span>
      <button class="btn btn-secondary" :disabled="busy" @click="exportSettings">Export…</button>
    </SRow>
    <SRow v-if="!pending" label="Import" description="Shows what changes before anything is saved.">
      <button class="btn btn-secondary" :disabled="busy" @click="chooseImport">Import…</button>
    </SRow>
    <SRow v-else :label="`Import ${pending.path.split(/[\\/]/).pop()}`" stacked>
      <div class="import">
        <div class="controls">
          <SSegmented
            v-if="pending.project && Object.keys(pending.project).length"
            :model-value="part"
            :options="[{ value: 'settings', label: 'Everywhere values' }, { value: 'project', label: 'Project values' }]"
            label="Values to import"
            @update:model-value="choosePart($event as 'settings' | 'project')"
          />
          <span class="s-note">Save for</span>
          <SSegmented v-model="target" :options="scopeOptions" label="Save imported values for" />
        </div>
        <ul v-if="plan.length" class="plan">
          <li v-for="item in plan" :key="item.key" :class="item.kind">
            <span class="name" :title="item.key">{{ item.label }}</span>
            <span class="kind">{{ KIND_TEXT[item.kind] }}</span>
            <span v-if="item.kind === 'change'" class="values"><code>{{ preview(item.current) }}</code> → <code>{{ preview(item.value) }}</code></span>
            <span v-else-if="item.kind === 'new'" class="values"><code>{{ preview(item.value) }}</code></span>
          </li>
        </ul>
        <p v-else class="s-note">This file has no settings to import.</p>
        <div class="actions">
          <button class="btn btn-secondary" :disabled="busy" @click="pending = null">Cancel</button>
          <button class="btn btn-primary" :disabled="busy || !applicable.length" @click="applyImport">
            {{ busy ? "Importing…" : `Import ${applicable.length} setting${applicable.length === 1 ? "" : "s"}` }}
          </button>
        </div>
      </div>
    </SRow>
    <template #after>
      <p v-if="error" role="alert" class="s-note bad">{{ error }}</p>
      <p v-if="notice" role="status" class="s-note">{{ notice }}</p>
    </template>
  </SGroup>
</template>

<style scoped>
.also { display: flex; align-items: center; gap: 6px; font-size: 12.5px; color: var(--text-muted); }
.import { display: flex; flex-direction: column; gap: 10px; width: 100%; }
.controls { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
.plan { margin: 0; padding: 0; list-style: none; max-height: 260px; overflow: auto; border: 1px solid var(--border); border-radius: var(--radius-sm); }
.plan li { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 2px 10px; padding: 7px 10px; font-size: 12.5px; border-top: 1px solid var(--border); }
.plan li:first-child { border-top: none; }
.plan .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); }
.plan .kind { color: var(--text-faint); }
.plan .values { grid-column: 1 / -1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-muted); }
.plan li.same, .plan li.secret { opacity: 0.6; }
.plan code { font-family: var(--font-mono); font-size: 11.5px; }
.actions { display: flex; justify-content: flex-end; gap: 8px; }
.bad { color: var(--danger, #c53030); white-space: pre-wrap; }
</style>
