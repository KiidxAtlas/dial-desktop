<script setup lang="ts">
// Inputs feeding formulas, with results updating as they change. Formulas run in `formula.ts`'s
// small arithmetic language, never as JavaScript.
import { computed, reactive } from 'vue';
import type { ChatApp } from '../../chatApp';
import { evaluate } from '../../formula';

const props = defineProps<{ app: Extract<ChatApp, { kind: 'calculator' }>; workspaceRoot: string }>();
const values = reactive<Record<string, number>>(Object.fromEntries(props.app.inputs.map(i => [i.id, i.value])));

const results = computed(() => {
  const scope: Record<string, number> = { ...values };
  return props.app.outputs.map(o => {
    const value = evaluate(o.formula, scope);
    if (o.id) scope[o.id] = value;
    return value;
  });
});

function format(value: number, decimals: number) {
  if (!Number.isFinite(value)) return '—';
  return value.toLocaleString(undefined, { maximumFractionDigits: decimals });
}
function setNumber(id: string, raw: string, min: number, max: number) {
  const n = Number(raw);
  if (raw.trim() && Number.isFinite(n)) values[id] = Math.min(Math.max(n, min), max);
}
const fill = (value: number, min: number, max: number) => `${((value - min) / (max - min)) * 100}%`;
/** Currency symbols read before the number ($40); other units after it (100 k). */
const leads = (unit: string | undefined) => !!unit && /^[$€£¥₹₩₽]$/.test(unit.trim());
</script>

<template>
  <div class="calc">
    <div class="inputs">
      <div v-for="input in app.inputs" :key="input.id" class="input" :class="`type-${input.type}`">
        <label :for="`in-${input.id}`" class="label">
          <span>{{ input.label }}</span>
          <span v-if="input.type === 'slider'" class="value"><small v-if="leads(input.unit)">{{ input.unit }}</small>{{ format(values[input.id], 6) }}<small v-if="input.unit && !leads(input.unit)"> {{ input.unit }}</small></span>
        </label>
        <input
          v-if="input.type === 'slider'"
          :id="`in-${input.id}`"
          v-model.number="values[input.id]"
          type="range"
          :min="input.min"
          :max="input.max"
          :step="input.step"
          :style="{ '--fill': fill(values[input.id], input.min, input.max) }"
        />
        <div v-else-if="input.type === 'number'" class="number">
          <input
            :id="`in-${input.id}`"
            type="number"
            :value="values[input.id]"
            :min="input.min"
            :max="input.max"
            :step="input.step"
            @input="setNumber(input.id, ($event.target as HTMLInputElement).value, input.min, input.max)"
          />
          <span v-if="input.unit">{{ input.unit }}</span>
        </div>
        <div v-else-if="input.type === 'select'" :id="`in-${input.id}`" class="segments" role="radiogroup" :aria-label="input.label">
          <button
            v-for="option in input.options"
            :key="option.label"
            type="button"
            role="radio"
            :aria-checked="values[input.id] === option.value"
            :class="{ on: values[input.id] === option.value }"
            @click="values[input.id] = option.value"
          >{{ option.label }}</button>
        </div>
        <button
          v-else
          :id="`in-${input.id}`"
          type="button"
          role="switch"
          class="switch"
          :aria-checked="values[input.id] === 1"
          @click="values[input.id] = values[input.id] ? 0 : 1"
        ><span /></button>
      </div>
    </div>
    <div class="outputs" aria-live="polite">
      <div v-for="(output, i) in app.outputs" :key="i" class="output" :class="{ lead: i === app.outputs.length - 1 }">
        <span class="label">{{ output.label }}</span>
        <span class="result"><small v-if="leads(output.unit)">{{ output.unit }}</small>{{ format(results[i], output.decimals) }}<small v-if="output.unit && !leads(output.unit)"> {{ output.unit }}</small></span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.calc { padding: 8px 16px 16px; }
.inputs { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: 14px 20px; }
.input { display: grid; gap: 7px; align-content: start; }
.label { display: flex; justify-content: space-between; gap: 8px; font-size: var(--text-sm); color: var(--text-muted); }
.value small, .result small { margin-left: 0.25em; font-weight: 400; color: var(--text-faint); }
.value { font: 500 var(--text-sm) / 1.4 var(--font-mono); color: var(--text); font-variant-numeric: tabular-nums; }
.type-number .number span { flex: none; }
input[type="range"] {
  width: 100%;
  height: 18px;
  margin: 0;
  background: none;
  -webkit-appearance: none;
  appearance: none;
  cursor: pointer;
}
input[type="range"]::-webkit-slider-runnable-track {
  height: 4px;
  border-radius: 2px;
  background: linear-gradient(90deg, var(--accent) var(--fill), var(--border-strong) var(--fill));
}
input[type="range"]::-webkit-slider-thumb {
  -webkit-appearance: none;
  width: 16px;
  height: 16px;
  margin-top: -6px;
  border-radius: 50%;
  background: var(--text-strong);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 45%, transparent);
}
input[type="range"]:focus-visible { outline: none; }
input[type="range"]:focus-visible::-webkit-slider-thumb { box-shadow: 0 0 0 4px var(--accent); }
.number { display: flex; align-items: center; gap: 8px; color: var(--text-faint); font: var(--text-sm) / 1 var(--font-mono); }
.number input {
  width: 100%;
  height: 30px;
  padding: 0 10px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-sm);
  background: var(--hover);
  color: var(--text);
  font: inherit;
}
.number input:focus { outline: none; border-color: var(--accent); }
.segments { display: flex; flex-wrap: wrap; gap: 2px; padding: 3px; border-radius: var(--radius-md); background: var(--hover); box-shadow: inset 0 0 0 1px var(--border); }
.segments button { flex: 1 0 auto; padding: 4px 10px; border-radius: 7px; font: 500 var(--text-xs) / 1.4 var(--font-mono); color: var(--text-muted); }
.segments button:hover { color: var(--text); }
.segments button.on { color: var(--on-accent); background: var(--accent); }
.switch { position: relative; width: 34px; height: 20px; border-radius: 999px; background: var(--border-strong); transition: background 0.15s var(--ease); }
.switch span { position: absolute; top: 3px; left: 3px; width: 14px; height: 14px; border-radius: 50%; background: var(--text-strong); transition: transform 0.15s var(--ease); }
.switch[aria-checked="true"] { background: var(--accent); }
.switch[aria-checked="true"] span { transform: translateX(14px); }
.outputs {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  gap: 8px;
  margin-top: 16px;
}
.output {
  display: grid;
  gap: 4px;
  padding: 10px 12px;
  border-radius: var(--radius-md);
  background: var(--hover);
  box-shadow: inset 0 0 0 1px var(--border);
}
.output .label { font: var(--text-xs) / 1.4 var(--font-mono); color: var(--text-faint); }
.result { font: 600 1.35em / 1.2 var(--font-mono); color: var(--text-strong); font-variant-numeric: tabular-nums; letter-spacing: -0.02em; }
.output.lead { background: color-mix(in srgb, var(--accent) 12%, transparent); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 45%, transparent); }
.output.lead .result { color: var(--accent); }
@media (prefers-reduced-motion: reduce) { .switch, .switch span { transition: none; } }
</style>
