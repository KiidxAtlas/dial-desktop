<script setup lang="ts">
import { computed, ref } from "vue";
import { showToast } from "../../bridge";
import { itemTitle, typeApi, type Full, type Item } from "../../savedTypes";
import FieldValue from "./FieldValue.vue";
import Icon from "../Icon.vue";

/**
 * A wheel: every option a slice. Spin picks one at random (the OS's randomness), turns the wheel
 * so that slice stops under the pointer, then shows it. Ticked-off options sit out.
 */
const props = defineProps<{ full: Full }>();
const emit = defineEmits<{ edit: [item: Item] }>();

const def = computed(() => props.full.def);
const options = computed(() => props.full.items.filter((i) => !i.done));
const SPIN_MS = 5200;

const rotation = ref(0);
const spinning = ref(false);
const picked = ref<Item | null>(null);

function hexToHsl(hex: string): [number, number, number] {
  let h = hex.replace("#", "");
  if (h.length === 3) h = [...h].map((c) => c + c).join("");
  const n = parseInt(h.slice(0, 6), 16);
  if (Number.isNaN(n)) return [16, 64, 60];
  const r = ((n >> 16) & 255) / 255, g = ((n >> 8) & 255) / 255, b = (n & 255) / 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b), l = (max + min) / 2;
  if (max === min) return [0, 0, l * 100];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  const hue = max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [hue * 60, s * 100, l * 100];
}

/** Slices walk the colour wheel from the accent, alternating light and deep. */
const colors = computed(() => {
  const [h, s] = hexToHsl(def.value.theme.accent || "#d97757");
  const n = Math.max(options.value.length, 1);
  return options.value.map((_, i) => `hsl(${(h + (i * 300) / n) % 360} ${Math.max(45, Math.min(80, s))}% ${i % 2 ? 46 : 60}%)`);
});

const R = 200;
function polar(angle: number, radius: number) {
  const a = ((angle - 90) * Math.PI) / 180;
  return [R + radius * Math.cos(a), R + radius * Math.sin(a)];
}
const slices = computed(() => {
  const n = options.value.length;
  const step = 360 / Math.max(n, 1);
  return options.value.map((item, i) => {
    const [x1, y1] = polar(i * step, R);
    const [x2, y2] = polar((i + 1) * step, R);
    const path = n === 1 ? `M ${R} 0 A ${R} ${R} 0 1 1 ${R - 0.01} 0 Z` : `M ${R} ${R} L ${x1} ${y1} A ${R} ${R} 0 ${step > 180 ? 1 : 0} 1 ${x2} ${y2} Z`;
    const mid = i * step + step / 2;
    const label = itemTitle(def.value, item);
    const text = label.length > 22 ? `${label.slice(0, 21)}…` : label;
    /** Bold text is about 0.58 em a character; fit it between the rim and the hub. */
    const size = Math.max(9, Math.min(fontSize.value, 118 / (text.length * 0.58)));
    return { item, path, mid, label: text, size, color: colors.value[i] };
  });
});
const fontSize = computed(() => Math.max(11, Math.min(17, 190 / Math.max(options.value.length, 1) + 8)));

function random(below: number) {
  const buf = new Uint32Array(1);
  crypto.getRandomValues(buf);
  return buf[0] % below;
}

function spin() {
  const n = options.value.length;
  if (spinning.value || n < 2) return;
  picked.value = null;
  const index = random(n);
  const step = 360 / n;
  const jitter = (random(1000) / 1000 - 0.5) * step * 0.7;
  const target = 360 - (index * step + step / 2 + jitter);
  const current = rotation.value % 360;
  rotation.value += 360 * 6 + ((target - current + 360) % 360);
  spinning.value = true;
  const choice = options.value[index];
  window.setTimeout(() => {
    spinning.value = false;
    picked.value = choice;
  }, SPIN_MS);
}

const shownFields = computed(() =>
  picked.value
    ? def.value.fields.filter((f) => f.key !== def.value.view.titleField && f.kind !== "image" && f.kind !== "images" && picked.value!.fields[f.key] !== undefined)
    : [],
);

