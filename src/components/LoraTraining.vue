<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { ask, open, save } from '@tauri-apps/plugin-dialog';
import { openTaskTerminal } from '../commandRunner';
import { engine, models } from '../bridge';
import { engineCall, readable } from '../engineTools';
import {
  EXL3_BPW_OPTIONS,
  GGUF_LORA_DTYPES,
  GGUF_QUANT_OPTIONS,
  formatLabel,
  jobTitle,
  trainingBases,
  trainingBlock,
  type TrainingStatus,
} from '../training';
import type { OpenSnapshot } from '../types';
import SPage from './settings/SPage.vue';
import SGroup from './settings/SGroup.vue';

const props = defineProps<{ snap: OpenSnapshot }>();
const emit = defineEmits<{ library: []; settings: [] }>();
// The parent keys this page by project, session and connection. Never send consent after unmount.
const root = props.snap.workspace.root;
const live = computed(() => props.snap.connection.state === 'live');
const model = ref(trainingBases.get(root) || (props.snap.model?.provider === 'mlx' ? props.snap.model.modelId : ''));
const choices = computed(() => {
  const entries = new Map(models.value.filter(m => m.provider === 'mlx' && m.local).map(m => [m.modelId, m.label]));
  if (model.value && !entries.has(model.value)) entries.set(model.value, model.value);
  return entries;
});
const label = computed(() => choices.value.get(model.value) || model.value);
const data = ref<TrainingStatus | null>(null), error = ref(''), notice = ref(''), pending = ref(false), loading = ref(false);
const savingSetting = ref(false);
const locked = computed(() => !live.value || props.snap.turnRunning || pending.value);
const blocked = computed(() => data.value ? trainingBlock(data.value) : 'Choose a training base first.');
const cfg = computed(() => data.value?.config);
const caps = computed(() => data.value?.capabilities);
const hasAdapter = computed(() => Boolean(data.value?.has_adapter || data.value?.version?.kind === 'lora'));

// Export workflow controls
const fuseDequantize = ref(false);
const ggufLoraDtype = ref<(typeof GGUF_LORA_DTYPES)[number]>('f32');
const ggufQuant = ref<(typeof GGUF_QUANT_OPTIONS)[number]>('q4_k_m');
const exl3Bpw = ref<number>(4.0);

let epoch = 0, mounted = true, timer: ReturnType<typeof setTimeout> | undefined;
async function refresh() {
  const id = ++epoch;
  clearTimeout(timer);
  if (!live.value || !model.value || props.snap.turnRunning) return;
  loading.value = true;
  try {
    const next = await engineCall<TrainingStatus>('training_status', { model: model.value }, root);
    if (id !== epoch) return;
    data.value = next;
    error.value = '';
    if (next.job?.active) timer = setTimeout(refresh, 2200);
  } catch (e) {
    if (id === epoch) { data.value = null; error.value = /unknown.method|not supported/i.test(String(e)) ? 'This engine does not expose LoRA training to Desktop. Select an updated engine in Settings → Updates.' : readable(e); }
  } finally { if (id === epoch) loading.value = false; }
}
watch([model, () => props.snap.turnRunning], () => {
  trainingBases.set(root, model.value); data.value = null; error.value = ''; notice.value = ''; loading.value = false;
  void refresh();
}, { immediate: true });
onBeforeUnmount(() => { mounted = false; ++epoch; clearTimeout(timer); });

async function browse(directory = true) {
  try {
    const path = await open({ directory, multiple: false, title: directory ? 'Choose checkpoint folder' : 'Choose GGUF weights', ...(!directory && { filters: [{ name: 'GGUF', extensions: ['gguf'] }] }) });
    if (mounted && typeof path === 'string') model.value = path;
  } catch (e) { if (mounted) error.value = readable(e); }
}

