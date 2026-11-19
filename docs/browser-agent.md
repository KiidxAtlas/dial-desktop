# Phase 2: the agent drives the in-app browser

Status: design only. Phase 1 (shipped) is the Browser panel: you browse, and hand a page's address,
selection, text or console errors to Dial. This note plans how the agent itself could navigate,
read and click in that same panel.

## What the engine has today (read from Dial `f5bf3d7`)

- A `browser` tool (`dial-tools/src/tools/net/browser.rs`) that drives **Chrome** through the
  **browser relay** (`dial-tools/src/browser_relay/`).
- The relay listens on loopback (default `http://127.0.0.1:9224`), and every request carries a
  token (`x-dial-relay-token` header, or `token` query on WebSockets).
- Chrome's extension connects on `/ext` and speaks protocol version 1 (`protocol.rs`): it sends
  `hello` (with its tabs), `cdpEvent`, `detached`, `tabCreated`/`tabUpdated`/`tabRemoved` and
  `rpcResult`, and receives RPC frames that carry **Chrome DevTools Protocol** commands, which it
  runs with `chrome.debugger`. CDP clients (Playwright) connect on `/cdp`.
- MCP servers can be added under the `mcp` setting, as stdio programs or `url` servers (streamable
  HTTP; plain `http` is accepted on localhost; a bearer token can come from an environment
  variable named by `bearer_env`).
- `dial rpc` has no per-launch config overlay (the CLI notes `--config` is not implemented), so a
  per-window server would have to be saved as a setting.

## The obstacle for the relay route

The relay expects a peer that executes CDP. The panel is a system web view: WKWebView on macOS
and WebKitGTK on Linux have no CDP; only WebView2 on Windows can expose one. Pretending to be the
extension would mean emulating enough CDP (`Page.navigate`, `Runtime.evaluate`, `Input.*`,
`Page.captureScreenshot`, DOM and accessibility queries, target and session events) for the
`browser` tool and Playwright, on top of `eval`. That is large, and it breaks whenever the tool
uses a method we did not emulate.

## Recommended route: the app serves a small MCP server

The app hosts a streamable-HTTP MCP server on `127.0.0.1` (random port, random bearer token)
whose tools act on the panel through what `browser.rs` already does:

| Tool | Does | Built on |
| --- | --- | --- |
| `app_browser_open(url)` | Show the panel and load a page | `browser_show` / `navigate` |
| `app_browser_read()` | URL, title, selection, visible text, console errors | `INSPECT_SCRIPT` (`PageInfo`) |
| `app_browser_snapshot()` | Interactive elements with stable ids (role, name, box) | a new inspection script |
| `app_browser_click(id)` / `type(id, text)` / `press(key)` | Act on an element from the snapshot | `eval` of a small script |
| `app_browser_screenshot()` | PNG of the panel | `WKWebView takeSnapshot` (and WebView2 `CapturePreview`) via `with_webview` |
| `app_browser_wait(text or ms)` | Wait for a page change | page-load events and polling |

Why this route:

- It works on every platform, with no CDP.
- The tools are ordinary MCP tools, so the engine's approvals apply. Navigation and clicks
  should not declare `readOnlyHint`; reads may.
- Everything the agent does is visible in the panel, and the user can take over at any time.

### Wiring it up

1. Start the server with the app, and keep the token in memory only.
2. Register it once as an `mcp` entry (`url = "http://127.0.0.1:<port>/mcp"`,
   `bearer_env = "DIAL_DESKTOP_BROWSER_TOKEN"`), and pass the token to each `dial rpc` the app
   launches as that environment variable (`dial_client::Launch::env`). The port changes each
   run, so the setting is rewritten at start. Alternatively, ask Dial for a per-launch MCP overlay
   and avoid the setting entirely.
3. If the server is down, the engine reports the server as unavailable. Nothing else breaks.

### Safety

- Listen on loopback only and require the bearer token. Reject requests whose `Origin` is a web
  page, because the panel's own pages must not reach the server.
- The panel keeps today's guard: http, https, about, blob and data URLs only, and no new windows.
- Scripts run in the page's world only, never in the app's web view.
- A visible "Dial is using the browser" bar with a Stop button while a tool runs.

### Open questions

- Should the agent's browsing get its own approval mode (for example, ask before leaving
  localhost)?
- Should agent sessions share cookies with the user's panel, or use a separate data store per
  session?
