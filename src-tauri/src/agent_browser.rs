//! The agent's hands on the Browser panel.
//!
//! Dial's engine reaches the panel through an MCP server: this app's own binary started with
//! [`BRIDGE_FLAG`] (see `browser_mcp`), which the engine runs from the `mcp.dial_desktop` setting
//! that [`register`] keeps current. The bridge forwards each tool call over a loopback TCP port
//! this module serves ([`start`]): one JSON line in ([`Request`], carrying a random token), one JSON
//! line out ([`Reply`]). The port and token sit in a file only this user can read
//! ([`endpoint_file`]); the engine does not pass custom environment variables to MCP servers.
//!
//! Every action is a small script run in the page (`eval_with_callback`), so it works with each
//! platform's system webview; nothing needs the Chrome DevTools Protocol. The user sees each step:
//! the panel opens on `browser_open` and shows what the agent does (`agent-browser` events).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::browser::{self, Browser};

#[cfg(target_os = "macos")]
mod screenshot;
#[cfg(windows)]
mod screenshot_windows;
#[cfg(any(target_os = "macos", windows, test))]
mod screenshot_wire;
mod desktop;
#[cfg(test)]
mod tests;

/// The engine's name for the server: tools appear as `mcp__dial_desktop__browser_open` and so on.
pub const SERVER_NAME: &str = "dial_desktop";
/// Starts this binary as the MCP bridge instead of the app.
pub const BRIDGE_FLAG: &str = "--browser-mcp";
/// Asks whether the app is there; answered without touching the page.
pub const PING: &str = "__ping";

/// How long one script may take to answer.
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a navigation may take before the tool answers anyway.
const LOAD_TIMEOUT: Duration = Duration::from_secs(20);
/// Longest wait `browser_wait` accepts.
const WAIT_LIMIT: Duration = Duration::from_secs(30);
/// Interactive elements a snapshot lists.
const SNAPSHOT_LIMIT: usize = 250;
/// Characters of page text `browser_read` returns.
const TEXT_LIMIT: usize = 20_000;
/// Largest request line accepted.
const MAX_REQUEST: u64 = 1 << 20;

/// Where the bridge finds the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

/// One tool call from the bridge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub token: String,
    pub workspace: String,
    pub tool: String,
    #[serde(default)]
    pub arguments: Value,
}

/// A tool's answer: text, and maybe a PNG (base64).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

impl Reply {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            ok: true,
            text: text.into(),
            image: None,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            ok: false,
            text: text.into(),
            image: None,
        }
    }
}

/// The endpoint file in the app's per-user folder.
pub fn endpoint_file(home: Option<&Path>) -> Option<PathBuf> {
    crate::launcher::app_dir(home).map(|dir| {
        dir.join("browser-endpoints")
            .join(format!("{}.json", std::process::id()))
    })
}

/// What the tools say about themselves, for the bridge's `initialize`.
pub const INSTRUCTIONS: &str = "These tools drive the Browser panel of Dial Desktop, a real \
browser the user watches beside this conversation. When these tools are available, use the Browser \
panel by default for opening, inspecting and interacting with pages, including local dev servers; \
the user need not name the panel. Keep follow-up actions in the same panel. Use standalone Chromium \
when the user requests it or the task needs a capability the panel lacks, such as Playwright tracing. \
browser_open loads a page and lists its interactive elements with refs like [3]; \
browser_click, browser_type and browser_select act on a ref; browser_read returns the page text \
and console errors; browser_screenshot shows how it looks. Take a new snapshot after the page \
changes. For a request to launch a graphical app, use app_desktop when the guest is configured; \
the user need not name the App panel. Check state first. When app_desktop state reports ssh for a \
VM, run shell commands and read files in that VM with the ssh tool (host ssh.host) instead of \
typing into its screen; keep frame and input for graphical apps. If setup or project files are \
missing, explain what is needed. Never download a guest or launch the app on the host as a \
fallback. Ordinary non-GUI build and test commands are unchanged. The types_* tools keep the \
user's Types: things they keep (recipes, cities, books, scores) that Dial Desktop opens as small \
web apps you build for them: a cookbook, a live weather app, a bookshelf, a game. When the user \
wants to keep something the conversation produced, check types_list and save it with \
types_add_item, or offer types_create for a fitting new Type. Read types_guide before building \
an app, and look at every change with types_screenshot and types_run. The desktop_* tools are \
Dial Desktop itself: desktop_view shows what the window shows (add screenshot for a picture), \
desktop_commands lists every palette action (sessions, panels, settings pages, theme, zoom, \
projects) and desktop_run runs one; desktop_settings reads Dial's settings and \
desktop_set_setting changes one, except permission and credential settings, which stay the \
user's. Use them when the user asks you to show, open, switch or change something in the app. \
The board_* tools work with microcontroller boards and other serial devices through the Boards \
panel: board_list shows the devices and this folder's firmware project, board_connect opens the \
console the user watches, board_read and board_send talk to the device, board_build and \
board_flash run the project's build and upload commands (the console's port is freed and reopened \
for you), and board_run and board_put run and save code on MicroPython boards. Edit the firmware \
source with your ordinary file tools, flash it, then read the console to check that it works.";