async function setTrainingSetting(key: string, value: unknown) {
  if (locked.value || savingSetting.value) return;
  savingSetting.value = true;
  error.value = '';
  try {
    await engineCall('set_setting', { key, value, scope: 'global' }, root);
    if (mounted) await refresh();
  } catch (e) {
    if (mounted) error.value = readable(e);
  } finally {
    savingSetting.value = false;
  }
}

function onNumericSetting(key: string, event: Event, isFloat = false) {
  const raw = (event.target as HTMLInputElement).value;
  const parsed = isFloat ? Number.parseFloat(raw) : Number.parseInt(raw, 10);
  if (Number.isFinite(parsed)) void setTrainingSetting(key, parsed);
}

async function prepareGguf() {
  const source = data.value?.gguf_file;
  if (!source || locked.value) return;
  pending.value = true;
  error.value = '';
  notice.value = '';
  try {
    const output = await save({ title: 'Name a new folder for the prepared checkpoint', defaultPath: (source.split('/').pop() || 'model').replace(/\.gguf$/i, '') + '-training' });
    if (!output || !mounted) return;
    const ok = await ask(`Dequantize this GGUF locally into an FP16 safetensors checkpoint for LoRA/DoRA training? This may require several times its size in RAM and disk space. No examples are imported and no training starts.\n\nFrom: ${source}\nNew folder: ${output}`, { title: 'Prepare training weights', okLabel: 'Prepare weights', cancelLabel: 'Cancel' });
    if (!ok || !mounted || !live.value || props.snap.turnRunning || data.value?.gguf_file !== source) return;
    if (props.snap.appleSilicon) {
      openTaskTerminal({ kind: 'prepareGguf', source, output }, root);
    } else {
      const res = await engineCall<{ output?: string }>('training_action', {
        action: 'prepare_gguf',
        model: model.value,
        confirm: true,
        output,
      }, root);
      if (mounted) {
        notice.value = `Prepared trainable checkpoint at ${res.output || output}. Select that folder above to start training.`;
        await refresh();
      }
    }
  } catch (e) { if (mounted) error.value = readable(e); }
  finally { pending.value = false; }
}

const actions = {
  approve: ['Approve session', 'Import eligible turns from this session for training? Saved requests may contain private code and tool data. Only approve material you have permission to use. This does not start a run.'],
  start: ['Start training', 'Queue local training and held-out evaluation? This uses compute and can admit a new version for this base. Your chat model and original weights stay unchanged.'],
  stop: ['Stop job', 'Request a safe stop and pause future learning jobs for this base? You can resume scheduling later.'],
  resume: ['Resume scheduling', 'Allow learning jobs for this base again? This does not start a training run.'],
} as const;
async function act(action: keyof typeof actions) {
  if (locked.value || !data.value) return;
  const target = model.value, session = props.snap.sessionId;
  pending.value = true; notice.value = ''; error.value = '';
  try {
    const [title, explanation] = actions[action];
    if (!await ask(`${explanation}\n\nTraining base: ${label.value}`, { title, okLabel: title, cancelLabel: 'Cancel' })) return;
    if (!mounted || props.snap.turnRunning || model.value !== target || props.snap.sessionId !== session) return;
    const reply = await engineCall<{ message?: string; approved?: number; already?: number; judged?: number; unusable?: number }>('training_action', {
      action, model: target, confirm: true, ...(action === 'approve' ? { session_id: session } : {}),
    }, root);
    if (!mounted) return;
    notice.value = action === 'approve' ? `${reply.approved} new turns approved · ${reply.already} previously approved · ${(reply.judged ?? 0) + (reply.unusable ?? 0)} skipped.${!reply.approved && !reply.already ? ' For future sessions, enable request retention in Learning settings before you chat.' : ''}` : reply.message || '';
    await refresh();
  } catch (e) { if (mounted) error.value = readable(e); }
  finally { pending.value = false; }
}

