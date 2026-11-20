<script setup lang="ts">
import { notifications, setNotifications } from "../notifications";
import SGroup from "./settings/SGroup.vue";
import SRow from "./settings/SRow.vue";
import SSwitch from "./settings/SSwitch.vue";
</script>

<template>
  <SGroup>
    <SRow
      label="Desktop notifications"
      description="An alert when a session needs approval, asks a question, or finishes while Dial is in the background. Click it to open the session."
    >
      <SSwitch
        :model-value="notifications.enabled"
        :disabled="notifications.busy"
        label="Desktop notifications"
        @update:model-value="setNotifications"
      />
    </SRow>
  </SGroup>
  <p v-if="notifications.error" class="s-error s-note" role="status">{{ notifications.error }}</p>
  <p class="s-note">Alerts never include prompts, session names or project names.</p>
</template>