/// The MCP tool list.
pub fn tools() -> Value {
    let target = json!({
        "ref": {"type": "integer", "description": "An element ref from the latest snapshot, like 3 for [3]"},
        "selector": {"type": "string", "description": "A CSS selector, when there is no ref"},
        "name": {"type": "string", "description": "The element's visible name or label, when there is no ref"}
    });
    let with_target = |extra: Value, required: &[&str]| {
        let mut properties = target.clone();
        if let (Some(map), Value::Object(more)) = (properties.as_object_mut(), extra) {
            map.extend(more);
        }
        json!({"type": "object", "properties": properties, "required": required})
    };
    let tool = |name: &str, title: &str, read_only: bool, description: &str, schema: Value| {
        json!({
            "name": name,
            "title": title,
            "description": description,
            "inputSchema": schema,
            "annotations": {"title": title, "readOnlyHint": read_only, "openWorldHint": true}
        })
    };
    let none = json!({"type": "object", "properties": {}});
    let mut list = json!([
        tool(
            "browser_open",
            "Open a page",
            false,
            "Open a page in the visible Dial Desktop Browser panel (the default for browsing in Desktop) \
             and wait for it to load. Takes a full URL, `localhost:3000`, a bare host, or words to search. Returns the page's title and its \
             interactive elements with refs (like browser_snapshot).",
            json!({"type": "object", "properties": {"url": {"type": "string", "description": "Address or search words"}}, "required": ["url"]}),
        ),
        tool(
            "browser_snapshot",
            "List the page's elements",
            true,
            "List the page's headings and visible interactive elements (links, buttons, fields) with \
             refs like [3] for browser_click, browser_type and browser_select. Refs last until the \
             next snapshot or page load.",
            none.clone(),
        ),
        tool(
            "browser_read",
            "Read the page",
            true,
            "The page's address, title, selected text, visible text, and the errors it logged to \
             the console.",
            none.clone(),
        ),
        tool(
            "browser_click",
            "Click",
            false,
            "Click an element: a ref from the snapshot, or a CSS selector, or a visible name. \
             Waits for a page load the click starts.",
            with_target(json!({}), &[]),
        ),
        tool(
            "browser_type",
            "Type",
            false,
            "Type text into a field (input, text area or editable element). Replaces what it holds \
             unless `clear` is false; `submit` presses Enter afterwards.",
            with_target(
                json!({
                    "text": {"type": "string"},
                    "clear": {"type": "boolean", "description": "Replace the field's text (default true)"},
                    "submit": {"type": "boolean", "description": "Press Enter afterwards (default false)"}
                }),
                &["text"],
            ),
        ),
        tool(
            "browser_select",
            "Choose an option",
            false,
            "Choose an option of a <select> by its value or text.",
            with_target(json!({"option": {"type": "string"}}), &["option"]),
        ),
        tool(
            "browser_press",
            "Press a key",
            false,
            "Send a page key to the focused element. Plain text fields support typing, deletion and \
             left/right caret movement; Tab/Shift+Tab focus page controls, select arrows choose an \
             option, Enter activates or requests submission. Other page keys (e.g. Escape) dispatch \
             events only; native browser shortcuts are not supported.",
            json!({"type": "object", "properties": {"key": {"type": "string"}}, "required": ["key"]}),
        ),
        tool(
            "browser_scroll",
            "Scroll",
            true,
            "Scroll the page `up`, `down`, to the `top` or `bottom`, or bring an element (ref, \
             selector or name) into view.",
            with_target(
                json!({"direction": {"type": "string", "enum": ["up", "down", "top", "bottom"]}}),
                &[],
            ),
        ),
        tool(
            "browser_navigate",
            "Back, forward or reload",
            false,
            "Go `back`, `forward`, or `reload` the page.",
            json!({"type": "object", "properties": {"action": {"type": "string", "enum": ["back", "forward", "reload"]}}, "required": ["action"]}),
        ),
        tool(
            "browser_wait",
            "Wait",
            true,
            "Wait until `text` shows on the page (up to `timeout_ms`, default 10000), or for \
             `timeout_ms` when no text is given.",
            json!({"type": "object", "properties": {"text": {"type": "string"}, "timeout_ms": {"type": "integer"}}}),
        ),
        tool(
            "browser_eval",
            "Run a script",
            false,
            "Run JavaScript in the page and return its result as JSON. `script` is a function body: \
             use `return`. A returned promise is awaited (up to 10 s).",
            json!({"type": "object", "properties": {"script": {"type": "string"}}, "required": ["script"]}),
        ),
        tool(
            "browser_screenshot",
            "Screenshot",
            true,
            "A PNG of what the Browser panel shows (macOS and Windows).",
            none,
        ),
        tool(
            "app_desktop",
            "App desktop",
            false,
            "Use the project's guest for explicit graphical app launches. Never use the host. For a registered VirtualBox or VMware VM, use guestTarget vm; state reports the user's one-time approvedVmId or current run (a running VM is attached, not restarted). Start only the approved VM. An installed VM is persistent and keeps its provider sharing settings. When state for guestTarget vm includes ssh {host, source}, the user linked that SSH host to the VM: run commands, read logs and check results with the ssh tool (host = ssh.host; read/grep also take ssh://host/path) instead of typing in a terminal and taking screenshots, and use frame/input only for graphical apps. ssh is reported only while Agent access is on or for the approved VM. Workflow: state -> frame {runId} -> input {runId, frameSeq, token, actions}. Put a whole step in one input: actions run in order (up to 64), e.g. [{kind:click,x,y},{kind:type,text:\"root\\n\"},{kind:wait,ms:800},{kind:press,keys:\"ctrl+l\"}]. type sends whole strings (US layout ASCII, \\n is Enter, \\t is Tab); press takes combos like \"ctrl+c\", \"alt+F4\", \"Enter\" with optional repeat. Coordinates are pixels of the frame you were given. Every input reply contains the screen after the batch with the next frameSeq and token, so chain from it; call frame only when told to. A token survives redraws until the next frame, a resize or 2 minutes. A batch stops early if the user acts in the app; the reply says how many actions ran. Frame and input need Agent access, which only the user switches on or off in the App sidebar; if it is off, ask the user to turn it on.",
            json!({"type":"object", "required":["action"], "additionalProperties":false, "properties":{
                "action":{"enum":["state","start","frame","input"], "description":"start opens a configured guest; it does not install one"},
                "guestTarget":{"enum":["default","linux","windows","macos","vm"], "description":"Omit for the private guest preference. default explicitly selects the unlabelled QEMU disk; vm selects the user-approved installed VM."},
                "vmId":{"type":"string", "description":"Only with start and guestTarget vm: exact approvedVmId from state; never a path or VM name."},
                "runId":{"type":"string", "description":"From state; required for frame and input"},
                "frameSeq":{"type":"integer", "description":"From the last frame or input reply; required for input"},
                "token":{"type":"string", "description":"Single-use token from the last frame or input reply; required for input. One token covers the whole actions list."},
                "actions":{"type":"array", "minItems":1, "maxItems":64, "description":"Input steps run in order on that frame; required for input", "items": app_desktop_action_items()}
            }}),
        ),
    ]);
    if let Value::Array(items) = &mut list {
        items.extend(desktop::tools());
        items.extend(crate::types::tools());
        items.extend(crate::boards::agent_tools());
    }
    list
}

