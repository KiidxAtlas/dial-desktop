import type { Snapshot, ToolEntry } from './types';

type Turn = Pick<Snapshot, 'turnRunning' | 'approvals' | 'question' | 'entries' | 'modelProgress' | 'stats'>;
/** `waiting`: the turn is paused on the user, not working (so Esc-to-stop is not the next step).
 * `tool`: the newest call still running, so the view can name its phase ("Reading", "Editing"). */
interface Progress { label: string; detail?: string; waiting?: boolean; tool?: ToolEntry }

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
    return { label: reply?.reasoning && !reply.text ? 'Thinking…' : 'Generating response…', detail };
  }
  if (p?.phase === 'loading') return { label: 'Loading model…' };
  if (p?.phase === 'processing') {
    const detail = p.processedTokens == null ? undefined : p.promptTokens == null
      ? `${p.processedTokens.toLocaleString()} tokens processed`
      : `${p.processedTokens.toLocaleString()} / ${p.promptTokens.toLocaleString()} tokens`;
    return { label: 'Processing prompt…', detail };
  }
  return { label: 'Waiting for the model…' };
}
