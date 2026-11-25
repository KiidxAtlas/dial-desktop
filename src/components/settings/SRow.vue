<script setup lang="ts">
// One row: what the setting is on the left, its control on the right (or below, `stacked`).
// Long descriptions show two lines until clicked.
import { ref } from "vue";
import Icon from "../Icon.vue";

defineProps<{
  label: string;
  description?: string;
  /** Changed from the default: a dot, and a reset button. */
  modified?: boolean;
  restart?: boolean;
  stacked?: boolean;
  /** Hover text for the label (the config key, the default). */
  hint?: string;
  resetTitle?: string;
  /** The saved layer that sets the value ("This project", "Everywhere"). */
  badge?: { label: string; title?: string } | null;
  /** A short state after saving ("Saved", "Restart needed"); `kind` colours it. */
  status?: { text: string; kind?: "ok" | "warn" | "bad" } | null;
}>();
const emit = defineEmits<{ reset: [] }>();
const expanded = ref(false);
const LONG = 150;
</script>

<template>
  <div class="s-row" :class="{ stacked }">
    <div class="s-row-text">
      <div class="s-label" :title="hint">
        <span>{{ label }}</span>
        <i v-if="modified" class="s-dot" title="Changed from the default" />
        <span v-if="restart" class="s-tag" title="Takes effect the next time Dial starts">Restart</span>
        <span v-if="badge" class="s-tag scope" :title="badge.title">{{ badge.label }}</span>
        <span v-if="status" class="s-tag" :class="status.kind" role="status">{{ status.text }}</span>
        <button
          v-if="modified"
          class="reset"
          :title="resetTitle ?? 'Reset to the default'"
          :aria-label="resetTitle ?? 'Reset to the default'"
          @click="emit('reset')"
        >
          <Icon name="undo" :size="12" />
        </button>
      </div>
      <p
        v-if="description"
        class="s-desc"
        :class="{ clamp: !expanded && description.length > LONG }"
        :title="!expanded && description.length > LONG ? 'Click to read more' : undefined"
        @click="expanded = true"
      >
        {{ description }}
      </p>
      <slot name="below-text" />
    </div>
    <div v-if="$slots.default" class="s-control"><slot /></div>
  </div>
</template>

<style scoped>
.reset {
  display: grid;
  place-items: center;
  width: 20px;
  height: 20px;
  border-radius: 5px;
  color: var(--text-faint);
  opacity: 0;
  transition: opacity 120ms var(--ease);
}
.s-row:hover .reset,
.reset:focus-visible {
  opacity: 1;
}
.reset:hover {
  background: var(--hover);
  color: var(--text);
}
.s-tag.scope {
  color: var(--accent);
  border-color: color-mix(in srgb, var(--accent) 35%, transparent);
}
.s-desc.clamp {
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
  cursor: pointer;
}
.s-desc.clamp:hover {
  color: var(--text-muted);
}
</style>
