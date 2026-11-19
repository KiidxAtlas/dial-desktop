<script setup lang="ts">
// A slim line under the top bar while a newer app or engine release exists.
import { computed } from "vue";
import { bannerKey, engineUpdateAvailable, installApp, updateEngine, updates } from "../updates";
import { ui } from "../ui";
import Icon from "./Icon.vue";

const engineLatest = computed(() =>
  updates.engine?.state === "checked" && engineUpdateAvailable() ? updates.engine.status : null,
);
const text = computed(() => {
  const parts = [];
  if (updates.app) parts.push(`Dial Desktop ${updates.app.version}`);
  if (engineLatest.value) parts.push(`Dial engine ${engineLatest.value.latest}`);
  return parts.length ? `${parts.join(" and ")} ${parts.length > 1 ? "are" : "is"} available` : "";
});
const shown = computed(() => !!text.value && updates.dismissed !== bannerKey());
const busy = computed(() => updates.appProgress !== undefined || !!updates.engineBusy);

async function update() {
  // The engine first: installing the app restarts it.
  if (engineLatest.value?.can_update) await updateEngine();
  if (updates.app) await installApp();
  else if (engineLatest.value && !engineLatest.value.can_update) openUpdates();
}

function openUpdates() {
  ui.settingsTab = "updates";
  ui.settingsOpen = true;
}
</script>

<template>
  <div v-if="shown" class="banner" role="status">
    <Icon name="download" :size="14" />
    <span class="truncate">{{ busy ? "Updating…" : text }}</span>
    <button class="link" :disabled="busy" @click="update">Update</button>
    <button class="link muted" @click="openUpdates">Details</button>
    <button class="close" aria-label="Hide until the next release" @click="updates.dismissed = bannerKey()">
      <Icon name="cross" :size="12" />
    </button>
  </div>
</template>

<style scoped>
.banner {
  flex: none;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 16px;
  border-bottom: 1px solid var(--border);
  background: color-mix(in srgb, var(--accent) 8%, var(--bg));
  font-size: var(--text-sm);
  color: var(--text-muted);
}
.banner > :first-child {
  color: var(--accent);
  flex: none;
}
.link {
  flex: none;
  color: var(--text);
  font-weight: 500;
}
.link:hover:not(:disabled) {
  text-decoration: underline;
}
.link:disabled {
  opacity: 0.5;
}
.muted {
  color: var(--text-muted);
  font-weight: 400;
}
.close {
  margin-left: auto;
  display: grid;
  place-items: center;
  width: 22px;
  height: 22px;
  border-radius: var(--radius-sm);
  color: var(--text-faint);
}
.close:hover {
  background: var(--hover);
  color: var(--text);
}
</style>
