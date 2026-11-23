<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import { canRemoveKey, removeSavedKey } from "../providerKeys";
import { engine, openExternal, providerApi, type KeyProvider, type ProviderAccount, type ProviderEvent } from "../bridge";
import type { OpenSnapshot } from "../types";
import SGroup from "./settings/SGroup.vue";
import SRow from "./settings/SRow.vue";

const props = defineProps<{ snap: OpenSnapshot }>();
const unavailable = computed(() => {
  const c = props.snap.connection;
  if (c.state === "preview") return "Sign-in is unavailable in the design preview.";
  if (c.state === "failed" && c.problem === "missing") return "Install the Dial engine in Settings → Updates first.";
  return "";
});
const accounts = ref<ProviderAccount[]>([]), keys = ref<KeyProvider[]>([]);
const accountId = ref(""), keyId = ref("");
const account = computed(() => accounts.value.find(p => p.provider === accountId.value));
const keyProvider = computed(() => keys.value.find(p => p.provider === keyId.value));
const loaded = ref(false), loading = ref(false), error = ref(""), notice = ref("");
const busy = ref(""), signingOut = ref(false), cancelling = ref(false);
const progress = ref(""), url = ref(""), answer = ref("");
const prompt = ref<Extract<ProviderEvent, {event: "auth_prompt"}> | null>(null);
const input = ref<HTMLInputElement | null>(null);
let session: number | null = null, generation = 0;

function close() {
  generation++;
  if (session !== null) void providerApi.close(session).catch(e => { error.value = String(e); });
  session = null;
  answer.value = "";
}
onBeforeUnmount(close);
function event(e: ProviderEvent) {
  if (e.event === "failed") { error.value = e.message; loaded.value = false; return; }
  if (e.provider !== busy.value) return;
  if (e.event === "auth_progress") progress.value = e.message;
  if (e.event === "auth_url") {
    url.value = /^https:\/\//i.test(e.url) ? e.url : "";
    progress.value = e.instructions || "Complete sign-in in your browser.";
  }
  if (e.event === "auth_prompt") {
    prompt.value = e;
    answer.value = "";
    void nextTick(() => input.value?.focus());
  }
}
async function list(id: number) {
  const result = await providerApi.list(id);
  if (id !== session) return;
  accounts.value = result.providers;
  keys.value = result.key_providers ?? [];
  if (!account.value) accountId.value = accounts.value[0]?.provider ?? "";
  if (!keyProvider.value) keyId.value = (keys.value.find(p => p.provider === "api-key:anthropic") ?? keys.value[0])?.provider ?? "";
  loaded.value = true;
}
async function load() {
  close();
  const run = generation;
  loaded.value = false;
  error.value = "";
  loading.value = true;
  try {
    const id = await providerApi.open(props.snap.workspace.root, e => { if (run === generation) event(e); });
    if (run !== generation) { await providerApi.close(id); return; }
    session = id;
    await list(id);
  } catch (e) { if (run === generation && !error.value) error.value = String(e); }
  finally { if (run === generation) loading.value = false; }
}
onMounted(() => { if (!unavailable.value) void load(); });

async function start(provider: string, logout = false) {
  const id = session;
  if (id === null || busy.value) return;
  busy.value = provider;
  signingOut.value = logout;
  error.value = notice.value = url.value = "";
  const key = provider === keyId.value;
  progress.value = logout ? "Signing out…" : key ? "Preparing secure input…" : "Starting sign-in…";
  try {
    await (logout ? providerApi.logout : providerApi.login)(id, provider);
    if (id !== session) return;
    notice.value = logout ? "Signed out." : key ? "API key saved. Choose a model in the model picker." : "Signed in. Choose a model in the model picker.";
    void engine.refreshModels();
    await list(id);
  } catch (e) { if (id === session) { if (cancelling.value) notice.value = "Setup cancelled."; else error.value = String(e); } }
  finally {
    if (id === session) {
      busy.value = url.value = answer.value = "";
      prompt.value = null;
      cancelling.value = signingOut.value = false;
    }
  }
}
async function removeKey() {
  const id = session, key = keyProvider.value;
  if (id === null || !key || !canRemoveKey(key) || busy.value) return;
  busy.value = key.provider;
  signingOut.value = true; // Not a cancellable sign-in flow.
  error.value = notice.value = "";
  progress.value = "Removing saved key…";
  try {
    const removed = await removeSavedKey(key,
      () => ask(`Remove the saved ${key.name} API key from Dial on this computer for all projects? This does not revoke the key with the provider, change environment variables, or sign you out.`,
        { title: "Remove saved API key?", kind: "warning", okLabel: "Remove key", cancelLabel: "Cancel" }),
      provider => providerApi.removeKey(id, provider),
      async () => { await list(id); return keys.value; }, () => id === session);
    if (removed && id === session) { notice.value = "Saved API key removed from this computer."; void engine.refreshModels(); }
  } catch (e) { if (id === session) error.value = String(e); }
  finally { if (id === session) { busy.value = ""; signingOut.value = false; } }
}
async function submit() {
  const id = session, p = prompt.value;
  if (id === null || !p || (!answer.value && !p.allow_empty)) return;
  const code = answer.value;
  answer.value = "";
  prompt.value = null;
  progress.value = "Waiting for the provider…";
  try { await providerApi.answer(id, p.prompt_id, code); }
  catch (e) { if (session === id && busy.value === p.provider && !prompt.value) { error.value = String(e); prompt.value = p; } }
}
async function cancel() {
  if (session === null || cancelling.value) return;
  cancelling.value = true;
  try { await providerApi.cancel(session, busy.value); }
  catch (e) { if (busy.value) error.value = String(e); cancelling.value = false; }
}
</script>

