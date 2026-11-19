<script setup lang="ts">
// The background behind the conversation pane: built-in artwork painted to the pane's size (or
// animated), or the user's image, under a scrim that darkens it while a conversation is on screen.
import { onBeforeUnmount, ref, watch } from "vue";
import { background, customUrl, motionChanged, preset, show } from "../backgrounds";

const props = defineProps<{ /** A conversation is on screen; darken for legibility. */ dimmed: boolean }>();

const canvas = ref<HTMLCanvasElement | null>(null);
let stop = () => {};

watch(
  [canvas, preset, () => background.motion, motionChanged],
  ([el, p]) => {
    stop();
    stop = el && p ? show(el, p) : () => {};
  },
  { flush: "post" },
);
onBeforeUnmount(() => stop());
</script>

<template>
  <div class="backdrop" aria-hidden="true">
    <canvas v-if="preset" ref="canvas" />
    <img v-else-if="customUrl" :src="customUrl" alt="" />
    <div class="scrim" :style="{ opacity: props.dimmed ? background.dim : 0 }" />
    <div class="floor" />
  </div>
</template>

<style scoped>
.backdrop {
  position: absolute;
  inset: 0;
  z-index: -1;
  overflow: hidden;
  pointer-events: none;
  animation: fade 500ms var(--ease) both;
}
canvas,
img {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.scrim {
  position: absolute;
  inset: 0;
  background: var(--bg);
  transition: opacity 400ms var(--ease);
}
/* Keeps the composer and its caption readable over bright artwork. */
.floor {
  position: absolute;
  inset: auto 0 0;
  height: 180px;
  background: linear-gradient(to top, rgb(0 0 0 / 35%), transparent);
}
@keyframes fade {
  from {
    opacity: 0;
  }
}
</style>
