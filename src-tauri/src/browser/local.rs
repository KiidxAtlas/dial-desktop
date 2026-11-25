//! A tiny static file server for **Open file…** in the Browser panel.
//!
//! The browser only shows web pages (see [`super::allowed`]), so a local HTML file is served over
//! HTTP instead of opened as `file://`: one folder, on `127.0.0.1` at a random port, under a random
//! path prefix, `GET` only, and never anything outside that folder (no `..`, no links that lead
//! out). The server stops when the panel navigates elsewhere, opens another file or closes.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

/// Largest file served; bigger files answer 413.
const MAX_FILE: u64 = 256 << 20;
/// Longest request head read.
const MAX_HEAD: usize = 16 << 10;
const IO_TIMEOUT: Duration = Duration::from_secs(10);

/// A running server for one folder. Dropping it stops the server.
pub struct LocalServer {
    addr: SocketAddr,
    token: String,
    stop: Arc<AtomicBool>,
}

impl LocalServer {
    /// Serve `root` (a directory) on loopback. `root` is canonicalized; everything served must
    /// resolve inside it.
    pub fn start(root: &Path) -> Result<Self, String> {
        let root = fs::canonicalize(root).map_err(|e| format!("Cannot open that folder: {e}"))?;
        if !root.is_dir() {
            return Err("Only a folder can be served".into());
        }
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|e| format!("Could not start the local file server: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let addr = listener.local_addr().map_err(|e| e.to_string())?;
        let token: String = crate::agent_browser::new_token().chars().take(24).collect();
        let stop = Arc::new(AtomicBool::new(false));
        let server = Self {
            addr,
            token: token.clone(),
            stop: stop.clone(),
        };
        thread::Builder::new()
            .name("dial-browser-files".into())
            .spawn(move || accept_loop(listener, root, token, addr, stop))
            .map_err(|e| e.to_string())?;
        Ok(server)
    }

    /// `http://127.0.0.1:PORT/TOKEN/` — the served folder's address.
    pub fn base_url(&self) -> String {
        format!("http://{}/{}/", self.addr, self.token)
    }

    /// The address of `file` (a path relative to the served folder).
    pub fn url_for(&self, relative: &Path) -> String {
        let parts: Vec<String> = relative
            .components()
            .filter_map(|c| match c {
                Component::Normal(name) => Some(encode(&name.to_string_lossy())),
                _ => None,
            })
            .collect();
        format!("{}{}", self.base_url(), parts.join("/"))
    }

    /// Whether `url` is served by this server.
    pub fn serves(&self, url: &str) -> bool {
        url.starts_with(&self.base_url())
    }
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn accept_loop(
    listener: TcpListener,
    root: PathBuf,
    token: String,
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
) {
    let root = Arc::new(root);
    let token = Arc::new(token);
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, peer)) => {
                if !peer.ip().is_loopback() {
                    let _ = stream.shutdown(Shutdown::Both);
                    continue;
                }
                let root = root.clone();
                let token = token.clone();
                let stop = stop.clone();
                let _ = thread::Builder::new()
                    .name("dial-browser-file".into())
                    .spawn(move || {
                        if !stop.load(Ordering::SeqCst) {
                            let _ = handle(stream, &root, &token, addr);
                        }
                    });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(40));
            }
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    }
}

fn handle(mut stream: TcpStream, root: &Path, token: &str, addr: SocketAddr) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut reader = BufReader::new(stream.try_clone()?).take(MAX_HEAD as u64);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let mut host = String::new();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.trim().eq_ignore_ascii_case("host")
        {
            host = value.trim().to_owned();
        }
    }
    if method != "GET" {
        return respond(&mut stream, 405, "text/plain", b"Only GET is served", &[("Allow", "GET")]);
    }
    // A page elsewhere cannot reach this server through a name it controls (DNS rebinding).
    let port = addr.port();
    if host != format!("127.0.0.1:{port}") && host != format!("localhost:{port}") {
        return respond(&mut stream, 403, "text/plain", b"Forbidden", &[]);
    }
    let path = match resolve(root, token, target) {
        Ok(path) => path,
        Err(status) => return respond(&mut stream, status, "text/plain", reason(status).as_bytes(), &[]),
    };
    let Ok(meta) = fs::metadata(&path) else {
        return respond(&mut stream, 404, "text/plain", b"Not found", &[]);
    };
    if meta.len() > MAX_FILE {
        return respond(&mut stream, 413, "text/plain", b"File too large", &[]);
    }
    match fs::read(&path) {
        Ok(body) => respond(&mut stream, 200, content_type(&path), &body, &[]),
        Err(_) => respond(&mut stream, 404, "text/plain", b"Not found", &[]),
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        _ => "Error",
    }
}

