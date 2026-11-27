<script setup lang="ts">
// Hooks, SSH, model profiles, computer use and the collaboration relay: the terminal's `/hooks`,
// `/ssh`, `/profile`, `/computer` and `dial relay`. Lists come from the engine's `list_hooks`,
// `ssh_hosts`, `get_settings` and `computer_status`; changes save through `set_setting`/
// `reset_setting` and reload the running session. The relay is a `dial relay` process Desktop owns.
import { computed, onMounted, ref } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import { sessionToolsApi, showToast, snapshot } from "../bridge";
import { applySaved, engineCall, readable } from "../engineTools";
import {
  engineTooOld, hookFromDraft, joinCommand, savedHook, sshFromDraft,
  type Hook, type HookDraft, type SshDraft, type SshHost,
} from "../integrations";
import Icon from "./Icon.vue";
import ComputerUseSettings from "./ComputerUseSettings.vue";
import SGroup from "./settings/SGroup.vue";
import SRow from "./settings/SRow.vue";

const preview = computed(() => snapshot.value?.connection.state === "preview");
const live = computed(() => snapshot.value?.connection.state === "live");
const tooOld = ref(false);

/** Plain names for the hook events; unknown events keep their own name. */
const EVENT_LABELS: Record<string, string> = {
  session_start: "Session starts",
  session_end: "Session ends",
  user_prompt_submit: "You send a message",
  pre_tool_use: "Before a tool runs",
  post_tool_use: "After a tool runs",
  stop: "A turn finishes",
  before_compact: "Before compacting",
  before_subagent_spawn: "Before a worker starts",
  before_provider_request: "Before a model request",
  context: "Context is assembled",
  assistant_message: "The model answers",
  after_provider_response: "After a model response",
  session_before_branch: "Before branching",
  session_before_switch: "Before switching sessions",
};
const eventLabel = (e: string) => EVENT_LABELS[e] ?? e;

// ── Hooks ────────────────────────────────────────────────────────────────────
const hooks = ref<Hook[] | null>(null);
const events = ref<string[]>([]);
const hooksError = ref("");
const hookBusy = ref(false);
const addingHook = ref(false);
const hookDraft = ref<HookDraft>({ event: "pre_tool_use", matcher: "", command: "", timeout: "", failure: "default" });
const hookError = ref("");

async function loadHooks() {
  hooksError.value = "";
  try {
    const r = await engineCall<{ hooks: Hook[]; events: string[] }>("list_hooks");
    hooks.value = r.hooks ?? [];
    events.value = r.events ?? [];
  } catch (e) {
    if (engineTooOld(e)) tooOld.value = true;
    else hooksError.value = readable(e);
  }
}

/** Save the whole list where `hooks` is set now (the project layer wins), or for every project. */
async function saveHooks(next: Hook[]) {
  const where = await engineCall<{ scope?: "project" | "global" }>("get_settings", { key: "hooks" })
    .then((r) => r.scope ?? "global")
    .catch(() => "global" as const);
  if (next.length) await engineCall("set_setting", { key: "hooks", value: next.map(savedHook), scope: where });
  else await engineCall("reset_setting", { key: "hooks", scope: where });
  await applySaved().catch((e) => showToast(`Saved, but the running session did not reload: ${readable(e)}`));
}

async function addHook() {
  if (hookBusy.value || !hooks.value) return;
  hookError.value = "";
  let hook: Hook;
  try {
    hook = hookFromDraft(hookDraft.value, events.value);
  } catch (e) {
    hookError.value = readable(e);
    return;
  }
  hookBusy.value = true;
  try {
    await saveHooks([...hooks.value, hook]);
    addingHook.value = false;
    hookDraft.value = { ...hookDraft.value, matcher: "", command: "", timeout: "" };
    showToast("Hook added");
  } catch (e) {
    hookError.value = readable(e);
  } finally {
    hookBusy.value = false;
  }
  await loadHooks();
}

