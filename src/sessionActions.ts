// What can be done to a session, shared by its sidebar menu, the title bar menu and the palette.

import { engine, showToast, snapshot } from "./bridge";
import { draftFor } from "./drafts";
import type { OpenSnapshot, Session } from "./types";
import { focusComposer, ui, type MenuItem } from "./ui";

/** Start editing the name of session `id` in `place`. */
export function startRename(id: string, place: "sidebar" | "title") {
  ui.renaming = { id, place };
}

/** Save the edited name; an unchanged or empty one just closes the editor. */
export function finishRename(id: string, before: string, after: string) {
  // Enter and Esc close the editor first; the blur that follows must not save again.
  if (ui.renaming?.id !== id) return;
  ui.renaming = null;
  const title = after.trim();
  if (title && title !== before) engine.renameSession(id, title);
}

/** The session on screen as its sidebar row, when it has one. */
export function openSession(snap: OpenSnapshot): Session | null {
  return snap.sessions.find((s) => s.selected) ?? null;
}

/** Whether the conversation on screen has a turn that can be undone. */
export function canUndo(snap: OpenSnapshot) {
  return !snap.turnRunning && snap.entries.some((e) => e.kind === "user");
}

/** The text of the last prompt, if any. */
export function lastPrompt(snap: OpenSnapshot): string | null {
  for (let i = snap.entries.length - 1; i >= 0; i--) {
    const entry = snap.entries[i];
    if (entry.kind === "user") return entry.text;
  }
  return null;
}

/** Whether the last turn failed, so its prompt can be sent again: the conversation ends in an error. */
export function canRetry(snap: OpenSnapshot) {
  if (snap.turnRunning || snap.connection.state !== "live") return false;
  for (let i = snap.entries.length - 1; i >= 0; i--) {
    const kind = snap.entries[i].kind;
    if (kind === "notice") continue;
    return kind === "error";
  }
  return false;
}

/** Whether the conversation can be compacted now. */
export function canCompact(snap: OpenSnapshot) {
  return snap.connection.state === "live" && !snap.turnRunning && snap.entries.length > 0;
}

/** Undo the last turn (after confirming) and put its prompt back in the composer to edit. */
export async function editLastPrompt(snap: OpenSnapshot) {
  const text = lastPrompt(snap);
  if (text === null) return;
  const draft = draftFor(snap);
  if (draft.text || draft.attachments.length) {
    showToast("Send or clear your current draft before editing the last prompt");
    return;
  }
  if (!(await engine.undoTurn()) || draft.text) return;
  draft.text = text;
  if (snapshot.value?.workspace?.root === snap.workspace.root && snapshot.value.sessionId === snap.sessionId) focusComposer();
}

/** Menu items for `session`; the open session also gets the conversation actions. */
export function sessionItems(snap: OpenSnapshot, session: Session, place: "sidebar" | "title"): MenuItem[] {
  const live = snap.connection.state === "live";
  const items: MenuItem[] = [
    { label: "Rename", icon: "pencil", run: () => startRename(session.id, place) },
    {
      label: session.pinned ? "Unpin" : "Pin to top",
      icon: session.pinned ? "unpin" : "pin",
      run: () => engine.pinSession(session.id, !session.pinned),
    },
    {
      label: "Duplicate",
      icon: "duplicate",
      disabled: !live || snap.turnRunning,
      run: () => engine.forkSession(session.id),
    },
  ];
  if (session.selected) {
    items.push(
      { divider: true },
      {
        label: "Copy as Markdown",
        icon: "copy",
        disabled: !snap.entries.length,
        run: () => copyConversation(snap),
      },
      {
        label: "Export…",
        icon: "download",
        disabled: !live || !snap.entries.length,
        run: () => engine.exportSession(snap.workspace.root, session.title),
      },
      { label: "Undo last turn", icon: "undo", disabled: !live || !canUndo(snap), run: () => engine.undoTurn() },
    );
  }
  items.push(
    { divider: true },
    {
      label: "Delete",
      icon: "trash",
      danger: true,
      disabled: session.running && !session.selected,
      run: () => engine.deleteSessions([session]),
    },
  );
  return items;
}

/** The conversation as Markdown: prompts, answers and one line per tool call. */
export function conversationMarkdown(snap: OpenSnapshot): string {
  const parts: string[] = [`# ${snap.title}`];
  for (const entry of snap.entries) {
    switch (entry.kind) {
      case "user":
        parts.push(`## You\n\n${entry.text}`);
        break;
      case "assistant":
        if (entry.text.trim() && !entry.abandoned) parts.push(`## Dial\n\n${entry.text.trim()}`);
        break;
      case "tool":
        parts.push(`- ${entry.label}${entry.target ? ` \`${entry.target}\`` : ""}`);
        break;
      case "notice":
      case "error":
        parts.push(`> ${entry.message}`);
        break;
    }
  }
  return parts.join("\n\n") + "\n";
}

export async function copyText(text: string, what = "Copied") {
  try {
    await navigator.clipboard.writeText(text);
    showToast(what);
  } catch {
    showToast("Could not copy to the clipboard");
  }
}

function copyConversation(snap: OpenSnapshot) {
  copyText(conversationMarkdown(snap), "Conversation copied as Markdown");
}
