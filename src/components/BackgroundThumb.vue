<script setup lang="ts">
// A preset's live thumbnail: the same painter as the backdrop, at tile size.
import { onBeforeUnmount, ref, watch } from "vue";
import { background, motionChanged, show, type Preset } from "../backgrounds";

const props = defineProps<{ preset: Preset }>();
const canvas = ref<HTMLCanvasElement | null>(null);
let stop = () => {};

watch(
  [canvas, () => props.preset, () => background.motion, motionChanged],
  ([el, p]) => {
    stop();
    stop = el ? show(el, p) : () => {};
  },
  { flush: "post" },
);
onBeforeUnmount(() => stop());
</script>

<template>
  <canvas ref="canvas" />
</template>
