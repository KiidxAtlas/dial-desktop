<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref, watch } from 'vue';
import catalog from '../modelLibrary.json';
import { downloadProgress, formatModelBytes as size } from '../modelDownloadProgress';
import { engine, modelDownloads, openExternal, showToast, snapshot, type CachedModel, type CachedModelFile, type DownloadStatus, type HubFile, type HubModel } from '../bridge';
const props = defineProps<{ preview: boolean }>();
const query = ref(''), format = ref('All'), error = ref(''), pending = ref(false);
const status = ref<DownloadStatus | null>(null);
const pollError = ref('');
const progress = computed(() => pollError.value ? null : downloadProgress(status.value));
const failedStart = ref<{ repo: string; bytes: number; file?: string } | null>(null);
const retry = computed(() => failedStart.value ?? (status.value?.retryable ? { repo: status.value.repo, bytes: status.value.expectedBytes, file: status.value.file ?? undefined } : null));
const settingUp = ref(false), setupMessage = ref('');
async function setupDownloads() {
  if (props.preview || settingUp.value) return;
  settingUp.value = true; setupMessage.value = '';
  try {
    if (await modelDownloads.setup() && mounted) {
      setupMessage.value = 'Model downloads are ready. Choose a model to download.';
      error.value = ''; await searchHub();
    }
  } catch (e) { if (mounted) setupMessage.value = String(e); }
  finally { settingUp.value = false; }
}
const cached = ref<CachedModel[] | null>(null);
const matches = (text: string) => text.toLowerCase().includes(query.value.trim().toLowerCase());
const results = computed(() => catalog.filter(m =>
  (snapshot.value?.appleSilicon || !(m.requiresAppleSilicon || m.format === 'MLX')) &&
  (format.value === 'All' || format.value === m.format) && matches(m.repo + ' ' + m.description + ' ' + m.name)));
