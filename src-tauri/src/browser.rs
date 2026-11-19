//! The integrated browser: a child webview laid over the Browser panel of the main window.
//!
//! The web UI owns the panel and reports where it sits ([`Bounds`], logical pixels); this module
//! creates the webview on first use, moves, hides and navigates it, and reports the page it shows
//! as the `browser` event. Pages get no access to the app: the webview has no capability (see
//! `capabilities/default.json`, which names only the `main` webview), and navigation is limited to
//! web pages. [`inspect`] reads the page's address, title, selected text, visible text and the
//! errors it logged, so the user can hand them to the agent.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError, mpsc};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Rect, Runtime, State, Url, Webview,
    WebviewBuilder, WebviewUrl,
};

/// The browser webview's label.
pub const LABEL: &str = "browser";
/// What a query without an address searches.
const SEARCH: &str = "https://duckduckgo.com/";
/// How long [`inspect`] waits for the page to answer.
const INSPECT_TIMEOUT: Duration = Duration::from_secs(3);

/// Collects the page's errors for [`inspect`]; runs before every page's own scripts.
const INIT_SCRIPT: &str = r#"(() => {
  if (window.__dialPage) return;
  const errors = [];
  const push = (message) => {
    errors.push(String(message).slice(0, 500));
    if (errors.length > 30) errors.shift();
  };
  window.__dialPage = { errors };
  addEventListener("error", (e) => push(e.message + (e.filename ? ` (${e.filename}:${e.lineno})` : "")));
  addEventListener("unhandledrejection", (e) => push("Unhandled rejection: " + ((e.reason && e.reason.message) || e.reason)));
  const original = console.error;
  console.error = function (...args) {
    push(args.map((a) => { try { return typeof a === "string" ? a : JSON.stringify(a); } catch { return String(a); } }).join(" "));
    return original.apply(this, args);
  };
})();"#;

/// Evaluates to the page as a JSON string (strings survive every platform's result conversion).
const INSPECT_SCRIPT: &str = r#"JSON.stringify({
  url: location.href,
  title: document.title,
  selection: String(getSelection() || "").slice(0, 20000),
  text: (document.body ? document.body.innerText : "").slice(0, 20000),
  errors: window.__dialPage ? window.__dialPage.errors.slice() : []
})"#;

/// Where the panel sits in the window, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Bounds {
    fn rect(self) -> Rect {
        Rect {
            position: LogicalPosition::new(self.x, self.y).into(),
            size: LogicalSize::new(self.width.max(1.0), self.height.max(1.0)).into(),
        }
    }
}

/// The page on show, published as the `browser` event.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserState {
    pub url: String,
    pub title: String,
    pub loading: bool,
}

/// What [`inspect`] read from the page.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PageInfo {
    pub url: String,
    pub title: String,
    pub selection: String,
    pub text: String,
    pub errors: Vec<String>,
}

/// The browser's state, managed by the app.
#[derive(Default)]
pub struct Browser {
    state: Mutex<BrowserState>,
    /// Pages finished loading so far; the agent's tools wait for it to move.
    loads: AtomicU64,
}

impl Browser {
    /// Change the state with `change` and publish it.
    fn update<R: Runtime>(&self, app: &AppHandle<R>, change: impl FnOnce(&mut BrowserState)) {
        let state = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            change(&mut state);
            state.clone()
        };
        let _ = app.emit("browser", state);
    }

    /// The page on show.
    pub fn current(&self) -> BrowserState {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// How many page loads have finished.
    pub fn loads(&self) -> u64 {
        self.loads.load(Ordering::SeqCst)
    }
}

