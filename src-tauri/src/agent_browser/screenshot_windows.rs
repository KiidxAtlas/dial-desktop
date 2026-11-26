//! Capture only the Browser panel, using WebView2's asynchronous PNG encoder.

use std::{sync::mpsc, time::Duration};
use tauri::{Runtime, Webview};
use webview2_com::{
    CapturePreviewCompletedHandler,
    Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
};
use windows::Win32::{
    Foundation::E_OUTOFMEMORY,
    System::Com::IStream,
    UI::Shell::{IStream_Read, IStream_Reset, IStream_Size, SHCreateMemStream},
};

pub fn capture<R: Runtime>(view: &Webview<R>) -> Result<Vec<u8>, String> {
    let (tx, rx) = mpsc::channel();
    view.with_webview(move |platform| {
        // SAFETY: with_webview runs on the controller's UI thread. The completion handler
        // owns a stream reference until WebView2 finishes; only bytes cross to the worker.
        let start = || unsafe {
            let web = platform.controller().CoreWebView2()?;
            let stream = SHCreateMemStream(None).ok_or(E_OUTOFMEMORY)?;
            let output = stream.clone();
            let completed = tx.clone();
            let done = CapturePreviewCompletedHandler::create(Box::new(move |status| {
                let png = status
                    .map_err(|e| e.to_string())
                    .and_then(|()| read_png(&output));
                let _ = completed.send(png);
                Ok(())
            }));
            web.CapturePreview(
                COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                &stream,
                &done,
            )
        };
        if let Err(error) = start() {
            let _ = tx.send(Err(format!("Could not start WebView2 capture: {error}")));
        }
    })
    .map_err(|e| e.to_string())?;
    // Wait on the bridge connection thread, never in the WebView2 message loop.
    rx.recv_timeout(Duration::from_secs(10))
        .map_err(|_| "WebView2 did not return a screenshot".to_owned())?
}

fn read_png(stream: &IStream) -> Result<Vec<u8>, String> {
    // SAFETY: this completed stream is no longer being written, and IStream_Read gets
    // exactly the size of the initialized output buffer (or reports a short read).
    unsafe {
        let size = IStream_Size(stream).map_err(|e| e.to_string())?;
        super::screenshot_wire::check_source_size(size)?;
        IStream_Reset(stream).map_err(|e| e.to_string())?;
        let mut bytes = vec![0; size as usize];
        IStream_Read(stream, bytes.as_mut_ptr().cast(), size as u32).map_err(|e| e.to_string())?;
        Ok(bytes)
    }
}
