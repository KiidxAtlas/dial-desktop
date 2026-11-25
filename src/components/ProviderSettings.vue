<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { ask } from "@tauri-apps/plugin-dialog";
import { canRemoveKey, removeSavedKey } from "../providerKeys";
import { engine, models, openExternal, providerApi, settings, settingsPage, setupApi, type KeyProvider, type ProviderAccount, type ProviderEvent, type StoredAccount } from "../bridge";
import { applySaved, engineCall, readable } from "../engineTools";
import { checkEndpoint, editedTable, endpointForm, endpointTable, ENDPOINT_PROTOCOLS, testProviderId, testResult, type EndpointForm, type ListedReply, type TestResult } from "../providerEndpoint";
import type { SettingScope, Snapshot } from "../types";
import SGroup from "./settings/SGroup.vue";
import SRow from "./settings/SRow.vue";

const props = defineProps<{ snap: Snapshot }>();
/** Connection tests and custom endpoints go through the project's engine. */
const hasProject = computed(() => !!props.snap.workspace);
const unavailable = computed(() => {
  const c = props.snap.connection;
  if (c.state === "preview") return "Sign-in is unavailable in the design preview.";
  if (c.state === "failed" && c.problem === "missing") return "Install the Dial engine in Settings → Updates first.";
  return "";
});
const accounts = ref<ProviderAccount[]>([]), keys = ref<KeyProvider[]>([]);
/** Custom endpoints that take a saved key (`api-key:<id>`); `null` when the engine has no such list. */
const endpointKeys = ref<KeyProvider[] | null>(null);
const endpointKey = (id: string) => endpointKeys.value?.find(k => k.provider === `api-key:${id}`);
/** The key provider a prompt is running for, catalog or custom endpoint. */
const busyKey = computed(() => [...keys.value, ...(endpointKeys.value ?? [])].find(k => k.provider === busy.value));
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
  endpointKeys.value = result.endpoint_keys ?? null;
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
    const id = await providerApi.open(props.snap.workspace?.root ?? null, e => { if (run === generation) event(e); });
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
  const key = provider.startsWith("api-key:");
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
async function removeKey(key: KeyProvider | undefined = keyProvider.value) {
  const id = session;
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
      async () => { await list(id); return [...keys.value, ...(endpointKeys.value ?? [])]; }, () => id === session);
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

// ── Stored accounts (`dial auth accounts`) ───────────────────────────────────

/** Sign-ins kept for the chosen provider; empty when it keeps one or the engine has no list. */
const stored = ref<StoredAccount[]>([]), storedBusy = ref(""), storedError = ref("");
let storedRun = 0;
async function loadStored() {
  const run = ++storedRun, provider = accountId.value;
  storedError.value = "";
  if (!provider || !account.value?.signed_in) { stored.value = []; return; }
  // Providers without stored accounts answer with an error; that is simply "none".
  const list = await setupApi.accounts(provider).catch(() => []);
  if (run === storedRun) stored.value = list;
}
watch([accountId, () => account.value?.signed_in], () => void loadStored());
async function changeStored(a: StoredAccount, action: "select" | "remove") {
  if (storedBusy.value || busy.value) return;
  if (action === "remove") {
    const ok = await ask(`Remove the stored ${account.value?.name ?? accountId.value} account “${a.detail || a.key}” from this computer? This does not close the account with the provider.`,
      { title: "Remove stored account?", kind: "warning", okLabel: "Remove", cancelLabel: "Cancel" }).catch(() => false);
    if (!ok) return;
  }
  storedBusy.value = a.key;
  storedError.value = "";
  try {
    stored.value = await setupApi.accounts(accountId.value, action, a.key);
    notice.value = action === "select" ? `Now using “${a.detail || a.key}”. New sessions use it; reconnect a running one to switch.` : "Stored account removed.";
    void engine.refreshModels();
    if (session !== null) await list(session);
  } catch (e) {
    storedError.value = String(e);
  } finally {
    storedBusy.value = "";
  }
}

// ── Connection tests ─────────────────────────────────────────────────────────

