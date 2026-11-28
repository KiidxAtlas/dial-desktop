# Sessions in Dial Desktop

How the sidebar lists sessions and what happens when you open one.

## The sidebar

Sessions are grouped, top to bottom:

| Group | What is in it |
| --- | --- |
| **Active** | Sessions running a turn or waiting for an approval or answer, in any engine. |
| **Pinned** | Pinned sessions that are idle. |
| **Recents** | Every other session with messages or a saved draft, newest activity first. When they span more than one age, Recents splits into **Today**, **Yesterday**, **This week** and **Older**. |
| **New session** | The blank session on screen, until it has a message or a draft. |

The order follows activity, not attention:

- **Only new messages move a session.** Opening a session touches its journal, and the engine
  reports it as recently updated; Desktop keeps the session's previous position and age group
  until its message count changes.
- **Selecting a session never changes Recents.** A blank session shows under **New session**,
  below Recents, so opening or leaving it does not shift other rows.
- Titles and pins update in place.
- Sessions open in another Dial process stay out of Recents and the home screen’s Continue suggestions. Search by title or message to find one intentionally.

The sidebar filter searches titles; switch it to **Messages** to search saved conversations.

## Opening a session

Each running session has its own `dial rpc` engine (see `src-tauri/src/tabs.rs`):

- If the engine on screen is idle, the session opens in it (`resume`).
- If that engine is working, it keeps running in the background and a second engine opens the
  session. Clicking a running session returns to the engine that owns it rather than opening a copy.
- A background engine closes when its turn ends and nothing waits for you.
- Opening a session held by another Dial process (a terminal or another window) shows **Attached chat** in the Terminal panel. It lets you follow the conversation and send input to its owner without creating a copy. Detaching closes that view, not the original session.
  Older engines could interpret `attach <id>` as a new prompt instead. Desktop now checks for support first; old conversations named exactly like that command are searchable but not suggested in Recents.

While Desktop waits for the engine to open the session and replay its history, it shows the
saved conversation instead of a blank **Loading conversation…**:

1. **At once, from Desktop's copy.** Desktop keeps a copy of each conversation it has shown, in
   its private folder (`session-previews/` under the app's folder, owner-only on macOS and
   Linux). It needs no engine, so it works while Dial is still starting. Copies over 4 MB are
   not kept, each project keeps its 200 most recent, and deleting a session permanently removes
   its copy.
2. **Then from the running engine.** An engine that supports `session_messages` reads the saved
   conversation without opening it or starting another engine, and Desktop shows (and keeps)
   that newer copy.

The preview is read-only: the composer waits, and approvals, questions and the plan appear once
the session has opened. A session Desktop has never shown, opened with an older engine, still
shows **Loading conversation…** until it opens. The project's session list stays on screen while
switching between engines, until the engine now on screen returns a fresh list.

You can type while the engine starts. Press **Enter** or **Send** to queue the prompt for that conversation, and keep typing your next message. If startup fails or you switch away before it is ready, Desktop does not send the queued prompt to a different conversation.

## Related

- [Browser agent and tabs](browser-agent.md)
- `/git` in a folder of repositories: `/git` uses the only nested repository, or lists them;
  choose one with `/git repo <folder>`. **Changes** offers to open a nested repository as the
  project.
- [Changelog](../CHANGELOG.md)
