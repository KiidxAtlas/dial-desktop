import type { ModelProgress, Snapshot, ToolEntry } from './types';

type Turn = Pick<Snapshot, 'turnRunning' | 'approvals' | 'question' | 'entries' | 'modelProgress' | 'stats'>;
/** `waiting`: the turn is paused on the user, not working (so Esc-to-stop is not the next step).
 * `tool`: the newest call still running, so the view can name its phase ("Reading", "Editing").
 * `fraction`: determinate 0..1 prefill ratio when processed and prompt token counts are both known.
 * `meta`: compact speed / cache / ETA telemetry when reported by the backend.
 * `thinking`: true while the model is actively streaming reasoning before answer text.
 * `cacheState`: qualitative KV cache reuse band when prompt size is large enough to matter. */
export type CacheState = 'high' | 'mid' | 'low';
interface Progress { label: string; detail?: string; meta?: string; fraction?: number; waiting?: boolean; thinking?: boolean; cacheState?: CacheState; tool?: ToolEntry }

const CACHE_BADGE_MIN_PROMPT_TOKENS = 2048;

function formatTps(tps: number | null | undefined): string {
  if (tps == null || !Number.isFinite(tps) || tps <= 0) return '';
  return `${tps >= 10 ? Math.round(tps) : tps.toFixed(1)} tok/s`;
}

export function cacheHitState(cached: number | null | undefined, prompt: number | null | undefined): CacheState | undefined {
  if (cached == null || prompt == null || cached < 0 || prompt < CACHE_BADGE_MIN_PROMPT_TOKENS) return undefined;
  const pct = Math.min(100, Math.round((cached / prompt) * 100));
  if (pct >= 70) return 'high';
  if (pct >= 30) return 'mid';
  return 'low';
}

function formatCache(cached: number | null | undefined, prompt: number | null | undefined): string {
  if (cached == null || prompt == null || cached < 0 || prompt <= 0) return '';
  const pct = Math.min(100, Math.round((cached / prompt) * 100));
  if (pct > 0) return `${pct}% cached`;
  return prompt >= CACHE_BADGE_MIN_PROMPT_TOKENS ? '0% cached' : '';
}
function formatEta(etaSecs: number | null | undefined): string {
  if (etaSecs == null || !Number.isFinite(etaSecs) || etaSecs < 1) return '';
  const s = Math.ceil(etaSecs);
  return s < 60 ? `~${s}s left` : `~${Math.floor(s / 60)}m ${s % 60}s left`;
}

/** Compact summary of the most recent inference step (speed and KV cache reuse) for local models. */
export function formatLastInference(p: ModelProgress | null | undefined): string {
  if (!p) return '';
  const parts = [formatTps(p.tokensPerSecond), formatCache(p.cachedTokens, p.promptTokens)].filter(Boolean);
  return parts.join(' · ');
}

/** Present the latest evidence, without assuming a slow first token means the model is loading. */
export function turnProgress(snap: Turn): Progress | null {
  if (!snap.turnRunning) return null;
  if (snap.approvals.length) return { label: 'Waiting for your approval', waiting: true };
  if (snap.question) return { label: 'Waiting for your answer', waiting: true };
  if (snap.stats.running) {
    let tool: ToolEntry | undefined;
    for (let i = snap.entries.length - 1; i >= 0 && !tool; i--) {
      const e = snap.entries[i];
      if (e.kind === 'tool' && e.state === 'running') tool = e;
    }
    return { label: snap.stats.running === 1 ? 'Running a tool…' : 'Running tools…', tool };
  }

  const p = snap.modelProgress;
  const last = snap.entries.at(-1);
  const reply = last?.kind === 'assistant' && last.streaming ? last : null;
  if (p?.phase === 'generating' || reply?.text || reply?.reasoning) {
    const n = p?.outputTokens;
    const detail = n == null ? undefined : `${p?.estimated ? '≈' : ''}${n.toLocaleString()} ${n === 1 ? 'token' : 'tokens'}`;
    const meta = [formatTps(p?.tokensPerSecond), formatCache(p?.cachedTokens, p?.promptTokens)].filter(Boolean).join(' · ') || undefined;
    const thinking = Boolean(reply?.reasoning && !reply.text);
    const cacheState = cacheHitState(p?.cachedTokens, p?.promptTokens);
    return { label: thinking ? 'Thinking…' : 'Generating response…', detail, meta, thinking, cacheState };
  }
  if (p?.phase === 'loading') return { label: 'Loading model…' };
  if (p?.phase === 'processing') {
    const detail = p.processedTokens == null ? undefined : p.promptTokens == null
      ? `${p.processedTokens.toLocaleString()} tokens processed`
      : `${p.processedTokens.toLocaleString()} / ${p.promptTokens.toLocaleString()} tokens`;
    const fraction = p.processedTokens != null && p.promptTokens != null && p.promptTokens > 0
      ? Math.min(1, Math.max(0, p.processedTokens / p.promptTokens))
      : undefined;
    const meta = [formatCache(p.cachedTokens, p.promptTokens), formatTps(p.tokensPerSecond), formatEta(p.etaSecs)].filter(Boolean).join(' · ') || undefined;
    const cacheState = cacheHitState(p.cachedTokens, p.promptTokens);
    return { label: 'Processing prompt…', detail, meta, fraction, cacheState };
  }
  return { label: 'Waiting for the model…' };
}
