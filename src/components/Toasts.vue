<script setup lang="ts">
import { dismissToast, holdToast, releaseToast, runToastAction, toasts } from "../bridge";
import Icon from "./Icon.vue";

/** The colour a message reads as: problems red, confirmations green, the rest neutral. */
function tone(message: string) {
  if (/\b(fail|failed|could not|couldn't|cannot|can't|error|denied|unable|not found|invalid)\b/i.test(message)) return "bad";
  if (/\b(copied|saved|done|updated|installed|downloaded|exported|restored|removed|enabled|ready)\b/i.test(message)) return "good";
  return "info";
}
</script>

<template>
  <div class="toasts" aria-live="polite">
    <TransitionGroup name="toast">
      <div
        v-for="t in toasts"
        :key="t.id"
        class="toast"
        :class="tone(t.message)"
        :role="tone(t.message) === 'bad' ? 'alert' : 'status'"
        @mouseenter="holdToast(t.id)"
        @mouseleave="releaseToast(t.id)"
      >
        <span class="dot" />
        <span class="message">{{ t.message }}</span>
        <button v-if="t.action" class="action" @click="runToastAction(t.id)">{{ t.action.label }}</button>
        <button class="close" aria-label="Dismiss" title="Dismiss" @click="dismissToast(t.id)">
          <Icon name="cross" :size="12" />
        </button>
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
  z-index: 80;
}
.toast {
  --tone: var(--info);
  display: flex;
  align-items: center;
  gap: 10px;
  max-width: min(520px, calc(100vw - 32px));
  padding: 8px 8px 8px 14px;
  border-radius: var(--radius-md);
  background: var(--raised);
  box-shadow: var(--shadow-pop), inset 3px 0 0 var(--tone);
  font-size: 13px;
  pointer-events: auto;
  user-select: text;
  -webkit-user-select: text;
}
.toast.bad {
  --tone: var(--danger);
}
.toast.good {
  --tone: var(--success);
}
.dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--tone);
  flex: none;
}
.message {
  flex: 1;
  min-width: 0;
  overflow-wrap: anywhere;
}
.action {
  flex: none;
  padding: 3px 10px;
  border-radius: var(--radius-sm);
  color: var(--accent);
  font-weight: 600;
}
.action:hover,
.action:focus-visible {
  background: var(--hover);
}
.close {
  display: grid;
  place-items: center;
  width: 22px;
  height: 22px;
  flex: none;
  border-radius: var(--radius-sm);
  color: var(--text-faint);
  opacity: 0.6;
  transition: opacity 120ms var(--ease), background 120ms var(--ease), color 120ms var(--ease);
}
.toast:hover .close,
.close:focus-visible {
  opacity: 1;
}
.close:hover {
  background: var(--hover);
  color: var(--text);
}
.toast-enter-active,
.toast-leave-active {
  transition:
    opacity 200ms var(--ease),
    transform 200ms var(--ease);
}
.toast-move {
  transition: transform 200ms var(--ease);
}
.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateY(-8px) scale(0.98);
}
</style>