/// The `actions` items of `app_desktop` (see `app_runner::actions::Action`). Built apart from the
/// tool schema, which would otherwise exceed `json!`'s macro recursion limit.
fn app_desktop_action_items() -> Value {
    let point = || json!({"type":"integer","minimum":0});
    let button = json!({"enum":["left","middle","right"]});
    json!({"oneOf": [
        json!({"type":"object","required":["kind","x","y"],"additionalProperties":false,"properties":{
            "kind":{"const":"click"},"x":point(),"y":point(),"button":button,
            "count":{"type":"integer","minimum":1,"maximum":3,"description":"2 double-clicks, 3 triple-clicks"}}}),
        json!({"type":"object","required":["kind","x","y"],"additionalProperties":false,"properties":{
            "kind":{"enum":["double_click","move"]},"x":point(),"y":point()}}),
        json!({"type":"object","required":["kind","x","y","toX","toY"],"additionalProperties":false,"properties":{
            "kind":{"const":"drag"},"x":point(),"y":point(),"toX":point(),"toY":point(),"button":button}}),
        json!({"type":"object","required":["kind","x","y","dy"],"additionalProperties":false,"properties":{
            "kind":{"const":"scroll"},"x":point(),"y":point(),
            "dy":{"type":"integer","minimum":-100,"maximum":100,"description":"Wheel notches; positive scrolls down"}}}),
        json!({"type":"object","required":["kind","keys"],"additionalProperties":false,"properties":{
            "kind":{"const":"press"},
            "keys":{"type":"string","description":"A key or combo: Enter, Escape, Tab, Backspace, Up, PageDown, F5, a, ctrl+c, ctrl+shift+t, alt+F4, cmd+space"},
            "repeat":{"type":"integer","minimum":1,"maximum":50}}}),
        json!({"type":"object","required":["kind","text"],"additionalProperties":false,"properties":{
            "kind":{"const":"type"},
            "text":{"type":"string","maxLength":4000,"description":"Typed as keystrokes (US layout, printable ASCII; \\n Enter, \\t Tab)"}}}),
        json!({"type":"object","required":["kind","ms"],"additionalProperties":false,"properties":{
            "kind":{"const":"wait"},"ms":{"type":"integer","minimum":0,"maximum":5000}}}),
    ]})
}

/// Shared helpers every action script installs once per page (`window.__dialAgent`).
const HELPERS: &str = r#"(() => {
  const A = window.__dialAgent || (window.__dialAgent = { refs: [], done: {} });
  if (A.ready) return;
  A.ready = true;
  A.SELECTOR = 'a[href],button,input:not([type=hidden]),select,textarea,summary,[role=button],[role=link],[role=checkbox],[role=radio],[role=tab],[role=menuitem],[role=option],[role=switch],[role=combobox],[role=textbox],[role=searchbox],[contenteditable=""],[contenteditable=true],[onclick],[tabindex]:not([tabindex="-1"])';
  A.clean = (s, n) => String(s == null ? '' : s).replace(/\s+/g, ' ').trim().slice(0, n || 80);
  A.visible = (el) => {
    const r = el.getBoundingClientRect();
    if (r.width < 1 || r.height < 1) return false;
    const s = getComputedStyle(el);
    return s.visibility !== 'hidden' && s.display !== 'none';
  };
  A.name = (el) => A.clean(
    el.getAttribute('aria-label') ||
    (el.labels && el.labels[0] && el.labels[0].innerText) ||
    el.getAttribute('placeholder') || el.getAttribute('title') || el.getAttribute('alt') ||
    (el.tagName === 'INPUT' ? (/^(submit|button|reset)$/.test(el.type) ? el.value : '') : el.tagName === 'SELECT' ? '' : el.innerText) ||
    el.getAttribute('name') || el.id || '');
  A.role = (el) => {
    const r = el.getAttribute('role');
    if (r) return r;
    const t = el.tagName;
    if (t === 'A') return 'link';
    if (t === 'BUTTON' || t === 'SUMMARY') return 'button';
    if (t === 'SELECT') return 'select';
    if (t === 'TEXTAREA') return 'textbox';
    if (t === 'INPUT') {
      const k = el.type;
      if (/^(submit|button|reset|image)$/.test(k)) return 'button';
      if (k === 'checkbox' || k === 'radio') return k;
      return k === 'text' ? 'textbox' : k + ' field';
    }
    return el.isContentEditable ? 'textbox' : 'clickable';
  };
  A.describe = (el) => `${A.role(el)} "${A.name(el)}"`;
  A.find = (q) => {
    if (q.ref != null) {
      const el = A.refs[q.ref - 1];
      if (!el) throw new Error(`No element [${q.ref}]; take a new browser_snapshot`);
      if (!el.isConnected) throw new Error(`Element [${q.ref}] left the page; take a new browser_snapshot`);
      return el;
    }
    if (q.selector) {
      const el = document.querySelector(q.selector);
      if (!el) throw new Error(`Nothing matches ${q.selector}`);
      return el;
    }
    if (q.name) {
      const want = q.name.toLowerCase();
      const all = [...document.querySelectorAll(A.SELECTOR)].filter(A.visible);
      const el = all.find((e) => A.name(e).toLowerCase() === want) || all.find((e) => A.name(e).toLowerCase().includes(want));
      if (!el) throw new Error(`No visible element named "${q.name}"`);
      return el;
    }
    throw new Error('Say which element: ref, selector or name');
  };
  A.press = (requested) => {
    const shiftKey = requested === 'Shift+Tab';
    const key = shiftKey ? 'Tab' : requested === 'Space' ? ' ' : requested;
    const printable = [...key].length === 1 && !/[\x00-\x1f\x7f]/.test(key);
    const codes = { Enter: 13, Escape: 27, Tab: 9, Backspace: 8, Delete: 46, ' ': 32, ArrowUp: 38, ArrowDown: 40, ArrowLeft: 37, ArrowRight: 39 };
    if (!printable && !Object.hasOwn(codes, key)) throw new Error('Unsupported page key or shortcut: ' + requested);
    const el = document.activeElement || document.body;
    const keyCode = codes[key] || (printable ? key.toUpperCase().charCodeAt(0) : 0);
    const code = /^[a-z]$/i.test(key) ? 'Key' + key.toUpperCase() : /^\d$/.test(key) ? 'Digit' + key : key === ' ' ? 'Space' : printable ? '' : key;
    const o = { key, code, keyCode, which: keyCode, shiftKey, bubbles: true, cancelable: true, composed: true };
    const send = (type, target = el) => target.dispatchEvent(new KeyboardEvent(type, o));
    const eventsOnly = `sent ${requested} to the page; no native default action simulated`;
    try {
      if (!send('keydown') || ((printable || key === 'Enter') && !send('keypress'))) return `page canceled ${requested}`;
      if (document.activeElement !== el || !el.isConnected) return eventsOnly;
      if (key === 'Tab') {
        // Stay within this document, even at the first/last control. Never tab into app chrome.
        const controls = [...document.querySelectorAll(A.SELECTOR)].filter(e => e.tabIndex >= 0 && A.visible(e) && !e.matches(':disabled') && !e.closest('[inert]'));
        const rank = e => e.tabIndex || Infinity;
        controls.sort((a, b) => rank(a) - rank(b));
        const i = controls.indexOf(el);
        const next = controls[i < 0 ? (shiftKey ? controls.length - 1 : 0) : i + (shiftKey ? -1 : 1)];
        if (!next) return 'no further focusable page control';
        next.focus();
        return document.activeElement === next ? `focused ${A.describe(next)}` : 'page did not focus the next control';
      }
      if (el.matches(':disabled')) return 'focused control is disabled';
      if (el.matches('select:not([multiple])') && (key === 'ArrowDown' || key === 'ArrowUp')) {
        const step = key === 'ArrowDown' ? 1 : -1;
        for (let i = el.selectedIndex + step; i >= 0 && i < el.length; i += step) {
          if (el.options[i].matches(':disabled')) continue;
          el.selectedIndex = i;
          for (const type of ['input', 'change']) el.dispatchEvent(new Event(type, { bubbles: true }));
          return `selected option ${el.selectedIndex + 1}`;
        }
        return 'no further selectable option';
      }
      if ((key === 'Enter' || key === ' ') && el.matches('button,summary,input:is([type=submit],[type=button],[type=reset],[type=image])') ||
          key === 'Enter' && el.matches('a[href]') || key === ' ' && el.matches('input:is([type=checkbox],[type=radio])')) {
        el.click();
        return `clicked ${A.describe(el)}`;
      }
      if (key === 'Enter' && el.form && el.tagName === 'INPUT' && /^(text|search|url|tel|email|password|number)$/.test(el.type)) {
        el.form.requestSubmit();
        return 'requested form submission';
      }
      // Only edit controls with a text selection. A page handler may cancel beforeinput.
      if (!(el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) || el.selectionStart == null) return eventsOnly;
      let start = el.selectionStart, end = el.selectionEnd;
      const value = el.value;
      const back = i => Math.max(0, i - (/^[\uD800-\uDBFF][\uDC00-\uDFFF]$/.test(value.slice(i - 2, i)) ? 2 : 1));
      const forward = i => Math.min(value.length, i + (value.codePointAt(i) > 0xffff ? 2 : 1));
      if (key === 'ArrowLeft' || key === 'ArrowRight') {
        const pos = key === 'ArrowLeft' ? (start < end ? start : back(start)) : (start < end ? end : forward(end));
        el.setSelectionRange(pos, pos);
        return `caret at ${pos}`;
      }
      if (!(printable || key === 'Backspace' || key === 'Delete' || key === 'Enter' && el.tagName === 'TEXTAREA')) return eventsOnly;
      if (el.readOnly) return 'text field is read-only';
      const inputType = printable ? 'insertText' : key === 'Enter' ? 'insertLineBreak' : key === 'Backspace' ? 'deleteContentBackward' : 'deleteContentForward';
      const text = printable ? key : key === 'Enter' ? '\n' : '';
      if (start === end && key === 'Backspace') start = back(start);
      if (start === end && key === 'Delete') end = forward(end);
      if (start === end && !text) return 'nothing to delete at the caret';
      if (text && el.maxLength >= 0 && value.length - (end - start) + text.length > el.maxLength) return 'field length limit reached';
      const input = { inputType, data: printable ? text : null, bubbles: true, composed: true };
      if (!el.dispatchEvent(new InputEvent('beforeinput', { ...input, cancelable: true }))) return `page canceled ${inputType}`;
      if (document.activeElement !== el || el.value !== value || el.readOnly || el.matches(':disabled')) return eventsOnly;
      el.setRangeText(text, start, end, 'end');
      el.dispatchEvent(new InputEvent('input', input));
      return `applied ${inputType} to ${A.describe(el)}`;
    } finally {
      send('keyup', document.activeElement || el);
    }
  };
  A.run = (fn) => {
    try {
      const v = fn();
      return JSON.stringify({ ok: true, value: v === undefined ? null : v });
    } catch (e) {
      return JSON.stringify({ ok: false, error: String((e && e.message) || e) });
    }
  };
})();"#;

