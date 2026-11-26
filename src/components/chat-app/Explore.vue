<script setup lang="ts">
// A diagram whose nodes open explanations. The diagram stays an inert image; clickable areas are
// laid over it from the node boxes in the rendered SVG.
import { computed, onBeforeUnmount, ref, shallowRef, watch, watchEffect } from 'vue';
import type { ChatApp } from '../../chatApp';
import { nodeBoxes, renderDiagram, type NodeBox } from '../../diagram';
import { revealIn, trackOverflow } from '../../scrollFade';
import { addToPrompt } from '../../workspace';
import Markdown from '../Markdown.vue';

const props = defineProps<{ app: Extract<ChatApp, { kind: 'explore' }>; workspaceRoot: string }>();
const emit = defineEmits<{ meta: [string] }>();
const stage = ref<HTMLElement | null>(null);
const scroller = ref<HTMLElement | null>(null);
const boxes = shallowRef<NodeBox[]>([]);
const failed = ref(false);
const rendered = ref(false);
const selected = ref<string | null>(null);
const node = computed(() => (selected.value ? props.app.nodes[selected.value] : undefined));
const label = computed(() => boxes.value.find(b => b.id === selected.value)?.label ?? selected.value);
/** Nodes with an explanation, in diagram order, for prev/next. */
const explorable = computed(() => boxes.value.filter(b => Object.hasOwn(props.app.nodes, b.id)));
const position = computed(() => explorable.value.findIndex(b => b.id === selected.value));
watchEffect(() => {
  const count = explorable.value.length;
  emit('meta', !count ? '' : position.value < 0 ? `${count} nodes` : `${position.value + 1} of ${count} nodes`);
});

let alive = true;
let stopTracking = () => {};
onBeforeUnmount(() => { alive = false; stopTracking(); });
renderDiagram(props.app.diagram).then(diagram => {
  if (!alive || !stage.value) return;
  stage.value.prepend(...diagram.images);
  boxes.value = nodeBoxes(diagram.svg);
  rendered.value = true;
  stopTracking = trackOverflow(scroller.value!);
}, () => { failed.value = true; });

// A tall or wide diagram scrolls inside the card; keep the chosen part in view there.
watch(selected, () => {
  const target = stage.value?.querySelector<HTMLElement>('.hotspot.on');
  if (target && scroller.value) revealIn(scroller.value, target);
}, { flush: 'post' });

function step(by: number) {
  const list = explorable.value;
  if (!list.length) return;
  selected.value = list[((position.value < 0 ? (by > 0 ? -1 : 0) : position.value) + by + list.length) % list.length].id;
}
</script>

<template>
  <div class="explore">
    <p v-if="failed" class="failed">Diagram preview unavailable.</p>
    <div v-else class="canvas">
      <div ref="scroller" class="scroller scroll-x">
        <div ref="stage" class="stage">
          <template v-for="box in boxes" :key="box.id">
            <button
              v-if="Object.hasOwn(app.nodes, box.id)"
              type="button"
              class="hotspot"
              :class="{ on: box.id === selected }"
              :style="{ left: `${box.left}%`, top: `${box.top}%`, width: `${box.width}%`, height: `${box.height}%` }"
              :aria-label="`Explore ${box.label}`"
              :aria-pressed="box.id === selected"
              @click="selected = box.id === selected ? null : box.id"
            />
          </template>
        </div>
      </div>
    </div>
    <div class="detail" aria-live="polite">
      <Transition name="swap" mode="out-in">
        <div v-if="node" :key="selected!" class="entry">
          <h4><span class="dot" aria-hidden="true">◉</span> {{ node.title ?? label }}</h4>
          <Markdown class="text" :text="node.text" :workspace-root="workspaceRoot" nested />
          <button v-if="node.ask" type="button" class="ask" @click="addToPrompt(node.ask)">Ask Dial about this <span aria-hidden="true">↗</span></button>
        </div>
        <div v-else key="intro" class="entry intro">
          <Markdown v-if="app.intro" class="text" :text="app.intro" :workspace-root="workspaceRoot" nested />
          <p v-if="explorable.length" class="hint">Click a highlighted node to explore it.</p>
          <p v-else-if="rendered" class="hint">No node in this diagram has an explanation.</p>
        </div>
      </Transition>
    </div>
    <nav v-if="explorable.length > 1" class="nav">
      <button type="button" @click="step(-1)">← prev</button>
      <button type="button" @click="step(1)">next →</button>
    </nav>
  </div>