/// What the user typed into the address bar as a URL to open: a web address as is, a bare host
/// over HTTPS (HTTP for this computer), anything else as a search.
pub fn normalize_url(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Type an address or a search".into());
    }
    if input == "about:blank" {
        return Ok(input.to_owned());
    }
    if let Some((scheme, _)) = input.split_once("://") {
        let url = Url::parse(input).map_err(|e| format!("Not a valid address: {e}"))?;
        return match scheme.to_ascii_lowercase().as_str() {
            "http" | "https" => Ok(url.to_string()),
            _ => Err("Only web pages (http and https) open in the browser".into()),
        };
    }
    let looks_like_host = !input.contains(char::is_whitespace)
        && (is_local(input) || host_part(input).contains('.'))
        && !input.starts_with('.');
    if looks_like_host {
        let scheme = if is_local(input) { "http" } else { "https" };
        if let Ok(url) = Url::parse(&format!("{scheme}://{input}")) {
            return Ok(url.to_string());
        }
    }
    Url::parse_with_params(SEARCH, &[("q", input)])
        .map(|url| url.to_string())
        .map_err(|e| e.to_string())
}

/// The host and port of an address without a scheme.
fn host_part(input: &str) -> &str {
    input.split(['/', '?', '#']).next().unwrap_or(input)
}

/// Whether an address without a scheme names this computer.
fn is_local(input: &str) -> bool {
    let host = host_part(input);
    let name = if host.starts_with('[') {
        host.split(']').next().map(|h| &h[1..]).unwrap_or(host)
    } else {
        host.split(':').next().unwrap_or(host)
    };
    matches!(name, "localhost" | "0.0.0.0" | "::1")
        || name.starts_with("127.")
        || name.ends_with(".localhost")
}

/// Whether the browser may show `url`: web pages and the documents they make, never local files
/// or app-internal schemes.
pub fn allowed(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "about" | "blob" | "data")
}

/// A script's JSON answer: platforms hand results back as JSON, and a string result (what our
/// scripts return) may arrive wrapped once more as a JSON string.
pub fn unwrap_json(raw: &str) -> Result<Value, String> {
    let value: Value = serde_json::from_str(raw).map_err(|e| format!("Unreadable answer: {e}"))?;
    Ok(match value {
        Value::String(inner) => serde_json::from_str(&inner).unwrap_or(Value::String(inner)),
        other => other,
    })
}

/// Read [`INSPECT_SCRIPT`]'s result.
pub fn parse_page(raw: &str) -> Result<PageInfo, String> {
    serde_json::from_value(unwrap_json(raw)?).map_err(|e| format!("Unreadable page info: {e}"))
}

fn webview<R: Runtime>(app: &AppHandle<R>) -> Result<Webview<R>, String> {
    app.get_webview(LABEL)
        .ok_or_else(|| "The browser is not open".to_owned())
}

/// Create the browser webview showing `url` at `bounds`.
fn create<R: Runtime>(app: &AppHandle<R>, url: Url, bounds: Bounds) -> Result<Webview<R>, String> {
    let window = app
        .get_window("main")
        .ok_or("The main window is not available")?;
    let loads = app.clone();
    let titles = app.clone();
    let popups = app.clone();
    let builder = WebviewBuilder::new(LABEL, WebviewUrl::External(url))
        .initialization_script(INIT_SCRIPT)
        .on_navigation(allowed)
        .on_page_load(move |_, payload| {
            let url = payload.url().to_string();
            let loading = matches!(payload.event(), PageLoadEvent::Started);
            let browser = loads.state::<Browser>();
            if !loading {
                browser.loads.fetch_add(1, Ordering::SeqCst);
            }
            browser.update(&loads, |state| {
                if state.url != url {
                    state.title.clear();
                }
                state.url = url;
                state.loading = loading;
            });
        })
        .on_document_title_changed(move |_, title| {
            titles
                .state::<Browser>()
                .update(&titles, |state| state.title = title);
        })
        // Links that open a new window open here instead: the panel shows one page at a time.
        .on_new_window(move |url, _| {
            if allowed(&url)
                && let Some(view) = popups.get_webview(LABEL)
            {
                let _ = view.navigate(url);
            }
            NewWindowResponse::Deny
        });
    let rect = bounds.rect();
    window
        .add_child(builder, rect.position, rect.size)
        .map_err(|e| format!("Could not open the browser: {e}"))
}