/** The last test per `list_models` provider id; shown under the row that ran it. */
const tests = ref<Record<string, { busy: boolean; result?: TestResult; error?: string }>>({});
async function testConnection(provider: string) {
  if (tests.value[provider]?.busy) return;
  tests.value = { ...tests.value, [provider]: { busy: true } };
  let next: { busy: boolean; result?: TestResult; error?: string };
  try {
    // A test right after saving an endpoint must see it: wait for that apply first.
    await applying?.catch(() => {});
    const reply = await engineCall<ListedReply>("list_models", { provider, refresh: true });
    next = { busy: false, result: testResult(reply, provider) };
    if (next.result?.ok) void engine.refreshModels();
  } catch (e) {
    next = { busy: false, error: `Test failed: ${readable(e)}` };
  }
  tests.value = { ...tests.value, [provider]: next };
}
const testText = (provider: string) => {
  const t = tests.value[provider];
  return !t ? "" : t.busy ? "Testing…" : t.error ?? t.result?.text ?? "";
};
/** A short tag beside the row's name; the description carries the full result. */
const testStatus = (provider: string) => {
  const t = tests.value[provider];
  if (!t) return null;
  if (t.busy) return { text: "Testing…" };
  return t.error || !t.result?.ok ? { text: "Failed", kind: "bad" as const } : { text: "Connected", kind: "ok" as const };
};

// ── Custom endpoints (`providers.<id>`) ──────────────────────────────────────

/** The saved `providers` table, and the layer that sets it (when the engine says). */
const providersRow = computed(() => settingsPage.value?.settings.find((s) => s.key === "providers"));
const savedTables = computed<Record<string, Record<string, unknown>>>(() => {
  const table = providersRow.value?.value;
  if (!table || typeof table !== "object" || Array.isArray(table)) return {};
  return Object.fromEntries(Object.entries(table as Record<string, unknown>).filter(([, spec]) => spec && typeof spec === "object" && !Array.isArray(spec))) as Record<string, Record<string, unknown>>;
});
/** Configured providers from the saved `providers` table (loaded with the settings page). */
const configured = computed(() =>
  Object.entries(savedTables.value).flatMap(([id, s]) => {
    if (typeof s.base_url !== "string" || !s.base_url) return [];
    return [{ id, label: typeof s.label === "string" && s.label ? s.label : id, baseUrl: s.base_url }];
  }),
);
/** Where an endpoint is saved: the `providers` layer the engine reports, else everywhere. A
 *  remove that leaves it in place (also saved in the other layer) flips to that one. */
const otherLayer = ref<Record<string, SettingScope>>({});
const scopeOf = (id: string): SettingScope => otherLayer.value[id] ?? (providersRow.value?.source === "project" ? "project" : "global");
/** Every provider id Dial already knows, so a new endpoint does not replace one. */
const knownIds = computed(() => new Set([
  ...configured.value.map((p) => p.id),
  ...models.value.map((m) => m.provider),
  ...accounts.value.map((p) => testProviderId(p.provider)),
  ...accounts.value.map((p) => p.provider),
  ...keys.value.map((p) => testProviderId(p.provider)),
].filter((id) => id !== editing.value)));
const blank = (): EndpointForm => ({ id: "", label: "", baseUrl: "", protocol: "chat-completions", apiKeyEnv: "", allowInsecure: false });
const adding = ref(false), saving = ref(false), tried = ref(false), formError = ref(""), formNotice = ref("");
/** The endpoint id being edited; `null` while adding a new one. */
const editing = ref<string | null>(null);
const removing = ref("");
const form = ref<EndpointForm>(blank());
/** A saved protocol this form does not list stays selectable (and valid) while editing. */
const savedProtocol = computed(() => {
  const p = editing.value ? savedTables.value[editing.value]?.protocol : undefined;
  return typeof p === "string" && !ENDPOINT_PROTOCOLS.some((x) => x.value === p) ? p : "";
});
const protocols = computed(() => [...ENDPOINT_PROTOCOLS, ...(savedProtocol.value ? [{ value: savedProtocol.value, label: savedProtocol.value }] : [])]);
const check = computed(() => {
  const result = checkEndpoint(form.value, knownIds.value);
  if (savedProtocol.value && form.value.protocol === savedProtocol.value) delete result.errors.protocol;
  return result;
});
const valid = computed(() => Object.keys(check.value.errors).length === 0);
/** The apply after the last endpoint change; tests wait for it. */
let applying: Promise<void> | null = null;
const applyingNow = ref(false);

