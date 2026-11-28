import { watch } from 'vue';
import { readyForFollowUp } from './draftInput';
import type { OpenSnapshot, Snapshot } from './types';

/** Resolve the requested launch to a real, sendable journal. A switch or failure cancels this
 * submission permanently; a later reconnect must not replay it. */
export async function startupDestination(target: OpenSnapshot, current: () => Snapshot | null, signal: AbortSignal): Promise<string | null> {
  const destination = (): string | null | undefined => {
    const s = current();
    if (signal.aborted || !target.startupDraftKey || !s?.workspace || s.workspace.root !== target.workspace.root || s.startupDraftKey !== target.startupDraftKey) return null;
    if (s.connection.state === 'starting' || (s.connection.state === 'live' && s.historyLoading)) return undefined;
    return readyForFollowUp(s, { workspace: s.workspace, sessionId: s.sessionId }) ? s.sessionId : null;
  };
  const initial = destination();
  if (initial !== undefined) return initial;
  const id = await new Promise<string | null>(resolve => {
    const finish = (id: string | null) => { stop(); signal.removeEventListener('abort', abort); resolve(id); };
    const abort = () => finish(null);
    const stop = watch(destination, id => { if (id !== undefined) finish(id); }, { flush: 'sync' });
    signal.addEventListener('abort', abort, { once: true });
  });
  return id && destination() === id ? id : null;
}
