// In-memory drafts follow a session, not the window's current selection.
import { reactive } from "vue";
import type { Attachment, OpenSnapshot } from "./types";

interface Draft {
  text: string;
  attachments: Attachment[];
  sending: boolean;
}

const drafts = new Map<string, Draft>();

export function draftFor(snap: Pick<OpenSnapshot, "workspace" | "sessionId">): Draft {
  const key = JSON.stringify([snap.workspace.root, snap.sessionId]);
  let draft = drafts.get(key);
  if (!draft) {
    draft = reactive({ text: "", attachments: [], sending: false });
    drafts.set(key, draft);
  }
  return draft;
}

export type { Draft };