const SNAPSHOT: &str = r#"() => {
  const A = window.__dialAgent;
  const refs = [], elements = [];
  let total = 0;
  for (const el of document.querySelectorAll(A.SELECTOR)) {
    if (!A.visible(el)) continue;
    total++;
    if (refs.length >= LIMIT) continue;
    refs.push(el);
    const bits = [`[${refs.length}] ${A.describe(el)}`];
    if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA') {
      if (el.type === 'checkbox' || el.type === 'radio') bits.push(el.checked ? 'checked' : 'unchecked');
      else if (el.type !== 'password' && el.value) bits.push(`value="${A.clean(el.value, 60)}"`);
    }
    if (el.tagName === 'SELECT' && el.selectedIndex >= 0) bits.push(`selected="${A.clean(el.options[el.selectedIndex].text, 40)}"`);
    if (el.disabled) bits.push('disabled');
    if (el.tagName === 'A') {
      const href = el.getAttribute('href');
      if (href && !href.startsWith('javascript:')) bits.push(`-> ${A.clean(href, 80)}`);
    }
    const r = el.getBoundingClientRect();
    if (r.bottom < 0 || r.top > innerHeight) bits.push('(off screen)');
    elements.push(bits.join(' '));
  }
  A.refs = refs;
  const headings = [...document.querySelectorAll('h1,h2,h3')].filter(A.visible).slice(0, 15)
    .map((h) => `${h.tagName.toLowerCase()} ${A.clean(h.innerText, 100)}`);
  return { url: location.href, title: document.title, scrollY: Math.round(scrollY),
    height: document.documentElement.scrollHeight, viewport: innerHeight, headings, elements, total };
}"#;

const CLICK: &str = r#"() => {
  const A = window.__dialAgent;
  const el = A.find(QUERY);
  el.scrollIntoView({ block: 'center', inline: 'center' });
  const r = el.getBoundingClientRect();
  const x = r.left + r.width / 2, y = r.top + r.height / 2;
  const hit = document.elementFromPoint(x, y);
  const t = hit && el.contains(hit) ? hit : el;
  const o = { bubbles: true, cancelable: true, composed: true, view: window, clientX: x, clientY: y, button: 0 };
  const p = Object.assign({ pointerId: 1, pointerType: 'mouse', isPrimary: true }, o);
  t.dispatchEvent(new PointerEvent('pointerdown', p));
  t.dispatchEvent(new MouseEvent('mousedown', o));
  if (el.focus) el.focus({ preventScroll: true });
  t.dispatchEvent(new PointerEvent('pointerup', p));
  t.dispatchEvent(new MouseEvent('mouseup', o));
  t.click();
  const covered = hit && !el.contains(hit) && !hit.contains(el) ? A.describe(hit) : null;
  return { clicked: A.describe(el), covered };
}"#;

const TYPE: &str = r#"() => {
  const A = window.__dialAgent;
  const el = A.find(QUERY);
  const text = TEXT;
  el.scrollIntoView({ block: 'center' });
  if (el.focus) el.focus();
  if (el.isContentEditable) {
    if (CLEAR) document.execCommand('selectAll');
    document.execCommand('insertText', false, text);
  } else if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, (CLEAR ? '' : el.value) + text);
    el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: text }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  } else {
    throw new Error(`${A.describe(el)} does not take text`);
  }
  const typed = `typed into ${A.describe(el)}`;
  return SUBMIT ? `${typed}; ${A.press('Enter')}` : typed;
}"#;

