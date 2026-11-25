<script setup lang="ts">
import { computed } from 'vue';
import type { BuildSteps } from '../../savedTypes';
import Icon from '../Icon.vue';

/** What Dial has done so far while it builds a type's first app. */
const props = defineProps<{ name: string; steps: BuildSteps; running: boolean }>();
const started = computed(() => Object.keys(props.steps).length > 0);
const rows = computed(() => {
  const { fields, saved, app, checked } = props.steps;
  return [
    { label: 'Read what you saved', detail: started.value ? 'Ready' : props.running ? 'Reading…' : 'Starting the conversation…', done: started.value },
    { label: 'Fields', detail: fields ? (fields.length ? fields.join(', ') : 'Free-form') : 'Choose what to keep', done: !!fields },
    { label: 'Saved', detail: saved ? `${saved} ${saved === 1 ? 'item' : 'items'}` : 'Save what you gave it', done: !!saved },
    { label: 'App', detail: app ? `Written (${app} KB)` : 'Build an app around it', done: !!app },
    { label: 'Checked', detail: checked ? 'Looked at it' : 'Look at it and polish', done: !!checked },
  ];
});
</script>

<template>
  <section class="build" aria-label="Type build progress" aria-live="polite">
    <header><span class="spinner" aria-hidden="true" /><h2>Building {{ name }}</h2></header>
    <p>Dial is making an app for what you keep here</p>
    <ol>
      <li v-for="(row, index) in rows" :key="row.label" :class="{ done: row.done }">
        <span class="mark" aria-hidden="true"><Icon v-if="row.done" name="check" :size="14" /><template v-else>{{ index + 1 }}</template></span>
        <div><strong>{{ row.label }}</strong><span>{{ row.detail }}</span></div>
      </li>
    </ol>
  </section>
</template>

<style scoped>
.build { flex: 1; min-height: 0; overflow: auto; align-content: safe center; padding: 20px; }
header, ol, p { width: min(100%, 380px); margin: 0 auto; }
header { display: flex; align-items: center; gap: 10px; }
h2 { font-size: 21px; line-height: 1.3; margin: 0; overflow-wrap: anywhere; }
p { color: var(--text-muted); font-size: var(--text-sm); padding-block: 8px; }
ol { display: grid; gap: 6px; padding: 6px 0; list-style: none; }
li { display: flex; align-items: center; gap: 12px; border: 1px dashed var(--border-strong); border-radius: var(--radius-md); padding: 9px 12px; color: var(--text-muted); }
li.done { border-style: solid; background: var(--surface); color: var(--text); }
.mark { flex: none; display: grid; place-items: center; width: 20px; height: 20px; font: 12px var(--font-mono); }
.done .mark { color: var(--accent); }
strong { display: block; font-size: 13px; }
li div span { font-size: var(--text-sm); color: var(--text-muted); overflow-wrap: anywhere; }
</style>
