<script setup lang="ts">
import { computed } from "vue";
import { assetPath, fieldText, itemTitle, type Full, type Item } from "../../savedTypes";
import FieldValue from "./FieldValue.vue";
import TypeImage from "./TypeImage.vue";

/** Cards in a grid, or the same items as a table. */
const props = defineProps<{ full: Full; table?: boolean }>();
const emit = defineEmits<{ edit: [item: Item] }>();

const def = computed(() => props.full.def);
const image = (item: Item) => {
  const key = def.value.view.imageField;
  const value = key ? item.fields[key] : null;
  return assetPath(props.full, Array.isArray(value) ? value[0] : value);
};
const subtitle = (item: Item) => (def.value.view.subtitleField ? fieldText(item.fields[def.value.view.subtitleField]) : "");
const preview = computed(() =>
  def.value.fields.find((f) => ["markdown", "longtext", "list", "steps"].includes(f.kind) && f.key !== def.value.view.subtitleField) ?? null,
);
const chips = computed(() => def.value.fields.filter((f) => ["tags", "rating", "time", "date", "number"].includes(f.kind)));
const columns = computed(() =>
  def.value.fields.filter((f) => !["image", "images", "markdown", "longtext", "steps"].includes(f.kind)).slice(0, 7),
);
const plainPreview = (item: Item) => {
  const f = preview.value;
  if (!f) return "";
  return fieldText(item.fields[f.key]).replace(/[#*_`>]/g, "").slice(0, 280);
};
</script>

<template>
  <div v-if="!table" class="cards">
    <button v-for="item in full.items" :key="item.id" class="card" @click="emit('edit', item)">
      <div v-if="image(item)" class="cover"><TypeImage :path="image(item)" /></div>
      <div v-else-if="def.view.imageField" class="cover blank" aria-hidden="true"><span>{{ itemTitle(def, item).slice(0, 1) }}</span></div>
      <div class="body">
        <strong>{{ itemTitle(def, item) }}</strong>
        <span v-if="subtitle(item)" class="sub">{{ subtitle(item) }}</span>
        <p v-if="plainPreview(item)">{{ plainPreview(item) }}</p>
        <div class="chips">
          <template v-for="f in chips" :key="f.key">
            <span v-if="item.fields[f.key] !== undefined" class="chip-wrap"><FieldValue :field="f" :value="item.fields[f.key]" /></span>
          </template>
        </div>
      </div>
    </button>
  </div>
  <div v-else class="table-wrap">
    <table>
      <thead>
        <tr><th v-for="f in columns" :key="f.key">{{ f.label }}</th></tr>
      </thead>
      <tbody>
        <tr v-for="item in full.items" :key="item.id" @click="emit('edit', item)">
          <td v-for="f in columns" :key="f.key">
            <FieldValue v-if="item.fields[f.key] !== undefined" :field="f" :value="item.fields[f.key]" />
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>

<style scoped>
.cards {
  display: grid;
  align-items: start;
  grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
  gap: 16px;
  padding: 28px;
}
.card {
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border-radius: var(--ty-radius);
  background: var(--ty-raised);
  box-shadow: 0 1px 0 var(--ty-line), 0 10px 24px rgb(0 0 0 / 10%);
  color: inherit;
  font: inherit;
  text-align: left;
  transition: transform 160ms var(--ease), box-shadow 160ms var(--ease);
}
.card:hover {
  transform: translateY(-3px);
  box-shadow: 0 1px 0 var(--ty-line), 0 18px 34px rgb(0 0 0 / 16%);
}
.cover {
  aspect-ratio: 4 / 3;
  overflow: hidden;
}
.cover.blank {
  display: grid;
  place-items: center;
  background:
    radial-gradient(circle at 30% 20%, color-mix(in srgb, var(--ty-accent) 28%, transparent), transparent 60%),
    color-mix(in srgb, var(--ty-accent) 14%, var(--ty-paper));
}
.cover.blank span {
  color: color-mix(in srgb, var(--ty-accent) 55%, var(--ty-paper));
  font-family: var(--ty-font);
  font-size: 64px;
  font-weight: 700;
}
.body {
  display: grid;
  gap: 6px;
  padding: 14px 16px 16px;
}
.body strong {
  font-size: 16px;
  line-height: 1.3;
}
.sub {
  color: var(--ty-muted);
  font-size: 13px;
}
.body p {
  display: -webkit-box;
  margin: 0;
  overflow: hidden;
  color: var(--ty-muted);
  font: 12.5px/1.55 var(--font-ui);
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
}
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  font: 12px var(--font-ui);
}
.chip-wrap:not(:has(.ty-chip)) {
  color: var(--ty-accent);
  font-weight: 600;
}
.table-wrap {
  padding: 28px;
  overflow: auto;
}
table {
  width: 100%;
  border-collapse: separate;
  border-spacing: 0;
  overflow: hidden;
  border-radius: var(--ty-radius);
  background: var(--ty-raised);
  box-shadow: 0 1px 0 var(--ty-line), 0 10px 24px rgb(0 0 0 / 8%);
  font-size: 13.5px;
}
th {
  padding: 11px 14px;
  background: var(--ty-soft);
  color: color-mix(in srgb, var(--ty-accent) 70%, var(--ty-ink));
  font: 700 11px var(--font-ui);
  letter-spacing: 0.12em;
  text-align: left;
  text-transform: uppercase;
}
td {
  padding: 11px 14px;
  border-top: 1px solid var(--ty-line);
  vertical-align: top;
}
tbody tr {
  cursor: pointer;
}
tbody tr:hover td {
  background: var(--ty-line);
}
td :deep(.list) li {
  padding: 2px 0 2px 14px;
  border: 0;
}
</style>