async function removeHook(index: number) {
  const list = hooks.value;
  if (hookBusy.value || !list) return;
  const hook = list[index];
  if (!(await ask(`Remove the hook that runs “${joinCommand(hook.command)}” when: ${eventLabel(hook.event).toLowerCase()}?`, { title: "Remove hook", kind: "warning", okLabel: "Remove" }))) return;
  hookBusy.value = true;
  try {
    await saveHooks(list.filter((_, i) => i !== index));
    showToast("Hook removed");
  } catch (e) {
    hooksError.value = readable(e);
  } finally {
    hookBusy.value = false;
  }
  await loadHooks();
}

const hookAbout = (h: Hook) =>
  [
    h.matcher ? `tools matching ${h.matcher}` : "",
    h.timeout_secs === 0 ? "no time limit" : `stops after ${h.timeout_secs}s`,
    h.fails_closed ? "a failure blocks the action" : "a failure is ignored",
  ].filter(Boolean).join(" · ");

// ── SSH hosts ────────────────────────────────────────────────────────────────
const hosts = ref<SshHost[] | null>(null);
const hostsError = ref("");
const sshBusy = ref("");
const addingHost = ref(false);
const sshDraft = ref<SshDraft>({ name: "", host: "", user: "", port: "", key: "" });
const sshError = ref("");

async function loadHosts() {
  hostsError.value = "";
  try {
    hosts.value = (await engineCall<{ hosts: SshHost[] }>("ssh_hosts")).hosts ?? [];
  } catch (e) {
    if (engineTooOld(e)) tooOld.value = true;
    else hostsError.value = readable(e);
  }
}

async function addHost() {
  if (sshBusy.value || !hosts.value) return;
  sshError.value = "";
  let entry: { name: string; value: Record<string, unknown> };
  try {
    entry = sshFromDraft(sshDraft.value, hosts.value.filter((h) => h.origin === "dial").map((h) => h.name));
  } catch (e) {
    sshError.value = readable(e);
    return;
  }
  sshBusy.value = entry.name;
  try {
    await engineCall("set_setting", { key: `ssh_hosts.${entry.name}`, value: entry.value, scope: "global" });
    await applySaved().catch((e) => showToast(`Saved, but the running session did not reload: ${readable(e)}`));
    addingHost.value = false;
    sshDraft.value = { name: "", host: "", user: "", port: "", key: "" };
    showToast(`Added ${entry.name}`);
  } catch (e) {
    sshError.value = readable(e);
  } finally {
    sshBusy.value = "";
  }
  await loadHosts();
}

async function removeHost(h: SshHost) {
  if (sshBusy.value) return;
  if (!(await ask(`Remove ${h.name} from Dial’s hosts? Your ~/.ssh/config is not changed.`, { title: "Remove SSH host", kind: "warning", okLabel: "Remove" }))) return;
  sshBusy.value = h.name;
  try {
    const where = await engineCall<{ scope?: "project" | "global" }>("get_settings", { key: `ssh_hosts.${h.name}` })
      .then((r) => r.scope ?? "global")
      .catch(() => "global" as const);
    await engineCall("reset_setting", { key: `ssh_hosts.${h.name}`, scope: where });
    await applySaved().catch((e) => showToast(`Removed, but the running session did not reload: ${readable(e)}`));
    showToast(`Removed ${h.name}`);
  } catch (e) {
    hostsError.value = readable(e);
  } finally {
    sshBusy.value = "";
  }
  await loadHosts();
}

const hostTarget = (h: SshHost) => `${h.user ? `${h.user}@` : ""}${h.host}${h.port ? `:${h.port}` : ""}`;

// ── Model profiles (`/profile`) ──────────────────────────────────────────────
interface Profile { provider?: string; model?: string; reasoning_effort?: string | null; context_window?: number | null }
const profiles = ref<Record<string, Profile> | null>(null);
const activeProfile = ref<string | null>(null);
const profilesError = ref("");
const profileBusy = ref(false);

async function loadProfiles() {
  profilesError.value = "";
  try {
    const [table, active] = await Promise.all([
      engineCall<{ value?: Record<string, Profile> | null }>("get_settings", { key: "model_profiles" }),
      engineCall<{ value?: string | null }>("get_settings", { key: "model_profile" }),
    ]);
    profiles.value = table.value && typeof table.value === "object" ? table.value : {};
    activeProfile.value = typeof active.value === "string" && active.value ? active.value : null;
  } catch (e) {
    profilesError.value = readable(e);
  }
}

