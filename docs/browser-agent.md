# The agent drives the Browser panel

Dial (the agent) can open pages in Dial Desktop's Browser panel, read them, click, type and take
screenshots, in the same web view the user watches. When the panel tools are available, this is
the default for ordinary browsing, including local dev servers. Ask for standalone Chromium when
you want a separate session or need a Playwright feature such as tracing.

## How it fits together

```
dial rpc (engine) ──stdio MCP──▶ dial-desktop --browser-mcp (bridge)
                                     │  one JSON line per call, with a token
                                     ▼
                          Dial Desktop app, 127.0.0.1:<random port>
                                     │  eval_with_callback / WKWebView snapshot
                                     ▼
                              Browser panel web view
```

- **Registration** (`agent_browser::register`): before starting a live engine, and again when
  installing or selecting one for new connections, Desktop checks the global MCP table with
  `dial config get mcp --json` in an empty project directory. It only sets `mcp.dial_desktop` if
  missing or pointing at the wrong bridge. Failed reads do not cause writes. `enabled = false`
  stays off, and per-project overrides still apply when a session starts. Preview mode does not
  register, and switching engines does not restart running work.
- **The bridge** (`browser_mcp.rs`) is a stdio MCP server (one JSON-RPC message per line):
  `initialize`, `ping`, `tools/list`, `tools/call`. It holds no browser. Each call goes to the
  running app. With no app running it lists no tools, so Dial sessions in a terminal see nothing.
- **Tabs** (the UI's `browser_tab` command and the agent's `browser_tab` tool): each tab is its own webview labelled `browser-<id>`. Only the active tab is shown; the others keep their pages loaded. A link that opens a new window becomes a new tab. Each tab keeps its own local-file server (**Open file…**), stopped when that tab leaves its page. The `browser` event carries `{url, title, loading, activeTab, tabs: [{id, url, title}]}`; top-level fields describe the active tab.
- **The app side** (`agent_browser.rs`) listens on a random loopback port and writes
  `{port, token, pid}` to `browser-endpoints/<pid>.json` in the app's folder
  (`~/Library/Application Support/dev.dial.desktop/` on macOS), mode `0600`. Each request carries
  the 256-bit token; anything else is refused. The file is removed when the app quits.
  The bridge matches its project directory to one live window; zero or ambiguous matches are not routed. The app checks the project again on every authenticated call.
- **The tools** run small scripts in the page through `eval_with_callback`, so they work with each
  platform's system web view; nothing needs the Chrome DevTools Protocol, which WKWebView and
  WebKitGTK lack. Browser panel screenshots use WebKit's `takeSnapshot` on macOS and WebView2's
  `CapturePreview` on Windows. Both capture only the page, not the surrounding Desktop window.
  Browser, window and Type screenshots use the same output budget: oversized captures are downsampled before base64 encoding to fit both the model's 5 MiB image limit and the engine's MCP message limit. Sources over 64 MiB are refused.

Why a stdio bridge and not a URL server: the engine clears the environment of MCP children and
offers no per-launch config overlay, so a URL server would need its port and token written into
the user's config on every start. The bridge's entry never changes (only the binary path), and
the secret stays in a private file.

## Tools

The engine offers them as `mcp__dial_desktop__<name>`.

