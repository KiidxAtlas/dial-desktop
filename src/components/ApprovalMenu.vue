<script setup lang="ts">
// How much Dial may do without asking, like a terminal agent's permission modes.
import { computed, onBeforeUnmount, ref, watch } from "vue";
import type { ApprovalMode, OpenSnapshot } from "../types";
import { engine } from "../bridge";
import Icon from "./Icon.vue";

const props = defineProps<{ snap: OpenSnapshot }>();

const MODES: { mode: ApprovalMode; label: string; detail: string }[] = [
  { mode: "readonly", label: "Read-only", detail: "Reads and searches; never changes anything" },
  { mode: "ask", label: "Ask first", detail: "Asks before edits and commands" },
  { mode: "write", label: "Auto-edit", detail: "Edits files freely; asks before risky commands" },
  { mode: "auto", label: "Full auto", detail: "Runs without asking" },
];

const root = ref<HTMLElement | null>(null);
const open = ref(false);
const current = computed(() => MODES.find((m) => m.mode === props.snap.approval) ?? null);
const usable = computed(() => props.snap.connection.state === "live" || props.snap.connection.state === "preview");
defineExpose({ show: () => { if (usable.value && current.value) open.value = true; } });

function pick(mode: ApprovalMode) {
  if (mode !== props.snap.approval) engine.setApproval(mode);
  open.value = false;
}

function onOutside(event: MouseEvent) {
  if (root.value && !root.value.contains(event.target as Node)) open.value = false;
}

watch(open, (isOpen) =>
  isOpen ? document.addEventListener("mousedown", onOutside) : document.removeEventListener("mousedown", onOutside),
);
onBeforeUnmount(() => document.removeEventListener("mousedown", onOutside));
</script>

<template>
  <div ref="root" class="approval-menu">
    <button
      class="trigger"
      :class="current?.mode"
      :disabled="!usable || !current"
      :title="current ? `${current.label}: ${current.detail}` : 'Approval mode'"
      @click.stop="open = !open"
    >
      <Icon name="shield" :size="13" />
      <span>{{ current?.label ?? "Approvals" }}</span>
      <Icon name="chevronDown" :size="12" />
    </button>
    <Transition name="pop">
      <div v-if="open" class="menu" @keydown.esc.stop="open = false">
        <button v-for="m in MODES" :key="m.mode" class="row" :class="{ on: m.mode === snap.approval }" @click="pick(m.mode)">
          <span class="label">{{ m.label }}</span>
          <span class="detail">{{ m.detail }}</span>
          <Icon v-if="m.mode === snap.approval" class="tick" name="check" :size="14" />
        </button>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.approval-menu {
  position: relative;
}
.trigger {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  padding: 0 8px;
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
  color: var(--text-muted);
  white-space: nowrap;
  transition: background 120ms var(--ease);
}
.trigger:hover:not(:disabled) {
  background: var(--hover);
  color: var(--text);
}
.trigger:disabled {
  opacity: 0.5;
}
.trigger.auto {
  color: var(--warning);
}
.menu {
  position: absolute;
  left: 0;
  bottom: calc(100% + var(--pop-gap));
  z-index: 30;
  width: 300px;
  padding: 6px;
  border-radius: var(--radius-md);
  background: var(--raised);
  box-shadow: var(--shadow-pop);
}
.row {
  position: relative;
  display: flex;
  flex-direction: column;
  width: 100%;
  padding: 7px 30px 7px 10px;
  border-radius: var(--radius-sm);
  text-align: left;
}
.row:hover {
  background: var(--hover);
}
.label {
  font-size: 13.5px;
}
.on .label {
  font-weight: 500;
}
.detail {
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.tick {
  position: absolute;
  right: 10px;
  top: 50%;
  transform: translateY(-50%);
  color: var(--text);
}
.pop-enter-active,
.pop-leave-active {
  transition:
    opacity 140ms var(--ease),
    transform 140ms var(--ease);
}
.pop-enter-from,
.pop-leave-to {
  opacity: 0;
  transform: translateY(4px);
}
</style>
