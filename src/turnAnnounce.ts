// What the screen-reader live region says as a turn moves along: started, waiting, finished.
// Not every streamed token, only changes of state.

export interface TurnView {
  /** Project and session; a change of session is not announced as a turn event. */
  key: string | null;
  turnRunning: boolean;
  approvals: number;
  question: boolean;
  /** The kind of the conversation's last entry. */
  lastKind: string | null;
}

/** The sentence to announce when the view goes from `prev` to `next`, or `null` for none. */
export function turnAnnouncement(prev: TurnView | null, next: TurnView): string | null {
  if (!prev || prev.key !== next.key) return null;
  if (next.approvals > prev.approvals) {
    return next.approvals === 1 ? "Dial is waiting for your approval" : `Dial is waiting for ${next.approvals} approvals`;
  }
  if (next.question && !prev.question) return "Dial has a question for you";
  if (next.turnRunning && !prev.turnRunning) return "Dial is working";
  if (!next.turnRunning && prev.turnRunning) return next.lastKind === "error" ? "Dial stopped with an error" : "Dial finished";
  return null;
}