const SELECT: &str = r#"() => {
  const A = window.__dialAgent;
  const el = A.find(QUERY);
  if (el.tagName !== 'SELECT') throw new Error(`${A.describe(el)} is not a select`);
  const want = String(OPTION).toLowerCase();
  const options = [...el.options];
  const opt = options.find((o) => o.value.toLowerCase() === want || o.text.trim().toLowerCase() === want)
    || options.find((o) => o.text.toLowerCase().includes(want));
  if (!opt) throw new Error(`No option "${OPTION}"; it offers: ${options.map((o) => o.text.trim()).slice(0, 30).join(', ')}`);
  el.value = opt.value;
  el.dispatchEvent(new Event('input', { bubbles: true }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
  return `chose "${opt.text.trim()}" in ${A.describe(el)}`;
}"#;

const SCROLL: &str = r#"() => {
  const A = window.__dialAgent;
  const q = QUERY, d = DIRECTION;
  const root = document.documentElement;
  if (q) A.find(q).scrollIntoView({ block: 'center', behavior: 'instant' });
  else if (d === 'top') scrollTo({ top: 0, behavior: 'instant' });
  else if (d === 'bottom') scrollTo({ top: root.scrollHeight, behavior: 'instant' });
  else scrollBy({ top: (d === 'up' ? -1 : 1) * innerHeight * 0.8, behavior: 'instant' });
  return `scrolled to ${Math.round(scrollY)} of ${root.scrollHeight}px (window ${innerHeight}px)`;
}"#;

const HAS_TEXT: &str = r#"() => (document.body ? document.body.innerText : '').toLowerCase().includes(TEXT.toLowerCase())"#;

/// Runs the agent's function body as an async function; its promise is parked in `done` for
/// [`EVAL_RESULT`], so `await` works and a sync result comes back the same way.
const EVAL: &str = r#"() => {
  const A = window.__dialAgent;
  const v = (async () => {
BODY
  })();
  if (v && typeof v.then === 'function') {
    const id = 'p' + Math.random().toString(36).slice(2);
    const settle = (entry) => {
      A.done[id] = entry;
      setTimeout(() => { delete A.done[id]; }, 30000);
    };
    v.then((x) => settle({ ok: true, value: x === undefined ? null : x }),
      (e) => settle({ ok: false, error: String((e && e.message) || e) }));
    return { pending: id };
  }
  return v;
}"#;

const EVAL_RESULT: &str = r#"() => {
  const d = window.__dialAgent.done[ID];
  if (!d) return { pending: true };
  delete window.__dialAgent.done[ID];
  if (!d.ok) throw new Error(d.error);
  return { value: d.value };
}"#;

/// `body` (an arrow function) as a full script that answers `{ok, value}` or `{ok, error}`.
pub fn action(body: &str) -> String {
    format!("{HELPERS}\nwindow.__dialAgent.run({body});")
}