/** Use profile `name` in this project, or with `null` go back to the selected model. */
async function useProfile(name: string | null) {
  if (profileBusy.value) return;
  profileBusy.value = true;
  try {
    if (name) await engineCall("set_setting", { key: "model_profile", value: name, scope: "project" });
    else await engineCall("reset_setting", { key: "model_profile", scope: "project" });
    await applySaved().catch((e) => showToast(`Saved, but the running session did not reload: ${readable(e)}`));
    showToast(name ? `Using the ${name} profile in this project` : "Profile off: using the selected model");
  } catch (e) {
    profilesError.value = readable(e);
  } finally {
    profileBusy.value = false;
  }
  await loadProfiles();
}

const profileAbout = (p: Profile) =>
  [p.provider && p.model ? `${p.provider}/${p.model}` : p.model, p.reasoning_effort ? `${p.reasoning_effort} reasoning` : "", p.context_window ? `${Math.round(p.context_window / 1000)}k context` : ""]
    .filter(Boolean).join(" · ");

// ── Collaboration relay (`dial relay`) ───────────────────────────────────────
const relayBind = ref<string | null>(null);
const relayAddress = ref("127.0.0.1:7331");
const relayBusy = ref(false);
const relayError = ref("");

async function relay(action: "status" | "start" | "stop") {
  if (relayBusy.value) return;
  relayError.value = "";
  if (action === "start" && !/^(127\.|localhost|\[::1\])/.test(relayAddress.value.trim())
    && !(await ask(`Listen on ${relayAddress.value.trim()}? Other machines on that network can then reach the relay; rooms still need their invite.`, { title: "Start relay", kind: "warning", okLabel: "Start" }))) return;
  relayBusy.value = true;
  try {
    relayBind.value = (await sessionToolsApi.relay(action, action === "start" ? relayAddress.value.trim() : undefined)).bind;
  } catch (e) {
    relayError.value = readable(e);
  } finally {
    relayBusy.value = false;
  }
}

async function load() {
  void relay("status");
  if (!live.value) return;
  // Profiles needs two reads; don't launch both alongside hooks, hosts and computer status.
  const root = snapshot.value?.workspace?.root;
  await Promise.all([loadHooks(), loadHosts()]);
  if (live.value && snapshot.value?.workspace?.root === root && !tooOld.value) await loadProfiles();
}
onMounted(load);
</script>

