<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import { showToast, snapshot } from "../bridge";
import { applySaved, engineCall, readable } from "../engineTools";
import { computerSetting, engineTooOld, retryIntegrationRead } from "../integrations";
import SGroup from "./settings/SGroup.vue";
import SRow from "./settings/SRow.vue";

interface ComputerStatus { enabled: boolean; max_width: number; max_height: number; capabilities: Record<string, unknown> | null; unavailable: string | null }
const props = defineProps<{ root: string }>();
const computer = ref<ComputerStatus | null>(null);
const error = ref("");
const busy = ref(false);
let mounted = true;
onUnmounted(() => { mounted = false; });
const current = () => mounted && snapshot.value?.workspace?.root === props.root;

async function load() {
  error.value = "";
  computer.value = null;
  try {
    computer.value = await retryIntegrationRead(() => engineCall<ComputerStatus>("computer_status", {}, props.root), current);
  } catch (e) {
    error.value = engineTooOld(e) ? "Update the engine in Settings → Updates to manage computer use here; until then /computer works in the terminal." : readable(e);
  }
}
onMounted(load);

const denied = computed(() => {
  const caps = computer.value?.capabilities ?? {};
  return ([["capturePermission", "Screen Recording"], ["inputPermission", "Accessibility"]] as const)
    .filter(([key]) => caps[key] === "denied").map(([, label]) => label);
});

async function setComputer(on: boolean) {
  if (busy.value || !computer.value || !current()) return;
  busy.value = true;
  try {
    if (on && !(await ask("Let Dial see the screen and use the mouse and keyboard? Every run and input action asks for approval first; screenshots and window lists do not.", { title: "Turn on computer use", kind: "warning", okLabel: "Turn on" }))) return;
    const setting = await retryIntegrationRead(() => engineCall<Parameters<typeof computerSetting>[0]>("get_settings", { key: "integrations" }, props.root), current);
    if (!current()) return;
    await engineCall("set_setting", computerSetting(setting, on), props.root);
    if (snapshot.value?.workspace?.root !== props.root) return;
    await applySaved(); // Desktop defers the reload if a turn is in progress.
    showToast(`Computer use preference saved: ${on ? "on" : "off"}`);
    if (current()) await load();
  } catch (e) {
    error.value = readable(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <SGroup title="Computer use" description="Lets Dial take screenshots and control the mouse and keyboard on this machine.">
    <SRow v-if="error" :label="computer ? 'Could not update computer use' : 'Could not check computer use'" :description="error">
      <button class="btn btn-secondary" :disabled="busy" @click="load">Check again</button>
    </SRow>
    <SRow v-else-if="!computer" label="Checking…" />
    <SRow v-else-if="computer.unavailable && !computer.enabled" label="Not available here" :description="computer.unavailable" />
    <template v-else>
      <SRow :label="computer.enabled ? 'On' : 'Off'"
        :description="computer.enabled
          ? `Screenshots are scaled to at most ${computer.max_width}×${computer.max_height}. Each run and input action asks first.`
          : 'Turn on to let Dial operate apps on screen, with your approval for each action.'">
        <button class="btn btn-secondary" :disabled="busy" @click="setComputer(!computer.enabled)">{{ busy ? "Saving…" : computer.enabled ? "Turn off" : "Turn on" }}</button>
      </SRow>
      <SRow v-if="computer.unavailable" label="Not available here" :description="computer.unavailable" />
      <SRow v-if="denied.length" label="Permission needed" :description="`Grant ${denied.join(' and ')} to Dial in System Settings → Privacy & Security, then restart Dial.`" />
    </template>
  </SGroup>
</template>