/// Fill a script template's `KEY` placeholders with JSON values in a single pass so substituted
/// values are never re-scanned or corrupted by later placeholder names.
fn fill(template: &str, values: &[(&str, Value)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while !rest.is_empty() {
        let next = values
            .iter()
            .filter_map(|(key, value)| rest.find(key).map(|pos| (pos, *key, value)))
            .min_by_key(|(pos, key, _)| (*pos, std::cmp::Reverse(key.len())));
        match next {
            Some((pos, key, value)) => {
                out.push_str(&rest[..pos]);
                out.push_str(&value.to_string());
                rest = &rest[pos + key.len()..];
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    out
}

/// Which element a call names: `{ref}`, `{selector}` or `{name}` as the page script wants it, or
/// `None` when it names none.
pub fn target(args: &Value) -> Result<Option<Value>, String> {
    if let Some(value) = args.get("ref").filter(|v| !v.is_null()) {
        let number = value
            .as_u64()
            .or_else(|| {
                value
                    .as_str()
                    .and_then(|s| s.trim_matches(['[', ']']).parse().ok())
            })
            .filter(|n| *n > 0)
            .ok_or("`ref` is a number from the snapshot, like 3 for [3]")?;
        return Ok(Some(json!({ "ref": number })));
    }
    for key in ["selector", "name"] {
        if let Some(text) = args.get(key).and_then(Value::as_str).map(str::trim)
            && !text.is_empty()
        {
            return Ok(Some(json!({ key: text })));
        }
    }
    Ok(None)
}

fn required_target(args: &Value) -> Result<Value, String> {
    target(args)?.ok_or_else(|| {
        "Say which element: `ref` (from browser_snapshot), `selector` or `name`".to_owned()
    })
}

fn string_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("`{key}` is required"))
}

/// A page script's answer: the `value` of `{ok: true, value}`, or the error it threw.
pub fn parse_outcome(raw: &str) -> Result<Value, String> {
    if raw.trim().is_empty() {
        return Err("The script did not run: check it for syntax errors".into());
    }
    let value = browser::unwrap_json(raw)?;
    match value.get("ok").and_then(Value::as_bool) {
        Some(true) => Ok(value.get("value").cloned().unwrap_or(Value::Null)),
        Some(false) => Err(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("The script failed")
            .to_owned()),
        None => Err(format!(
            "Unexpected answer from the page: {}",
            clip(raw, 200)
        )),
    }
}

/// A snapshot's answer as text for the model.
pub fn format_snapshot(value: &Value) -> String {
    let text = |key: &str| value.get(key).and_then(Value::as_str).unwrap_or("");
    let number = |key: &str| value.get(key).and_then(Value::as_i64).unwrap_or(0);
    let list = |key: &str| -> Vec<String> {
        value
            .get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|i| i.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    let title = if text("title").is_empty() {
        "(untitled)"
    } else {
        text("title")
    };
    let mut out = format!("Page: {title}\nURL: {}\n", text("url"));
    out.push_str(&format!(
        "Scrolled to {} of {}px (window {}px)\n",
        number("scrollY"),
        number("height"),
        number("viewport")
    ));
    let headings = list("headings");
    if !headings.is_empty() {
        out.push_str("\nHeadings:\n");
        for heading in headings {
            out.push_str(&format!("  {heading}\n"));
        }
    }
    let elements = list("elements");
    let total = number("total");
    if elements.is_empty() {
        out.push_str("\nNo visible interactive elements.\n");
    } else {
        out.push_str(&format!(
            "\nElements ({} of {total} visible):\n",
            elements.len()
        ));
        for element in elements {
            out.push_str(&element);
            out.push('\n');
        }
    }
    out
}

fn clip(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(limit).collect();
    out.push('…');
    out
}

/// A short line for the panel about what the agent does.
pub fn describe_call(tool: &str, args: &Value) -> String {
    let which = || match target(args) {
        Ok(Some(target)) => {
            if let Some(n) = target.get("ref") {
                format!("[{n}]")
            } else {
                target
                    .as_object()
                    .and_then(|m| m.values().next())
                    .and_then(Value::as_str)
                    .map(|s| format!("\"{}\"", clip(s, 40)))
                    .unwrap_or_default()
            }
        }
        _ => String::new(),
    };
    let arg = |key: &str| {
        args.get(key)
            .and_then(Value::as_str)
            .map(|s| clip(s, 60))
            .unwrap_or_default()
    };
    match tool {
        "browser_open" => format!("Opening {}", arg("url")),
        "browser_snapshot" => "Looking at the page".into(),
        "browser_read" => "Reading the page".into(),
        "browser_click" => format!("Clicking {}", which()),
        "browser_type" => format!("Typing into {}", which()),
        "browser_select" => format!("Choosing \"{}\" in {}", arg("option"), which()),
        "browser_press" => format!("Pressing {}", arg("key")),
        "browser_scroll" => "Scrolling".into(),
        "browser_navigate" => format!("Going {}", arg("action")),
        "browser_wait" => "Waiting for the page".into(),
        "browser_eval" => "Running a script in the page".into(),
        "browser_screenshot" => "Taking a screenshot".into(),
        other => other.to_owned(),
    }
}

/// Answer one request line: check the token, then run the tool with `run`.
pub fn handle(
    line: &str,
    token: &str,
    project_matches: impl FnOnce(&str) -> bool,
    run: impl FnOnce(&Request) -> Result<Reply, String>,
) -> Reply {
    let Ok(request) = serde_json::from_str::<Request>(line) else {
        return Reply::error("Bad request");
    };
    if !same(&request.token, token) {
        return Reply::error("Not authorized");
    }
    if !project_matches(&request.workspace) {
        return Reply::error("Different project window");
    }
    if request.tool == PING {
        return Reply::text("pong");
    }
    run(&request).unwrap_or_else(Reply::error)
}

/// Compare without stopping at the first difference.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

/// 256 random bits as hex: the OS's randomness, through `RandomState`'s seed elsewhere.
pub fn new_token() -> String {
    #[cfg(unix)]
    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        let mut bytes = [0u8; 32];
        if file.read_exact(&mut bytes).is_ok() {
            return bytes.iter().map(|b| format!("{b:02x}")).collect();
        }
    }
    use std::hash::{BuildHasher, Hasher};
    (0..4u64)
        .map(|i| {
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_u64(i);
            hasher.write_u128(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or_default(),
            );
            format!("{:016x}", hasher.finish())
        })
        .collect()
}

/// Write `endpoint` to `file`, readable only by this user.
pub fn write_endpoint(file: &Path, endpoint: &Endpoint) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = file.with_extension("json.partial");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut out = options.open(&partial)?;
    out.write_all(&serde_json::to_vec(endpoint).map_err(std::io::Error::other)?)?;
    out.sync_all()?;
    std::fs::rename(&partial, file)
}

/// Read the endpoint the app wrote.
pub fn read_endpoint(file: &Path) -> Option<Endpoint> {
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

/// Remove the endpoint file if this process wrote it.
pub fn remove_endpoint(file: &Path) {
    if read_endpoint(file).is_some_and(|e| e.pid == std::process::id()) {
        let _ = std::fs::remove_file(file);
    }
}

/// Serve tool calls on a loopback port and publish it in `file`.
pub fn start(app: &AppHandle, file: &Path) -> std::io::Result<Endpoint> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let endpoint = Endpoint {
        port: listener.local_addr()?.port(),
        token: new_token(),
        pid: std::process::id(),
    };
    write_endpoint(file, &endpoint)?;
    let app = app.clone();
    let token = endpoint.token.clone();
    thread::Builder::new()
        .name("agent-browser".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let app = app.clone();
                let token = token.clone();
                thread::spawn(move || serve(&app, &stream, &token));
            }
        })?;
    Ok(endpoint)
}

fn serve(app: &AppHandle, stream: &TcpStream, token: &str) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut line = String::new();
    if BufReader::new(stream.take(MAX_REQUEST))
        .read_line(&mut line)
        .is_err()
    {
        return;
    }
    let reply = handle(
        line.trim(),
        token,
        |root| crate::app::browser_project(app, root),
        |request| {
            let tool = request.tool.as_str();
            let args = &request.arguments;
            if tool == "app_desktop" {
                return crate::app_runner::agent_call(app, &request.workspace, args);
            }
            if tool == "types_screenshot" {
                return capture_type(app, args);
            }
            if tool.starts_with("types_") {
                let store = app.state::<crate::types::Store>();
                return crate::types::agent_call(app, &store, tool, args).map(Reply::text);
            }
            if tool.starts_with("board_") {
                return crate::boards::agent_call(app, &request.workspace, tool, args);
            }
            if desktop::TOOLS.contains(&tool) {
                return desktop::call(app, tool, args);
            }
            let _ = app.emit(
                "agent-browser",
                json!({"kind": "activity", "text": describe_call(tool, args), "busy": true}),
            );
            let result = call(app, tool, args);
            let _ = app.emit("agent-browser", json!({"kind": "activity", "busy": false}));
            result
        },
    );
    let mut out = serde_json::to_string(&reply).unwrap_or_default();
    out.push('\n');
    let mut stream = stream;
    let _ = stream.write_all(out.as_bytes());
}

/// Run `body` in the page and return its value.
fn run_script<R: Runtime>(app: &AppHandle<R>, body: &str) -> Result<Value, String> {
    parse_outcome(&browser::eval_raw(app, &action(body), SCRIPT_TIMEOUT)?)
}

fn ensure_open<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if app.get_webview(browser::LABEL).is_none() {
        return Err("The browser has no page yet; open one with browser_open".into());
    }
    Ok(())
}