type ExportOp = 'fuse' | 'export_peft' | 'export_gguf_lora' | 'export_gguf' | 'export_exl3';
async function runExport(op: ExportOp) {
  if (locked.value || !data.value) return;
  const baseName = (label.value.split('/').pop() || 'adapter').replace(/[^a-zA-Z0-9._-]+/g, '-');
  const meta: Record<ExportOp, { title: string; defaultPath: string; detail: string; extra: Record<string, unknown> }> = {
    fuse: {
      title: 'Choose a new folder for the fused checkpoint',
      defaultPath: `${baseName}-fused${fuseDequantize.value ? '-fp16' : ''}`,
      detail: fuseDequantize.value
        ? 'Merge the active adapter into a full-precision (FP16) safetensors checkpoint?'
        : 'Merge the active adapter into a standalone MLX checkpoint keeping the base quantization?',
      extra: { dequantize: fuseDequantize.value },
    },
    export_peft: {
      title: 'Choose a new folder for the PEFT adapter',
      defaultPath: `${baseName}-peft-adapter`,
      detail: 'Export the active adapter to HuggingFace PEFT format (adapter_config.json + adapter_model.safetensors) for ExLlamaV3/TabbyAPI --lora-dir, vLLM, or HuggingFace?',
      extra: {},
    },
    export_gguf_lora: {
      title: 'Choose a new .gguf file for the GGUF LoRA adapter',
      defaultPath: `${baseName}-lora-${ggufLoraDtype.value}.gguf`,
      detail: `Export the active LoRA adapter as a standalone GGUF LoRA (${ggufLoraDtype.value}) for llama-server --lora? (DoRA adapters must use fused GGUF export.)`,
      extra: { dtype: ggufLoraDtype.value },
    },
    export_gguf: {
      title: 'Choose a new .gguf file for the fused GGUF model',
      defaultPath: `${baseName}-${ggufQuant.value}.gguf`,
      detail: `Fuse the active adapter into FP16 weights, convert to GGUF, and quantize to ${ggufQuant.value}? This requires several times the model size in temporary disk space.`,
      extra: { quant: ggufQuant.value },
    },
    export_exl3: {
      title: 'Choose a new folder for the EXL3 quantized checkpoint',
      defaultPath: `${baseName}-exl3-${exl3Bpw.value}bpw`,
      detail: `Fuse the active adapter (if present) and quantize to EXL3 at ${exl3Bpw.value} bits per weight? Requires an NVIDIA CUDA GPU and exllamav3.`,
      extra: { bpw: exl3Bpw.value },
    },
  };
  const spec = meta[op];
  pending.value = true; notice.value = ''; error.value = '';
  try {
    const output = await save({
      title: spec.title,
      defaultPath: spec.defaultPath,
      ...((op === 'export_gguf_lora' || op === 'export_gguf') ? { filters: [{ name: 'GGUF', extensions: ['gguf'] }] } : {}),
    });
    if (!output || !mounted) return;
    const confirmed = await ask(`${spec.detail}\n\nBase: ${label.value}\nOutput: ${output}`, {
      title: spec.title,
      okLabel: 'Export',
      cancelLabel: 'Cancel',
    });
    if (!confirmed || !mounted) return;
    const reply = await engineCall<{ version?: number; fused?: { output?: string }; exported?: { output?: string; format?: string } }>('training_action', {
      action: op,
      model: model.value,
      confirm: true,
      output,
      ...spec.extra,
    }, root);
    if (!mounted) return;
    const dest = reply.fused?.output || reply.exported?.output || output;
    notice.value = `Export complete (${op.replace(/_/g, ' ')}) → ${dest}`;
    await refresh();
  } catch (e) {
    if (mounted) error.value = readable(e);
  } finally {
    pending.value = false;
  }
}