/** Apply saved settings and resolve once the engine has (a connection test then sees them).
 *  Mid-turn the engine refuses; the session applies them after the turn: `false`. */
function applyNow(): Promise<boolean> {
  const run = (async () => {
    if (props.snap.turnRunning) {
      await applySaved();
      return false;
    }
    try {
      await engineCall("apply_settings");
    } catch {
      await applySaved();
      return false;
    }
    // Re-read the saved values and wait for the fresh page, so the list shows the change.
    await settings.load();
    const shown = settingsPage.value;
    await new Promise<void>((resolve) => {
      const timer = setTimeout(() => { stop(); resolve(); }, 5000);
      const stop = watch(settingsPage, (page) => { if (page !== shown) { clearTimeout(timer); stop(); resolve(); } });
    });
    void engine.refreshModels();
    return true;
  })();
  applyingNow.value = true;
  const done = run.then(() => {}, () => {}).finally(() => { if (applying === done) { applying = null; applyingNow.value = false; } });
  applying = done;
  return run;
}

function openForm(id: string | null = null) {
  editing.value = id;
  form.value = id && savedTables.value[id] ? endpointForm(id, savedTables.value[id]) : blank();
  tried.value = false;
  formError.value = formNotice.value = "";
  adding.value = true;
}
async function saveEndpoint() {
  tried.value = true;
  if (!valid.value || saving.value) return;
  const id = form.value.id.trim(), edit = editing.value;
  const where: SettingScope = edit ? scopeOf(edit) : "global";
  const name = form.value.label.trim() || id;
  const keyNote = form.value.apiKeyEnv.trim() ? ` Dial reads its key from $${form.value.apiKeyEnv.trim()} when it starts.` : "";
  saving.value = true;
  formError.value = formNotice.value = "";
  try {
    // One table, so the engine never sees a half-made provider.
    const value = edit ? editedTable(savedTables.value[edit] ?? {}, form.value) : endpointTable(form.value);
    await engineCall("set_setting", { key: `providers.${id}`, value, scope: where });
    adding.value = false;
    editing.value = null;
    formNotice.value = `Saved “${name}”${where === "project" ? " for this project" : " for every project"}. Applying…`;
    saving.value = false;
    const applied = await applyNow();
    // The sign-in connection reads the saved configuration, so the new endpoint can take a key.
    if (session !== null) await list(session).catch(() => {});
    formNotice.value = `Saved “${name}”${where === "project" ? " for this project" : " for every project"}.${keyNote}${applied ? " Testing the connection…" : " It applies when the running turn ends; test it then."}`;
    if (applied) await testConnection(id);
  } catch (e) {
    formError.value = `Could not save the endpoint: ${readable(e)}`;
  } finally {
    saving.value = false;
  }
}
async function removeEndpoint(p: { id: string; label: string }) {
  if (removing.value) return;
  const where = scopeOf(p.id);
  const ok = await ask(
    `Remove the “${p.label}” endpoint (providers.${p.id}) from Dial’s configuration ${where === "project" ? "for this project" : "for every project"}? Sessions using its models need another model afterwards. The server itself and any API key variable are untouched.${endpointKey(p.id)?.key_saved ? " Its saved key stays on this computer; use Remove key first to delete it." : ""}`,
    { title: "Remove endpoint?", kind: "warning", okLabel: "Remove", cancelLabel: "Cancel" },
  ).catch(() => false);
  if (!ok) return;
  removing.value = p.id;
  formError.value = formNotice.value = "";
  try {
    await engineCall("reset_setting", { key: `providers.${p.id}`, scope: where });
    const applied = await applyNow();
    if (session !== null) await list(session).catch(() => {});
    const left = configured.value.some((c) => c.id === p.id);
    if (left) otherLayer.value = { ...otherLayer.value, [p.id]: where === "project" ? "global" : "project" };
    else { const { [p.id]: _, ...rest } = otherLayer.value; otherLayer.value = rest; }
    const { [p.id]: __, ...tested } = tests.value;
    tests.value = tested;
    formNotice.value = left
      ? `Removed “${p.label}” ${where === "project" ? "from this project" : "from every project"}, but it is also saved ${where === "project" ? "for every project" : "for this project"}. Remove it again to remove that one too.`
      : `Removed “${p.label}”.${applied ? "" : " The session drops it when the running turn ends."}`;
  } catch (e) {
    formError.value = `Could not remove the endpoint: ${readable(e)}`;
  } finally {
    removing.value = "";
  }
}
</script>