/// Wait until a page load finishes after `before` loads (or the page is idle and `any` is false).
fn wait_load<R: Runtime>(app: &AppHandle<R>, before: u64, timeout: Duration) -> bool {
    let browser = app.state::<Browser>();
    let start = Instant::now();
    while start.elapsed() < timeout {
        if app.get_webview(browser::LABEL).is_some()
            && browser.loads() > before
            && !browser.current().loading
        {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

/// After an action: wait for a page load it started, then say where the page is.
fn settle<R: Runtime>(app: &AppHandle<R>, before: u64) -> String {
    thread::sleep(Duration::from_millis(350));
    let browser = app.state::<Browser>();
    let navigated = browser.loads() != before || browser.current().loading;
    if navigated {
        wait_load(app, before, LOAD_TIMEOUT);
    }
    let state = browser.current();
    let title = if state.title.is_empty() {
        "(untitled)"
    } else {
        &state.title
    };
    if navigated {
        format!(
            "The page changed: {title} ({}). Take a new snapshot for refs.",
            state.url
        )
    } else {
        format!("Still on {title} ({}).", state.url)
    }
}

fn snapshot<R: Runtime>(app: &AppHandle<R>) -> Result<String, String> {
    let body = SNAPSHOT.replace("LIMIT", &SNAPSHOT_LIMIT.to_string());
    Ok(format_snapshot(&run_script(app, &body)?))
}

/// Run one tool. Blocks; runs on a connection thread.
pub fn call(app: &AppHandle, tool: &str, args: &Value) -> Result<Reply, String> {
    let browser = app.state::<Browser>();
    let before = browser.loads();
    match tool {
        "browser_open" => {
            let url = browser::normalize_url(string_arg(args, "url")?)?;
            let _ = app.emit("agent-browser", json!({"kind": "open", "url": url}));
            let loaded = wait_load(app, before, LOAD_TIMEOUT);
            ensure_open(app).map_err(|_| {
                "The Browser panel did not open; is the Dial Desktop window showing?".to_owned()
            })?;
            let mut text = snapshot(app)?;
            if !loaded {
                text.insert_str(0, "(The page is still loading.)\n");
            }
            Ok(Reply::text(text))
        }
        "browser_snapshot" => {
            ensure_open(app)?;
            snapshot(app).map(Reply::text)
        }
        "browser_read" => {
            ensure_open(app)?;
            let page = browser::inspect(app)?;
            let mut out = format!("Page: {}\nURL: {}\n", page.title, page.url);
            if !page.selection.trim().is_empty() {
                out.push_str(&format!(
                    "\nSelected text:\n{}\n",
                    clip(&page.selection, 4000)
                ));
            }
            if page.errors.is_empty() {
                out.push_str("\nConsole errors: none\n");
            } else {
                out.push_str("\nConsole errors:\n");
                for error in &page.errors {
                    out.push_str(&format!("- {error}\n"));
                }
            }
            out.push_str(&format!("\nText:\n{}\n", clip(&page.text, TEXT_LIMIT)));
            Ok(Reply::text(out))
        }
        "browser_click" => {
            ensure_open(app)?;
            let script = fill(CLICK, &[("QUERY", required_target(args)?)]);
            let value = run_script(app, &script)?;
            let mut text = format!(
                "Clicked {}.",
                value
                    .get("clicked")
                    .and_then(Value::as_str)
                    .unwrap_or("the element")
            );
            if let Some(cover) = value.get("covered").and_then(Value::as_str) {
                text.push_str(&format!(" Note: {cover} lies on top of it."));
            }
            Ok(Reply::text(format!("{text} {}", settle(app, before))))
        }
        "browser_type" => {
            ensure_open(app)?;
            let flag = |key: &str, default: bool| {
                args.get(key).and_then(Value::as_bool).unwrap_or(default)
            };
            let script = fill(
                TYPE,
                &[
                    ("QUERY", required_target(args)?),
                    ("TEXT", json!(string_arg(args, "text")?)),
                    ("CLEAR", json!(flag("clear", true))),
                    ("SUBMIT", json!(flag("submit", false))),
                ],
            );
            let value = run_script(app, &script)?;
            Ok(Reply::text(format!(
                "{}. {}",
                value.as_str().unwrap_or("Typed"),
                settle(app, before)
            )))
        }
        "browser_select" => {
            ensure_open(app)?;
            let script = fill(
                SELECT,
                &[
                    ("QUERY", required_target(args)?),
                    ("OPTION", json!(string_arg(args, "option")?)),
                ],
            );
            let value = run_script(app, &script)?;
            Ok(Reply::text(format!(
                "{}. {}",
                value.as_str().unwrap_or("Chosen"),
                settle(app, before)
            )))
        }
        "browser_press" => {
            ensure_open(app)?;
            let key = string_arg(args, "key")?;
            let value = run_script(
                app,
                &format!("() => window.__dialAgent.press({})", json!(key)),
            )?;
            Ok(Reply::text(format!(
                "{}. {}",
                value.as_str().ok_or("The page did not return a key action result")?,
                settle(app, before)
            )))
        }
        "browser_scroll" => {
            ensure_open(app)?;
            let direction = args
                .get("direction")
                .and_then(Value::as_str)
                .unwrap_or("down");
            if !matches!(direction, "up" | "down" | "top" | "bottom") {
                return Err("`direction` is up, down, top or bottom".into());
            }
            let script = fill(
                SCROLL,
                &[
                    ("QUERY", target(args)?.unwrap_or(Value::Null)),
                    ("DIRECTION", json!(direction)),
                ],
            );
            let value = run_script(app, &script)?;
            Ok(Reply::text(value.as_str().unwrap_or("Scrolled").to_owned()))
        }
        "browser_navigate" => {
            ensure_open(app)?;
            let view = app
                .get_webview(browser::LABEL)
                .ok_or("The browser is not open")?;
            let result = match string_arg(args, "action")? {
                "back" => view.eval("history.back()"),
                "forward" => view.eval("history.forward()"),
                "reload" => view.reload(),
                other => {
                    return Err(format!(
                        "Unknown action `{other}`: use back, forward or reload"
                    ));
                }
            };
            result.map_err(|e| e.to_string())?;
            Ok(Reply::text(settle(app, before)))
        }
        "browser_wait" => {
            ensure_open(app)?;
            let limit = args
                .get("timeout_ms")
                .and_then(Value::as_u64)
                .map(Duration::from_millis)
                .unwrap_or(Duration::from_secs(10))
                .min(WAIT_LIMIT);
            let Some(text) = args
                .get("text")
                .and_then(Value::as_str)
                .filter(|t| !t.is_empty())
            else {
                thread::sleep(limit);
                return Ok(Reply::text(format!("Waited {} ms.", limit.as_millis())));
            };
            let script = fill(HAS_TEXT, &[("TEXT", json!(text))]);
            let start = Instant::now();
            loop {
                if run_script(app, &script).ok().and_then(|v| v.as_bool()) == Some(true) {
                    return Ok(Reply::text(format!(
                        "\"{}\" shows on the page (after {} ms).",
                        clip(text, 60),
                        start.elapsed().as_millis()
                    )));
                }
                if start.elapsed() >= limit {
                    return Err(format!(
                        "\"{}\" did not show within {} ms",
                        clip(text, 60),
                        limit.as_millis()
                    ));
                }
                thread::sleep(Duration::from_millis(250));
            }
        }
        "browser_eval" => {
            ensure_open(app)?;
            let body = string_arg(args, "script")?;
            let mut value = run_script(app, &EVAL.replace("BODY", body))?;
            if let Some(id) = value
                .get("pending")
                .and_then(Value::as_str)
                .map(str::to_owned)
            {
                let script = fill(EVAL_RESULT, &[("ID", json!(id))]);
                let start = Instant::now();
                value = loop {
                    let answer = run_script(app, &script)?;
                    if answer.get("pending").is_none() {
                        break answer.get("value").cloned().unwrap_or(Value::Null);
                    }
                    if start.elapsed() > Duration::from_secs(10) {
                        return Err("The promise did not settle within 10 s".into());
                    }
                    thread::sleep(Duration::from_millis(100));
                };
            }
            let text = serde_json::to_string_pretty(&value).unwrap_or_default();
            Ok(Reply::text(clip(&text, TEXT_LIMIT)))
        }
        "browser_screenshot" => {
            ensure_open(app)?;
            let _ = app.emit("agent-browser", json!({"kind": "show"}));
            capture(app)
        }
        other => Err(format!("Unknown tool `{other}`")),
    }
}

#[cfg(any(target_os = "macos", windows))]
fn capture(app: &AppHandle) -> Result<Reply, String> {
    // Let the panel come up if the agent's call just showed it.
    thread::sleep(Duration::from_millis(250));
    let view = app
        .get_webview(browser::LABEL)
        .ok_or("The browser is not open")?;
    #[cfg(target_os = "macos")]
    let png = screenshot::capture(&view, None)?;
    #[cfg(windows)]
    let png = screenshot_windows::capture(&view)?;
    let state = app.state::<Browser>().current();
    screenshot_wire::reply(
        format!("Screenshot of {} ({})", clip(&state.title, 512), clip(&state.url, 2048)),
        png,
    )
}

#[cfg(not(any(target_os = "macos", windows)))]
fn capture(_app: &AppHandle) -> Result<Reply, String> {
    Err("Screenshots of the Browser panel are available on macOS and Windows".into())
}

#[cfg(target_os = "macos")]
fn capture_type(app: &AppHandle, args: &Value) -> Result<Reply, String> {
    let id = app.state::<crate::types::Store>().find(string_arg(args, "type")?)?;
    crate::types::live::wait_loaded(app, &id, Duration::from_secs(10))?;
    // Let web fonts, images and entrance animations settle.
    thread::sleep(Duration::from_millis(400));
    let view = app.get_webview("main").ok_or("The Dial window is not open")?;
    let png = screenshot::capture_type(&view, &id)?;
    let reply = screenshot_wire::reply(
        format!("Screenshot of Type {id}'s app.\n{}", crate::types::live::console(app, &id, false)),
        png,
    )?;
    crate::types::progress(app, &id, json!({"checked": true}));
    Ok(reply)
}

#[cfg(not(target_os = "macos"))]
fn capture_type(_app: &AppHandle, _args: &Value) -> Result<Reply, String> {
    Err("Type screenshots are supported on macOS only; inspect the app with types_run instead (read the DOM, sizes and computed styles)".into())
}

/// The `mcp.dial_desktop` entry that runs `exe` as the bridge, as a TOML inline table.
pub fn mcp_entry(exe: &Path) -> String {
    format!(
        "{{ program = {}, args = [\"{BRIDGE_FLAG}\"], enabled = true, trust_read_only_hint = true, \
         timeout_secs = 120, connect_timeout_secs = 15 }}",
        toml_string(&exe.to_string_lossy())
    )
}

/// A TOML basic string.
fn toml_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Whether the saved entry (the `value` of `dial config get --json`) must be rewritten for `exe`.
/// An entry the user turned off stays off.
pub fn needs_registration(saved: Option<&Value>, exe: &Path) -> bool {
    let Some(saved) = saved else {
        return true;
    };
    if saved.get("enabled") == Some(&Value::Bool(false)) {
        return false;
    }
    saved.get("program").and_then(Value::as_str) != Some(&*exe.to_string_lossy())
        || saved.get("args") != Some(&json!([BRIDGE_FLAG]))
        || saved.get("url").is_some_and(|url| !url.is_null())
}

/// Make sure Dial's settings start this binary as the browser's MCP server. Returns whether the
/// setting was written.
pub fn register(dial: &Path, exe: &Path) -> Result<bool, String> {
    let key = format!("mcp.{SERVER_NAME}");
    let timeout = Duration::from_secs(10);
    // Config resolves a project layer in the process cwd (and rejects the filesystem root,
    // common for GUI launches). Inspect only global settings, regardless of the launch folder.
    let scratch = tempfile::tempdir().map_err(|error| format!("Could not prepare a config directory: {error}"))?;
    let absolute = |path: &Path| std::path::absolute(path).map_err(|error| error.to_string());
    let dial = if dial.is_relative() && dial.components().count() > 1 {
        absolute(dial)?
    } else {
        dial.to_owned()
    };
    // Keep an explicitly relative DIAL_HOME relative to the original cwd, not the scratch one.
    let dial_home = std::env::var_os("DIAL_HOME").filter(|p| !p.is_empty())
        .map(PathBuf::from).filter(|p| p.is_relative()).map(|p| absolute(&p)).transpose()?;
    let config = |args: &[&str]| {
        let mut command = Command::new(&dial);
        command.current_dir(scratch.path()).arg("config").args(args);
        if let Some(home) = &dial_home { command.env("DIAL_HOME", home); }
        crate::launcher::run_with_timeout(&mut command, timeout)
    };

    // Unlike querying a single absent entry, an empty `mcp` table is a successful read. A
    // failed or unrecognizable read must never be interpreted as permission to enable tools.
    let out = config(&["get", "mcp", "--json"]).ok_or("`dial config get mcp` failed")?;
    let saved: Value = serde_json::from_slice(&out).map_err(|_| "Invalid `dial config get mcp` response")?;
    let servers = saved.get("value").and_then(Value::as_object)
        .filter(|_| saved.get("key").and_then(Value::as_str) == Some("mcp"))
        .ok_or("Missing MCP table in config response")?;
    let entry = servers.get(SERVER_NAME);
    if entry.is_some_and(|entry| !entry.is_object()) {
        return Err(format!("Invalid {key} entry in config response"));
    }
    if !needs_registration(entry, exe) {
        return Ok(false);
    }
    // `config set` without `--project` writes only the global layer.
    config(&["set", &key, &mcp_entry(exe)])
        .map(|_| true)
        .ok_or_else(|| format!("`dial config set {key}` failed"))
}