<template>
  <p v-if="unavailable" class="s-note">{{ unavailable }}</p>
  <template v-else>
    <SGroup title="Provider account" description="Connect a subscription or sign in with your provider.">
      <SRow v-if="loaded && account" label="Account" :description="account.signed_in ? 'Signed in' : 'Not signed in'">
        <select v-model="accountId" aria-label="Account provider" :disabled="!!busy"><option v-for="p in accounts" :key="p.provider" :value="p.provider">{{ p.name }}</option></select>
        <button class="btn btn-secondary" :disabled="!!busy" @click="start(accountId, account.signed_in)">{{ account.signed_in ? 'Sign out' : 'Sign in' }}</button>
      </SRow>
      <SRow v-else :label="loading ? 'Loading providers…' : 'Providers unavailable'" :description="loading ? undefined : 'Check the engine in Settings → Updates, then try again.'">
        <button v-if="!loading" class="btn btn-secondary" @click="load">Try again</button>
      </SRow>
    </SGroup>
    <SGroup v-if="loaded" title="API key" description="Use a key instead of a subscription. Dial stores it on this computer.">
      <SRow v-if="keyProvider" label="Key provider" :description="!keyProvider.key_saved ? 'No saved key' : canRemoveKey(keyProvider) ? 'Key saved' : 'Key saved. This engine does not support removing saved keys from Desktop.'">
        <select v-model="keyId" aria-label="API key provider" :disabled="!!busy"><option v-for="p in keys" :key="p.provider" :value="p.provider">{{ p.name }}</option></select>
        <button class="btn btn-secondary" :disabled="!!busy" @click="start(keyId)">{{ keyProvider.key_saved ? 'Replace key' : 'Add key' }}</button>
        <button v-if="canRemoveKey(keyProvider)" class="btn btn-secondary" :disabled="!!busy" @click="removeKey">Remove key</button>
      </SRow>
      <SRow v-else label="API-key setup unavailable" description="Update the engine in Settings → Updates to add a key here." />
    </SGroup>
    <SGroup v-if="busy" :title="busy === keyId ? keyProvider?.name : account?.name">
      <SRow :label="prompt?.message || progress" stacked>
        <form v-if="prompt" @submit.prevent="submit">
          <input ref="input" v-model="answer" :aria-label="prompt.message" :placeholder="prompt.placeholder || undefined" :type="prompt.secret ? 'password' : 'text'" autocomplete="off" spellcheck="false" />
          <button class="btn btn-primary" :disabled="cancelling || (!answer && !prompt.allow_empty)">{{ busy === keyId ? 'Save key' : 'Continue' }}</button>
        </form>
        <button v-if="url" class="btn btn-secondary" @click="openExternal(url)">Open sign-in page</button>
        <button v-if="!signingOut" class="btn btn-secondary" :disabled="cancelling" @click="cancel">Cancel</button>
      </SRow>
    </SGroup>
    <p v-if="error" role="alert" class="s-note">{{ error }}</p>
    <p v-if="notice" role="status" class="s-note">{{ notice }}</p>
  </template>
</template>

<style scoped>
select, input { min-width: 0; max-width: 100%; border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--surface); color: var(--text); padding: 6px 8px; font: inherit; }
select { max-width: min(240px, 100%); }
form { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; }
</style>