<template>
  <p v-if="unavailable" class="s-note">{{ unavailable }}</p>
  <template v-else>
    <SGroup title="Provider account" description="Connect a subscription or sign in with your provider.">
      <SRow v-if="loaded && account" label="Account" :description="[account.signed_in ? 'Signed in' : 'Not signed in', account.signed_in && testText(testProviderId(account.provider))].filter(Boolean).join(' · ')" :status="account.signed_in ? testStatus(testProviderId(account.provider)) : null">
        <select v-model="accountId" class="s-select" aria-label="Account provider" :disabled="!!busy"><option v-for="p in accounts" :key="p.provider" :value="p.provider">{{ p.name }}</option></select>
        <button v-if="account.signed_in && hasProject" class="btn btn-secondary" :disabled="!!busy || tests[testProviderId(account.provider)]?.busy" @click="testConnection(testProviderId(account.provider))">Test connection</button>
        <button class="btn btn-secondary" :disabled="!!busy" @click="start(accountId, account.signed_in)">{{ account.signed_in ? 'Sign out' : 'Sign in' }}</button>
      </SRow>
      <SRow v-else :label="loading ? 'Loading providers…' : 'Providers unavailable'" :description="loading ? undefined : 'Check the engine in Settings → Updates, then try again.'">
        <button v-if="!loading" class="btn btn-secondary" @click="load">Try again</button>
      </SRow>
      <template v-if="loaded && account?.signed_in && stored.length">
        <SRow v-for="a in stored" :key="a.key" :label="a.detail || a.key" :description="a.detail ? a.key : undefined" :status="a.active ? { text: 'Active', kind: 'ok' } : null">
          <button v-if="!a.active" class="btn btn-secondary" :disabled="!!storedBusy || !!busy" @click="changeStored(a, 'select')">{{ storedBusy === a.key ? "Switching…" : "Use this account" }}</button>
          <button class="btn btn-secondary" :disabled="!!storedBusy || !!busy" @click="changeStored(a, 'remove')">Remove</button>
        </SRow>
        <SRow label="Add another account" description="Sign in again to keep a second account for this provider and switch between them here.">
          <button class="btn btn-secondary" :disabled="!!busy" @click="start(accountId)">Sign in to another</button>
        </SRow>
      </template>
      <template v-if="storedError" #after><p role="alert" class="s-note bad">{{ storedError }}</p></template>
    </SGroup>
    <SGroup v-if="loaded" title="API key" description="Use a key instead of a subscription. Dial stores it on this computer.">
      <SRow v-if="keyProvider" label="Key provider" :description="[!keyProvider.key_saved ? 'No saved key' : canRemoveKey(keyProvider) ? 'Key saved' : 'Key saved. This engine does not support removing saved keys from Desktop.', keyProvider.key_saved && testText(testProviderId(keyProvider.provider))].filter(Boolean).join(' · ')" :status="keyProvider.key_saved ? testStatus(testProviderId(keyProvider.provider)) : null">
        <select v-model="keyId" class="s-select" aria-label="API key provider" :disabled="!!busy"><option v-for="p in keys" :key="p.provider" :value="p.provider">{{ p.name }}</option></select>
        <button class="btn btn-secondary" :disabled="!!busy" @click="start(keyId)">{{ keyProvider.key_saved ? 'Replace key' : 'Add key' }}</button>
        <button v-if="keyProvider.key_saved && hasProject" class="btn btn-secondary" :disabled="!!busy || tests[testProviderId(keyProvider.provider)]?.busy" @click="testConnection(testProviderId(keyProvider.provider))">Test connection</button>
        <button v-if="canRemoveKey(keyProvider)" class="btn btn-secondary" :disabled="!!busy" @click="removeKey()">Remove key</button>
      </SRow>
      <SRow v-else label="API-key setup unavailable" description="Update the engine in Settings → Updates to add a key here." />
    </SGroup>
    <SGroup v-if="busy" :title="busyKey?.name ?? account?.name">
      <SRow :label="prompt?.message || progress" stacked>
        <form v-if="prompt" @submit.prevent="submit">
          <input ref="input" v-model="answer" class="s-input grow" :aria-label="prompt.message" :placeholder="prompt.placeholder || undefined" :type="prompt.secret ? 'password' : 'text'" autocomplete="off" spellcheck="false" />
          <button class="btn btn-primary" :disabled="cancelling || (!answer && !prompt.allow_empty)">{{ busyKey ? 'Save key' : 'Continue' }}</button>
        </form>
        <button v-if="url" class="btn btn-secondary" @click="openExternal(url)">Open sign-in page</button>
        <button v-if="!signingOut" class="btn btn-secondary" :disabled="cancelling" @click="cancel">Cancel</button>
      </SRow>
    </SGroup>
    <p v-if="!hasProject" class="s-note">Accounts and keys are saved for every project. Open a project to test a connection or add a custom endpoint.</p>
    <SGroup v-if="hasProject" title="Custom endpoints" description="OpenAI-compatible and self-hosted servers: Ollama, LM Studio, vLLM, OpenRouter or a company proxy. New ones are saved for every project.">
      <SRow v-for="p in configured" :key="p.id" :label="p.label" :description="[p.baseUrl, endpointKey(p.id)?.key_saved && 'Key saved', testText(p.id)].filter(Boolean).join(' · ')" :hint="`providers.${p.id}${scopeOf(p.id) === 'project' ? ' · this project' : ''}`" :status="applyingNow && !tests[p.id] ? { text: 'Applying…' } : testStatus(p.id)">
        <button class="btn btn-secondary" :disabled="tests[p.id]?.busy || removing === p.id" :title="applyingNow ? 'Waits for the saved settings to apply' : undefined" @click="testConnection(p.id)">Test connection</button>
        <button v-if="endpointKey(p.id)" class="btn btn-secondary" :disabled="!!busy || removing === p.id" title="Dial stores the key on this computer; it is never written to the settings file" @click="start(`api-key:${p.id}`)">{{ endpointKey(p.id)?.key_saved ? "Replace key" : "Add key" }}</button>
        <button v-if="endpointKey(p.id)?.key_saved && endpointKey(p.id)?.can_remove" class="btn btn-secondary" :disabled="!!busy || removing === p.id" @click="removeKey(endpointKey(p.id))">Remove key</button>
        <button class="btn btn-secondary" :disabled="saving || removing === p.id || (adding && editing === p.id)" @click="openForm(p.id)">Edit</button>
        <button class="btn btn-secondary" :disabled="saving || !!removing" @click="removeEndpoint(p)">{{ removing === p.id ? "Removing…" : "Remove" }}</button>
      </SRow>
      <SRow v-if="!adding" :label="configured.length ? 'Add another endpoint' : 'No custom endpoints'" description="Point Dial at any server that speaks a supported protocol.">
        <button class="btn btn-secondary" :disabled="unavailable !== ''" @click="openForm()">Add custom endpoint</button>
      </SRow>
      <SRow v-else :label="editing ? `Edit “${configured.find((c) => c.id === editing)?.label ?? editing}”` : 'Add custom endpoint'" :description="editing ? `Saved ${scopeOf(editing) === 'project' ? 'for this project' : 'for every project'}; other keys in its table are kept.` : undefined" stacked>
        <form class="endpoint" novalidate @submit.prevent="saveEndpoint">
          <label>Id
            <input v-model="form.id" placeholder="my-proxy" autocomplete="off" spellcheck="false" class="s-input mono" :readonly="!!editing" :title="editing ? 'The id names the provider in model ids; remove and add again to change it' : undefined" :aria-invalid="tried && !!check.errors.id" />
            <span v-if="tried && check.errors.id" class="s-error">{{ check.errors.id }}</span>
          </label>
          <label>Name
            <input v-model="form.label" class="s-input" :placeholder="form.id || 'My proxy'" autocomplete="off" />
          </label>
          <label class="wide">Base URL
            <input v-model="form.baseUrl" placeholder="http://localhost:11434/v1" autocomplete="off" spellcheck="false" class="s-input mono" :aria-invalid="tried && !!check.errors.baseUrl" />
            <span v-if="tried && check.errors.baseUrl" class="s-error">{{ check.errors.baseUrl }}</span>
            <span v-for="w in check.warnings" :key="w" class="s-warn">{{ w }}</span>
          </label>
          <label>Protocol
            <select v-model="form.protocol" class="s-select"><option v-for="p in protocols" :key="p.value" :value="p.value">{{ p.label }}</option></select>
          </label>
          <label>API key variable (optional)
            <input v-model="form.apiKeyEnv" placeholder="MY_PROXY_API_KEY" autocomplete="off" spellcheck="false" class="s-input mono" :aria-invalid="tried && !!check.errors.apiKeyEnv" />
            <span v-if="tried && check.errors.apiKeyEnv" class="s-error">{{ check.errors.apiKeyEnv }}</span>
          </label>
          <label class="check wide"><input v-model="form.allowInsecure" type="checkbox" /> Allow plain HTTP to a non-local host (allow_insecure)</label>
          <p v-if="endpointKeys" class="s-note wide">Needs a key? Save the endpoint, then choose <b>Add key</b> on its row: Dial stores the key on this computer and never writes it to the settings file. The variable above is only for a key you already keep in your environment. Local servers usually need no key.</p>
          <p v-else class="s-note wide">The key itself is never stored here: set the variable in your shell (for example <code>export MY_PROXY_API_KEY=…</code>) before starting Dial, or update the engine in Settings → Updates to save a key from this page. Local servers usually need no key.</p>
          <div class="actions wide">
            <button class="btn btn-secondary" type="button" :disabled="saving" @click="adding = false; editing = null">Cancel</button>
            <button class="btn btn-primary" type="submit" :disabled="saving || (tried && !valid)">{{ saving ? "Saving…" : editing ? "Save changes" : "Save endpoint" }}</button>
          </div>
        </form>
      </SRow>
      <p v-if="formError" role="alert" class="s-note bad">{{ formError }}</p>
      <p v-if="formNotice" role="status" class="s-note">{{ formNotice }}</p>
    </SGroup>
    <p v-if="error" role="alert" class="s-note">{{ error }}</p>
    <p v-if="notice" role="status" class="s-note">{{ notice }}</p>
  </template>
</template>

<style scoped>
/* Inputs and selects use the shared .s-input / .s-select (settings.css). */
form { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; }
.grow { flex: 1; }
.endpoint { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); align-items: start; gap: 10px 12px; width: 100%; }
.endpoint label { display: flex; flex-direction: column; gap: 4px; font-size: var(--text-sm); color: var(--text-muted); }
.endpoint label.check { flex-direction: row; align-items: center; gap: 6px; }
.endpoint .wide { grid-column: 1 / -1; }
.endpoint .actions { display: flex; justify-content: flex-end; gap: 8px; }
.endpoint .s-input, .endpoint .s-select { width: 100%; max-width: 100%; }
.mono { font-family: var(--font-mono); }
.s-warn { font-size: 12px; color: var(--warning); }
.bad { color: var(--danger); }
</style>
