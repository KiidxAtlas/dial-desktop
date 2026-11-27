//! Custom `dialtype:` protocol serving each Type's sandboxed HTML app with the `window.dial` runtime.

use super::Store;

/// The scheme a type's app loads from, in a sandboxed frame of the types view.
pub const CUSTOM_SCHEME: &str = "dialtype";

/// A type's app runs its own code and may use the web over https (APIs with CORS, CDNs, fonts),
/// in a frame without same-origin access: it cannot reach the app, its files or its IPC.
pub const CUSTOM_CSP: &str = "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval' https: blob:; \
style-src 'unsafe-inline' https:; img-src data: blob: https:; font-src data: https:; \
media-src data: blob: https:; connect-src https: wss:; worker-src blob:; \
form-action 'none'; base-uri 'none'; frame-ancestors *";

/// The `window.dial` runtime injected into every app.
const RUNTIME: &str = include_str!("runtime.js");

/// `html` with the runtime's script first in its head, before any of the app's own scripts.
pub fn with_runtime(html: &str) -> String {
    let script = format!("<script>{RUNTIME}</script>");
    let lower = html.to_ascii_lowercase();
    let after_tag = |name: &str| -> Option<usize> {
        let mut from = 0;
        while let Some(at) = lower[from..].find(name) {
            let at = from + at;
            let next = lower.as_bytes().get(at + name.len()).copied();
            if matches!(next, Some(b'>' | b' ' | b'\t' | b'\n' | b'\r')) {
                return lower[at..].find('>').map(|end| at + end + 1);
            }
            from = at + name.len();
        }
        None
    };
    let at = after_tag("<head")
        .or_else(|| after_tag("<html"))
        .or_else(|| after_tag("<!doctype"))
        .unwrap_or(0);
    let mut out = String::with_capacity(html.len() + script.len());
    out.push_str(&html[..at]);
    out.push_str(&script);
    out.push_str(&html[at..]);
    out
}

/// Answer `dialtype://localhost/<id>`: type `id`'s app under [`CUSTOM_CSP`], with the runtime.
pub fn custom_page(store: &Store, path: &str) -> tauri::http::Response<Vec<u8>> {
    let id = path
        .trim_start_matches('/')
        .split(['/', '?'])
        .next()
        .unwrap_or("");
    let page = store
        .app_html(id)
        .ok()
        .filter(|html| !html.trim().is_empty());
    let (status, body) = match page {
        Some(html) => (200, with_runtime(&html).into_bytes()),
        None => (404, b"This type has no app yet".to_vec()),
    };
    tauri::http::Response::builder()
        .status(status)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Content-Security-Policy", CUSTOM_CSP)
        .header("X-Content-Type-Options", "nosniff")
        .header("Cache-Control", "no-store")
        .body(body)
        .unwrap_or_default()
}
