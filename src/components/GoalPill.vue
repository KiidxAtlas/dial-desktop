<script setup lang="ts">
// The session's persistent goal (`/goal` in the terminal) as a title-bar pill: shown only while
// a goal exists, with pause, resume, budget and drop in its menu.
import { computed, watch } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import type { OpenSnapshot } from "../types";
import { openMenu, type MenuItem } from "../ui";
import { goal, goalAction, loadGoal } from "../engineTools";

const props = defineProps<{ snap: OpenSnapshot }>();
const live = computed(() => props.snap.connection.state === "live");

// Fresh for each session, after reconnecting, and when a turn ends (the goal loop counts tokens
// and can finish, pause or run out of budget during one).
watch(
  () => [props.snap.sessionId, live.value, props.snap.turnRunning] as const,
  ([, now, running]) => {
    if (!now) goal.value = null;
    else if (!running) void loadGoal();
  },
  { immediate: true },
);

const shown = computed(() => (goal.value && goal.value.status !== "dropped" ? goal.value : null));
const STATUS: Record<string, string> = { active: "Goal", paused: "Goal paused", "budget-limited": "Goal out of budget", complete: "Goal done" };
const label = computed(() => STATUS[shown.value?.status ?? ""] ?? "Goal");
const compact = (n: number) => (n >= 1_000_000 ? `${(n / 1_000_000).toFixed(1)}M` : n >= 1000 ? `${Math.round(n / 1000)}k` : String(n));
const used = computed(() => {
  const g = shown.value;
  if (!g) return "";
  return g.token_budget ? `${compact(g.tokens_used)} of ${compact(g.token_budget)} tokens` : `${compact(g.tokens_used)} tokens`;
});

function menu(event: MouseEvent) {
  const g = shown.value;
  if (!g) return;
  const busy = props.snap.turnRunning;
  const items: MenuItem[] = [];
  if (g.status === "active") items.push({ label: "Pause the goal", icon: "stop", run: () => void goalAction({ action: "pause" }) });
  if (g.status === "paused" || g.status === "budget-limited")
    items.push({ label: "Resume the goal", icon: "refresh", disabled: busy, run: () => void goalAction({ action: "resume" }) });
  items.push({
    label: g.token_budget ? `Change the budget (${compact(g.token_budget)} tokens)…` : "Set a token budget…",
    icon: "activity",
    prompt: {
      label: "Token budget",
      placeholder: "For example 200000; 0 for no limit",
      maxLength: 12,
      confirm: "Save",
      submit: (value) => {
        const n = Number(value.trim().replace(/[,_\s]/g, ""));
        if (!Number.isInteger(n) || n < 0) return;
        void goalAction({ action: "budget", token_budget: n || null });
      },
    },
  });
  items.push(
    { divider: true },
    {
      label: "Drop the goal",
      icon: "trash",
      danger: true,
      run: async () => {
        if (await ask("Drop this goal? Dial stops working toward it; the conversation stays.", { title: "Drop goal", kind: "warning", okLabel: "Drop" })) void goalAction({ action: "drop" });
      },
    },
  );
  openMenu(event, items, event.currentTarget as HTMLElement, g.objective);
}
</script>

<template>
  <button v-if="shown" class="pill goal" :class="shown.status" :title="`${shown.objective}\n${used}`" aria-haspopup="menu" @click="menu">
    <span class="dot" /><span class="goal-label">{{ label }}</span><span class="goal-used">{{ used }}</span>
  </button>
</template>

<style scoped>
.goal {
  flex: none;
  gap: 6px;
  max-width: 260px;
  cursor: pointer;
  transition: background 120ms var(--ease), color 120ms var(--ease);
}
.goal:hover { color: var(--text); }
.dot { width: 6px; height: 6px; flex: none; border-radius: 50%; background: currentColor; }
.goal.active { color: var(--accent); background: color-mix(in srgb, var(--accent) 12%, transparent); }
.goal.complete { color: var(--success); }
.goal.budget-limited { color: var(--warning); }
.goal-used { color: var(--text-faint); font-weight: 400; overflow: hidden; text-overflow: ellipsis; }
@container topbar (max-width: 640px) {
  .goal-used { display: none; }
}
@container topbar (max-width: 480px) {
  .goal-label { display: none; }
  .goal { padding: 0 7px; }
}
</style>
