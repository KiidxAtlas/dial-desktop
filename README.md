# Dial Desktop

A native desktop interface for [Dial](https://github.com/KiidxAtlas/dial), built with
[Tauri 2](https://tauri.app) and [Vue 3](https://vuejs.org).

Dial Desktop is a separate process: it starts `dial rpc` and talks to it through
[`dial-client`](https://github.com/KiidxAtlas/dial/tree/main/crates/dial-client), Dial's Rust
client for the versioned RPC protocol. It never links engine crates.

## Setup

Needs Rust (see `edition` in `src-tauri/Cargo.toml`), Node 20+ and the
[Tauri system prerequisites](https://tauri.app/start/prerequisites/) for your OS.

```sh
npm install
```

Or use the Makefile (`make` lists every target):

```sh
make run DIR=~/code/my-project       # live engine, hot reload
make preview                          # design preview, no engine
make app && make open DIR=~/code/app  # debug build, then launch it
make test                             # Rust tests and the type check
```

## Run

During development (Vite hot reload for the UI, Rust rebuilt on change). Arguments for the app go
after `-- -- --`:

```sh
npm run dev -- -- -- ~/code/my-project                 # live engine (`dial` on PATH)
npm run dev -- -- -- --dial /path/to/dial ~/code/app   # a specific engine build
npm run dev -- -- -- ~/code/app -- --model local       # extra `dial rpc` flags after `--`
npm run dev -- -- -- --preview                         # design preview, no engine needed
npm run dev -- -- -- --preview=empty                   # the empty-session state
npm run dev -- -- -- --preview=question                # the session waiting on a question
```

Under `npm run dev` the app runs from `src-tauri/`, so pass the workspace explicitly.

Release build and bundle (`.app`/`.dmg`, `.msi`, `.deb`/`.AppImage` depending on the OS):

```sh
npm run build
./src-tauri/target/release/dial-desktop ~/code/my-project   # or $CARGO_TARGET_DIR/release/…
```

`DIAL_BIN` also selects the engine binary.

Shortcuts: <kbd>Enter</kbd> sends, <kbd>Shift</kbd>+<kbd>Enter</kbd> adds a line,
<kbd>Esc</kbd> stops a running turn, <kbd>⌘/Ctrl</kbd>+<kbd>N</kbd> starts a session,
<kbd>⌘/Ctrl</kbd>+<kbd>,</kbd> opens settings.

Running sessions and sessions waiting for a reply appear under **Active** in the sidebar. Open
one there to return to its running conversation. To browse local chat models that Dial already
detects on disk, open the composer’s model picker and choose **Downloaded**. Model details show
the source, folder and size. Use **Get models…** to download a library model, or reload the list
after adding one outside Desktop. You can also confirm removal of an unused model from a supported store.

Desktop scans connected providers when it connects and about every five minutes while a project is open. Returning to Desktop or opening the model picker also checks for updates if the last scan was at least a minute ago. Sign-ins saved by the CLI are picked up from the same Dial home; you don’t need to run `/models` or change your current model.

If Desktop cannot load its initial state, **Reconnect** retries the UI connection without
resending a prompt. **Copy error** copies the diagnostic for troubleshooting.

Open **Settings › Providers** to sign in to a subscription or save a provider API key. Complete browser authorization if prompted; enter keys and codes in the Settings form, not in chat. Desktop refreshes the model picker after setup without switching your active model. If the engine is missing or too old to offer provider setup, install or update it in **Settings › Updates**.

Choose **Remove key** to forget that endpoint’s saved API key on this computer across all projects. This does not sign you out of a subscription, change environment variables, or revoke the key at its issuer. Engines that can save keys but cannot remove them still offer **Add key** and **Replace key**. Desktop checks the engine’s `can_remove` capability rather than its version and never reads its credential store directly.

## Updates

The app and the Dial engine update separately, and nothing installs until the user asks
(**Settings › Updates**, the banner under the top bar, or **Check for updates** in the palette).

- **The app** uses the Tauri updater: it polls
  `https://github.com/KiidxAtlas/dial-desktop/releases/latest/download/latest.json` and installs
  only bundles signed with the key in `src-tauri/tauri.release.conf.json` (`plugins.updater.pubkey`).
  Builds without that config (development, local `npm run build`) do not update themselves. On Linux only
  the AppImage self-updates; the `.deb` updates through its package manager.
- **The engine** is not bundled. Desktop checks the selected `dial` and uses its own updater
  for signed releases, Homebrew, Cargo or source checkouts. If the engine is missing or a
  newer release is available for an externally managed install, choose **Install Desktop copy**
  in Settings. This runs Dial's signed release installer in a separate Desktop folder without
  replacing the original engine. Desktop prefers that copy on future launches unless you set
  `--dial` or `DIAL_BIN`. On Windows the installer also adds that folder to the user PATH.
  Running sessions can reconnect when idle to use the new engine.
- **CLI only**: people who do not want the app install the engine alone with the same one-line
  installer from <https://dial.rootatlas.dev/install?cli>.

## Releasing

Push a tag `vX.Y.Z` matching the version in `package.json`, `src-tauri/tauri.conf.json` and
`src-tauri/Cargo.toml`. `.github/workflows/release.yml` builds macOS (arm64 and x86-64),
Windows and Linux with `--config src-tauri/tauri.release.conf.json` (which adds updater
artifacts), uploads them and `latest.json` into a draft release, publishes it once every platform
succeeds, and asks the website to refresh its download links.

Before tagging, smoke-test the candidate from the app icon in a clean OS account, with no
`dial` or `hf` on PATH. Open a project, install the engine, connect a test provider or download
a local model, and send a prompt with a screenshot. Edit and resend that prompt; confirm the
image is still attached. Create and remove a read-only schedule. Using a signed update fixture,
interrupt a download, retry in Settings, and verify a running turn finishes before restart.
No step should require opening a shell or copying a command.

One-time repository setup:

| Kind | Name | Value |
| --- | --- | --- |
| Config | `plugins.updater.pubkey` in `src-tauri/tauri.release.conf.json` | public key from `npx tauri signer generate -w ~/.tauri/dial-desktop.key` (committed; it is public) |
| Secret | `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | that private key and its password |
| Secrets | `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | Developer ID Application certificate (base64 `.p12`) and notarization login; without them the macOS app is only ad-hoc signed and users must approve its first launch (right-click › Open, or System Settings › Privacy & Security) |
| Variable / secret | `DIAL_WEB_REFRESH_URL` / `DIAL_WEB_REFRESH_TOKEN` | `https://dial.rootatlas.dev/release/refresh` and the site's `NUXT_RELEASE_REFRESH_TOKEN` (optional) |

Keep the private key safe: losing it means installed apps can never verify another update.

Windows release signing uses Azure Artifact Signing (`rootatlas`, Public Trust profile
`dial-desktop`), independently of the Tauri updater key. Create a GitHub environment named
`windows-signing`, restrict deployments to release tags (`v*`), and configure reviewers.
In Entra ID, add a GitHub federated credential on the release app registration for this repo's
`windows-signing` environment; assign the app **Artifact Signing Certificate Profile Signer**
on the profile. Set environment variables `AZURE_CLIENT_ID`, `AZURE_TENANT_ID`,
`AZURE_SUBSCRIPTION_ID`, `AZURE_SIGNING_ENDPOINT` (the account's regional HTTPS endpoint),
and `AZURE_SIGNING_PUBLISHER` (the exact certificate common name). No client secret or PFX
is needed. The Tauri signing hook verifies the publisher and timestamp before packaging
continues. Local builds without the release config do not contact Azure.

## What it does today

- **Sessions sidebar**: the project's session history (pinned first, most recent next) with search.
  Click any session to open it, even while another one is working: the busy session keeps running
  in its own `dial rpc` process in the background (one engine per running session, as the protocol
  asks), and clicking it again brings it back. A pulsing dot marks running sessions, an amber dot
  one that waits for an approval or answer; you are told when a background session finishes. Hover
  a session and click × to delete it (after a confirmation); deleting the open session starts a
  fresh one first. <kbd>⌘/Ctrl</kbd>-click picks several sessions and <kbd>Shift</kbd>-click a
  range; then delete them together from the bar above the list or with <kbd>Delete</kbd>
  (<kbd>Esc</kbd> clears the pick, <kbd>⌘/Ctrl</kbd>+<kbd>A</kbd> picks every listed session).
  **Search project messages** searches saved transcripts across the project. Select a result to
  open its conversation and jump to the message; if it is no longer in the displayed history or
  can’t be identified uniquely, Desktop shows the saved snippet instead.
  Switching projects asks before stopping running work or closing terminals. **Reconnect**
  resumes the foreground conversation without restarting other session engines or terminals.
- **Save an answer as a Type**: click **Save** under a reply, choose an existing type or name a
  **New type**, then **Create & open**. On its first save, the design checklist shows the layout,
  fields and look as Dial commits them. Choices saved together appear together. The finished
  item replaces the checklist; a new book opens directly to its first entry.
- **Session actions**: right-click a session, use its `…` button, or click the title in the top
  bar to rename (also double-click or <kbd>F2</kbd>), pin to the top, duplicate (a forked copy),
  copy the conversation as Markdown, export it to a Markdown file in the project, undo the last
  turn (its file changes are restored after a confirmation), or delete. Several picked sessions
  can be pinned or deleted together from the same menu.
- **Command palette** (<kbd>⌘/Ctrl</kbd>+<kbd>K</kbd>): every action above plus switching
  sessions, the model, the theme, zoom and panels, by typing. <kbd>⌘/Ctrl</kbd>+<kbd>/</kbd> lists
  every keyboard shortcut. <kbd>⌘/Ctrl</kbd>+<kbd>B</kbd> hides the sidebar,
  <kbd>⌥⌘I</kbd> toggles the side panel, <kbd>⇧⌘B</kbd> the browser, <kbd>⇧⌘E</kbd> the files,
  <kbd>Ctrl</kbd>+<kbd>`</kbd> the terminal, <kbd>⌘+</kbd>/<kbd>⌘−</kbd>/<kbd>⌘0</kbd> zoom.
  Search for a **Task recipe** by name (for example, **Review my changes** or **Fix a failing
  test**) to add an editable prompt to the current draft. The same recipes are available on an
  empty conversation. They preserve existing draft content and attachments and never send on their own.
- **File attachments:** paste copied files into the composer, drop them onto the chat, or use **Attach files**. A prompt can carry up to 10 files, 64 MiB each, including PDFs and files without an extension. On send, documents are copied into a new `.dial-attachments-*` directory in the open project. It contains its own Git ignore rule. Dial can read these copies with its normal tools; what it can extract depends on the file format and installed readers. Keep the directory for follow-ups or delete it when it is no longer needed.
- **Appearance** (Settings → Appearance): light, dark or the system's theme, and the zoom level.
  Saved on this computer. A pane over background artwork keeps the dark palette so text stays
  readable on the art.
- **Settings** (button at the bottom of the sidebar, or <kbd>⌘/Ctrl</kbd>+<kbd>,</kbd>): every
  engine setting from Dial's own settings reference (`settings_schema`), grouped in its tabs and
  sections, with search, a "changed" filter, the default beside each value and a per-setting reset.
  Each setting gets a fitting control: switch, menu, number (with bounds and "Auto"), text with
  suggestions, or a structured editor for lists and nested groups. Add and remove fields, choose a value type, then Save or Cancel without editing JSON. Save to every project or to this one; a change
  is written with `set_setting` and applied to the running session (`apply_settings`) at once, or
  when the running turn ends. Settings marked "Restart" take effect the next time Dial starts.
  Credentials are never shown and cannot be edited here.
- **Model library** (Settings, or **Get models…** in the model picker): lists the models already in your Hugging Face cache (honoring `HF_HUB_CACHE` and `HF_HOME`). **Browse Hugging Face** finds MLX and GGUF models; open a result to see its files and sizes, then download the whole MLX model or one GGUF file. Suggested models are pinned to tested revisions. Each download asks for confirmation with its size. If the official Hugging Face helper is missing, Desktop offers to download it and a private Python runtime into its app data, then continues the selected download. **Set up model downloads** also lets you do this after a search error. No terminal, administrator access or restart is needed. You can leave Settings while a model downloads, return to see the outcome, or cancel. Downloading does not select a session model or install an inference runtime.
- **Backgrounds** (Settings → Background): artwork behind your sessions, drawn by the app at the
  window's resolution. Choose still artwork (Warp or Dune), animated artwork
  (Fireflies, Trace Route, Koi Pond, Crab Beach, Cloudprint, or Orbital Earth), or your own image. Over a
  background the composer and cards turn to frosted glass. Choose whether it shows behind every
  session or only a new one (the default), how much a conversation dims it, and whether animated ones move.
  They hold still when the system asks to reduce motion. Saved on this computer, for every project.
  Orbital Earth labels real spacecraft using representative orbital heights and inclinations from the
  [ILRS mission catalog](https://ilrs.gsfc.nasa.gov/missions/satellite_missions/index.html); positions are illustrative, not live.
- **Conversation**: prompts, streamed answers with Markdown and syntax-highlighted code blocks
  (with copy buttons), collapsible reasoning, and compact tool-call rows. Expand a row to see its
  arguments, live output, result and changed files. Images an `image_gen` call writes always show
  inline under its row, including in resumed sessions; click one to show it in its folder. Text is
  selectable; links open in the browser, and links to project files open in the Files panel.
  Images an answer cannot load (remote addresses, unsupported formats) show as a link or label.
  Replies can mix headings, bullet lists, comparison tables, images, and diagrams. Choose **Explain
  with a diagram** in a new chat and add a topic, or ask for a flowchart in a `mermaid` code block.
  When the reply finishes, click the diagram to enlarge it or expand **Diagram source** to copy the text.
  Mermaid `xychart-beta` blocks render as bar and line charts in the same colours.
  Choose **Make it interactive** for an inline card. A `dial-app` code block holds JSON with a
  `title`, an optional `kind`, and optional `followUps` (`[{"label":"Go deeper","ask":"…"}]`).
  Text fields are Markdown, so cards can include code, tables, and diagrams. Kinds:
  - `screens` (the default): branching screens, each with a unique `id`, `text`, and `choices` that
    open another screen (`{"label":"Try again","to":"start"}`) or offer a prompt (`{"label":"…","ask":"…"}`).
  - `guide`: `steps` with a `title`, `text`, and optional `checklist`; ticks are remembered on this computer.
  - `compare`: two to eight `tabs`, each with a `label` and `text`.
  - `calculator`: `inputs` (`slider`, `number`, `select`, or `toggle`, each with an `id`) and `outputs`
    with a `formula` such as `round(params * bits / 8, 1)`. Formulas are plain arithmetic with
    comparisons, `a ? b : c`, and functions like `min`, `max`, `round`, and `sqrt`; an output with an
    `id` can be used by later outputs.
  - `explore`: a Mermaid `diagram` plus `nodes` keyed by node id, each with `text` and an optional
    `ask`; click a node to read about it. Flowchart and state diagram nodes and flowchart subgraphs
    are clickable; give each one an entry.

  Cards are declarative: nothing in them runs as code, and a prompt button only adds text to the
  prompt box. It never sends. You don't have to ask for them: each engine the app starts is told
  (with `--append-system-prompt`) to add a diagram or card when one explains better than prose.
  That guidance is about 450 tokens per request (about 150 with diagrams alone), cached by providers with the rest of the
  system prompt. Settings → Appearance → In answers turns either kind off: the model is no longer
  asked for them, existing blocks show as plain code, and their starter prompts are hidden. The
  change restarts an idle session into itself; a running turn finishes first. Engines older than
  the flag start without the guidance, so cards and diagrams then appear only when asked for.
  Hover a message to copy it (the last answer keeps its copy button in view); the last prompt also
  offers undo, and edit (undo, then restore the draft). Editing the most recent local submission
  restores its attachments when Desktop can identify the original row. For replayed image turns,
  Desktop warns before a text-only edit so you can cancel without losing the turn. A failed turn offers Retry.
  Scrolling up stops following new output, and a button jumps back to the latest.
  Use the search button or <kbd>⌘/Ctrl</kbd>+<kbd>F</kbd> to find matching messages;
  <kbd>Enter</kbd> and <kbd>Shift</kbd>+<kbd>Enter</kbd> cycle through them.
  A ring by the send button shows how full the model's context is; click it (or type `/compact`)
  to summarize the conversation and free context.
- **Composer**: unsent text and images are saved locally per project and session, including across
  restarts. Enter clears the composer immediately and shows “Sending…” while it waits. If Dial rejects a submission or disconnects before confirming it, the draft remains
  available and is never resent automatically. Check the transcript before retrying an unconfirmed
  send. While Dial works, Enter or the send-note button adds guidance to the running turn.
  <kbd>↑</kbd> in an empty box recalls the last prompt.
  During a live turn, **Queue for next turn** (⌘/Ctrl+Enter) saves text and images for the next turn instead. The model picker also holds **Session modes**; permission settings stay beside it.
  Queues are saved locally per project and session, and restart paused. Switching sessions, stopping a
  turn, or a failed turn also pauses automatic sending. Review the transcript if a send may have
  been interrupted, then use **Send next** from the open, idle session.
  Type `@` and part of a filename to insert its project-relative path. Use ↑/↓ to select a match
  and Tab or Enter to insert it. Slash commands include `/model`, `/think`, `/mode`, `/fork`,
  `/retry`, `/new`, `/clear`, `/compact`, `/undo`, `/open`, and the task recipes (e.g. `/fix-test`).
- **Approvals**: allow once, always, for the project, or deny, right above the composer.
- **Questions**: when the agent asks (`ask`), its questions appear above the composer with their
  options (single or multiple choice, the suggested one preselected) and a free-text answer.
  Answer or skip from the app; the engine is started as an interactive client for this.
- **Desktop notifications**: opt in under Settings → Notifications to get an alert for approvals,
  questions, and ended turns while Dial is unfocused. Click it to return to the session while Dial
  is running. Alerts do not show prompts, session names, or project names. macOS uses the installed
  app bundle; Linux needs a notification daemon with action support for click-through.
- **Images**: pasted screenshots and supported image files (PNG, JPEG, GIF, WebP) can be sent as vision inputs. On macOS, the attach menu also offers native clipboard image paste and can convert TIFF screenshots to PNG. Relative Markdown image links in replies preview supported images inside the open project.
- **Side panel** (drag its left edge to resize; the width is remembered per kind of panel):
  - **Plan** and **Changes** are always named. The panel menu opens **Files**, **Terminal**, **Browser**, **App**, and **Activity** (recent tool calls).
  - **App**: Dial defaults to a private Windows guest on Windows and a private Linux guest on Linux. The [packaged QEMU runtime](docs/app-runtime-packaging.md) tries Windows Hypervisor Platform (WHPX) or Linux KVM first; it still needs an imported guest disk. On Apple Silicon, [prepare and import a virtual Mac](packaging/macos-guest/prepare/README.md) to use the native macOS default. You can still explicitly select a different guest, including an existing unlabelled QEMU disk. Dial never downloads an OS or runs the project on the host as a fallback.
    Agent-started guests have no project files. To run a project, select **Share a read-only snapshot of committed HEAD** before starting. With a committed [`.dial/app.json` recipe](packaging/guest/README.md), open the project disc inside the guest and run `python3 -I DIAL-LAUNCH.py .` (Windows: `py -3 -I DIAL-LAUNCH.py .`). The guest needs its own toolchain and dependencies. Hiding the pane leaves it running; **Stop** discards run-local guest changes. The ordinary Terminal is unchanged.
    In the top-right **VM** mode, choose an existing registered VirtualBox or VMware machine instead. Connecting to it uses its real disk and configured devices, not a disposable copy. You can hand the guest console to the agent, take control back by interacting, and **Disconnect** without powering the machine off. See [provider prerequisites](docs/app-runtime-packaging.md#registered-virtual-machines).
  - **Changes**: files Dial touched in this session. Open a file to inspect its current Git diff,
    which may include unrelated edits and is not a per-turn patch. Expand the review view, step
    through files, or use **Ask Dial** beside a hunk to add it to the draft. Both sides’ line numbers
    are shown. You can also copy the diff, open it in Files, or add all listed paths with **Review files**.
    Expanded tool rows link to their files’ diffs, and failed calls can add their error to the draft.
  - **Files**: the project tree (folders first, `.git` hidden) with a filter and a viewer with
    syntax highlighting. Right-click to mention a file in the prompt, copy its path, or show it in
    the system file browser. Reads stay inside the project (symlinks that leave it are refused)
    and stop at 512 KB; binary files are not shown.
    Links such as `src/app.ts#L12-L18` open the cited range without the tree; **Browse files** restores it. Reported edits
    refresh the affected folder and open file, including repeated edits to the same path.
  - **Terminal**: your own login shell in the project folder (a real terminal, xterm.js over a
    PTY), kept running while you switch panels; send its selection or screen to the prompt. The
    "Dial's commands" view lists the commands the agent ran with their output.
  - **Browser**: a real web view in the panel with address bar, back, forward and reload. Bare
    hosts get `https://`, `localhost` gets `http://`, anything else is searched. The send button
    hands the page to Dial: ask it to check the page, or add the address, selected text, page text
    or console errors to the prompt. Pages get no access to the app's commands (the capability
    covers only the `main` web view) and cannot open windows. Local links in the conversation open
    here; ⌘-click opens them in the system browser.
  - **Live previews**: local server announcements such as `Local: http://localhost:5173/` offer a
    preview in the Browser panel without switching panels. To open new previews automatically,
    opt in from the Browser panel or command palette. Session history never opens previews.
  - **The agent's browser**: Dial can drive the Browser panel itself through MCP tools
    (`mcp__dial_desktop__browser_open`, `_snapshot`, `_click`, `_type`, `_read`, `_screenshot`
    and more): open your dev server, click through it, read console errors and look at it. The
    app registers the `mcp.dial_desktop` setting at start (this binary with `--browser-mcp`); set
    its `enabled = false` to turn it off. See `docs/browser-agent.md`.
- **Type boards**: drop PNG, JPEG, GIF or WebP images onto the open board, or use **Add images**.
  Each file becomes a separate item. On macOS, the agent can ask to capture the visible Type
  canvas with `types_screenshot`; the user must open the requested Type first.
- **Reliability**: lost events trigger one `recovery_snapshot` reload once the turn is idle. A
  dropped engine is shown as stopped, and tool calls it left running are marked as having
  unknown outcomes, never as succeeded.

- **Turn review**: turns with reported file changes or explicit checks have an expandable review. Inspect recorded edit hunks and check receipts there; missing patches or checks aren't reconstructed. **Compare current working tree** opens today's Git diff separately.
- **Quick-open** (⌘/Ctrl+P): search project paths without expanding folders; Enter opens the selected file in Files.
- **Open project in new window…** in the palette starts an independent Desktop process without stopping the original project's engines or terminals.
- **Fork before a prompt**: hover a user message in an idle live session and choose the fork action. Desktop verifies the selected occurrence against the saved history and branches only the copy, even when prompts repeat. Files are not rewound.

Not yet: images in question answers, agent screenshots of the browser outside macOS.

## Layout

| Path | Owns |
| --- | --- |
| `src-tauri/src/engine.rs` | Worker thread that owns the `dial_client::Client`; commands in, updates out |
| `src-tauri/src/model.rs` | Pure app state: connection, `dial_client::Conversation`, sessions, toasts, follow-up requests |
| `src-tauri/src/view.rs` | The serializable `Snapshot` the UI renders; every display decision that needs engine knowledge |
| `src-tauri/src/tabs.rs` | One engine per running session: routes opening a session, closes idle background engines |
| `src-tauri/src/settings.rs` | The settings page: the engine's settings reference joined with saved values into typed controls |
| `src-tauri/src/app.rs` | Tauri setup: window, pump thread (`snapshot`, `models`, `settings` and `toast` events), UI commands |
| `src-tauri/src/browser.rs` | The Browser panel's child web view: address normalizing, navigation guard, page inspection script, `browser` events |
| `src-tauri/src/agent_browser.rs` | The agent's browser tools: page scripts, the token-guarded loopback port, the `mcp.dial_desktop` registration, screenshots |
| `src-tauri/src/browser_mcp.rs` | `--browser-mcp`: the stdio MCP server the engine runs, forwarding calls to the app |
| `src-tauri/src/files.rs` | Project files for the Files and Changes panels: listing, reading, `git diff`, all confined to the project |
| `src-tauri/src/terminal.rs` | PTY shells for the Terminal panel (`terminal` events) |
| `src-tauri/src/preview.rs` | Built-in session fed through the real `Conversation` reducer, for design work |
| `src-tauri/src/attachments.rs` | Image previews, native clipboard files and workspace copies for document attachments |
| `src-tauri/src/cli.rs`, `time.rs` | Command-line options; session timestamps and relative labels |
| `src/bridge.ts` | The only UI code that talks to Rust (`invoke`, events, toasts) |
| `src/types.ts` | TypeScript mirror of `view.rs` and `settings.rs` |
| `src/picking.ts` | Picking several sidebar sessions (⌘/Ctrl-click, Shift-click ranges) |
| `src/sessionActions.ts` | What can be done to a session (rename, pin, duplicate, export, undo, delete) and its menu items |
| `src/commands.ts` | The command palette's actions and filter, and the shortcut list |
| `src/appearance.ts` | Theme (light, dark, system) and zoom, saved on this computer |
| `src/workspace.ts` | Side panel widths, live-preview detection, handing files and pages to the prompt |
| `src/backgrounds.ts` | Background artwork painters (still and animated), the saved preference, the user's image |
| `src/markdown.ts` | markdown-it rendering and Shiki highlighting (JavaScript regex engine, no WebAssembly) |
| `src/attachments.ts` | The draft's file attachments (picker, paste, drop) |
| `src/components/` | `Sidebar`, `TopBar`, `Timeline`, `ToolRow`, `Composer` (+ approvals, attachments, `ContextMeter`), `QuestionCard`, `Inspector` (+ `workspace/BrowserPane`, `FilesPane`, `DiffPane`, `TerminalPane`), `Settings` (+ `SettingRow`, `AppearanceSettings`, `BackgroundSettings`), `ContextMenu`, `Palette`, `Shortcuts`, `EmptyState`, `Toasts` |
| `src/styles.css` | Design tokens (palette, spacing, radii, type) and shared styles |

The data flow is one-way: engine updates are folded into the `Model` on the Rust side, turned into a
`Snapshot` (coalesced to about one per frame while text streams), and rendered by Vue. Only what
changed since the last published snapshot is sent: changed fields and the changed end of the
transcript (`view::Published`); the UI applies it, and reloads the whole snapshot if it ever
misses one. While an answer streams, its unfinished last code block is highlighted once it is done.
The UI keeps only its own state (draft, filter, expanded rows, inspector tab) and sends requests back
through Tauri commands.

Rust unit tests live next to each module in `<module>/tests.rs`:

```sh
cargo test --manifest-path src-tauri/Cargo.toml
npm run typecheck
```

## Design system

The look follows Claude's desktop app and Claude Code. The palette is warm charcoal with one
clay accent (`#D97757`), used sparingly: the logo, the send button and the working indicator.
Chrome is minimal: hairline borders instead of boxes, and no glows. The transcript reads like a
terminal agent's, with `●` tool lines, `⎿` results and a `✻ Working…` line. The side panel (plan,
changes, activity, files, terminal, browser) is hidden until it is toggled from the top bar.

Every colour, spacing step, radius and text size is a CSS custom property in `src/styles.css`,
with a dark and a light set of colours; code highlighting carries both palettes.
Icons are inline SVG, so they look the same with any font. On macOS the window uses an overlay
title bar; the top bar, sidebar header and inspector header are drag regions.

## Developing against a local Dial

`dial-client` is pinned to a Dial commit in `src-tauri/Cargo.toml`. To build against a local
checkout instead, add this to `src-tauri/Cargo.toml` (and do not commit it):

```toml
[patch."https://github.com/KiidxAtlas/dial.git"]
dial-client = { path = "../../dial-engine/crates/dial-client" }
```

When the desktop app needs a capability the protocol lacks, add the RPC method in Dial first.
That means its engine owner, a `docs/rpc-schema.json` entry and parity classification. Then
bump `rev` here.