const draft = ref("");
async function add() {
  const key = def.value.view.titleField;
  const text = draft.value.trim();
  if (!key || !text) return;
  try {
    await typeApi.addItem(def.value.id, { [key]: text });
    draft.value = "";
  } catch (error) {
    showToast(String(error));
  }
}
async function setOut(item: Item, out: boolean) {
  try {
    await typeApi.updateItem(def.value.id, item.id, { done: out });
  } catch (error) {
    showToast(String(error));
  }
}
async function remove(item: Item) {
  try {
    await typeApi.removeItem(def.value.id, item.id);
  } catch (error) {
    showToast(String(error));
  }
}
</script>

<template>
  <div class="decider">
    <section class="wheel-side">
      <div class="wheel-wrap" :class="{ spinning }">
        <svg class="pointer" viewBox="0 0 40 48" aria-hidden="true"><path d="M20 46 L4 10 A17 17 0 1 1 36 10 Z" /><circle cx="20" cy="17" r="6" /></svg>
        <svg class="wheel" viewBox="0 0 400 400" :style="{ transform: `rotate(${rotation}deg)`, transitionDuration: `${SPIN_MS}ms` }" role="img" :aria-label="`${options.length} options`">
          <circle cx="200" cy="200" r="199" class="rim" />
          <g v-for="slice in slices" :key="slice.item.id">
            <path :d="slice.path" :fill="slice.color" class="slice" />
            <text
              :transform="`rotate(${slice.mid > 180 ? slice.mid + 90 : slice.mid - 90} 200 200)`"
              :x="slice.mid > 180 ? 16 : 384" y="200" :text-anchor="slice.mid > 180 ? 'start' : 'end'" dominant-baseline="middle"
              :style="{ fontSize: `${slice.size}px` }"
            >{{ slice.label }}</text>
          </g>
          <circle v-if="!options.length" cx="200" cy="200" r="196" class="empty-wheel" />
        </svg>
        <button class="hub" :disabled="spinning || options.length < 2" @click="spin">
          {{ spinning ? "…" : "Spin" }}
        </button>
      </div>
      <Transition name="pop">
        <div v-if="picked" class="result">
          <span class="eyebrow">{{ def.view.tagline || "The wheel says" }}</span>
          <h2>{{ itemTitle(def, picked) }}</h2>
          <div v-for="f in shownFields" :key="f.key" class="result-field">
            <FieldValue :field="f" :value="picked.fields[f.key]" />
          </div>
          <div class="result-actions">
            <button class="ty-button" @click="spin"><Icon name="refresh" :size="14" /> Spin again</button>
            <button class="ty-ghost" title="Edit" @click="emit('edit', picked)"><Icon name="pencil" :size="13" /></button>
          </div>
        </div>
        <p v-else-if="options.length < 2" class="need">Add at least two options to spin.</p>
      </Transition>
    </section>

    <aside class="options">
      <h3>Options <small>{{ options.length }}</small></h3>
      <ul>
        <li v-for="item in full.items" :key="item.id" :class="{ out: item.done }">
          <span class="dot" :style="{ background: item.done ? 'var(--ty-line)' : colors[options.indexOf(item)] }" />
          <button class="name truncate" @click="emit('edit', item)">{{ itemTitle(def, item) }}</button>
          <button class="ty-ghost" :title="item.done ? 'Put back on the wheel' : 'Leave out for now'" @click="setOut(item, !item.done)">
            <Icon :name="item.done ? 'undo' : 'minus'" :size="13" />
          </button>
          <button class="ty-ghost" title="Remove" @click="remove(item)"><Icon name="cross" :size="12" /></button>
        </li>
      </ul>
      <form class="add" @submit.prevent="add">
        <input v-model="draft" class="ty-input" placeholder="Add an option" />
      </form>
    </aside>
  </div>
</template>

