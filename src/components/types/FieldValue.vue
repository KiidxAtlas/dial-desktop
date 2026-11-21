<script setup lang="ts">
import { computed } from "vue";
import { renderMarkdown } from "../../markdown";
import { fieldText, timeLabel, type Field } from "../../savedTypes";
import { dateOf } from "../../typeExtract";

/** One saved value, drawn for its kind. Images are drawn by the layouts, which size them. */
const props = defineProps<{ field: Field; value: unknown }>();

const list = computed(() => (Array.isArray(props.value) ? props.value.map(String) : []));
const html = computed(() => (props.field.kind === "markdown" ? renderMarkdown(fieldText(props.value)) : ""));
const stars = computed(() => Math.max(0, Math.min(5, Number(props.value) || 0)));
const href = computed(() => {
  const text = fieldText(props.value);
  return /^https?:\/\//i.test(text) ? text : null;
});
</script>

<template>
  <div v-if="field.kind === 'markdown'" class="prose ty-md" v-html="html" />
  <p v-else-if="field.kind === 'longtext'" class="long">{{ fieldText(value) }}</p>
  <ul v-else-if="field.kind === 'list'" class="list">
    <li v-for="(line, i) in list" :key="i">{{ line }}</li>
  </ul>
  <ol v-else-if="field.kind === 'steps'" class="steps">
    <li v-for="(line, i) in list" :key="i"><span class="n">{{ i + 1 }}</span><span>{{ line }}</span></li>
  </ol>
  <span v-else-if="field.kind === 'tags'" class="tags">
    <span v-for="(tag, i) in list" :key="i" class="ty-chip">{{ tag }}</span>
  </span>
  <span v-else-if="field.kind === 'rating'" class="stars" :aria-label="`${stars} of 5`">
    <span v-for="n in 5" :key="n" :class="{ on: n <= stars }">★</span>
  </span>
  <a v-else-if="field.kind === 'url' && href" :href="href" class="url">{{ href.replace(/^https?:\/\/(www\.)?/, "") }}</a>
  <span v-else-if="field.kind === 'time' || field.kind === 'datetime'">{{ timeLabel(value) }}</span>
  <span v-else-if="field.kind === 'date' && dateOf(value)">{{ dateOf(value)!.at.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" }) }}</span>
  <span v-else-if="field.kind === 'toggle'">{{ value ? "Yes" : "No" }}</span>
  <span v-else>{{ fieldText(value) }}</span>
</template>

<style scoped>
.long {
  margin: 0;
  white-space: pre-wrap;
  line-height: 1.6;
}
.list {
  margin: 0;
  padding: 0;
  list-style: none;
}
.list li {
  position: relative;
  padding: 5px 0 5px 18px;
  border-bottom: 1px dashed var(--ty-line);
  line-height: 1.45;
}
.list li::before {
  content: "";
  position: absolute;
  left: 3px;
  top: 0.95em;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--ty-accent);
}
.steps {
  display: grid;
  gap: 12px;
  margin: 0;
  padding: 0;
  list-style: none;
}
.steps li {
  display: grid;
  grid-template-columns: 26px 1fr;
  gap: 10px;
  line-height: 1.55;
}
.steps .n {
  display: grid;
  place-items: center;
  width: 24px;
  height: 24px;
  border-radius: 50%;
  background: var(--ty-accent);
  color: var(--ty-on-accent);
  font: 700 12px var(--font-ui);
}
.tags {
  display: inline-flex;
  flex-wrap: wrap;
  gap: 5px;
}
.stars span {
  color: var(--ty-line);
  font-size: 15px;
}
.stars span.on {
  color: var(--ty-accent);
}
.url {
  color: var(--ty-accent);
  text-decoration: underline;
  text-underline-offset: 3px;
}
</style>