/// Show the browser at `bounds`, creating it on first use; opens `url` when given.
#[tauri::command]
pub async fn browser_show(
    app: AppHandle,
    bounds: Bounds,
    url: Option<String>,
) -> Result<BrowserState, String> {
    let url = url.as_deref().map(normalize_url).transpose()?;
    match app.get_webview(LABEL) {
        Some(view) => {
            view.set_bounds(bounds.rect()).map_err(|e| e.to_string())?;
            view.show().map_err(|e| e.to_string())?;
            if let Some(url) = url {
                navigate_to(&view, &url)?;
            }
        }
        None => {
            let start = url.unwrap_or_else(|| "about:blank".to_owned());
            let parsed = Url::parse(&start).map_err(|e| e.to_string())?;
            app.state::<Browser>()
                .update(&app, |state| state.url = start.clone());
            create(&app, parsed, bounds)?;
        }
    }
    Ok(app
        .state::<Browser>()
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone())
}

/// Move the browser to follow its panel.
#[tauri::command]
pub async fn browser_bounds(app: AppHandle, bounds: Bounds) -> Result<(), String> {
    if let Some(view) = app.get_webview(LABEL) {
        view.set_bounds(bounds.rect()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Hide the browser (another panel or an overlay is on top); the page stays loaded.
#[tauri::command]
pub async fn browser_hide(app: AppHandle) -> Result<(), String> {
    if let Some(view) = app.get_webview(LABEL) {
        view.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn navigate_to<R: Runtime>(view: &Webview<R>, url: &str) -> Result<(), String> {
    let url = Url::parse(url).map_err(|e| e.to_string())?;
    view.navigate(url).map_err(|e| e.to_string())
}

/// Open what the user typed; returns the address opened.
#[tauri::command]
pub async fn browser_navigate(
    app: AppHandle,
    browser: State<'_, Browser>,
    url: String,
) -> Result<String, String> {
    let url = normalize_url(&url)?;
    navigate_to(&webview(&app)?, &url)?;
    browser.update(&app, |state| {
        state.url = url.clone();
        state.loading = true;
    });
    Ok(url)
}

/// `back`, `forward`, `reload` or `stop`.
#[tauri::command]
pub async fn browser_go(app: AppHandle, action: String) -> Result<(), String> {
    let view = webview(&app)?;
    let result = match action.as_str() {
        "back" => view.eval("history.back()"),
        "forward" => view.eval("history.forward()"),
        "reload" => view.reload(),
        "stop" => view.eval("window.stop()"),
        other => return Err(format!("Unknown browser action `{other}`")),
    };
    result.map_err(|e| e.to_string())
}

/// The page's address, title, selection, text and logged errors.
#[tauri::command]
pub async fn browser_inspect(app: AppHandle) -> Result<PageInfo, String> {
    tauri::async_runtime::spawn_blocking(move || inspect(&app))
        .await
        .map_err(|e| e.to_string())?
}

/// [`browser_inspect`], blocking.
pub fn inspect<R: Runtime>(app: &AppHandle<R>) -> Result<PageInfo, String> {
    parse_page(&eval_raw(app, INSPECT_SCRIPT, INSPECT_TIMEOUT)?)
}

/// Run `script` in the page and return its answer as the platform hands it back. Blocks; never
/// call it on the main thread, which delivers the answer.
pub fn eval_raw<R: Runtime>(
    app: &AppHandle<R>,
    script: &str,
    timeout: Duration,
) -> Result<String, String> {
    let view = webview(app)?;
    let (tx, rx) = mpsc::channel();
    view.eval_with_callback(script, move |raw| {
        let _ = tx.send(raw);
    })
    .map_err(|e| e.to_string())?;
    rx.recv_timeout(timeout).map_err(|_| {
        "The page did not answer: it may be busy or still loading, or the script failed".to_owned()
    })
}

/// The page on show (the panel asks when it mounts).
#[tauri::command]
pub fn browser_state(browser: State<'_, Browser>) -> BrowserState {
    browser.current()
}

#[cfg(test)]
mod tests;