const downloaded = computed(() => (cached.value ?? []).filter(m => matches(m.repo) || m.files.some(f => matches(f.path))));
const isCached = (repo: string) => !!cached.value?.some(m => m.repo === repo);
const suggestedCached = (m: typeof catalog[number]) => m.file ? [m.file, ...(m.companions ?? [])].every(file => cached.value?.some(r => r.repo === m.repo && r.files.some(f => f.path === file && f.revision === m.revision))) : isCached(m.repo);
async function loadCached() {
  if (props.preview) return;
  try { const list = await modelDownloads.cached(); if (mounted) cached.value = list; } catch (e) { if (mounted) error.value = String(e); }
}
const elapsed = (s: number) => s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
const count = (n: number) => n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${Math.round(n / 1e3)}k` : String(n);

// Hugging Face search: the most downloaded MLX or GGUF models matching the words typed.
const hubQuery = ref(''), hubFormat = ref<'MLX' | 'GGUF'>(snapshot.value?.appleSilicon ? 'MLX' : 'GGUF');
const hub = ref<HubModel[] | null>(null), hubBusy = ref(false), hubError = ref('');
const opened = ref<{ repo: string; files: HubFile[] | null; error: string } | null>(null);
let hubVersion = 0;
async function searchHub() {
  if (props.preview) return;
  const id = ++hubVersion;
  hubBusy.value = true; hubError.value = ''; opened.value = null;
  try { const found = await modelDownloads.search(hubQuery.value, hubFormat.value); if (mounted && id === hubVersion) hub.value = found; }
  catch (e) { if (mounted && id === hubVersion) { hubError.value = String(e); hub.value = null; } }
  finally { if (id === hubVersion) hubBusy.value = false; }
}
async function openRepo(repo: string) {
  if (opened.value?.repo === repo) { opened.value = null; return; }
  opened.value = { repo, files: null, error: '' };
  try { const files = await modelDownloads.files(repo); if (opened.value?.repo === repo) opened.value = { repo, files, error: '' }; }
  catch (e) { if (opened.value?.repo === repo) opened.value = { repo, files: null, error: String(e) }; }
}
/** GGUF: each weight file is its own download. MLX: the whole repo is one. */
const ggufFiles = computed(() => (opened.value?.files ?? []).filter(f => f.path.toLowerCase().endsWith('.gguf')).sort((a, b) => a.size - b.size));
const repoBytes = computed(() => {
  const files = opened.value?.files;
  // The helper uses zero when size metadata is missing; a partial sum isn't the selection size.
  return files?.every(f => f.size > 0) ? files.reduce((sum, f) => sum + f.size, 0) : 0;
});
async function downloadHub(repo: string, bytes: number, file?: string) {
  if (pending.value || settingUp.value) return;
  pending.value = true; error.value = ''; pollError.value = ''; ++version; clearTimeout(timer);
  try {
    const next = await modelDownloads.start(repo, bytes ? size(bytes) : 'unknown size', bytes, file);
    if (next) { failedStart.value = null; status.value = next; if (mounted && hubError.value) void searchHub(); }
  }
  catch (e) { failedStart.value = { repo, bytes, file }; error.value = String(e); }
  finally { pending.value = false; if (mounted) void refresh(); }
}
watch(hubFormat, () => void searchHub());
// Typing narrows the list as you go; an empty box shows the most downloaded models.
let typing: ReturnType<typeof setTimeout> | undefined;
watch(hubQuery, () => { clearTimeout(typing); typing = setTimeout(() => void searchHub(), 400); });
const removing = ref('');
async function removeCached(m: CachedModel, file?: CachedModelFile) {
  if (removing.value) return;
  removing.value = m.repo;
  try {
    const removed = file ? await modelDownloads.removeFile(m.repo, file) : (await modelDownloads.remove(m.repo, size(m.bytes))) !== null;
    if (removed) { showToast(`Removed ${file ? file.path : m.repo}`); await loadCached(); if (snapshot.value?.connection.state === 'live') void engine.refreshModels(); }
  } catch (e) { error.value = String(e); }
  finally { removing.value = ''; }
}
const cachedTotal = computed(() => (cached.value ?? []).reduce((sum, m) => sum + m.bytes, 0));
let timer: ReturnType<typeof setTimeout> | undefined;
let mounted = true, version = 0;
async function refresh() {
  if (props.preview || pending.value) return;
  const id = ++version;
  try {
    const next = await modelDownloads.status();
    if (mounted && id === version) { status.value = next; pollError.value = ''; }
  } catch (e) { if (mounted && id === version) pollError.value = String(e); }
  if (mounted && id === version && (status.value?.running || pollError.value)) timer = setTimeout(refresh, 1000);
}
async function act(model?: typeof catalog[number]) {
  if (model) return downloadHub(model.repo, model.bytes);
  if (pending.value) return;
  pending.value = true; error.value = ''; ++version; clearTimeout(timer);
  try {
    status.value = await modelDownloads.cancel();
  } catch(e) { error.value = String(e); }
  finally { pending.value = false; if (mounted) void refresh(); }
}
watch(() => status.value?.running, (now, was) => {
  if (!was || now) return;
  void loadCached();
  if (snapshot.value?.connection.state === 'live') void engine.refreshModels();
});
onMounted(() => { void refresh(); void loadCached(); void searchHub(); });
onBeforeUnmount(() => { mounted = false; clearTimeout(timer); clearTimeout(typing); });
</script>
<template>
  <section class="library">
    <h2>Model library</h2><p class="note">Download a local model for offline use. Choose MLX for Apple silicon, or GGUF for a compatible local runner. Downloading doesn’t change the model in your session.</p>
    <div class="filters"><input aria-label="Filter downloaded and suggested models" placeholder="Filter downloaded and suggested models…" v-model="query" /><select aria-label="Suggested model format" v-model="format"><option>All</option><option v-if="snapshot?.appleSilicon">MLX</option><option>GGUF</option></select></div>
    <p v-if="preview">Preview only. Open Desktop normally to download models.</p>
    <p v-if="pending && !status?.running" role="status">Preparing your download. If this is your first download, helper setup can take a few minutes.</p>
    <aside v-if="status?.message && (!pending || status.running)"><strong>{{ status.repo }}</strong><span v-if="status.file"> / {{ status.file }}</span><p role="status">{{ status.message }}</p>
      <template v-if="status.running">
        <template v-if="progress">
          <progress :aria-label="progress.label" :aria-valuetext="progress.detail" :max="progress.max" :value="progress.value"></progress>
          <p class="progress-detail">{{ progress.detail }}</p>
          <p>{{ elapsed(status.elapsedSecs) }} elapsed<span v-if="status.expectedBytes && status.totalBytes == null"> · Selection about {{ size(status.expectedBytes) }}</span></p>
        </template>
        <button class="btn btn-secondary" :disabled="pending" @click="act()">Cancel download</button>
      </template>
    </aside>
    <aside v-if="pollError" role="alert">Download status unavailable. Retrying… {{ pollError }}</aside>
    <aside v-if="error" role="alert">{{ error }}</aside>
    <button v-if="retry && !status?.running" class="btn btn-secondary" :disabled="pending || settingUp" @click="downloadHub(retry.repo, retry.bytes, retry.file)">Retry download · {{ retry.file || retry.repo }}</button>
    <template v-if="cached !== null">
      <h3>Downloaded <span class="head-tools"><span v-if="cached.length">{{ cached.length }} · {{ size(cachedTotal) }}</span><button class="guide reload" title="Check the Hugging Face cache again" @click="loadCached">Reload</button></span></h3>
      <p v-if="!cached.length">Nothing in your Hugging Face cache yet. Choose a model below to download it.</p>
      <p v-else-if="!downloaded.length">No downloaded model matches.</p>
      <article v-for="m in downloaded" :key="'cached-' + m.repo" class="hub-row">
        <div class="hub-head"><div><strong>{{ m.repo }}</strong><p>{{ size(m.bytes) }} cached</p></div>
          <div class="row-actions">
            <button class="guide" @click="openExternal(`https://huggingface.co/${m.repo}`)">Model card ↗</button>
            <button class="btn btn-secondary danger" :disabled="!!removing || (status?.running && status.repo === m.repo)" @click="removeCached(m)">Delete entire repo</button>
          </div>
        </div>
        <details v-if="m.files.length"><summary>{{ m.files.length }} cached GGUF {{ m.files.length === 1 ? 'file' : 'files' }}</summary>
          <div v-for="f in m.files" :key="f.revision + '/' + f.path" class="file">
            <div>{{ f.path }} · {{ size(f.bytes) }}<p :title="f.revision">Revision {{ f.revision.slice(0, 12) }}</p></div>
            <button class="btn btn-secondary danger" :aria-label="`Remove ${f.path} at ${f.revision}`" :disabled="!!removing || (status?.running && status.repo === m.repo)" @click="removeCached(m, f)">Remove file</button>
          </div>
        </details>
      </article>
    </template>
    <h3>Browse Hugging Face</h3>
    <div v-if="!preview && (hubError || error || settingUp || setupMessage)"><button class="btn btn-secondary" :disabled="settingUp || pending || status?.running" @click="setupDownloads">{{ settingUp ? 'Setting up model downloads…' : 'Set up model downloads' }}</button>
      <p class="note" role="status">{{ setupMessage || (settingUp ? 'This can take a few minutes.' : 'Use this if the Hugging Face helper is missing or incompatible. No terminal or administrator access needed.') }}</p>
    </div>
    <form class="filters" @submit.prevent="searchHub">
      <input aria-label="Search Hugging Face models" placeholder="Narrow by name, e.g. qwen coder, llama, gemma…" v-model="hubQuery" :disabled="preview" />
      <div class="segmented" role="radiogroup" aria-label="Model format">
        <button v-for="f in (['MLX', 'GGUF'] as const)" :key="f" type="button" role="radio" :aria-checked="hubFormat === f" :class="{ on: hubFormat === f }" :disabled="preview" @click="hubFormat = f">{{ f }}</button>
      </div>
    </form>
    <p v-if="!hubQuery.trim() && hub?.length" class="note">Most downloaded {{ hubFormat }} models{{ hubFormat === 'MLX' ? ' (Apple silicon)' : '' }}.</p>
    <p v-if="hubBusy && !hub" class="note">Loading models…</p>
    <aside v-if="hubError && hubError !== error" role="alert">{{ hubError }}</aside>
    <p v-if="hub && !hub.length">No {{ hubFormat }} models match “{{ hubQuery }}”.</p>
    <div v-if="hub?.length" class="hub" :class="{ stale: hubBusy }">
      <article v-for="m in hub" :key="m.id" class="hub-row">
        <div class="hub-head">
          <div><strong>{{ m.id }}</strong><p>{{ count(m.downloads) }} downloads · {{ count(m.likes) }} likes<template v-if="m.pipeline_tag"> · {{ m.pipeline_tag }}</template><template v-if="isCached(m.id)"> · in your cache</template></p></div>
          <button class="btn btn-secondary" :aria-expanded="opened?.repo === m.id" @click="openRepo(m.id)">{{ opened?.repo === m.id ? 'Hide' : 'Files & size' }}</button>
        </div>
        <div v-if="opened?.repo === m.id" class="files">
          <p v-if="opened.error" role="alert">{{ opened.error }}</p>
          <p v-else-if="!opened.files">Loading files…</p>
          <template v-else-if="hubFormat === 'GGUF'">
            <p v-if="!ggufFiles.length">No .gguf files in this repo.</p>
            <div v-for="f in ggufFiles" :key="f.path" class="file">
              <span>{{ f.path }} · {{ f.size ? size(f.size) : 'Size unknown' }}</span>
              <span v-if="f.split" class="note">Split model · not supported</span>
              <button v-else class="btn btn-secondary" :disabled="pending || status?.running" @click="downloadHub(m.id, f.size, f.path)">{{ cached?.some(r => r.repo === m.id && r.files.some(c => c.path === f.path)) ? 'Update' : 'Download' }}</button>
            </div>
          </template>
          <div v-else class="file">
            <span>Whole model · {{ opened.files.length }} files · {{ repoBytes ? size(repoBytes) : 'Size unknown' }}</span>
            <button class="btn btn-secondary" :disabled="pending || status?.running" @click="downloadHub(m.id, repoBytes)">{{ status?.running && status.repo === m.id ? 'Downloading…' : isCached(m.id) ? 'Update' : 'Download' }}</button>
          </div>
          <button class="guide" @click="openExternal(`https://huggingface.co/${m.id}`)">Model card ↗</button>
        </div>
      </article>
    </div>
    <h3>Suggested</h3>
    <article v-for="m in results" :key="m.repo">
      <div><strong>{{ m.name }}</strong><p>{{ m.description }} · {{ m.format }} · {{ size(m.bytes) }}</p><small>{{ m.repo }}</small></div>
      <button class="btn btn-secondary" :aria-label="`Download ${m.name} (${m.format})`" :disabled="preview || pending || status?.running" @click="act(m)">{{ status?.running && status.repo === m.repo ? 'Downloading…' : suggestedCached(m) ? 'Update' : 'Download' }}</button>
    </article>
    <p v-if="!results.length">No matches. Try another name or format.</p>
    <p>Downloads use your Hugging Face cache.</p>
  </section>
