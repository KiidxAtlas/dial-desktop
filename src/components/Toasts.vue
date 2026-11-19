<script setup lang="ts">
import { toasts } from "../bridge";
</script>

<template>
  <div class="toasts" aria-live="polite">
    <TransitionGroup name="toast">
      <div v-for="t in toasts" :key="t.id" class="toast">
        <span class="dot" />
        <span>{{ t.message }}</span>
      </div>
    </TransitionGroup>
  </div>
</template>

<style scoped>
.toasts {
  position: fixed;
  left: 50%;
  top: 16px;
  transform: translateX(-50%);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  pointer-events: none;
  z-index: 10;
}
.toast {
  display: flex;
  align-items: center;
  gap: 10px;
  max-width: 520px;
  padding: 9px 14px;
  border-radius: var(--radius-md);
  background: var(--raised);
  box-shadow: var(--shadow-pop);
  font-size: 13px;
  pointer-events: auto;
  user-select: text;
  -webkit-user-select: text;
}
.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--warning);
  flex: none;
}
.toast-enter-active,
.toast-leave-active {
  transition:
    opacity 200ms var(--ease),
    transform 200ms var(--ease);
}
.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateY(-8px) scale(0.98);
}
</style>