fn respond(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
    extra: &[(&str, &str)],
) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n",
        reason(status),
        body.len()
    );
    for (name, value) in extra {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

/// The file `target` (a request target like `/TOKEN/docs/index.html?x=1`) names inside `root`,
/// or the HTTP status to answer. `root` must already be canonical. A folder serves its
/// `index.html`. Anything that is not under the token prefix, uses `..`, a backslash or a NUL, or
/// resolves (through links) outside `root` is refused.
pub fn resolve(root: &Path, token: &str, target: &str) -> Result<PathBuf, u16> {
    let path = target.split(['?', '#']).next().unwrap_or("");
    let prefix = format!("/{token}/");
    let rest = if path == format!("/{token}") {
        ""
    } else {
        path.strip_prefix(&prefix).ok_or(404u16)?
    };
    let decoded = decode(rest).ok_or(400u16)?;
    if decoded.contains('\0') || decoded.contains('\\') {
        return Err(400);
    }
    let mut joined = root.to_path_buf();
    for segment in decoded.split('/') {
        match segment {
            "" | "." => continue,
            ".." => return Err(403),
            name => {
                let part = Path::new(name);
                if part.components().count() != 1
                    || !matches!(part.components().next(), Some(Component::Normal(_)))
                {
                    return Err(403);
                }
                joined.push(name);
            }
        }
    }
    let mut real = fs::canonicalize(&joined).map_err(|_| 404u16)?;
    if !real.starts_with(root) {
        return Err(403);
    }
    if real.is_dir() {
        real = fs::canonicalize(real.join("index.html")).map_err(|_| 404u16)?;
        if !real.starts_with(root) {
            return Err(403);
        }
    }
    if !real.is_file() {
        return Err(404);
    }
    Ok(real)
}

/// Percent-decode a URL path; `None` when it is not valid UTF-8 or has a broken escape.
fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Percent-encode one path segment.
pub fn encode(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn content_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "wasm" => "application/wasm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "txt" | "md" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let site = dir.path().join("site");
        fs::create_dir_all(site.join("sub")).unwrap();
        fs::write(site.join("index.html"), "<h1>home</h1>").unwrap();
        fs::write(site.join("sub").join("a b.css"), "p{}").unwrap();
        fs::write(dir.path().join("secret.txt"), "nope").unwrap();
        let root = fs::canonicalize(&site).unwrap();
        (dir, root)
    }

    #[test]
    fn serves_only_files_inside_the_folder() {
        let (_dir, root) = fixture();
        let t = "tok";
        let ok = |target: &str| resolve(&root, t, target);
        assert_eq!(ok("/tok/index.html").unwrap(), root.join("index.html"));
        assert_eq!(ok("/tok/").unwrap(), root.join("index.html"));
        assert_eq!(ok("/tok").unwrap(), root.join("index.html"));
        assert_eq!(ok("/tok/sub/a%20b.css?v=1#x").unwrap(), root.join("sub").join("a b.css"));
        for (target, status) in [
            ("/index.html", 404),
            ("/other/index.html", 404),
            ("/tok/../secret.txt", 403),
            ("/tok/sub/../../secret.txt", 403),
            ("/tok/%2e%2e/secret.txt", 403),
            ("/tok/sub/%2E%2E/%2E%2E/secret.txt", 403),
            ("/tok/..%5csecret.txt", 400),
            ("/tok/%00", 400),
            ("/tok/%zz", 400),
            ("/tok/missing.html", 404),
            ("/tok/sub", 404),
        ] {
            assert_eq!(ok(target), Err(status), "{target}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn links_that_lead_out_are_refused() {
        let (dir, root) = fixture();
        std::os::unix::fs::symlink(dir.path().join("secret.txt"), root.join("leak.txt")).unwrap();
        std::os::unix::fs::symlink(dir.path(), root.join("up")).unwrap();
        assert_eq!(resolve(&root, "tok", "/tok/leak.txt"), Err(403));
        assert_eq!(resolve(&root, "tok", "/tok/up/secret.txt"), Err(403));
    }

    #[test]
    fn urls_are_encoded_per_segment() {
        assert_eq!(encode("a b#?.html"), "a%20b%23%3F.html");
        assert_eq!(decode(&encode("ünï code.html")).unwrap(), "ünï code.html");
    }

    #[test]
    fn the_server_answers_get_on_loopback_only_for_its_folder() {
        let (_dir, root) = fixture();
        let server = LocalServer::start(&root).unwrap();
        let base = server.base_url();
        assert!(base.starts_with("http://127.0.0.1:"));
        assert!(server.serves(&server.url_for(Path::new("sub/a b.css"))));
        assert!(!server.serves("http://127.0.0.1:1/x"));
        let request = |method: &str, path: &str, host: Option<String>| {
            let mut s = TcpStream::connect(server.addr).unwrap();
            let host = host.unwrap_or_else(|| server.addr.to_string());
            write!(s, "{method} {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
            out
        };
        let token = server.token.clone();
        let home = request("GET", &format!("/{token}/"), None);
        assert!(home.starts_with("HTTP/1.1 200"), "{home}");
        assert!(home.contains("text/html") && home.ends_with("<h1>home</h1>"));
        assert!(request("POST", &format!("/{token}/"), None).starts_with("HTTP/1.1 405"));
        assert!(request("GET", &format!("/{token}/../secret.txt"), None).starts_with("HTTP/1.1 403"));
        assert!(request("GET", "/index.html", None).starts_with("HTTP/1.1 404"));
        assert!(
            request("GET", &format!("/{token}/"), Some("evil.example".into())).starts_with("HTTP/1.1 403")
        );
        let addr = server.addr;
        drop(server);
        thread::sleep(Duration::from_millis(150));
        // Stopped: the listener is gone once the accept loop sees the flag.
        let gone = TcpStream::connect_timeout(&addr, Duration::from_millis(300))
            .map(|mut s| {
                let _ = write!(s, "GET / HTTP/1.1\r\nHost: {addr}\r\n\r\n");
                let mut out = String::new();
                let _ = s.read_to_string(&mut out);
                out
            })
            .unwrap_or_default();
        assert!(gone.is_empty(), "{gone}");
    }
}
