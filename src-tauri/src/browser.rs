//! The integrated browser: one child webview per tab, laid over the Browser panel of the main window.
//!
//! The web UI owns the panel and reports where it sits ([`Bounds`], logical pixels); this module
//! creates the active tab's webview on first use, moves, hides and switches the tabs, and reports
//! them as the `browser` event ([`BrowserState`]). Only the active tab is shown; the others keep
//! their pages loaded. Pages get no access to the app: each webview has no capability (see
//! `capabilities/default.json`, which names only the `main` webview), and navigation is limited to
//! web pages. [`inspect`] reads the active page's address, title, selected text, visible text and
//! the errors it logged, so the user can hand them to the agent.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, mpsc};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Rect, Runtime, State, Url, Webview,
    WebviewBuilder, WebviewUrl,
};

mod local;

/// The prefix of every tab's webview label; tab `n` is `browser-n` (see [`tab_label`]).
pub const LABEL: &str = "browser";
/// What a query without an address searches, unless the user chose another engine.
const SEARCH: &str = "https://duckduckgo.com/";
/// Stands for the query in a search engine template.
const QUERY: &str = "%s";
/// How long [`inspect`] waits for the page to answer.
const INSPECT_TIMEOUT: Duration = Duration::from_secs(3);
/// The address of a tab that has no page yet.
const BLANK: &str = "about:blank";
/// Most tabs open at once; each one is a web view with its own page.
pub const MAX_TABS: usize = 16;

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

/// The webview label of tab `id`.
pub fn tab_label(id: u64) -> String {
    format!("{LABEL}-{id}")
}

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

/// One tab as the UI and the agent see it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabState {
    pub id: u64,
    pub url: String,
    pub title: String,
}

/// The browser's tabs, published as the `browser` event. `url`, `title` and `loading` describe the
/// active tab; `activeTab` is 0 and `tabs` is empty before any tab exists.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserState {
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub active_tab: u64,
    pub tabs: Vec<TabState>,
}

/// A navigation of a tab: which tab, and its sequence number (larger is later).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nav {
    pub tab: u64,
    pub started: u64,
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

/// One tab: its page as the webview reports it, and the folder it serves, if any.
struct Tab {
    id: u64,
    url: String,
    title: String,
    loading: bool,
    /// Sequence number of the tab's latest navigation start (see [`Browser::navs`]).
    started: u64,
    /// The address that navigation asked for.
    started_url: String,
    /// The folder **Open file…** serves for this tab, while its page is on show.
    local: Option<local::LocalServer>,
}

impl Tab {
    fn new(id: u64, url: &str) -> Self {
        Self {
            id,
            url: url.to_owned(),
            title: String::new(),
            loading: false,
            started: 0,
            started_url: String::new(),
            local: None,
        }
    }

    /// Stop serving the folder unless the page `url` is served by it.
    fn keep_local(&mut self, url: &str) {
        if self.local.as_ref().is_some_and(|server| !server.serves(url)) {
            self.local = None;
        }
    }
}

/// The tabs in order, which one is active, and whether the panel shows them.
#[derive(Default)]
struct Tabs {
    list: Vec<Tab>,
    active: u64,
    /// The last id handed out; ids are never reused.
    last_id: u64,
    /// Where the panel sits, when it has reported that.
    bounds: Option<Bounds>,
    /// Whether the panel shows the active tab.
    shown: bool,
}

impl Tabs {
    /// Add a tab showing `url` and make it active.
    fn add(&mut self, url: &str) -> Result<u64, String> {
        if self.list.len() >= MAX_TABS {
            return Err(format!("Too many tabs open ({MAX_TABS}); close one first"));
        }
        self.last_id += 1;
        let id = self.last_id;
        self.list.push(Tab::new(id, url));
        self.active = id;
        Ok(id)
    }

    /// The active tab's id, adding a blank tab first when there is none. The flag says whether it
    /// added one.
    fn ensure(&mut self) -> (u64, bool) {
        if self.list.is_empty() {
            (self.add(BLANK).unwrap_or(0), true)
        } else {
            (self.active, false)
        }
    }

    fn select(&mut self, id: u64) -> Result<(), String> {
        if !self.list.iter().any(|t| t.id == id) {
            return Err(format!("No browser tab {id}"));
        }
        self.active = id;
        Ok(())
    }