| Tool | Does |
| --- | --- |
| `browser_open(url)` | Load a page in the panel's active tab (address, `localhost:3000`, bare host or search), wait until that exact address has finished loading there, and return a snapshot. Fails instead of reading an older page when the panel does not navigate. |
| `browser_snapshot()` | Headings and visible interactive elements with refs like `[3]` |
| `browser_read()` | Address, title, selected text, visible text, console errors |
| `browser_click(ref \| selector \| name)` | Click (pointer and mouse events, then `click`); waits for a load it starts |
| `browser_type(target, text, clear?, submit?)` | Fill a field (works with React/Vue inputs) or editable element |
| `browser_select(target, option)` | Choose a `<select>` option by value or text |
| `browser_press(key)` | Page keyboard events, with in-page Tab/Shift+Tab, typing/deletion and left/right caret movement in plain text controls, up/down in single selects, and Enter/Space activation on applicable controls. Escape and keys without an implemented default send events only; native browser/OS shortcuts are not supported. |
| `browser_scroll(direction \| target)` | Scroll up, down, top, bottom, or to an element |
| `browser_navigate(action)` | Back, forward, reload |
| `browser_tab(action, id?)` | `list` the tabs (active one marked), `new` a blank tab, `select` one by id, or `close` one by id. The other tools act on the active tab. Up to 16 tabs; closing the last leaves a blank one. |
| `browser_wait(text?, timeout_ms?)` | Wait for text to show (up to 30 s) |
| `browser_eval(script)` | Run a function body (async; `await` works) and return JSON |
| `browser_screenshot()` | PNG of the panel, returned as an MCP image block |
| `app_desktop(action, runId?, frameSeq?, token?, actions?)` | Launch graphical apps in a configured private guest or a connected VM. `state` checks readiness; `start` opens the guest in the App panel; `frame` returns the screen and a token; `input` runs a batch of up to 64 actions (`click` with `count`, `double_click`, `move`, `drag`, `scroll`, `press` with combos like `ctrl+c`, `type` with whole strings, `wait`) on that frame with one token and replies with the screen after it, plus the next token. Agent access is the user's switch in the App panel. |
| `desktop_view(screenshot?)` | What the Dial Desktop window shows, as JSON (project, session, model, approval mode, pending approvals, plan, changes, sessions, open panels and settings page, appearance, notices); `screenshot` adds a PNG of the window (macOS) |
| `desktop_commands(query?)` | Every command-palette action available now, with ids; filtered like the palette |
| `desktop_run(id)` | Run one palette action as if the user picked it; the user sees a "Dial: …" notice |
| `desktop_settings(query?, tab?, changed?)` | Dial's settings as the Settings page shows them; credentials hidden |
| `desktop_set_setting(key, value \| reset, scope?)` | Change or reset one setting (default scope: the layer that sets it, else `project`) |

Reads, waits, scrolls and screenshots declare `readOnlyHint`, and the entry sets
`trust_read_only_hint`, so they run without approval. Everything else follows the session's
approval mode.

### The app itself (`desktop_*`)

The agent sees and works the app through the same lists the user does, so a new feature needs no
separate agent wiring:

- **Actions** are the command palette's (`src/commands.ts`), with the same availability rules.
  `src/agentDesktop.ts` answers `window.__dialDesktop.call(...)`; the backend
  (`agent_browser/desktop.rs`) runs that one fixed call in the main webview with JSON arguments,
  never agent code. **To make a feature agent-controllable, give it a palette command.**
- **Settings** come from the engine's settings reference, as on the Settings page.
- **The user's alone:** approvals, Agent access, VM grants and provider sign-in are not palette
  commands and are not reachable. Settings that govern the agent (`approval`, `sandbox`, `shell`,
  `escalation`, `mcp`, `hooks`, `tools`, `providers`, `keys`, `secrets`, browser and image
  `allow_*`, and any key naming a token, password or API key) are readable but refused by
  `desktop_set_setting` (`desktop::protected`). Redacted values are never shown.

## What the user sees

The panel opens on `browser_open` and shows each step. A "Dial: Clicking [4]" line sits above
the page while a tool runs (`agent-browser` events, `workspace.agentActivity`). The user can take
over at any time; it is one shared browser.

The panel's own controls, beside the address bar:

- **Recent pages** lists the pages shown in this panel session; pick one to open it again.
- **⋯** opens a menu with **Open file…**, the viewport and the search engine:
  - **Open file…** picks a local HTML file in a native dialog. The webview still opens only web
    pages, so Dial serves that file's folder on `127.0.0.1` at a random port under a random path
    prefix (`browser/local.rs`): `GET` only, a `Host` check, nothing outside the folder (no `..`,
    no links that lead out). The server stops when the panel opens another address or file, or
    closes.
  - **Viewport**: Fit panel, Phone 390×844, Tablet 820×1180 or Desktop 1280×800. A preset sizes the
    page inside a frame, shrinking to the panel when it is smaller; the choice is remembered.
  - **Search engine**: DuckDuckGo (default), Google, Bing, Brave Search, Kagi, or a custom
    `https://…%s` address. It applies to what the user types in the address bar
    (`browser_set_search`) and is kept in the frontend's local storage; the agent's `browser_open`
    still searches DuckDuckGo.
