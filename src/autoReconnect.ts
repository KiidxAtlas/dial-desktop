// One automatic reconnect after the engine of a live session goes away unexpectedly.

import type { Connection } from "./types";

/** Seconds the banner counts down before reconnecting on its own. */
export const AUTO_RECONNECT_SECONDS = 3;

export interface ReconnectState {
  /** The session (project + id) last seen connected. */
  liveKey: string | null;
  /** The automatic attempt for this outage was made (or cancelled); no second one. */
  tried: boolean;
}

export const initialReconnect: ReconnectState = { liveKey: null, tried: false };

/**
 * The next state after `connection` is seen for session `key`, and whether to start the countdown.
 * Only a session seen live that then ended or failed for an ordinary reason reconnects, once until
 * it is live again: not one that never started, not a missing or incompatible engine (reconnecting
 * cannot fix those), and not another session opened onto an engine that had already stopped.
 */
export function nextReconnect(state: ReconnectState, key: string | null, connection: Connection): { state: ReconnectState; start: boolean } {
  switch (connection.state) {
    case "live":
      return { state: { liveKey: key, tried: false }, start: false };
    case "starting":
      return { state, start: false };
    case "ended":
    case "failed": {
      if (connection.state === "failed" && connection.problem !== "other") return { state: { ...state, tried: true }, start: false };
      const start = !state.tried && key !== null && state.liveKey === key;
      return { state: { ...state, tried: state.tried || start }, start };
    }
    default:
      return { state: initialReconnect, start: false };
  }
}