const missingRuntime = computed<{ id: string; label: string } | null>(() => {
  const msg = data.value?.unavailable || '';
  if (!msg) return null;
  if (/llama\.cpp|llama-server/i.test(msg)) return { id: 'llama-cpp', label: 'llama.cpp' };
  if (/exl3|exllamav3|tabbyapi/i.test(msg) && !props.snap.appleSilicon) return { id: 'exl3', label: 'ExLlamaV3' };
  if (/mlx-serve/i.test(msg)) return { id: 'mlx-serve', label: 'MLX-Serve' };
  if (/vllm-mlx/i.test(msg)) return { id: 'vllm-mlx', label: 'vLLM-MLX' };
  if (/mistral\.rs|mistralrs/i.test(msg)) return { id: 'mistral-rs', label: 'mistral.rs' };
  if (/ik_llama|ik-llama/i.test(msg)) return { id: 'ik-llama', label: 'ik_llama.cpp' };
  if (/atomic engine/i.test(msg)) return { id: 'atomic', label: 'Atomic Engine' };
  if (/vllm\b/i.test(msg)) return { id: 'vllm', label: 'vLLM' };
  if (/sglang/i.test(msg)) return { id: 'sglang', label: 'SGLang' };
  if (/python|mlx runtime|dial setup/i.test(msg) && props.snap.appleSilicon) return { id: 'mlx', label: 'MLX runtime' };
  return null;
});

async function installMissingRuntime() {
  const target = missingRuntime.value;
  if (!target || locked.value) return;
  pending.value = true;
  error.value = '';
  notice.value = '';
  try {
    if (await engine.setupRuntime(target.id) && mounted) {
      notice.value = `${target.label} is installed and ready.`;
      await engine.refreshModels();
      await refresh();
    }
  } catch (e) {
    if (mounted) error.value = readable(e);
  } finally {
    pending.value = false;
  }
}
</script>

