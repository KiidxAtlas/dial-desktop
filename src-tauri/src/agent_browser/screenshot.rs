//! A PNG of the browser webview through WebKit's own snapshot (`WKWebView takeSnapshot`).

use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
use objc2_foundation::{NSDictionary, NSError, NSNumber};
use objc2_web_kit::{WKSnapshotConfiguration, WKWebView};
use tauri::{Runtime, Webview};

/// Widest screenshot, in points; WebKit renders at the screen's scale on top.
const MAX_WIDTH: f64 = 1000.0;

/// What the webview shows, as PNG bytes.
pub fn capture<R: Runtime>(view: &Webview<R>) -> Result<Vec<u8>, String> {
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
            if web.frame().size.width > MAX_WIDTH {
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

fn png(image: &NSImage) -> Result<Vec<u8>, String> {
    let tiff = image.TIFFRepresentation().ok_or("The image has no data")?;
    let rep = NSBitmapImageRep::imageRepWithData(&tiff).ok_or("Unreadable image data")?;
    // SAFETY: an empty properties dictionary is valid for every file type.
    let data = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }
    .ok_or("Could not encode PNG")?;
    Ok(data.to_vec())
}
