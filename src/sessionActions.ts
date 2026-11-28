// What can be done to a session, shared by its sidebar menu, the title bar menu and the palette.

import { engine, files, folders, hiddenSessions, lastSubmission, showToast, snapshot } from './bridge';
import { engineCall, goal, goalAction, readable } from './engineTools';
import { beginSend, draftFor, hasDraft } from './drafts';
import { draftReady, readyForFollowUp, restoreInput } from './draftInput';
import type { OpenSnapshot, Session } from "./types";
import { focusComposer, ui, type MenuItem } from "./ui";
import { dismissType, isTypeSession, typeIdFor, typeState } from "./savedTypes";
import { watch } from "vue";

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

type SidebarSnapshot = Pick<OpenSnapshot, 'sessions' | 'workspace'>;

export function listedSession(s: Session, snap: SidebarSnapshot): boolean {
  // Deleted with Undo still offered: gone from the list (and ⌘1–9) until Undo brings it back.
  if (hiddenSessions.has(s.id)) return false;
  if (isTypeSession(s.id) || (s.selected && typeState.pending)) return false;
  return !s.empty || s.running || s.waiting || s.pinned || s.selected || hasDraft(snap.workspace.root, s.id);
}

/** External sessions are attachable by explicit search, not suggestions to continue. */
function omitFromRecents(s: Session): boolean {
  // Older CLIs interpreted Desktop's literal attach invocation as the first prompt/title.
  // Match only the whole command with a full journal id; these chats stay searchable.
  return !!s.openElsewhere || /^attach (?:-- )?[0-9a-f]{16}$/i.test(s.title);
}

export function recentSession(s: Session, snap: SidebarSnapshot): boolean {
  return !omitFromRecents(s) && !s.empty && listedSession(s, snap);
}

/** The age bucket of a session row, read from its "3h ago · 12 messages" line. */
function ageGroup(meta: string): string {
  const age = meta.split("·")[0].trim().toLowerCase();
  if (age === "just now" || /^\d+[mh] ago$/.test(age)) return "Today";
  if (age === "yesterday") return "Yesterday";
  if (/^\d+d ago$/.test(age)) return "This week";
  return "Older";
}

/**
 * Sessions as the sidebar groups them (Active, Pinned, then Recents by age: Today, Yesterday, This
 * week, Older), keeping those whose title matches `filter`. A type's own conversations are not
 * listed: they open with their type. Recents that all fall in one age stay one "Recents" group.
 */
export function sessionGroups(snap: SidebarSnapshot, filter = ""): { name: string; sessions: Session[] }[] {
  const f = filter.trim().toLowerCase();
  const matching = snap.sessions.filter((s) => listedSession(s, snap) && (!f || s.title.toLowerCase().includes(f)));
  const idle = matching.filter((s) => !s.pinned && !s.running && !s.waiting);
  // Keep a fresh conversation reachable immediately without counting selection as history.
  const blank = idle.find((s) => s.selected && s.empty && !hasDraft(snap.workspace.root, s.id));
  const recents = idle.filter((s) => s !== blank && (f || !omitFromRecents(s)));
  const byAge = new Map<string, Session[]>();
  for (const s of recents) {
    const name = ageGroup(s.meta ?? "");
    const group = byAge.get(name);
    if (group) group.push(s);
    else byAge.set(name, [s]);
  }
  const aged = f || byAge.size < 2
    ? [{ name: "Recents", sessions: recents }]
    : ["Today", "Yesterday", "This week", "Older"].map((name) => ({ name, sessions: byAge.get(name) ?? [] }));
  return [
    { name: "Active", sessions: matching.filter((s) => s.running || s.waiting) },
    { name: "Pinned", sessions: matching.filter((s) => s.pinned && !s.running && !s.waiting) },
    ...aged,
    { name: "New session", sessions: blank ? [blank] : [] },
  ].filter((g) => g.sessions.length);
}

/** The sidebar's sessions top to bottom: what ⌘1–9 and next/previous session step through. */
export function sessionOrder(snap: OpenSnapshot): Session[] {
  return sessionGroups(snap, ui.sessionFilter).flatMap((g) => g.sessions);
}

/** Open `session` from the keyboard or a list. */
export function goToSession(session: Session) {
  ui.transcriptJump = null;
  ui.settingsOpen = false;
  dismissType();
  if (!session.selected) engine.resume(session.id);
}

/** Open the session `step` places below (or above, when negative) the open one, wrapping around. */
export function stepSession(snap: OpenSnapshot, step: number) {
  const list = sessionOrder(snap);
  if (!list.length) return;
  const at = list.findIndex((s) => s.selected);
  const next = at < 0 ? (step > 0 ? 0 : list.length - 1) : (at + step + list.length) % list.length;
  goToSession(list[next]);
}

/** Whether the conversation on screen has a turn that can be undone. */
export function canUndo(snap: OpenSnapshot) {
  return !snap.turnRunning && snap.entries.some((e) => e.kind === "user");
}