</template>
<style scoped>
.library {max-width:760px;} p,small {color:var(--text-muted);line-height:1.5;margin:4px 0;} .filters,article {display:flex;align-items:center;gap:12px;} .filters {margin:16px 0;} input,select {background:var(--surface);border:1px solid var(--border-strong);border-radius:var(--radius-sm);padding:7px 10px;color:var(--text);} input {min-width:0;flex:1;} article {padding:16px 0;border-bottom:1px solid var(--border);justify-content:space-between;} article div, aside {overflow-wrap:anywhere;min-width:0;} article button {flex:none;} aside {padding:10px;background:var(--raised);border-radius:var(--radius-sm);} .guide {text-decoration:underline;}
.hub-row {flex-direction:column;align-items:stretch;} .hub-head,.file {display:flex;align-items:center;justify-content:space-between;gap:12px;} .files {display:flex;flex-direction:column;gap:6px;padding:4px 0 0 12px;border-left:2px solid var(--border);} .file span {color:var(--text-muted);overflow-wrap:anywhere;min-width:0;} .file button {flex:none;} .files .guide {align-self:flex-start;color:var(--text-muted);}
.library h2 {margin:0;font-family:ui-serif, Georgia, "Times New Roman", serif;font-size:24px;font-weight:400;} .library .note {margin:10px 0 4px;font-size:var(--text-sm);color:var(--text-faint);}
.library h3 {display:flex;align-items:baseline;justify-content:space-between;gap:12px;margin:28px 0 4px;padding-bottom:8px;border-bottom:1px solid var(--border-strong);font-size:var(--text-xs);font-weight:600;letter-spacing:0.04em;text-transform:uppercase;color:var(--text-faint);} .library h3 .reload {font-size:var(--text-xs);font-weight:400;letter-spacing:0;text-transform:none;color:var(--text-muted);}
.head-tools {display:inline-flex;gap:12px;align-items:baseline;font-weight:400;letter-spacing:0;text-transform:none;}
.row-actions {display:flex;align-items:center;gap:12px;flex:none;} .row-actions .guide {color:var(--text-muted);font-size:var(--text-sm);}
.danger:hover:not(:disabled) {color:var(--danger);border-color:var(--danger);}
.library summary {cursor:pointer;color:var(--text-muted);} details .file {margin-top:8px;} progress {width:100%;accent-color:var(--accent);}
.progress-detail {color:var(--text);font-variant-numeric:tabular-nums;}
.segmented {display:inline-flex;flex:none;border:1px solid var(--border-strong);border-radius:var(--radius-sm);overflow:hidden;} .segmented button {padding:6px 12px;font-size:var(--text-sm);color:var(--text-muted);} .segmented button.on {background:var(--active);color:var(--text);}
.hub.stale {opacity:0.6;transition:opacity 120ms;}
</style>