</template>

<style scoped>
.canvas {
  margin-top: 8px;
  border-block: 1px solid var(--border);
}
/* Readable size first: a big diagram scrolls inside the card (both ways, edges fading) instead of
 * shrinking, and stays short enough that the explanation below remains in view. */
.scroller {
  max-height: min(62vh, 560px);
  overflow-y: auto;
  overscroll-behavior-y: contain;
  padding: 14px 12px;
  background: radial-gradient(circle, var(--border-strong) 1px, transparent 1.4px) center / 18px 18px;
}
/* Shrink-wraps the diagram, so the hotspots' percentages match it as it scales or scrolls. */
.stage { position: relative; width: fit-content; margin: 0 auto; }
.stage :deep(img) { display: block; max-width: 100%; height: auto; padding: 0; border: 0; border-radius: 0; background: none; }
.stage :deep(img.diagram-light) { display: none; }
:root[data-theme="light"] .stage :deep(img.diagram-light) { display: block; }
:root[data-theme="light"] .stage :deep(img.diagram-dark) { display: none; }
:root[data-theme="light"] .has-backdrop:not(.conversing) .stage :deep(img.diagram-light) { display: none; }
:root[data-theme="light"] .has-backdrop:not(.conversing) .stage :deep(img.diagram-dark) { display: block; }
.hotspot {
  position: absolute;
  border-radius: var(--radius-md);
  background: transparent;
  box-shadow: 0 0 0 1.5px color-mix(in srgb, var(--accent) 45%, transparent);
  cursor: pointer;
  animation: beckon 2.4s var(--ease) 0.8s 2;
  transition: box-shadow 0.15s var(--ease), background 0.15s var(--ease);
}
.hotspot:hover { background: color-mix(in srgb, var(--accent) 10%, transparent); box-shadow: 0 0 0 2px var(--accent); }
.hotspot.on { background: color-mix(in srgb, var(--accent) 16%, transparent); box-shadow: 0 0 0 2px var(--accent), 0 0 0 6px color-mix(in srgb, var(--accent) 18%, transparent); animation: none; }
.hotspot:focus-visible { outline: 2px solid var(--accent); outline-offset: 3px; }
@keyframes beckon { 50% { box-shadow: 0 0 0 4px color-mix(in srgb, var(--accent) 30%, transparent); } }
.detail { min-height: 64px; padding: 12px 16px 14px; }
h4 { display: flex; align-items: center; gap: 8px; margin: 0 0 6px; font-size: 1em; font-weight: 600; }
.dot { font-size: 11px; color: var(--accent); }
.text { color: var(--text-muted); }
.hint { margin: 0; font: var(--text-xs) / 1.4 var(--font-mono); color: var(--text-faint); }
.intro .text + .hint { margin-top: 8px; }
.failed { margin: 8px 16px; color: var(--text-faint); font-size: var(--text-sm); }
.ask { margin-top: 10px; padding: 4px 10px; border-radius: 999px; font-size: var(--text-sm); color: var(--text-muted); box-shadow: inset 0 0 0 1px var(--border-strong); }
.ask span { color: var(--accent); }
.ask:hover { color: var(--text); background: color-mix(in srgb, var(--accent) 9%, transparent); box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--accent) 55%, transparent); }
.nav { display: flex; justify-content: space-between; padding: 4px 8px; border-top: 1px solid var(--border); font: var(--text-xs) / 1.4 var(--font-mono); }
.nav button { padding: 5px 8px; border-radius: var(--radius-sm); color: var(--text-muted); }
.nav button:hover { background: var(--hover); color: var(--accent); }
.swap-enter-active, .swap-leave-active { transition: opacity 0.14s var(--ease), transform 0.14s var(--ease); }
.swap-enter-from { opacity: 0; transform: translateY(4px); }
.swap-leave-to { opacity: 0; }
@media (prefers-reduced-motion: reduce) {
  .hotspot { animation: none; transition: none; }
  .swap-enter-active, .swap-leave-active { transition: none; }
}
</style>