<template>
  <SPage title="LoRA / DoRA training" description="Teach a local model with verified examples from your work. Supports Apple Silicon MLX, GGUF (--lora &amp; fused), and CUDA PyTorch/PEFT + EXL3.">
    <div v-if="!live || snap.turnRunning" class="banner" role="status">{{ live ? 'Wait for this turn to finish to manage training.' : 'Connect to the project’s engine to manage training.' }}</div>
    <SGroup title="1 · Choose a base">
      <div class="step">
        <label for="training-base">Local checkpoint</label>
        <div class="line"><select id="training-base" v-model="model" class="s-select" :disabled="locked">
          <option value="" disabled>Select a downloaded model…</option>
          <option v-for="[id, name] in choices" :key="id" :value="id">{{ name }}</option>
        </select><button class="btn btn-secondary" :disabled="locked" @click="browse()">Browse folder…</button><button class="link" :disabled="locked" @click="browse(false)">Open GGUF file…</button></div>
        <div v-if="data" class="badges">
          <span class="badge">Format: <b>{{ formatLabel(data.format) }}</b></span>
          <span v-if="data.backend" class="badge">Backend: <b>{{ data.backend === 'mlx' ? 'MLX (Apple Silicon)' : 'PyTorch / PEFT' }}</b></span>
          <span v-if="data.version" class="badge">Version: <b>v{{ data.version.id }} · {{ data.version.kind }}</b></span>
        </div>
        <p>Your chat model stays {{ snap.model?.label || 'unchanged' }}. Choose the weights you want to adapt, not the model that wrote the examples.</p>
        <p v-if="loading && !data" role="status">Checking this checkpoint…</p>
        <div v-else-if="data?.unavailable" class="banner">
          <span>{{ data.unavailable }}</span>
          <button v-if="missingRuntime" class="btn btn-secondary" style="margin-top:8px" :disabled="locked" @click="installMissingRuntime">Download {{ missingRuntime.label }}…</button>
        </div>
        <template v-if="data?.gguf_file"><button class="btn btn-secondary prepare" :disabled="locked" @click="prepareGguf">Prepare training weights…</button><p>Dequantizes a single unsplit GGUF into a trainable FP16 safetensors folder. After preparation, select the new checkpoint folder above.</p></template>
        <button v-if="!model || data?.unavailable" class="link" @click="emit('library')">Open model library →</button>
      </div>
    </SGroup>

    <SGroup v-if="cfg" title="2 · Training hyperparameters">
      <div class="step">
        <div class="grid">
          <label class="field"><span>Adapter family</span>
            <select class="s-select" :value="cfg.fine_tune_type" :disabled="locked || savingSetting" @change="setTrainingSetting('training.fine_tune_type', ($event.target as HTMLSelectElement).value)">
              <option value="lora">LoRA (compatible with GGUF --lora &amp; EXL3)</option>
              <option value="dora">DoRA (magnitude + direction; fused export)</option>
            </select>
          </label>
          <label class="field"><span>Target modules</span>
            <select class="s-select" :value="['all-linear', 'attention', 'q-proj'].includes(cfg.target_modules) ? cfg.target_modules : 'all-linear'" :disabled="locked || savingSetting" @change="setTrainingSetting('training.target_modules', ($event.target as HTMLSelectElement).value)">
              <option value="all-linear">All linear projections (recommended)</option>
              <option value="attention">Attention projections (Q/K/V/O)</option>
              <option value="q-proj">Query projection only (legacy)</option>
            </select>
          </label>
          <label class="field"><span>Backend</span>
            <select class="s-select" :value="cfg.backend" :disabled="locked || savingSetting" @change="setTrainingSetting('training.backend', ($event.target as HTMLSelectElement).value)">
              <option value="auto">Auto (MLX on Apple Silicon, PEFT on CUDA)</option>
              <option value="mlx">MLX (Apple Silicon)</option>
              <option value="peft">PyTorch / PEFT (CUDA / EXL3)</option>
            </select>
          </label>
          <label class="field"><span>Evaluation gate</span>
            <select class="s-select" :value="cfg.gate_mode" :disabled="locked || savingSetting" @change="setTrainingSetting('improve.gate_mode', ($event.target as HTMLSelectElement).value)">
              <option value="auto">Auto (unblocked guard check)</option>
              <option value="practical">Practical (point-estimate guard)</option>
              <option value="strict">Strict (Clopper-Pearson everywhere)</option>
            </select>
          </label>
          <label class="field"><span>Rank (r)</span>
            <input class="s-input" type="number" min="1" max="128" :value="cfg.rank" :disabled="locked || savingSetting" @change="onNumericSetting('training.rank', $event)" />
          </label>
          <label class="field"><span>Iterations</span>
            <input class="s-input" type="number" min="1" max="10000" :value="cfg.iters" :disabled="locked || savingSetting" @change="onNumericSetting('training.iters', $event)" />
          </label>
          <label class="field"><span>Grad accumulation</span>
            <input class="s-input" type="number" min="1" max="64" :value="cfg.grad_accumulation_steps" :disabled="locked || savingSetting" @change="onNumericSetting('training.grad_accumulation_steps', $event)" />
          </label>
          <label class="field"><span>Learning rate</span>
            <input class="s-input" type="number" step="0.00001" min="0.000001" max="0.01" :value="cfg.learning_rate" :disabled="locked || savingSetting" @change="onNumericSetting('training.learning_rate', $event, true)" />
          </label>
          <label class="field"><span>LR schedule</span>
            <select class="s-select" :value="cfg.lr_schedule" :disabled="locked || savingSetting" @change="setTrainingSetting('training.lr_schedule', ($event.target as HTMLSelectElement).value)">
              <option value="cosine">Cosine with linear warmup</option>
              <option value="constant">Constant</option>
            </select>
          </label>
          <label class="field"><span>Optimizer</span>
            <select class="s-select" :value="cfg.optimizer" :disabled="locked || savingSetting" @change="setTrainingSetting('training.optimizer', ($event.target as HTMLSelectElement).value)">
              <option value="adamw">AdamW (decoupled weight decay)</option>
              <option value="adam">Adam</option>
            </select>
          </label>
        </div>
        <div class="line">
          <label class="toggle"><input type="checkbox" :checked="cfg.grad_checkpoint" :disabled="locked || savingSetting" @change="setTrainingSetting('training.grad_checkpoint', ($event.target as HTMLInputElement).checked)" /> Gradient checkpointing (lower memory)</label>
          <button class="link" @click="emit('settings')">All Learning settings →</button>
        </div>
      </div>
    </SGroup>

    <SGroup :title="cfg ? '3 · Prepare examples' : '2 · Prepare examples'">
      <div class="step">
        <div v-if="data && !data.unavailable" class="facts"><span><b>{{ data.approved_snapshots }}</b> approved session turns</span><span><b>{{ data.verified_turns }}</b> verified local turns</span></div>
        <p v-if="data">Dial needs {{ data.minimum }} curated training examples and at least {{ data.validation_minimum }} separate validation example{{ data.validation_minimum === 1 ? '' : 's' }}. One session stays on one side of the split.</p>
        <p v-else>Approve useful sessions, including work done with a hosted, GGUF, or EXL3 model. Use more than one session so Dial can keep validation examples separate.</p>
        <div class="line"><button class="btn btn-secondary" :disabled="locked || !data || !!data.unavailable || !!data.job?.active || !snap.sessionId" @click="act('approve')">Approve this session…</button><button class="link" @click="emit('settings')">Learning settings</button></div>
      </div>
    </SGroup>

    <SGroup :title="cfg ? '4 · Train and evaluate' : '3 · Train and evaluate'">
      <div class="step">
        <template v-if="data?.job?.scheduler || data?.job?.active"><strong role="status">{{ jobTitle(data.job!) }}</strong>
          <p v-if="data.job?.scheduler?.error" class="problem">{{ data.job.scheduler.error }}</p>
          <div v-if="data.job?.active && data.job.progress?.training?.total" class="line">
            <progress aria-label="Training iterations" :value="data.job.progress.training.iteration" :max="data.job.progress.training.total" />
            <span>{{ data.job.progress.training.iteration }} / {{ data.job.progress.training.total }}</span>
            <span v-if="data.job.progress.training.train_loss != null" class="metric">loss {{ data.job.progress.training.train_loss.toFixed(4) }}</span>
            <span v-if="data.job.progress.training.learning_rate != null" class="metric">lr {{ data.job.progress.training.learning_rate.toExponential(1) }}</span>
            <span v-if="data.job.progress.training.peak_memory_gb != null" class="metric">{{ data.job.progress.training.peak_memory_gb.toFixed(1) }} GB</span>
          </div>
        </template>
        <p v-if="data?.version">Stored version: <b>v{{ data.version.id }}</b> · {{ data.version.kind }}. {{ hasAdapter ? 'This is the admitted adapter for this base.' : 'A completed run is not automatically an admitted adapter.' }}</p>
        <p v-else>Training runs in the background with pre-screening and adapter soup. Only a candidate that passes held-out checks can become the active version.</p>
        <div class="line"><button v-if="data?.job?.active" class="btn btn-secondary" :disabled="locked || data.job.stop_requested" @click="act('stop')">Stop job…</button>
          <button v-else-if="data?.paused" class="btn btn-secondary" :disabled="locked" @click="act('resume')">Resume scheduling…</button>
          <button v-else class="btn btn-primary" :disabled="locked || !!blocked" @click="act('start')">Start training…</button>
          <button v-if="model" class="link" :disabled="locked || loading" @click="refresh">Refresh status</button></div>
        <p v-if="data && blocked && !data.unavailable && !data.job?.active">{{ blocked }}</p>
      </div>
    </SGroup>

    <SGroup v-if="data && (hasAdapter || caps?.exl3_export)" :title="cfg ? '5 · Export &amp; deploy' : '4 · Export &amp; deploy'">
      <div class="step">
        <p v-if="!hasAdapter">Train and admit an adapter first to export LoRA/DoRA weights, or quantize the base checkpoint directly to EXL3 on a CUDA host.</p>
        <div class="export-row">
          <div>
            <strong>Standalone MLX / Safetensors (Fuse)</strong>
            <p>Merge the active adapter directly into base weights for Dial, LM Studio, or mlx_lm.</p>
          </div>
          <div class="line">
            <label class="toggle"><input v-model="fuseDequantize" type="checkbox" :disabled="locked || !hasAdapter" /> Dequantize to FP16</label>
            <button class="btn btn-secondary" :disabled="locked || !hasAdapter || !!data.job?.active" @click="runExport('fuse')">Fuse checkpoint…</button>
          </div>
        </div>
        <div class="export-row">
          <div>
            <strong>PEFT Adapter (ExLlamaV3 / TabbyAPI / vLLM)</strong>
            <p>Standard HuggingFace PEFT directory (<code>adapter_config.json</code> + <code>adapter_model.safetensors</code>) for EXL3 <code>--lora-dir</code> or vLLM.</p>
          </div>
          <div class="line">
            <button class="btn btn-secondary" :disabled="locked || !hasAdapter || !!data.job?.active" @click="runExport('export_peft')">Export PEFT…</button>
          </div>
        </div>
        <div class="export-row">
          <div>
            <strong>GGUF LoRA Adapter (<code>llama-server --lora</code>)</strong>
            <p>Small standalone <code>.gguf</code> LoRA file attached at runtime to any matching GGUF quant. (LoRA only; DoRA uses fused GGUF below.)</p>
          </div>
          <div class="line">
            <select v-model="ggufLoraDtype" class="s-select compact" :disabled="locked || !hasAdapter">
              <option v-for="dt in GGUF_LORA_DTYPES" :key="dt" :value="dt">{{ dt.toUpperCase() }}</option>
            </select>
            <button class="btn btn-secondary" :disabled="locked || !hasAdapter || !!data.job?.active || cfg?.fine_tune_type === 'dora'" @click="runExport('export_gguf_lora')">Export GGUF LoRA…</button>
          </div>
        </div>
        <div class="export-row">
          <div>
            <strong>Fused GGUF Model (Standalone)</strong>
            <p>Fuse to FP16, convert via <code>convert_hf_to_gguf.py</code>, and quantize via <code>llama-quantize</code>.
              <span v-if="caps && !caps.gguf_fused_export"> (Requires <code>convert_hf_to_gguf.py</code> via <code>DIAL_CONVERT_HF_TO_GGUF</code> or llama.cpp install.)</span>
            </p>
          </div>
          <div class="line">
            <select v-model="ggufQuant" class="s-select compact" :disabled="locked || !hasAdapter">
              <option v-for="q in GGUF_QUANT_OPTIONS" :key="q" :value="q">{{ q.toUpperCase() }}</option>
            </select>
            <button class="btn btn-secondary" :disabled="locked || !hasAdapter || !!data.job?.active || (caps && !caps.gguf_fused_export)" @click="runExport('export_gguf')">Export GGUF…</button>
          </div>
        </div>
        <div class="export-row">
          <div>
            <strong>EXL3 Quantized Checkpoint (NVIDIA CUDA)</strong>
            <p>Fuse and quantize with <code>exllamav3</code> for ExLlamaV3/TabbyAPI serving.
              <span v-if="caps && !caps.exl3_export"> Unavailable on macOS (requires NVIDIA CUDA on Linux/Windows).</span>
            </p>
          </div>
          <div class="line">
            <select v-model.number="exl3Bpw" class="s-select compact" :disabled="locked || (caps && !caps.exl3_export)">
              <option v-for="b in EXL3_BPW_OPTIONS" :key="b" :value="b">{{ b.toFixed(1) }} bpw</option>
            </select>
            <button class="btn btn-secondary" :disabled="locked || !!data.job?.active || (caps && !caps.exl3_export)" @click="runExport('export_exl3')">Export EXL3…</button>
          </div>
        </div>
      </div>
    </SGroup>

    <p v-if="error" role="alert" class="banner problem">{{ error }}</p>
    <p v-if="notice" role="status" class="banner">{{ notice }}</p>
    <details class="formats">
      <summary>Format guide: MLX, GGUF, and EXL3</summary>
      <p><b>MLX (Apple Silicon):</b> Trains LoRA/DoRA directly on MLX checkpoints and serves adapters natively. Use Fuse to build a standalone checkpoint.</p>
      <p><b>GGUF (llama.cpp / Atomic Engine):</b> Prepare an FP16 checkpoint with <i>Prepare training weights</i> (or train on the matching MLX/HF base), then export either a tiny <b>GGUF LoRA</b> for <code>llama-server --lora</code> or a <b>Fused GGUF</b> quantized with <code>llama-quantize</code>.</p>
      <p><b>EXL3 (NVIDIA CUDA):</b> Train on an FP16/BF16 HuggingFace safetensors base via PyTorch/PEFT (or on MLX), export a <b>PEFT adapter</b> to attach via ExLlamaV3/TabbyAPI <code>--lora-dir</code>, or run <b>Export EXL3</b> on a CUDA host to produce a standalone EXL3 checkpoint.</p>
    </details>
  </SPage>
