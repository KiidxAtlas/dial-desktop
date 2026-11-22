<script setup lang="ts">
import { computed } from 'vue';
import type { DesignChoices } from '../../savedTypes';
import Icon from '../Icon.vue';

const props = defineProps<{ name: string; choices: DesignChoices; running: boolean }>();
const started = computed(() => Object.keys(props.choices).length > 0);
const rows = computed(() => {
  const { layout, fields, look } = props.choices;
  return [
    { label: 'Read your answer', detail: started.value ? 'Ready' : props.running ? 'Reading…' : 'Starting the conversation…', done: started.value },
    { label: 'Layout', detail: layout ? [layout.kind, layout.style].filter(Boolean).join(' · ') : 'Choose a layout', done: !!layout },
    { label: 'Fields', detail: fields ? fields.join(', ') : 'Choose what to keep', done: !!fields },
    { label: 'Look', detail: look ? [look.font, look.texture === 'none' ? 'plain' : look.texture, look.accent].filter(Boolean).join(' · ') : 'Choose a font, texture and accent', done: !!look },
  ];
});
</script>

<template>
  <section class="design" aria-label="Type design progress" aria-live="polite">
    <header><span class="spinner" aria-hidden="true" /><h2>Designing {{ name }}</h2></header>
    <p>From the answer you saved</p>
    <ol>
      <li v-for="(row, index) in rows" :key="row.label" :class="{ done: row.done }">
        <span class="mark" aria-hidden="true"><Icon v-if="row.done" name="check" :size="14" /><template v-else>{{ index + 1 }}</template></span>
        <div><strong>{{ row.label }}</strong><span>{{ row.detail }}</span></div>
      </li>
    </ol>
    <p v-if="choices.layout && choices.fields && choices.look">Waiting for the first item to be saved…</p>
  </section>
</template>

<style scoped>
.design { flex: 1; min-height: 0; overflow: auto; align-content: safe center; padding: 20px; }
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