<style scoped>
.decider {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 280px;
  gap: 28px;
  align-items: start;
  max-width: 1080px;
  margin: 0 auto;
  padding: 34px 28px 50px;
}
.wheel-side {
  display: grid;
  justify-items: center;
  gap: 22px;
}
.wheel-wrap {
  position: relative;
  width: min(460px, 100%, calc(100vh - 260px));
  aspect-ratio: 1;
}
.wheel {
  width: 100%;
  height: 100%;
  filter: drop-shadow(0 18px 30px rgb(0 0 0 / 28%));
  transition-property: transform;
  transition-timing-function: cubic-bezier(0.12, 0.72, 0.1, 1);
}
.rim {
  fill: var(--ty-raised);
  stroke: color-mix(in srgb, var(--ty-ink) 18%, transparent);
  stroke-width: 2;
}
.slice {
  stroke: var(--ty-raised);
  stroke-width: 2.5;
}
.wheel text {
  fill: #fff;
  font-family: var(--ty-font);
  font-weight: 700;
  paint-order: stroke;
  stroke: rgb(0 0 0 / 18%);
  stroke-width: 3px;
  pointer-events: none;
}
.empty-wheel {
  fill: none;
  stroke: var(--ty-line);
  stroke-width: 3;
  stroke-dasharray: 8 8;
}
.pointer {
  position: absolute;
  top: -18px;
  left: 50%;
  z-index: 2;
  width: 40px;
  translate: -50% 0;
  transform-origin: 50% 35%;
  filter: drop-shadow(0 4px 6px rgb(0 0 0 / 30%));
}
.pointer path {
  fill: var(--ty-ink);
}
.pointer circle {
  fill: var(--ty-accent);
}
.spinning .pointer {
  animation: tick 120ms ease-in-out infinite alternate;
}
@keyframes tick {
  from { rotate: -9deg; }
  to { rotate: 6deg; }
}
.hub {
  position: absolute;
  top: 50%;
  left: 50%;
  width: 26%;
  aspect-ratio: 1;
  translate: -50% -50%;
  border-radius: 50%;
  background: var(--ty-raised);
  box-shadow: 0 0 0 6px color-mix(in srgb, var(--ty-raised) 60%, transparent), 0 8px 20px rgb(0 0 0 / 30%);
  color: var(--ty-accent);
  font: 800 clamp(15px, 2.2vw, 22px) var(--ty-font);
  letter-spacing: 0.04em;
  transition: transform 100ms var(--ease);
}
.hub:hover:not(:disabled) {
  transform: scale(1.04);
}
.hub:active:not(:disabled) {
  transform: scale(0.95);
}
.hub:disabled {
  color: var(--ty-faint);
}
.result {
  width: min(460px, 100%);
  padding: 18px 22px 20px;
  border-radius: var(--ty-radius);
  background: var(--ty-raised);
  box-shadow: 0 0 0 2px var(--ty-accent), 0 18px 40px color-mix(in srgb, var(--ty-accent) 25%, transparent);
  text-align: center;
}
.eyebrow {
  color: var(--ty-accent);
  font: 700 10.5px var(--font-ui);
  letter-spacing: 0.2em;
  text-transform: uppercase;
}
.result h2 {
  margin: 6px 0 10px;
  font-size: 28px;
  font-weight: 800;
}
.result-field {
  margin-bottom: 8px;
  color: var(--ty-muted);
  font-size: 14px;
}
.result-actions {
  display: flex;
  justify-content: center;
  align-items: center;
  gap: 8px;
  margin-top: 12px;
}
.need {
  color: var(--ty-muted);
  font: 13px var(--font-ui);
}
.options {
  padding: 16px;
  border-radius: var(--ty-radius);
  background: var(--ty-raised);
  box-shadow: 0 1px 0 var(--ty-line), 0 12px 30px rgb(0 0 0 / 10%);
}
.options h3 {
  display: flex;
  align-items: baseline;
  gap: 8px;
  margin: 2px 4px 10px;
  font-size: 15px;
}
.options h3 small {
  color: var(--ty-faint);
  font: 12px var(--font-ui);
}
.options ul {
  max-height: calc(100vh - 330px);
  margin: 0 0 10px;
  padding: 0;
  overflow: auto;
  list-style: none;
}
.options li {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 3px 2px 3px 6px;
  border-radius: 8px;
}
.options li:hover {
  background: var(--ty-line);
}
.options li.out .name {
  color: var(--ty-faint);
  text-decoration: line-through;
}
.dot {
  width: 10px;
  height: 10px;
  flex: none;
  border-radius: 50%;
}
.name {
  flex: 1;
  min-width: 0;
  color: inherit;
  font: inherit;
  font-size: 14px;
  text-align: left;
}
.pop-enter-active {
  animation: pop 320ms cubic-bezier(0.2, 1.4, 0.4, 1) both;
}
@keyframes pop {
  from { opacity: 0; transform: scale(0.85) translateY(8px); }
}
@media (max-width: 900px) {
  .decider {
    grid-template-columns: 1fr;
  }
}
</style>