- **Inspect** opens Web Inspector for the page. Only builds compiled with it (debug builds) show
  the button.

The page is a native view drawn above the app, so it hides while a menu, the palette or settings
is open; the panel says so in its place (no snapshot is drawn).

## Native apps in the private guest

When asked to launch a graphical app, the agent checks the private guest first; the user need not
name the panel each time. If setup or project files are missing, explain what is needed. Do not
download an image or fall back to the host. This guidance does not redirect ordinary shell commands.

- **Start** (`app_desktop` `start`) opens a fresh guest with no network and no shares. The agent
  holds Agent access from the first frame, and the App panel opens at once so the user sees it. The
  agent cannot start a second run while one is open. Only the user's **Agent access** switch gives
  or removes access; while it is off the agent gets no frame and no input.
- **Input batches:** one `input` call carries a whole step (click a field, type a command with
  `\n`, wait, press a combo). The batch is checked before anything is sent, runs in order, and
  stops early if the user acts in the guest, turns access off, or the screen changes size; the
  reply says how many actions ran. A token survives screen redraws until the next frame, a resize
  or two minutes. `type` uses a US keyboard layout (printable ASCII, `\n`, `\t`).
- **Project files** are never shared by an agent start. The user's own start in the App panel can
  attach a read-only snapshot of committed HEAD (the share option), and only for that start.
- **Guest target:** Omit `guestTarget` on `state` or `start` to use Windows on Windows, Linux on Linux, or a virtual Mac on Apple Silicon. Other hosts prefer the QEMU disk with no OS label. An explicit `default` always requests that disk with no OS label. `macos`, `linux`, and `windows` select only that target. A virtual Mac requires its own imported base and guest companion. Never substitute another guest or the host. Windows guest boot is unverified.
- **Project recipe:** With an explicitly shared project containing a committed `.dial/app.json`, run `python3 -I DIAL-LAUNCH.py .` from the mounted project disc **inside the guest** (`py -3 -I DIAL-LAUNCH.py .` on Windows). A failure is not permission to download dependencies or run it on the host.

### Using an existing VirtualBox or VMware VM

Use `app_desktop` with `action: "state", guestTarget: "vm"` when asked to use an existing machine. The agent does not get the host's VM inventory. In **App → VM**, the user chooses a machine and can either connect personally and hand off control, or grant one agent launch. For the latter, send `action: "start", guestTarget: "vm", vmId: approvedVmId` using the ID returned by `state`. A grant for one project or machine does not authorize another. Only the local app UI can grant permission.

Use the returned run ID with the usual `frame` and `input` actions. A VM already running is attached as it is, not restarted. No agent frame or input is allowed while Agent access is off. A user gesture voids the agent's outstanding token and stops a running batch, but does not change access. **Disconnect** releases the console; it does **not** stop the VM or revert disk writes. **Shut down** (user only, after a confirmation) disconnects and asks the guest OS to shut down gracefully (VirtualBox ACPI power button, VMware soft stop); it never forces power off. The VM's own network, clipboard and shared-folder settings still apply. Dial does not install a guest agent or add a host share.

## Safety

- Loopback only, a random token per run, compared in constant time; the endpoint file is
  readable only by the user.
- The panel keeps its navigation guard (http, https, about, blob and data only; no new windows),
  and pages still get no access to the app's commands.
- Scripts run in the page, never in the app's own web view.

## Open questions

- Should the agent's browsing get its own approval rule (for example, ask before leaving
  localhost)?
- Separate cookie stores for agent sessions?
- Browser panel screenshots on Linux.