<template>
  <p v-if="preview" class="s-note">The design preview has no engine, so there is nothing to show.</p>
  <p v-else-if="!live" class="s-note">These load once Dial is connected.</p>
  <SGroup v-else-if="tooOld" title="Update the engine">
    <SRow label="This engine cannot list hooks or SSH hosts" description="Update Dial in Settings → Updates to manage them here. Until then, /hooks and /ssh work in the terminal." />
  </SGroup>
  <template v-else>
    <SGroup title="Lifecycle hooks" description="Programs Dial runs at moments in a session. They run unsandboxed, like MCP servers, and get the event as JSON on standard input.">
      <template #aside>
        <button class="btn btn-secondary" :disabled="hookBusy" @click="loadHooks"><Icon name="refresh" :size="14" /> Refresh</button>
      </template>
      <SRow v-if="hooksError" label="Could not load hooks" :description="hooksError" />
      <SRow v-else-if="!hooks" label="Loading…" />
      <SRow v-else-if="!hooks.length && !addingHook" label="No hooks" description="Add one to check a command before it runs, log every tool call, or notify you when a turn ends." />
      <SRow
        v-for="(h, i) in hooks ?? []"
        :key="i"
        :label="eventLabel(h.event)"
        :description="hookAbout(h)"
        :hint="h.event"
      >
        <code class="cmd" :title="joinCommand(h.command)">{{ joinCommand(h.command) }}</code>
        <button class="btn btn-secondary" :disabled="hookBusy" @click="removeHook(i)">Remove</button>
      </SRow>
      <SRow v-if="hooks && !addingHook" :label="hooks.length ? 'Add another hook' : 'Add a hook'">
        <button class="btn btn-secondary" :disabled="hookBusy || !events.length" @click="addingHook = true; hookError = ''">Add hook</button>
      </SRow>
      <SRow v-else-if="hooks" label="Add a hook" stacked>
        <form class="grid" novalidate @submit.prevent="addHook">
          <label>When
            <select v-model="hookDraft.event" class="s-select"><option v-for="e in events" :key="e" :value="e">{{ eventLabel(e) }}</option></select>
          </label>
          <label>Only for tools matching (optional)
            <input v-model="hookDraft.matcher" class="s-input mono" placeholder="bash|edit" autocomplete="off" spellcheck="false" />
          </label>
          <label class="wide">Program and arguments
            <input v-model="hookDraft.command" class="s-input mono" placeholder="./scripts/guard.sh --strict" autocomplete="off" spellcheck="false" />
          </label>
          <label>Time limit in seconds
            <input v-model="hookDraft.timeout" class="s-input" inputmode="numeric" placeholder="30 (0 for none)" autocomplete="off" />
          </label>
          <label>If the hook fails
            <select v-model="hookDraft.failure" class="s-select">
              <option value="default">{{ hookDraft.event === "pre_tool_use" ? "Block the action (default)" : "Carry on (default)" }}</option>
              <option value="closed">Block the action</option>
              <option value="open">Carry on</option>
            </select>
          </label>
          <p class="s-note wide">Saved {{ hooks.length ? "with your other hooks" : "for every project" }}. Quotes group words; nothing is expanded by a shell.</p>
          <p v-if="hookError" role="alert" class="s-note bad wide">{{ hookError }}</p>
          <div class="actions wide">
            <button class="btn btn-secondary" type="button" :disabled="hookBusy" @click="addingHook = false">Cancel</button>
            <button class="btn btn-primary" type="submit" :disabled="hookBusy">{{ hookBusy ? "Saving…" : "Save hook" }}</button>
          </div>
        </form>
      </SRow>
    </SGroup>

    <SGroup title="SSH hosts" description="Machines Dial may run commands on, with your approval. Hosts from ~/.ssh/config appear on their own; add others here.">
      <template #aside>
        <button class="btn btn-secondary" :disabled="!!sshBusy" @click="loadHosts"><Icon name="refresh" :size="14" /> Refresh</button>
      </template>
      <SRow v-if="hostsError" label="Could not load hosts" :description="hostsError" />
      <SRow v-else-if="!hosts" label="Loading…" />
      <SRow v-else-if="!hosts.length && !addingHost" label="No hosts" description="Nothing in ~/.ssh/config, and none added to Dial yet." />
      <SRow
        v-for="h in hosts ?? []"
        :key="h.name"
        :label="h.name"
        :description="[hostTarget(h), h.key ? `key ${h.key}` : ''].filter(Boolean).join(' · ')"
        :badge="{ label: h.origin === 'dial' ? 'Dial' : 'ssh config', title: h.source ?? 'Saved in Dial’s settings' }"
      >
        <button v-if="h.origin === 'dial'" class="btn btn-secondary" :disabled="!!sshBusy" @click="removeHost(h)">{{ sshBusy === h.name ? "Removing…" : "Remove" }}</button>
      </SRow>
      <SRow v-if="hosts && !addingHost" label="Add a host">
        <button class="btn btn-secondary" :disabled="!!sshBusy" @click="addingHost = true; sshError = ''">Add host</button>
      </SRow>
      <SRow v-else-if="hosts" label="Add a host" stacked>
        <form class="grid" novalidate @submit.prevent="addHost">
          <label>Name
            <input v-model="sshDraft.name" class="s-input mono" placeholder="build-box" autocomplete="off" spellcheck="false" />
          </label>
          <label>Host name or address
            <input v-model="sshDraft.host" class="s-input mono" placeholder="10.0.0.9" autocomplete="off" spellcheck="false" />
          </label>
          <label>User (optional)
            <input v-model="sshDraft.user" class="s-input mono" placeholder="ubuntu" autocomplete="off" spellcheck="false" />
          </label>
          <label>Port (optional)
            <input v-model="sshDraft.port" class="s-input" inputmode="numeric" placeholder="22" autocomplete="off" />
          </label>
          <label class="wide">Key file (optional)
            <input v-model="sshDraft.key" class="s-input mono" placeholder="~/.ssh/id_ed25519" autocomplete="off" spellcheck="false" />
          </label>
          <p v-if="sshError" role="alert" class="s-note bad wide">{{ sshError }}</p>
          <div class="actions wide">
            <button class="btn btn-secondary" type="button" :disabled="!!sshBusy" @click="addingHost = false">Cancel</button>
            <button class="btn btn-primary" type="submit" :disabled="!!sshBusy">{{ sshBusy ? "Saving…" : "Save host" }}</button>
          </div>
        </form>
      </SRow>
    </SGroup>

    <SGroup title="Model profiles" description="Named model setups from [model_profiles] in your settings. A profile replaces the selected model in this project until you turn it off.">
      <template #aside>
        <button class="btn btn-secondary" :disabled="profileBusy" @click="loadProfiles"><Icon name="refresh" :size="14" /> Refresh</button>
      </template>
      <SRow v-if="profilesError" label="Could not load profiles" :description="profilesError" />
      <SRow v-else-if="!profiles" label="Loading…" />
      <SRow v-else-if="!Object.keys(profiles).length" label="No profiles" description="Define one under [model_profiles] in config.toml to switch whole model setups at once." />
      <SRow
        v-for="(p, name) in profiles ?? {}"
        :key="name"
        :label="String(name)"
        :description="profileAbout(p)"
        :badge="activeProfile === name ? { label: 'In use', title: 'Active in this project' } : null"
      >
        <button v-if="activeProfile === name" class="btn btn-secondary" :disabled="profileBusy" @click="useProfile(null)">Turn off</button>
        <button v-else class="btn btn-secondary" :disabled="profileBusy" @click="useProfile(String(name))">Use</button>
      </SRow>
    </SGroup>
  </template>
  <ComputerUseSettings v-if="live && snapshot?.workspace" :key="snapshot.workspace.root" :root="snapshot.workspace.root" />

  <SGroup title="Collaboration relay" description="A relay others can join your shared sessions through. Rooms stay end-to-end encrypted; the relay only forwards them. It stops when Dial quits.">
    <SRow v-if="relayBind" label="Running" :description="`Listening on ${relayBind}`">
      <button class="btn btn-secondary" :disabled="relayBusy" @click="relay('stop')">{{ relayBusy ? "Stopping…" : "Stop" }}</button>
    </SRow>
    <SRow v-else label="Stopped" description="Address to listen on; 127.0.0.1 keeps it on this machine.">
      <input v-model="relayAddress" class="s-input mono" aria-label="Relay address" autocomplete="off" spellcheck="false" />
      <button class="btn btn-secondary" :disabled="relayBusy || !relayAddress.trim()" @click="relay('start')">{{ relayBusy ? "Starting…" : "Start" }}</button>
    </SRow>
    <p v-if="relayError" role="alert" class="s-note bad">{{ relayError }}</p>
  </SGroup>
</template>

<style scoped>
.grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); align-items: start; gap: 10px 12px; width: 100%; }
.grid label { display: flex; flex-direction: column; gap: 4px; font-size: var(--text-sm); color: var(--text-muted); }
.grid .wide { grid-column: 1 / -1; }
.grid .actions { display: flex; justify-content: flex-end; gap: 8px; }
.grid .s-input, .grid .s-select { width: 100%; max-width: 100%; }
.mono { font-family: var(--font-mono); }
.cmd {
  max-width: 260px;
  overflow: hidden;
  font: var(--text-xs) / 1.5 var(--font-mono);
  color: var(--text-muted);
  text-overflow: ellipsis;
  white-space: nowrap;
}
.bad { color: var(--danger); }
</style>
