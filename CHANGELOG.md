# Changelog

Notable changes to Dial Desktop, grouped by version, following
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Dates are the release tag's commit date.
**Unreleased** describes the current checkout, not an installed release. Desktop and the Dial
engine are released separately; the engine's notes are in its own
[CHANGELOG](https://github.com/KiidxAtlas/dial/blob/main/CHANGELOG.md).

## [Unreleased]

## [0.4.2] - 2026-11-27

### Sessions
- Opening an older session shows its conversation at once instead of **Loading conversation…**:
  first the copy Desktop kept from the last time it was shown (no engine needed), then the copy
  the running engine reads without opening it. You can read while the session opens; sending
  waits until it is open. The live read needs an engine with `session_messages`.
- You can type and press Send while the engine starts. The prompt waits for that conversation.
- Sessions held elsewhere and legacy `attach <id>` rows stay out of Recents but remain searchable. Desktop checks for attach support before opening the terminal view.
- Clicking a session no longer reorders the sidebar or moves it between age groups (Today,
  Yesterday, This week, Older). Only new messages move a session. [Sessions](docs/sessions.md).
- Switching to a session that is running in another engine no longer brings back that engine's
  older copy of the list; the list on screen stays until a fresh one arrives.
- A new, blank session shows at once under **New session**, below Recents, so it never pushes
  existing conversations down. It joins Recents once it has a message or draft.
- A new session appears in the sidebar immediately, before the engine confirms it, and holding
  ⌘/Ctrl+N no longer creates several "Untitled" sessions.

### Git
- `/git` works from a folder that is not itself a repository: with exactly one nested repository
  it uses that one; with several it lists them and asks you to choose with `/git repo <folder>`.
  Choices must be folders inside the project that are their own repository; links out of the
  project are refused. Needs an engine with this support.
- **Changes** lists the nested repositories of a non-repository project and opens one as the
  project to use its Git actions.

### Browser
- The Browser panel has tabs: open, switch and close them from the tab bar. Each tab keeps its
  page and form state while another is shown; up to 16 tabs, and closing the last leaves a blank
  one. Links that open a new window open as a new tab. [Browser agent](docs/browser-agent.md).
- The agent can list, open, select and close tabs (`browser_tab`). `browser_open` now waits for
  the requested address to finish loading in the active tab and fails instead of returning an
  older page.

## [0.4.1] - 2026-11-27
- Accumulated Desktop changes for 0.4.1, including attaching to a session that is running in
  another Dial process from the sidebar.
- Release-platform fixes for native test fixtures, Windows workspace paths and WebView2 profile
  cleanup after the browser smoke test.

## [0.4.0] - 2026-11-26
- Release 0.4.0. Git test repositories no longer depend on the host's `core.autocrlf`.

## [0.3.5] - 2026-11-25
- Computer-use preferences apply to the effective settings layer (the project's integrations
  when it has its own list, otherwise the global preference).

## [0.3.4] - 2026-11-25
- Windows terminals report their exit: ConPTY closes when its shell ends. Linux builds.

## [0.3.3] - 2026-11-23
- Attach any file to a prompt; Windows consoles are hidden.

## [0.3.2] - 2026-11-22
- Automatic model discovery; provider sign-in and setup work without an RPC connection.

## [0.3.1] - 2026-11-21
- Types design progress and views, image actions, the model library, tabs and terminal updates.

## [0.3.0] - 2026-11-21
- Release 0.3.0.

## [0.2.1] - 2026-11-20
- Updated background presets; Enter with pasted images sends correctly.

## [0.2.0] - 2026-11-20
- Release 0.2.0.

## [0.1.2] - 2026-11-19
- Voice mode, changes review, notifications and persistent follow-up queues.

## [0.1.1] - 2026-11-19
- Browser, files and terminal panels; self-updating app and Dial engine management; Windows
  signing; release workflow.

[Unreleased]: https://github.com/KiidxAtlas/dial-desktop/compare/v0.4.2...HEAD
[0.4.2]: https://github.com/KiidxAtlas/dial-desktop/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/KiidxAtlas/dial-desktop/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/KiidxAtlas/dial-desktop/compare/v0.3.5...v0.4.0
[0.3.5]: https://github.com/KiidxAtlas/dial-desktop/compare/v0.3.4...v0.3.5
[0.3.4]: https://github.com/KiidxAtlas/dial-desktop/compare/v0.3.3...v0.3.4
[0.3.3]: https://github.com/KiidxAtlas/dial-desktop/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/KiidxAtlas/dial-desktop/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/KiidxAtlas/dial-desktop/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/KiidxAtlas/dial-desktop/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/KiidxAtlas/dial-desktop/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/KiidxAtlas/dial-desktop/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/KiidxAtlas/dial-desktop/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/KiidxAtlas/dial-desktop/releases/tag/v0.1.1
