//! Capture the Browser panel or the visible Type canvas through WebKit's own snapshot API.

use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
use objc2_foundation::{NSDictionary, NSError, NSNumber, NSPoint, NSRect, NSSize};
use objc2_web_kit::{WKSnapshotConfiguration, WKWebView};
use tauri::{Runtime, Webview};

/// Widest screenshot, in points; WebKit renders at the screen's scale on top.
const MAX_WIDTH: f64 = 1000.0;

/// What the webview shows, as PNG bytes. Snapshot only `rect` when the caller requests a region.
pub fn capture<R: Runtime>(view: &Webview<R>, rect: Option<NSRect>) -> Result<Vec<u8>, String> {
    let (tx, rx) = mpsc::channel::<Result<Vec<u8>, String>>();
    let failed = tx.clone();
    view.with_webview(move |platform| {
        let Some(mtm) = MainThreadMarker::new() else {
            let _ = failed.send(Err("Not on the main thread".into()));
            return;
        };
        // SAFETY: on macOS the platform webview is the `WKWebView` wry created, alive for this
        // call; WebKit is used on the main thread, which `with_webview` runs us on.
        unsafe {
            let web: &WKWebView = &*platform.inner().cast::<WKWebView>();
            let config = WKSnapshotConfiguration::new(mtm);
            let width = if let Some(rect) = rect {
                config.setRect(rect);
                rect.size.width
            } else {
                web.frame().size.width
            };
            if width > MAX_WIDTH {
                config.setSnapshotWidth(Some(&NSNumber::new_f64(MAX_WIDTH)));
            }
            let done = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
                let result = match image.as_ref() {
                    Some(image) => png(image),
                    None => Err(error
                        .as_ref()
                        .map(|e| e.localizedDescription().to_string())
                        .unwrap_or_else(|| "WebKit returned no image".into())),
                };
                let _ = tx.send(result);
            });
            web.takeSnapshotWithConfiguration_completionHandler(Some(&config), &done);
        }
    })
    .map_err(|e| e.to_string())?;
    rx.recv_timeout(Duration::from_secs(10))
        .map_err(|_| "The screenshot did not finish".to_owned())?
}

/// Only the specified, currently visible Type may be captured. Check again after the asynchronous
/// snapshot so switching views or resizing the window does not disclose the wrong screen.
pub fn capture_type<R: Runtime>(view: &Webview<R>, id: &str) -> Result<Vec<u8>, String> {
    let script = format!("({TYPE_RECT})({})", serde_json::json!(id));
    let bounds = || {
        let raw = crate::browser::eval_view_raw(view, &script, super::SCRIPT_TIMEOUT)?;
        type_rect(crate::browser::unwrap_json(&raw)?)
    };
    let before = bounds()?;
    let png = capture(view, Some(before))?;
    if bounds()? != before {
        return Err("The Type view changed during capture; try again once it settles".into());
    }
    Ok(png)
}

const TYPE_RECT: &str = r#"(id) => {
  const canvas = document.querySelector('[data-type-capture]');
  if (canvas?.dataset.typeCapture !== id || canvas.closest('[hidden], [inert]') || document.visibilityState !== 'visible') return 'null';
  const {left, top, right, bottom} = canvas.getBoundingClientRect();
  const x = Math.ceil(Math.max(0, left)), y = Math.ceil(Math.max(0, top));
  const r = Math.floor(Math.min(innerWidth, right)), b = Math.floor(Math.min(innerHeight, bottom));
  if (r - x < 2 || b - y < 2) return 'null';
  // Sample inside the rounded corners, which are transparent by design.
  const dx = Math.min(16, (r - x) / 2), dy = Math.min(16, (b - y) / 2);
  const xs = [x + dx, (x + r) / 2, r - dx], ys = [y + dy, (y + b) / 2, b - dy];
  if (!xs.every(px => ys.every(py => canvas.contains(document.elementFromPoint(px, py))))) return 'null';
  return JSON.stringify([x, y, r - x, b - y]);
}"#;

fn type_rect(value: serde_json::Value) -> Result<NSRect, String> {
    let [x, y, width, height]: [u32; 4] = serde_json::from_value(value).map_err(|_| {
        "Open the requested Type and close any overlays before taking a screenshot".to_owned()
    })?;
    if width < 2 || height < 2 {
        return Err("The Type has no visible canvas".into());
    }
    Ok(NSRect::new(
        NSPoint::new(x.into(), y.into()),
        NSSize::new(width.into(), height.into()),
    ))
}

fn png(image: &NSImage) -> Result<Vec<u8>, String> {
    let tiff = image.TIFFRepresentation().ok_or("The image has no data")?;
    let rep = NSBitmapImageRep::imageRepWithData(&tiff).ok_or("Unreadable image data")?;
    // SAFETY: an empty properties dictionary is valid for every file type.
    let data = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }
    .ok_or("Could not encode PNG")?;
    super::screenshot_wire::check_source_size(data.length() as u64)?;
    Ok(data.to_vec())
}
