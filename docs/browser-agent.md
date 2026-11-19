# The agent drives the Browser panel

Dial (the agent) can open pages in Dial Desktop's Browser panel, read them, click, type and take
screenshots, in the same web view the user watches. This note explains how, and why it is built
this way.

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

- **Registration** (`agent_browser::register`, called from `main.rs` before any engine starts):
  the app makes sure Dial's global setting `mcp.dial_desktop` runs this very binary with
  `--browser-mcp`. It reads the entry with `dial config get mcp.dial_desktop --json` and writes it
  with `dial config set` only when the program path changed. An entry you set to
  `enabled = false` stays off. Preview mode (`--preview`) does not register.
- **The bridge** (`browser_mcp.rs`) is a stdio MCP server (one JSON-RPC message per line):
  `initialize`, `ping`, `tools/list`, `tools/call`. It holds no browser. Each call goes to the
  running app. With no app running it lists no tools, so Dial sessions in a terminal see nothing.
- **The app side** (`agent_browser.rs`) listens on a random loopback port and writes
  `{port, token, pid}` to `browser-endpoint.json` in the app's folder
  (`~/Library/Application Support/dev.dial.desktop/` on macOS), mode `0600`. Each request carries
  the 256-bit token; anything else is refused. The file is removed when the app quits.
- **The tools** run small scripts in the page through `eval_with_callback`, so they work with each
  platform's system web view; nothing needs the Chrome DevTools Protocol, which WKWebView and
  WebKitGTK lack. Screenshots use WebKit's own `takeSnapshot` (macOS only for now).

Why a stdio bridge and not a URL server: the engine clears the environment of MCP children and
offers no per-launch config overlay, so a URL server would need its port and token written into
the user's config on every start. The bridge's entry never changes (only the binary path), and
the secret stays in a private file.

## Tools

The engine offers them as `mcp__dial_desktop__<name>`.

| Tool | Does |
| --- | --- |
| `browser_open(url)` | Show the panel, load a page (address, `localhost:3000`, bare host or search), wait for it, return a snapshot |
| `browser_snapshot()` | Headings and visible interactive elements with refs like `[3]` |
| `browser_read()` | Address, title, selected text, visible text, console errors |
| `browser_click(ref \| selector \| name)` | Click (pointer and mouse events, then `click`); waits for a load it starts |
| `browser_type(target, text, clear?, submit?)` | Fill a field (works with React/Vue inputs) or editable element |
| `browser_select(target, option)` | Choose a `<select>` option by value or text |
| `browser_press(key)` | Key events in the focused element; Enter submits a form |
| `browser_scroll(direction \| target)` | Scroll up, down, top, bottom, or to an element |
| `browser_navigate(action)` | Back, forward, reload |
| `browser_wait(text?, timeout_ms?)` | Wait for text to show (up to 30 s) |
| `browser_eval(script)` | Run a function body (async; `await` works) and return JSON |
| `browser_screenshot()` | PNG of the panel, returned as an MCP image block |

Reads, waits, scrolls and screenshots declare `readOnlyHint`, and the entry sets
`trust_read_only_hint`, so they run without approval. Everything else follows the session's
approval mode.

## What the user sees

The panel opens on `browser_open` and shows each step. A "Dial: Clicking [4]" line sits above
the page while a tool runs (`agent-browser` events, `workspace.agentActivity`). The user can take
over at any time; it is one shared browser.

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
- Screenshots on Windows (WebView2 `CapturePreview`) and Linux.
