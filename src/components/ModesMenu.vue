<script setup lang="ts">
// Session modes the terminal toggles with /plan, /vibe, /fast and friends, as one menu.
import { computed, onBeforeUnmount, ref, watch } from "vue";
import type { OpenSnapshot } from "../types";
import { loadModes, modes, setMode, setPlanMode } from "../engineTools";
import Icon from "./Icon.vue";

const props = defineProps<{ snap: OpenSnapshot }>();

const root = ref<HTMLElement | null>(null);
const open = ref(false);
const busy = ref("");
const live = computed(() => props.snap.connection.state === "live");
defineExpose({ show: () => { if (live.value) open.value = true; } });
interface Row { id: string; label: string; detail: string; on: boolean; run: () => Promise<boolean> }

const rows = computed<{ title: string; items: Row[] }[]>(() => {
  const m = modes.value;
  if (!m) return [];
  const toggle = (mode: string, on: boolean) => () => setMode({ mode, enabled: !on });
  return [
    {
      title: "How Dial works",
      items: [
        { id: "plan", label: "Plan mode", detail: "Read-only: Dial plans and asks before changing anything", on: m.plan, run: () => setPlanMode(!m.plan) },
        { id: "vibe", label: "Vibe mode", detail: "Read-only exploring and chatting for this session", on: m.modes.vibe, run: toggle("vibe", m.modes.vibe) },
        { id: "external_thinking", label: "Private thinking", detail: "Reason in a hidden scratchpad, kept out of the transcript", on: m.external_thinking, run: toggle("external_thinking", m.external_thinking) },
        { id: "advisor", label: "Advisor", detail: "A coach reviews each tool round and the final reply", on: m.advisor.enabled, run: () => setMode({ mode: "advisor", state: m.advisor.enabled ? "off" : "on" }) },
        { id: "skillful", label: "Offer skills", detail: "Show the skill catalog to the model this session", on: m.modes.skillful === true, run: () => setMode({ mode: "skillful", enabled: m.modes.skillful !== true }) },
      ],
    },
    {
      title: "Speed and cost",
      items: [
        { id: "fast", label: "Fast", detail: "Priority service tier where the provider offers one", on: m.settings.fast_mode, run: () => setMode({ mode: "fast", state: m.settings.fast_mode ? "off" : "on" }) },
        { id: "slow", label: "Slow and cheaper", detail: "Flex service tier for lower cost", on: m.settings.slow_mode, run: toggle("slow", m.settings.slow_mode) },
        { id: "extended_context", label: "Extended context", detail: "Ask for the model's larger context window", on: m.settings.extended_context, run: toggle("extended_context", m.settings.extended_context) },
        { id: "drafter", label: "Draft prompts first", detail: "Review an improved draft before it goes to the agent (this project)", on: m.settings.turn_optimizer, run: toggle("drafter", m.settings.turn_optimizer) },
      ],
    },
  ];
});
const active = computed(() => rows.value.flatMap((g) => g.items).filter((r) => r.on));
const summary = computed(() => {
  const list = active.value;
  if (!list.length) return "Modes";
  return list.length === 1 ? list[0].label : `${list.length} modes`;
});

async function run(row: Row) {
  if (busy.value) return;
  busy.value = row.id;
  try {
    await row.run();
  } finally {
    busy.value = "";
  }
}

watch(open, (isOpen) => {
  if (isOpen) void loadModes();
  isOpen ? document.addEventListener("mousedown", onOutside) : document.removeEventListener("mousedown", onOutside);
});
// Fresh modes for each session and after reconnecting.
watch(() => [props.snap.sessionId, live.value] as const, ([, now]) => now && void loadModes(), { immediate: true });
function onOutside(event: MouseEvent) {
  if (root.value && !root.value.contains(event.target as Node)) open.value = false;
}
onBeforeUnmount(() => document.removeEventListener("mousedown", onOutside));
</script>

<template>
  <div v-if="live" ref="root" class="modes-menu">
    <button
      class="trigger"
      :class="{ lit: active.length }"
      :title="active.length ? `On: ${active.map((r) => r.label).join(', ')}` : 'Plan mode, vibe, fast and other session modes'"
      @click.stop="open = !open"
    >
      <Icon name="sparkle" :size="13" />
      <span>{{ summary }}</span>
      <Icon name="chevronDown" :size="12" />
    </button>
    <Transition name="pop">
      <div v-if="open" class="menu" @keydown.esc.stop="open = false">
        <p v-if="!modes" class="empty">Loading modes…</p>
        <template v-for="group in rows" :key="group.title">
          <div class="head">{{ group.title }}</div>
          <button
            v-for="r in group.items"
            :key="r.id"
            class="row"
            role="switch"
            :aria-checked="r.on"
            :disabled="!!busy || snap.turnRunning"
            :title="snap.turnRunning ? 'Modes change between turns' : ''"
            @click="run(r)"
          >
            <span class="text">
              <span class="label">{{ r.label }}</span>
              <span class="detail">{{ r.detail }}</span>
            </span>
            <span class="switch" :class="{ on: r.on, busy: busy === r.id }"><i /></span>
          </button>
        </template>
        <p v-if="snap.turnRunning" class="note">Modes change between turns.</p>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.modes-menu {
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
.trigger:hover {
  background: var(--hover);
  color: var(--text);
}
.trigger.lit {
  color: var(--accent);
}
.menu {
  position: absolute;
  left: 0;
  bottom: calc(100% + 10px);
  z-index: 30;
  width: 330px;
  max-height: 60vh;
  overflow: auto;
  padding: 6px;
  border-radius: var(--radius-md);
  background: var(--raised);
  box-shadow: var(--shadow-pop);
}
.head {
  padding: 8px 10px 4px;
  font-size: var(--text-xs);
  color: var(--text-faint);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}
.row {
  display: flex;
  align-items: center;
  gap: 12px;
  width: 100%;
  padding: 7px 10px;
  border-radius: var(--radius-sm);
  text-align: left;
}
.row:hover:not(:disabled) {
  background: var(--hover);
}
.row:disabled {
  opacity: 0.6;
}
.text {
  display: flex;
  flex: 1;
  min-width: 0;
  flex-direction: column;
}
.label {
  font-size: 13.5px;
}
.detail {
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.switch {
  position: relative;
  flex: none;
  width: 28px;
  height: 16px;
  border-radius: 999px;
  background: var(--border-strong);
  transition: background 140ms var(--ease);
}
.switch i {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 12px;
  height: 12px;
  border-radius: 50%;
  background: var(--text);
  transition: transform 140ms var(--ease);
}
.switch.on {
  background: var(--accent);
}
.switch.on i {
  transform: translateX(12px);
}
.switch.busy {
  opacity: 0.5;
}
.empty,
.note {
  margin: 0;
  padding: 8px 10px;
  font-size: var(--text-sm);
  color: var(--text-faint);
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
