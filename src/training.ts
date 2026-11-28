export type LocalFormatKind = 'mlx' | 'gguf' | 'exl3' | 'splash';

export interface TrainingConfigSummary {
  backend: 'auto' | 'mlx' | 'peft';
  fine_tune_type: 'lora' | 'dora';
  target_modules: string;
  iters: number;
  batch_size: number;
  grad_accumulation_steps: number;
  num_layers: number;
  rank: number;
  scale: number;
  dropout: number;
  learning_rate: number;
  lr_schedule: 'cosine' | 'constant';
  warmup_ratio: number;
  optimizer: 'adamw' | 'adam';
  weight_decay: number;
  grad_checkpoint: boolean;
  checkpoint_every: number;
  max_seq_length: number;
  max_train_examples: number;
  max_replay_examples: number;
  max_valid_examples: number;
  gate_mode: 'auto' | 'practical' | 'strict';
}

export interface TrainingCapabilities {
  peft_export: boolean;
  gguf_lora_export: boolean;
  gguf_fused_export: boolean;
  llama_quantize: boolean;
  exl3_server: boolean;
  exl3_export: boolean;
}

export interface TrainingProgress {
  stage?: string;
  phase?: string;
  iteration: number;
  total: number;
  train_loss?: number | null;
  val_loss?: number | null;
  learning_rate?: number | null;
  peak_memory_gb?: number | null;
}

export interface TrainingStatus {
  format: LocalFormatKind | null;
  gguf_file?: string | null;
  unavailable: string | null;
  enabled: boolean;
  backend?: 'mlx' | 'peft';
  compute_paused: boolean;
  paused: boolean;
  minimum: number;
  validation_minimum: number;
  verified_turns: number;
  approved_snapshots: number;
  version: { id: number; kind: string } | null;
  has_adapter?: boolean;
  adapter_path?: string | null;
  config?: TrainingConfigSummary;
  capabilities?: TrainingCapabilities;
  job: {
    active: boolean;
    stop_requested: boolean;
    scheduler: { state: string; error?: string; admitted?: boolean } | null;
    progress: { stage?: string; training?: TrainingProgress } | null;
  } | null;
}

export const GGUF_QUANT_OPTIONS = ['f16', 'bf16', 'q8_0', 'q6_k', 'q5_k_m', 'q4_k_m', 'iq4_xs'] as const;
export const GGUF_LORA_DTYPES = ['f32', 'f16', 'bf16'] as const;
export const EXL3_BPW_OPTIONS = [2.5, 3.0, 3.5, 4.0, 4.5, 5.0, 6.0] as const;

export function formatLabel(format: LocalFormatKind | null | undefined): string {
  switch (format) {
    case 'mlx': return 'MLX / Safetensors';
    case 'gguf': return 'GGUF';
    case 'exl3': return 'EXL3';
    case 'splash': return 'Splash';
    default: return 'Unknown';
  }
}

export function trainingBlock(s: TrainingStatus): string {
  if (s.unavailable) return s.unavailable;
  if (s.job?.active) return 'A learning job is already active for this base.';
  if (!s.enabled) return 'Turn on improvement and training in Learning settings first.';
  if (s.paused) return 'Scheduling is paused for this base.';
  if (s.compute_paused) return 'Background compute is paused. Resume it in Dial before starting a run.';
  return '';
}

export function jobTitle(job: NonNullable<TrainingStatus['job']>): string {
  if (job.active) return job.stop_requested ? 'Stopping at a safe point' : job.progress?.stage || (job.scheduler?.state === 'queued' ? 'Queued for compute' : 'Learning job active');
  switch (job.scheduler?.state) {
    case 'running': case 'queued': return 'Worker no longer active';
    case 'idle': return job.scheduler.admitted === true ? 'Evaluation passed' : job.scheduler.admitted === false ? 'Not admitted · version unchanged' : 'Job finished';
    case 'failed': return 'Job failed';
    case 'stopped': return 'Stopped';
    case 'disabled': case 'paused': return 'Paused';
    default: return 'No run yet';
  }
}

// Preserve the selection across Settings navigation, but never share it between projects.
export const trainingBases = new Map<string, string>();
