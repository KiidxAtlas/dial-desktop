<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { focusComposer } from "../../ui";
import { sendInType, typeApi, typeState, types, type Full } from "../../savedTypes";
import type { OpenSnapshot } from "../../types";
import { attach } from "../../typeHost";
import AppFrame from "./AppFrame.vue";
import BuildProgress from "./BuildProgress.vue";
import Icon from "../Icon.vue";

/**
 * One type, in the middle of the window where the transcript would be: the app Dial built for it,
 * or, until there is one, what Dial is doing to build it. The composer below talks to the type's
 * own conversation. It reloads whenever the type changes (here, in its app, or by the agent).
 */
const props = defineProps<{ id: string; snap: OpenSnapshot }>();

const full = ref<Full | null>(null);
const error = ref("");
const canvas = ref<HTMLElement | null>(null);

watch(() => [props.id, types.revision[props.id]], async (_, __, cleanup) => {
  let current = true;
  cleanup(() => { current = false; });
  try {
    const next = await typeApi.get(props.id);
    if (current) { full.value = next; error.value = ''; }
  } catch (failure) {
    if (current) error.value = String(failure);
  }
}, { immediate: true });

// The agent's types_* tools find the open app (and whether it is on screen) through this.
let detach: (() => void) | null = null;
watch(() => props.id, (id) => {
  detach?.();
  full.value = null;
  detach = attach(id, () => canvas.value);
}, { immediate: true });
onBeforeUnmount(() => detach?.());

const def = computed(() => full.value?.def ?? null);
const progress = computed(() => typeState.firstSaves.get(props.id));
/** Dial is at work in this type's own conversation. */
const running = computed(() => !!def.value && props.snap.turnRunning && def.value.sessions[props.snap.workspace.root] === props.snap.sessionId);

/** Ideas for a type that has nothing yet, phrased for its name. */
const ideas = computed(() => {
  const name = def.value?.name ?? "this";
  return [
    `Make ${name} a beautiful app for what I'll keep here`,
    `What could ${name} be? Suggest three ideas`,
  ];
});
const ask = (text: string) => void sendInType(props.id, text);
</script>

<template>
  <section class="type-view">
    <div v-if="error" class="failed">
      <p>{{ error }}</p>
    </div>
    <div v-else-if="full && def && full.hasApp" ref="canvas" class="canvas" :data-type-capture="id" :style="{ '--ty-accent': def.accent || 'var(--accent)' }">
      <AppFrame :key="id" :full="full" :platform="snap.platform" />
    </div>
    <BuildProgress v-else-if="def && (progress || running)" :name="def.name" :steps="progress ?? {}" :running="running" />
    <div v-else-if="def" class="fresh">
      <span class="glyph">{{ def.icon }}</span>
      <h2>{{ def.name }}</h2>
      <p>
        Dial builds {{ def.name }} into its own small app: a cookbook for recipes, a live weather app
        for your cities, a game. Tell it below what this is for, or save an answer here with the
        bookmark under any reply.
        <template v-if="full?.items.length"> It already holds {{ full.items.length }} {{ full.items.length === 1 ? "item" : "items" }}.</template>
      </p>
      <div class="ideas">
        <button v-for="idea in ideas" :key="idea" class="btn btn-secondary" :disabled="!snap.canSend" @click="ask(idea)">
          <Icon name="sparkle" :size="13" /> {{ idea }}
        </button>
        <button class="btn btn-ghost" @click="focusComposer()">Describe it yourself</button>
      </div>
    </div>
  </section>
</template>

<style scoped>
.type-view {
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.canvas {
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  margin: 0 12px;
  border-radius: 14px;
  overflow: hidden;
  background: var(--surface);
  box-shadow: 0 0 0 1px var(--border);
}
.fresh {
  margin: auto;
  max-width: 460px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding: 24px;
  text-align: center;
  color: var(--text-muted);
}
.fresh .glyph {
  font-size: 44px;
  line-height: 1;
}
.fresh h2 {
  margin: 4px 0 0;
  color: var(--text);
  font-size: 22px;
  font-weight: 600;
}
.fresh p {
  margin: 0 0 6px;
  font-size: 13.5px;
  line-height: 1.55;
}
.ideas {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 6px;
  width: 100%;
}
.ideas .btn {
  justify-content: center;
  gap: 6px;
}
.failed {
  display: grid;
  place-items: center;
  gap: 12px;
  margin: auto;
  color: var(--text-muted);
  font-size: 13px;
}
</style>