    /// Remove tab `id`. Closing the active tab activates its neighbour; closing the last tab
    /// leaves a blank one, so there is always a tab to show.
    fn remove(&mut self, id: u64) -> Result<Tab, String> {
        let index = self
            .list
            .iter()
            .position(|t| t.id == id)
            .ok_or_else(|| format!("No browser tab {id}"))?;
        let tab = self.list.remove(index);
        if self.list.is_empty() {
            self.add(BLANK)?;
        } else if self.active == id {
            self.active = self.list[index.min(self.list.len() - 1)].id;
        }
        Ok(tab)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut Tab> {
        self.list.iter_mut().find(|t| t.id == id)
    }

    fn active_tab(&self) -> Option<&Tab> {
        self.list.iter().find(|t| t.id == self.active)
    }

    /// The active tab's navigation to `url` that started after sequence number `since` and has
    /// finished. The address must match exactly: a stale page on the same origin does not count.
    fn opened(&self, url: &Url, since: u64) -> Option<Nav> {
        let tab = self.active_tab()?;
        let same = Url::parse(&tab.started_url).is_ok_and(|u| &u == url);
        (same && !tab.loading && tab.started > since).then_some(Nav {
            tab: tab.id,
            started: tab.started,
        })
    }

    /// The state the UI and the agent see.
    fn state(&self) -> BrowserState {
        let active = self.active_tab();
        BrowserState {
            url: active.map(|t| t.url.clone()).unwrap_or_default(),
            title: active.map(|t| t.title.clone()).unwrap_or_default(),
            loading: active.is_some_and(|t| t.loading),
            active_tab: self.active,
            tabs: self
                .list
                .iter()
                .map(|t| TabState {
                    id: t.id,
                    url: t.url.clone(),
                    title: t.title.clone(),
                })
                .collect(),
        }
    }
}

/// The browser's state, managed by the app.
#[derive(Default)]
pub struct Browser {
    tabs: Mutex<Tabs>,
    /// Pages finished loading so far; the agent's tools wait for it to move.
    loads: AtomicU64,
    /// Navigation starts so far, across all tabs; numbers each navigation for [`Browser::opened`].
    navs: AtomicU64,
    /// The user's search engine template (`%s` is the query); `None` searches DuckDuckGo.
    search: Mutex<Option<String>>,
}

impl Browser {
    fn tabs(&self) -> MutexGuard<'_, Tabs> {
        self.tabs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Change tab `id` with `change` and publish the state. The lock is never held while
    /// publishing or while a webview call may wait for the main thread.
    fn update_tab<R: Runtime>(&self, app: &AppHandle<R>, id: u64, change: impl FnOnce(&mut Tab)) {
        let state = {
            let mut tabs = self.tabs();
            let Some(tab) = tabs.get_mut(id) else {
                return;
            };
            change(tab);
            tabs.state()
        };
        publish(app, &state);
    }

    /// The tabs as they are now.
    pub fn current(&self) -> BrowserState {
        self.tabs().state()
    }

    /// The active tab's id (0 when there is none).
    pub fn active(&self) -> u64 {
        self.tabs().active
    }

    /// The active tab's latest navigation.
    pub fn nav(&self) -> Option<Nav> {
        self.tabs().active_tab().map(|t| Nav {
            tab: t.id,
            started: t.started,
        })
    }

    /// How many navigations have started so far (across tabs).
    pub fn navs(&self) -> u64 {
        self.navs.load(Ordering::SeqCst)
    }

    /// The active tab's navigation to `url` that started after `since` and has finished.
    pub fn opened(&self, url: &Url, since: u64) -> Option<Nav> {
        self.tabs().opened(url, since)
    }

    /// How many page loads have finished.
    pub fn loads(&self) -> u64 {
        self.loads.load(Ordering::SeqCst)
    }

