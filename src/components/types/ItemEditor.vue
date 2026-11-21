<script setup lang="ts">
import { computed, reactive, ref } from "vue";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { showToast } from "../../bridge";
import { assetPath, typeApi, type Field, type Fields, type Full, type Item } from "../../savedTypes";
import { vFocus } from "../Shortcuts.vue";
import TypeImage from "./TypeImage.vue";
import Icon from "../Icon.vue";

/** Add or change one item: an input for each field of the type, in its kind's shape. */
const props = defineProps<{ full: Full; item: Item | null }>();
const emit = defineEmits<{ close: [] }>();

const def = computed(() => props.full.def);
const LIST = ["list", "steps", "tags"];

/** Lists edit as one line per entry; everything else as it is saved. */
const draft = reactive<Record<string, unknown>>({});
for (const f of props.full.def.fields) {
  const value = props.item?.fields[f.key];
  if (LIST.includes(f.kind)) draft[f.key] = Array.isArray(value) ? value.join("\n") : "";
  else if (f.kind === "images") draft[f.key] = Array.isArray(value) ? [...value] : [];
  else if (f.kind === "datetime" && typeof value === "string") draft[f.key] = value.replace(" ", "T");
  else draft[f.key] = value ?? (f.kind === "toggle" ? false : f.kind === "rating" ? 0 : "");
}
const saving = ref(false);
const showSource = ref(false);

function collect(): Fields {
  const out: Fields = {};
  for (const f of def.value.fields) {
    const value = draft[f.key];
    if (LIST.includes(f.kind)) out[f.key] = String(value ?? "").split("\n").map((s) => s.trim()).filter(Boolean);
    else if (f.kind === "datetime") out[f.key] = value ? String(value).replace("T", " ") : null;
    else if (f.kind === "number") out[f.key] = value === "" || value === null ? null : Number(value);
    else if (value === "" || value === undefined) out[f.key] = null;
    else out[f.key] = value;
  }
  return out;
}

async function save() {
  saving.value = true;
  try {
    const fields = collect();
    if (props.item) await typeApi.updateItem(def.value.id, props.item.id, { fields });
    else {
      const filled = Object.fromEntries(Object.entries(fields).filter(([, v]) => v !== null && !(Array.isArray(v) && !v.length)));
      await typeApi.addItem(def.value.id, filled);
    }
    emit("close");
  } catch (error) {
    showToast(String(error));
  } finally {
    saving.value = false;
  }
}

async function remove() {
  if (!props.item) return;
  try {
    await typeApi.removeItem(def.value.id, props.item.id);
    emit("close");
  } catch (error) {
    showToast(String(error));
  }
}

async function pick(field: Field) {
  const picked = await openDialog({ multiple: field.kind === "images", filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "gif", "webp"] }] });
  const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
  for (const path of paths) {
    try {
      const name = await typeApi.importImage(def.value.id, path);
      if (field.kind === "images") (draft[field.key] as string[]).push(name);
      else draft[field.key] = name;
    } catch (error) {
      showToast(String(error));
    }
  }
}

function onKey(event: KeyboardEvent) {
  if (event.key === "Escape") emit("close");
  else if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) void save();
}
</script>

