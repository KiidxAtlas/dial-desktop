<script setup lang="ts">
import { notifications, setNotifications } from "../notifications";
</script>

<template>
  <section>
    <h2>Notifications</h2>
    <p>Get a desktop alert when a session needs your approval, asks a question, or ends a turn while Dial is unfocused.</p>
    <p>Click an alert to open its session. Alerts don’t include prompts, session names, or project names.</p>
    <button
      class="btn btn-secondary"
      role="switch"
      :aria-checked="notifications.enabled"
      :disabled="notifications.busy"
      @click="setNotifications(!notifications.enabled)"
    >Desktop notifications: {{ notifications.enabled ? 'On' : 'Off' }}</button>
    <p v-if="notifications.error" role="status">{{ notifications.error }}</p>
  </section>
</template>

<style scoped>
h2 {
  margin: 0;
  font-family: ui-serif, Georgia, "Times New Roman", serif;
  font-size: 24px;
  font-weight: 400;
}
p {
  margin: 10px 0;
  max-width: 56ch;
  font-size: var(--text-sm);
  color: var(--text-muted);
}
</style>