    fn search(&self) -> Option<String> {
        self.search
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Publish the tabs as the `browser` event.
fn publish<R: Runtime>(app: &AppHandle<R>, state: &BrowserState) {
    let _ = app.emit("browser", state.clone());
}

/// What the user typed into the address bar as a URL to open: a web address as is, a bare host
/// over HTTPS (HTTP for this computer), anything else as a search.
pub fn normalize_url(input: &str) -> Result<String, String> {
    normalize_url_with(input, None)
}

/// [`normalize_url`], searching with `search` (a template where `%s` is the query) when given.
pub fn normalize_url_with(input: &str, search: Option<&str>) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Type an address or a search".into());
    }
    if input == BLANK {
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
    search_url(search, input)
}

/// Check a search engine template: an http(s) address with `%s` where the query goes.
pub fn check_search_template(template: &str) -> Result<(), String> {
    let template = template.trim();
    if !template.contains(QUERY) {
        return Err("A search address needs %s where the search words go".into());
    }
    let url = Url::parse(&template.replace(QUERY, "test"))
        .map_err(|e| format!("Not a valid search address: {e}"))?;
    match url.scheme() {
        "http" | "https" => Ok(()),
        _ => Err("A search address must start with https:// or http://".into()),
    }
}

/// The address that searches for `query`: with the user's `template`, else DuckDuckGo.
fn search_url(template: Option<&str>, query: &str) -> Result<String, String> {
    let Some(template) = template.map(str::trim).filter(|t| !t.is_empty()) else {
        return Url::parse_with_params(SEARCH, &[("q", query)])
            .map(|url| url.to_string())
            .map_err(|e| e.to_string());
    };
    check_search_template(template)?;
    let encoded: String = query
        .split(' ')
        .map(local::encode)
        .collect::<Vec<_>>()
        .join("+");
    Url::parse(&template.replace(QUERY, &encoded))
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

/// The active tab's webview.
pub fn active_view<R: Runtime>(app: &AppHandle<R>) -> Result<Webview<R>, String> {
    let id = app.state::<Browser>().active();
    app.get_webview(&tab_label(id))
        .ok_or_else(|| "The browser is not open".to_owned())
}

/// Create the webview of tab `id`, showing `url` at `bounds`.
fn create<R: Runtime>(app: &AppHandle<R>, id: u64, url: &str, bounds: Bounds) -> Result<(), String> {
    let url = Url::parse(url).map_err(|e| e.to_string())?;
    let window = app
        .get_window("main")
        .ok_or("The main window is not available")?;
    let loads = app.clone();
    let titles = app.clone();
    let popups = app.clone();
    let builder = WebviewBuilder::new(tab_label(id), WebviewUrl::External(url))
        .initialization_script(INIT_SCRIPT)
        .on_navigation(allowed)
        .on_page_load(move |_, payload| {
            let url = payload.url().to_string();
            let started = matches!(payload.event(), PageLoadEvent::Started);
            let browser = loads.state::<Browser>();
            let seq = if started {
                browser.navs.fetch_add(1, Ordering::SeqCst) + 1
            } else {
                browser.loads.fetch_add(1, Ordering::SeqCst);
                0
            };
            browser.update_tab(&loads, id, |tab| {
                if tab.url != url {
                    tab.title.clear();
                }
                tab.url = url.clone();
                tab.loading = started;
                if started {
                    tab.started = seq;
                    tab.started_url = url.clone();
                    tab.keep_local(&url);
                }
            });
        })
        .on_document_title_changed(move |_, title| {
            titles
                .state::<Browser>()
                .update_tab(&titles, id, |tab| tab.title = title);
        })
        // Links that open a new window open as a new tab, which the panel then shows.
        .on_new_window(move |url, _| {
            if allowed(&url) {
                let app = popups.clone();
                drop(tauri::async_runtime::spawn_blocking(move || open_popup(&app, url)));
            }
            NewWindowResponse::Deny
        });
    let rect = bounds.rect();
    window
        .add_child(builder, rect.position, rect.size)
        .map(|_| ())
        .map_err(|e| format!("Could not open the browser: {e}"))
}

/// A link opened a new window: add it as a tab, which becomes active and is shown.
fn open_popup<R: Runtime>(app: &AppHandle<R>, url: Url) {
    let browser = app.state::<Browser>();
    if browser.tabs().add(url.as_str()).is_ok() {
        let _ = present(app);
        publish(app, &browser.current());
    }
}

/// Make the webviews match the tabs: the active tab is shown at the panel's bounds while the panel
/// shows, every other tab is hidden, and the active tab's webview is created if it is missing.
/// Returns whether it created one.
fn present<R: Runtime>(app: &AppHandle<R>) -> Result<bool, String> {
    let browser = app.state::<Browser>();
    let (active, shown, bounds, tabs) = {
        let tabs = browser.tabs();
        (
            tabs.active,
            tabs.shown,
            tabs.bounds,
            tabs.list
                .iter()
                .map(|t| (t.id, t.url.clone()))
                .collect::<Vec<_>>(),
        )
    };
    let mut created = false;
    for (id, url) in tabs {
        let show = shown && id == active;
        match (app.get_webview(&tab_label(id)), bounds) {
            (Some(view), Some(rect)) if show => {
                let _ = view.set_bounds(rect.rect());
                let _ = view.show();
            }
            (Some(view), _) => {
                let _ = view.hide();
            }
            (None, Some(rect)) if show => {
                create(app, id, &url, rect)?;
                created = true;
            }
            (None, _) => {}
        }
    }
    Ok(created)
}

/// **New**, **select** or **close** a tab (`action`); `id` is needed for select and close. Returns
/// the tabs and publishes them. Blocks; never call it on the main thread.
pub fn tab_action<R: Runtime>(
    app: &AppHandle<R>,
    action: &str,
    id: Option<u64>,
) -> Result<BrowserState, String> {
    let browser = app.state::<Browser>();
    let need = || id.ok_or_else(|| "Say which tab: `id`".to_owned());
    match action {
        "new" => {
            browser.tabs().add(BLANK)?;
        }
        "select" => browser.tabs().select(need()?)?,
        "close" => {
            let id = need()?;
            let closed = browser.tabs().remove(id)?;
            if let Some(view) = app.get_webview(&tab_label(id)) {
                let _ = view.close();
            }
            drop(closed);
        }
        other => {
            return Err(format!(
                "Unknown tab action `{other}`: use new, select or close"
            ));
        }
    }
    let result = present(app);
    let state = browser.current();
    publish(app, &state);
    result?;
    Ok(state)
}

/// **New**, **select** or **close** a tab; see [`tab_action`].
#[tauri::command]
pub async fn browser_tab(
    app: AppHandle,
    action: String,
    id: Option<u64>,
) -> Result<BrowserState, String> {
    tauri::async_runtime::spawn_blocking(move || tab_action(&app, &action, id))
        .await
        .map_err(|e| e.to_string())?
}

/// The tabs as they are now (the panel asks when it mounts).
#[tauri::command]
pub fn browser_state(browser: State<'_, Browser>) -> BrowserState {
    browser.current()
}

/// Search with `template` (an http(s) address where `%s` is the query); `None` or empty
/// returns to DuckDuckGo. Only what the user types in the address bar uses it.
#[tauri::command]
pub fn browser_set_search(
    browser: State<'_, Browser>,
    template: Option<String>,
) -> Result<(), String> {
    let template = template
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty());
    if let Some(template) = &template {
        check_search_template(template)?;
    }
    *browser.search.lock().unwrap_or_else(PoisonError::into_inner) = template;
    Ok(())
}

/// Show the active tab at `bounds`, creating its webview on first use; opens `url` in it when
/// given (a new tab's first page is opened by its creation).
#[tauri::command]
pub async fn browser_show(
    app: AppHandle,
    bounds: Bounds,
    url: Option<String>,
) -> Result<BrowserState, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let search = app.state::<Browser>().search();
        let url = url
            .as_deref()
            .map(|u| normalize_url_with(u, search.as_deref()))
            .transpose()?;
        let browser = app.state::<Browser>();
        {
            let mut tabs = browser.tabs();
            tabs.ensure();
            tabs.bounds = Some(bounds);
            tabs.shown = true;
            if let Some(url) = &url {
                let id = tabs.active;
                if let Some(tab) = tabs.get_mut(id) {
                    tab.url = url.clone();
                }
            }
        }
        let created = present(&app)?;
        if let Some(url) = url.filter(|_| !created) {
            navigate_to(&active_view(&app)?, &url)?;
        }
        let state = browser.current();
        publish(&app, &state);
        Ok(state)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Move the browser to follow its panel.
#[tauri::command]
pub async fn browser_bounds(app: AppHandle, bounds: Bounds) -> Result<(), String> {
    app.state::<Browser>().tabs().bounds = Some(bounds);
    present(&app).map(|_| ())
}

/// Hide the browser (another panel or an overlay is on top); the pages stay loaded.
#[tauri::command]
pub async fn browser_hide(app: AppHandle) -> Result<(), String> {
    app.state::<Browser>().tabs().shown = false;
    present(&app).map(|_| ())
}

fn navigate_to<R: Runtime>(view: &Webview<R>, url: &str) -> Result<(), String> {
    let url = Url::parse(url).map_err(|e| e.to_string())?;
    view.navigate(url).map_err(|e| e.to_string())
}

/// Open what the user typed in the active tab; returns the address opened.
#[tauri::command]
pub async fn browser_navigate(
    app: AppHandle,
    browser: State<'_, Browser>,
    url: String,
) -> Result<String, String> {
    let url = normalize_url_with(&url, browser.search().as_deref())?;
    let view = active_view(&app)?;
    let id = browser.active();
    browser.update_tab(&app, id, |tab| tab.url = url.clone());
    navigate_to(&view, &url)?;
    Ok(url)
}

/// `back`, `forward`, `reload` or `stop`, in the active tab.
#[tauri::command]
pub async fn browser_go(app: AppHandle, action: String) -> Result<(), String> {
    let view = active_view(&app)?;
    let result = match action.as_str() {
        "back" => view.eval("history.back()"),
        "forward" => view.eval("history.forward()"),
        "reload" => view.reload(),
        "stop" => view.eval("window.stop()"),
        other => return Err(format!("Unknown browser action `{other}`")),
    };
    result.map_err(|e| e.to_string())
}

/// The active page's address, title, selection, text and logged errors.
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

/// Run `script` in the active page and return its answer as the platform hands it back. Blocks;
/// never call it on the main thread, which delivers the answer.
pub fn eval_raw<R: Runtime>(
    app: &AppHandle<R>,
    script: &str,
    timeout: Duration,
) -> Result<String, String> {
    let view = active_view(app)?;
    eval_view_raw(&view, script, timeout)
}

/// Evaluate a fixed internal script in a caller-selected webview, off the main thread.
pub(crate) fn eval_view_raw<R: Runtime>(
    view: &Webview<R>,
    script: &str,
    timeout: Duration,
) -> Result<String, String> {
    let (tx, rx) = mpsc::channel();
    view.eval_with_callback(script, move |raw| {
        let _ = tx.send(raw);
    })
    .map_err(|e| e.to_string())?;
    rx.recv_timeout(timeout).map_err(|_| {
        "The page did not answer: it may be busy or still loading, or the script failed".to_owned()
    })
}

/// What this build's browser can do beyond browsing.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserFeatures {
    /// Web Inspector is compiled in (via the `tauri/devtools` feature).
    pub devtools: bool,
}

#[tauri::command]
pub fn browser_features() -> BrowserFeatures {
    BrowserFeatures { devtools: true }
}

/// Open the active page's Web Inspector.
#[tauri::command]
pub async fn browser_devtools(app: AppHandle) -> Result<(), String> {
    let view = active_view(&app)?;
    view.open_devtools();
    Ok(())
}

/// **Open file…**: pick a local HTML file in a native dialog and serve its folder on loopback.
/// Returns the page's `http://127.0.0.1:…` address for the panel to open, or `None` when
/// cancelled. The webview itself still only opens web pages.
#[tauri::command]
pub async fn browser_open_file(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(picked) = app
            .dialog()
            .file()
            .set_title("Open a local web page")
            .add_filter("Web page", &["html", "htm", "xhtml", "svg"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = picked.into_path().map_err(|e| e.to_string())?;
        let browser = app.state::<Browser>();
        let (id, created) = browser.tabs().ensure();
        if created {
            publish(&app, &browser.current());
        }
        let url = serve_file(&browser, id, &path)?;
        Ok(Some(url))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Serve `file`'s folder for tab `id` (replacing the folder that tab served before) and return the
/// file's address.
fn serve_file(browser: &Browser, id: u64, file: &Path) -> Result<String, String> {
    let file = std::fs::canonicalize(file).map_err(|e| format!("Cannot open that file: {e}"))?;
    if !file.is_file() {
        return Err("Choose a file".into());
    }
    let folder = file.parent().ok_or("That file has no folder")?;
    let name = file.file_name().ok_or("That file has no name")?;
    let server = local::LocalServer::start(folder)?;
    let url = server.url_for(Path::new(name));
    let mut tabs = browser.tabs();
    let tab = tabs.get_mut(id).ok_or("That tab is closed")?;
    tab.local = Some(server);
    Ok(url)
}

/// Stop serving local folders (the panel closed).
#[tauri::command]
pub fn browser_stop_local(browser: State<'_, Browser>) {
    for tab in browser.tabs().list.iter_mut() {
        tab.local = None;
    }
}

#[cfg(test)]
mod tests;