/** The last user entry, including the image count when replay cannot supply the files. */
function lastPrompt(snap: Pick<OpenSnapshot, 'entries'>) {
  for (let i = snap.entries.length - 1; i >= 0; i--) {
    const entry = snap.entries[i];
    if (entry.kind === 'user') return entry;
  }
  return null;
}

/** Local preview bytes only, tied to a confirmed transcript row (never resolve paths from chat). */
export const sentImagesFor = (row: Extract<OpenSnapshot['entries'][number], { kind: 'user' }>) => lastSubmission.imagesFor(row);

/** This session's prompts, newest first, with repeats removed (for ↑/↓ in the composer). */
export function promptHistory(snap: OpenSnapshot): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (let i = snap.entries.length - 1; i >= 0; i--) {
    const entry = snap.entries[i];
    if (entry.kind !== "user" || !entry.text.trim() || seen.has(entry.text)) continue;
    seen.add(entry.text);
    out.push(entry.text);
  }
  return out;
}

/** Whether the last turn failed, so its prompt can be sent again: the conversation ends in an error. */
export function canRetry(snap: OpenSnapshot) {
  if (snap.turnRunning || snap.connection.state !== "live") return false;
  for (let i = snap.entries.length - 1; i >= 0; i--) {
    const kind = snap.entries[i].kind;
    if (kind === "notice" || kind === "received") continue;
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
  const entry = lastPrompt(snap);
  if (!entry) return;
  const draft = draftFor(snap);
  if (!draftReady(draft)) return showToast('Wait for your draft and attachments to finish preparing or sending');
  if (draft.text || draft.attachments.length) {
    showToast("Send or clear your current draft before editing the last prompt");
    return;
  }
  const original = lastSubmission.forRow(entry);
  const stillCurrent = () => {
    const current = snapshot.value;
    return current?.workspace?.root === snap.workspace.root && current.sessionId === snap.sessionId &&
      !current.turnRunning && lastPrompt(current) === entry && !draft.text && !draft.attachments.length && draftReady(draft);
  };
  if (!await engine.undoTurn({ missingImages: original ? 0 : entry.images, stillCurrent })) return;
  // Preserve any typing that arrived while the native undo request was in flight.
  restoreInput(draft, original ?? { text: entry.text, attachments: [] });
  if (snapshot.value?.workspace?.root === snap.workspace.root && snapshot.value.sessionId === snap.sessionId) focusComposer();
}

/** Resolves once `test` holds for the snapshot (true), or after `ms` (false). */
function snapshotSettles(test: (snap: typeof snapshot.value) => boolean, ms = 3000): Promise<boolean> {
  return new Promise((resolve) => {
    if (test(snapshot.value)) return resolve(true);
    let stop = () => {};
    const timer = setTimeout(() => { stop(); resolve(false); }, ms);
    stop = watch(snapshot, (now) => {
      if (!test(now)) return;
      clearTimeout(timer);
      stop();
      resolve(true);
    });
  });
}

/** Why the last answer cannot be regenerated, or `""` when it can (see {@link regenerateLast}). */
export function regenerateBlocker(snap: OpenSnapshot): string {
  if (!readyForFollowUp(snap, snap) || !canUndo(snap)) return "Regenerate needs Dial ready to send in this session";
  const entry = lastPrompt(snap);
  if (!entry) return "There is no prompt to send again";
  if (entry.images && !lastSubmission.forRow(entry)) return "Its images are no longer available; use Edit to attach them again";
  return "";
}

/**
 * Regenerate the last answer: undo the last turn (files included, after confirming) and send the
 * same prompt, with its files, again. Built from undo + send because the engine's retry only
 * resends a prompt whose turn failed.
 */
export async function regenerateLast(snap: OpenSnapshot) {
  const blocker = regenerateBlocker(snap);
  if (blocker) return showToast(blocker);
  const entry = lastPrompt(snap)!;
  const original = lastSubmission.forRow(entry);
  const draft = draftFor(snap);
  if (!draftReady(draft)) return showToast('Wait for your draft and attachments to finish preparing or sending');
  const content = original ?? { text: entry.text, attachments: [] };
  const previous = lastPrompt({ entries: snap.entries.slice(0, snap.entries.lastIndexOf(entry)) });
  const here = (s: typeof snapshot.value) => s?.workspace?.root === snap.workspace.root && s.sessionId === snap.sessionId;
  const stillCurrent = () => readyForFollowUp(snapshot.value, snap) && lastPrompt(snapshot.value) === entry && draftReady(draft);
  // A selected file can disappear after its original submission. Discover that before
  // undoing the turn; send_prompt also rechecks in case it changes during confirmation.
  try {
    for (const file of content.attachments) if (file.path) await files.validateAttachment(file.path);
  } catch (error) { return showToast(`Cannot regenerate with these attachments: ${error}`); }
  if (!stillCurrent()) return;
  if (!await engine.undoTurn({ missingImages: 0, stillCurrent, regenerate: true })) return;
  if (!draftReady(draft)) {
    restoreInput(draft, content);
    return showToast('The turn was undone. Its prompt is back in the original draft.');
  }
  const finish = beginSend(draft, content);
  let sent = false;
  try {
    // Only resend when the *expected* preceding turn is back, never on a timeout,
    // transient empty history, different prompt or a switch to another session.
    const ready = (s: typeof snapshot.value) => readyForFollowUp(s, snap) && lastPrompt(s) === previous;
    const settled = await snapshotSettles(s => !here(s) || ready(s));
    if (!settled || !ready(snapshot.value)) return;
    const scope = { workspaceRoot: snap.workspace.root, sessionId: snap.sessionId, typeId: typeIdFor(snap.sessionId) };
    sent = await engine.send(content.text, content.attachments, scope);
  } finally {
    finish(sent);
    if (!sent) showToast('Regeneration was not confirmed. The prompt is back in the original draft; check the transcript before sending.');
  }
}

/** Which of the prompts with the same text prompt `index` is (1-based), and how many there are; how `fork_before` finds it. */
export function promptOccurrence(entries: OpenSnapshot["entries"], index: number): { occurrence: number; matches: number } {
  const target = entries[index];
  let occurrence = 0, matches = 0;
  if (target?.kind !== "user") return { occurrence, matches };
  for (const [i, e] of entries.entries()) {
    // fork_before counts persisted user-role records, including harness mail; only the
    // genuine prompt is actionable, even when someone pasted the exact same body in chat.
    if ((e.kind !== 'user' && !(e.kind === 'received' && e.source === 'journal')) || e.text !== target.text) continue;
    matches++;
    if (i === index) occurrence = matches;
  }
  return { occurrence, matches };
}

/** Copy the conversation before prompt `index` into a new session and put that prompt in its box. */
export async function editFromHere(snap: OpenSnapshot, index: number) {
  const entry = snap.entries[index];
  if (entry?.kind !== "user" || !snap.sessionId) return;
  const original = lastSubmission.forRow(entry);
  const { occurrence, matches } = promptOccurrence(snap.entries, index);
  const id = await engine.forkBefore(snap.workspace.root, snap.sessionId, entry.text, occurrence, matches, true);
  if (!id) return;
  const draft = draftFor({ workspace: snap.workspace, sessionId: id });
  restoreInput(draft, original ?? { text: entry.text, attachments: [] });
  if (entry.images && !original) showToast(`Attach ${entry.images === 1 ? 'the image' : `the ${entry.images} images`} again; only the prompt text is copied`);
  if (await snapshotSettles((s) => s?.workspace?.root === snap.workspace.root && s.sessionId === id)) focusComposer();
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
        label: "Export as Markdown…",
        icon: "download",
        disabled: !live || !snap.entries.length,
        run: () => engine.exportSession(snap.workspace.root, session.title),
      },
      {
        label: "Export as HTML…",
        icon: "download",
        disabled: !live || !snap.entries.length,
        run: () => engine.exportSession(snap.workspace.root, session.title, "html"),
      },
      { label: "Undo last turn", icon: "undo", disabled: !live || !canUndo(snap), run: () => engine.undoTurn() },
      { label: "Compact conversation", icon: "refresh", disabled: !canCompact(snap), run: () => engine.compact() },
      {
        label: "Pack to a file",
        icon: "download",
        disabled: !live || snap.turnRunning || !snap.entries.length,
        run: () => engineCall<{ path?: string }>("pack_session", {}).then((r) => showToast(r.path ? `Packed to ${r.path}` : "Session packed"), (e) => showToast(`Could not pack the session: ${readable(e)}`)),
      },
      {
        label: "Label the latest message…",
        icon: "pin",
        disabled: !live || !snap.entries.length,
        prompt: {
          label: "Label, so the message is easy to find later",
          placeholder: "For example: before the refactor",
          maxLength: 80,
          confirm: "Label",
          submit: (value) =>
            void engineCall("label_session", { label: value.trim() }).then(() => showToast(`Labeled “${value.trim()}”`), (e) => showToast(`Could not label the message: ${readable(e)}`)),
        },
      },
      {
        label: goal.value && goal.value.status !== "dropped" && goal.value.status !== "complete" ? "Replace the goal…" : "Set a goal…",
        icon: "check",
        disabled: !live || snap.turnRunning,
        prompt: {
          label: "Goal: what Dial keeps working toward across turns",
          placeholder: "For example: all tests pass on the release branch",
          maxLength: 2000,
          confirm: "Start",
          submit: async (value) => {
            const current = goal.value;
            if (current && current.status !== "dropped" && current.status !== "complete" && !(await goalAction({ action: "drop" }))) return;
            void goalAction({ action: "set", objective: value.trim() });
          },
        },
      },
      {
        label: "Move to another project…",
        icon: "folder",
        disabled: !live || snap.turnRunning,
        run: async () => {
          const path = await folders.pick("Move this session to a project folder");
          if (path) (await import("./commandRunner")).runTyped(snap, "move", path, []);
        },
      },
      {
        label: "Archived sessions",
        icon: "list",
        disabled: !live,
        run: async () => (await import("./commandRunner")).runTyped(snap, "archive", "", []),
      },
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
      case "received":
        parts.push(`## Message received from ${entry.sender}\n\n${entry.text}`);
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