<template>
  <Teleport to="body">
    <div class="ty-overlay" @mousedown.self="emit('close')">
      <div v-focus class="ty-sheet" role="dialog" :aria-label="item ? 'Edit item' : 'Add item'" tabindex="-1" @keydown="onKey">
        <header>
          <span class="glyph">{{ def.icon }}</span>
          <h2>{{ item ? "Edit" : "Add to" }} {{ def.name }}</h2>
          <button class="icon-btn" title="Close (Esc)" aria-label="Close" @click="emit('close')"><Icon name="cross" :size="14" /></button>
        </header>
        <div class="body">
          <template v-for="f in def.fields" :key="f.key">
            <div v-if="f.kind === 'image' || f.kind === 'images'" class="row images">
              <span class="row-label">{{ f.label }}</span>
              <div class="thumbs">
                <template v-if="f.kind === 'image'">
                  <div v-if="draft[f.key]" class="thumb">
                    <TypeImage :path="assetPath(full, draft[f.key])" />
                    <button class="x" title="Remove image" @click="draft[f.key] = ''"><Icon name="cross" :size="10" /></button>
                  </div>
                </template>
                <template v-else>
                  <div v-for="(name, i) in (draft[f.key] as string[])" :key="name" class="thumb">
                    <TypeImage :path="assetPath(full, name)" />
                    <button class="x" title="Remove image" @click="(draft[f.key] as string[]).splice(i, 1)"><Icon name="cross" :size="10" /></button>
                  </div>
                </template>
                <button class="thumb add" :title="`Choose ${f.label.toLowerCase()}`" @click="pick(f)"><Icon name="image" :size="16" /></button>
              </div>
            </div>
            <label v-else-if="f.kind === 'toggle'" class="row check">
              <input v-model="draft[f.key]" type="checkbox" />
              <span>{{ f.label }}</span>
            </label>
            <div v-else-if="f.kind === 'rating'" class="row">
              <span class="row-label">{{ f.label }}</span>
              <div class="stars">
                <button v-for="n in 5" :key="n" :class="{ on: n <= Number(draft[f.key]) }" :aria-label="`${n} of 5`" @click="draft[f.key] = Number(draft[f.key]) === n ? 0 : n">★</button>
              </div>
            </div>
            <label v-else class="row">
              <span>{{ f.label }}<small v-if="LIST.includes(f.kind)"> · one per line</small></span>
              <textarea v-if="['longtext', 'markdown', 'list', 'steps', 'tags'].includes(f.kind)" v-model="draft[f.key] as string" :rows="f.kind === 'markdown' ? 7 : 4" />
              <input v-else-if="f.kind === 'time'" v-model="draft[f.key]" type="time" />
              <input v-else-if="f.kind === 'date'" v-model="draft[f.key]" type="date" />
              <input v-else-if="f.kind === 'datetime'" v-model="draft[f.key]" type="datetime-local" />
              <input v-else-if="f.kind === 'number'" v-model="draft[f.key]" type="number" step="any" />
              <input v-else-if="f.kind === 'url'" v-model="draft[f.key]" type="url" placeholder="https://" />
              <input v-else v-model="draft[f.key]" type="text" />
            </label>
          </template>
          <div v-if="item?.source?.text" class="source">
            <button class="source-toggle" @click="showSource = !showSource">
              <Icon :name="showSource ? 'chevronDown' : 'chevronRight'" :size="12" /> Saved from the conversation
            </button>
            <pre v-if="showSource">{{ item.source.text }}</pre>
          </div>
        </div>
        <footer>
          <button v-if="item" class="btn btn-danger" @click="remove"><Icon name="trash" :size="13" /> Remove</button>
          <span class="spacer" />
          <button class="btn btn-secondary" @click="emit('close')">Cancel</button>
          <button class="btn btn-primary" :disabled="saving" title="⌘Enter" @click="save">{{ item ? "Save" : "Add" }}</button>
        </footer>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.glyph {
  font-size: 20px;
}
.row small {
  color: var(--text-faint);
  font-weight: 400;
}
.row.images,
.row:not(label) {
  display: grid;
  gap: 6px;
  margin-bottom: 14px;
}
.thumbs {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.thumb {
  position: relative;
  width: 84px;
  height: 84px;
  overflow: hidden;
  border-radius: var(--radius-md);
  background: var(--hover);
}
.thumb.add {
  display: grid;
  place-items: center;
  box-shadow: inset 0 0 0 1.5px var(--border-strong);
  color: var(--text-faint);
}
.thumb.add:hover {
  color: var(--text);
}
.x {
  position: absolute;
  top: 4px;
  right: 4px;
  display: grid;
  place-items: center;
  width: 20px;
  height: 20px;
  border-radius: 50%;
  background: rgb(0 0 0 / 60%);
  color: #fff;
}
.row.check {
  display: flex !important;
  align-items: center;
  gap: 8px;
}
.stars button {
  color: var(--border-strong);
  font-size: 22px;
}
.stars button.on {
  color: var(--accent);
}
.source {
  margin-top: 4px;
}
.source-toggle {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  color: var(--text-faint);
  font-size: 12px;
}
.source pre {
  max-height: 220px;
  margin: 8px 0 0;
  padding: 10px 12px;
  overflow: auto;
  border-radius: var(--radius-sm);
  background: var(--hover);
  color: var(--text-muted);
  font: 12px/1.5 var(--font-mono);
  white-space: pre-wrap;
}
</style>