</template>

<style scoped>
.step { padding: 15px 17px; display: grid; gap: 10px; min-width: 0; }
.step label, .step strong { font-size: var(--text-ui); font-weight: 600; color: var(--text); }
.step p, .formats p, .banner { margin: 0; color: var(--text-muted); font-size: var(--text-sm); line-height: 1.55; overflow-wrap: anywhere; }
.step .line, .facts, .badges { display: flex; align-items: center; flex-wrap: wrap; gap: 8px 12px; }
.step select { flex: 1 1 180px; max-width: 100%; }
.step select.compact { flex: 0 0 auto; width: auto; min-width: 96px; }
.badges { gap: 6px 8px; }
.badge { font-size: var(--text-xs, 12px); color: var(--text-muted); background: var(--hover); padding: 3px 8px; border-radius: var(--radius-sm); }
.badge b { color: var(--text); font-weight: 600; }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(210px, 1fr)); gap: 10px 14px; }
.field { display: grid; gap: 4px; }
.field > span { font-size: var(--text-xs, 12px); font-weight: 500; color: var(--text-muted); }
.s-input { width: 100%; box-sizing: border-box; padding: 6px 8px; border-radius: var(--radius-sm); border: 1px solid var(--border); background: var(--surface, transparent); color: var(--text); font-size: var(--text-sm); }
.toggle { display: inline-flex; align-items: center; gap: 6px; font-weight: 500; cursor: pointer; font-size: var(--text-sm); color: var(--text); }
.metric { font-size: var(--text-xs, 12px); color: var(--text-muted); font-variant-numeric: tabular-nums; }
.export-row { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: center; gap: 10px 16px; padding-top: 10px; border-top: 1px solid var(--border); }
.export-row:first-of-type { padding-top: 0; border-top: 0; }
.export-row > div:first-child { flex: 1 1 260px; min-width: 0; display: grid; gap: 2px; }
.facts { font-size: var(--text-sm); color: var(--text-muted); }
.prepare { justify-self: start; }
.facts b { font-variant-numeric: tabular-nums; color: var(--text); }
.link, summary { width: fit-content; color: var(--text-muted); cursor: pointer; font-size: var(--text-sm); }
.link { text-decoration: underline; text-underline-offset: 3px; }
.link:hover, summary:hover { color: var(--text); }
.banner { background: var(--hover); padding: 10px 13px; border-radius: var(--radius-sm); }
.s-group + .banner, .formats { margin-top: 16px; }
.problem { color: var(--danger, var(--text)); white-space: pre-wrap; user-select: text; }
.formats p { margin-top: 8px; }
progress { accent-color: var(--accent); }
</style>
