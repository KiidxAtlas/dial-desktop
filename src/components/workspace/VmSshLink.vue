<script setup lang="ts">
// The SSH host linked to a VM in App → VM. With a host, the agent runs commands and reads files in the VM through
// Dial's `ssh` tool instead of typing into its screen. Presentation only: AppPane saves the link and re-reads it.
import { computed, ref, watch } from "vue";
import { isSshAlias, type AppVmProvider, type AppVmSshLink } from "../../appRunner";

const props = defineProps<{
  vmId: string;
  vmName: string;
  provider: AppVmProvider;
  link: AppVmSshLink | null;
  hosts: string[];
  busy: boolean;
  /** This VM is the connected run, so Set up SSH can type into it. */
  connected: boolean;
}>();

const emit = defineEmits<{ save: [host: string | null]; setup: [user: string, openTerminal: boolean]; cancel: [] }>();

const user = ref(localStorage.getItem("dial.vmSshUser") ?? "");
const terminalOpen = ref(false);
const validUser = computed(() => /^[A-Za-z0-9._][A-Za-z0-9._-]{0,31}$/.test(user.value.trim()));
const canSetup = computed(() => props.connected && props.provider === "virtualbox" && validUser.value && !props.busy);
function setup() {
  if (!canSetup.value) return;
  localStorage.setItem("dial.vmSshUser", user.value.trim());
  emit("setup", user.value.trim(), !terminalOpen.value);
}

const draft = ref("");
watch(
  () => [props.vmId, props.link?.host] as const,
  () => {
    draft.value = props.link?.host ?? "";
  },
  { immediate: true },
);

const trimmed = computed(() => draft.value.trim());
const valid = computed(() => isSshAlias(trimmed.value));
const unchanged = computed(() => props.link?.source === "linked" && props.link.host === trimmed.value);
const listId = computed(() => `vm-ssh-hosts-${props.vmId.replace(/[^A-Za-z0-9_-]/g, "_")}`);
</script>

<template>
  <section class="vm-ssh" aria-label="SSH access">
    <div v-if="provider === 'virtualbox'" class="setup">
      <div class="row">
        <input
          v-model="user"
          class="field user"
          type="text"
          spellcheck="false"
          autocomplete="off"
          placeholder="VM user name, e.g. kali"
          aria-label="User name inside the VM"
          :disabled="busy"
          @keydown.enter.prevent="setup"
        />
        <button type="button" class="btn btn-primary" :disabled="!canSetup" @click="setup">
          {{ busy ? "Setting up SSH…" : link ? "Set up SSH again" : "Set up SSH" }}
        </button>
        <button v-if="busy && connected" type="button" class="btn btn-secondary" @click="emit('cancel')">Cancel</button>
      </div>
      <label class="check">
        <input v-model="terminalOpen" type="checkbox" :disabled="busy" />
        A terminal is already open and focused in the VM
      </label>
      <p v-if="busy" class="hint">
        Typing the setup command in the VM. <strong>Enter the VM user's sudo password in the VM screen if asked.</strong>
        Dial waits up to 5 minutes for SSH to answer.
      </p>
      <p v-else-if="!connected" class="hint">Connect to the VM and log in to its desktop, then press Set up SSH.</p>
      <template v-else>
        <p class="hint">One press sets up SSH in the VM and links it here.</p>
        <details class="tech">
          <summary>Technical details</summary>
          <p class="hint">
            Dial opens a terminal in the VM (Ctrl+Alt+T), types a command that enables sshd and trusts a new key,
            then adds a <code>Host</code> entry for this VM to <code>~/.ssh/config</code> and links it here.
          </p>
          <p class="hint">
            Needs a VirtualBox VM running a Debian/Ubuntu-style guest with <code>apt</code>, logged in to a desktop where
            Ctrl+Alt+T opens a terminal (or tick the box above with a terminal already focused). Dial may wait up to
            5 minutes for SSH to answer; Cancel stops the wait.
          </p>
        </details>
      </template>
    </div>
    <label class="title" :for="`${listId}-input`">SSH host for commands</label>
    <div class="row">
      <input
        :id="`${listId}-input`"
        v-model="draft"
        :list="listId"
        class="field"
        type="text"
        spellcheck="false"
        autocomplete="off"
        placeholder="Host alias from ~/.ssh/config"
        :disabled="busy"
        @keydown.enter.prevent="valid && !unchanged && emit('save', trimmed)"
      />
      <datalist :id="listId">
        <option v-for="host in hosts" :key="host" :value="host" />
      </datalist>
      <button type="button" class="btn btn-secondary" :disabled="busy || !valid || unchanged" @click="emit('save', trimmed)">Link</button>
      <button v-if="link?.source === 'linked'" type="button" class="btn btn-danger" :disabled="busy" @click="emit('save', null)">Unlink</button>
    </div>
    <p v-if="draft && !valid" class="hint warn">Use a single host alias: letters, digits and . _ - @ : only.</p>
    <p v-else-if="link" class="hint">
      The agent runs commands in <strong>{{ vmName }}</strong> with <code>ssh {{ link.host }}</code>
      <template v-if="link.source === 'auto'"> (found in your SSH config by the VM's name)</template>
      while Agent access is on, and keeps the screen for graphical apps. Each command still needs your usual approval.
    </p>
    <p v-else class="hint">
      Optional. Link a host alias from <code>~/.ssh/config</code> that reaches this VM, and the agent can run commands
      and read results over SSH instead of taking screenshots. An alias named like the VM is used automatically.
    </p>
  </section>
</template>

<style scoped>
.vm-ssh {
  display: flex;
  flex-direction: column;
  gap: 6px;
  flex: none;
}
.title {
  font-size: var(--text-sm);
  font-weight: 600;
}
.row {
  display: flex;
  gap: 6px;
  align-items: center;
}
.row > button {
  flex: none;
}
.field {
  flex: 1;
  min-width: 0;
  height: 30px;
  padding: 0 8px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--surface);
  color: var(--text);
  font-size: var(--text-sm);
  font-family: var(--font-mono, monospace);
}
.setup {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--border);
}
.field.user {
  font-family: inherit;
}
.check {
  display: flex;
  gap: 6px;
  align-items: center;
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.hint {
  margin: 0;
  font-size: var(--text-sm);
  color: var(--text-faint);
}
.hint.warn {
  color: var(--danger);
}
.tech summary {
  cursor: pointer;
  font-size: var(--text-sm);
  color: var(--text-muted);
}
.tech .hint {
  margin-top: 4px;
}
</style>
