<script setup lang="ts">
import { notifications, setNotifications, setNotificationKind } from '../notifications';
const kinds = { approval: 'Approval requests', question: 'Questions', finished: 'Finished turns' } as const;
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
    <template v-if="notifications.enabled">
      <SRow v-for="(label, kind) in kinds" :key="kind" :label="label">
        <SSwitch :label="label" :model-value="notifications.kinds[kind]" :disabled="notifications.busy" @update:model-value="setNotificationKind(kind, $event)" />
      </SRow>
    </template>
  </SGroup>
  <p v-if="notifications.error" class="s-error s-note" role="status">{{ notifications.error }}</p>
  <p class="s-note">Alerts never include prompts, session names or project names.</p>
</template>
