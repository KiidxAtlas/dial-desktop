//! Native file-manager copies, for paste events that expose no DOM files. Never parse clipboard
//! text as paths or take ownership of the clipboard data.

#[cfg(any(windows, unix, test))]
fn read_list(
    count: usize,
    path_at: impl FnMut(usize) -> Result<String, String>,
) -> Result<Vec<String>, String> {
    let limit = super::MAX_IMAGES;
    if count > limit {
        return Err(format!("Attach at most {limit} files to one prompt"));
    }
    (0..count).map(path_at).collect()
}

#[cfg(windows)]
pub(crate) use windows::read;

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::ptr::null_mut;

    const CF_HDROP: u32 = 15;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn OpenClipboard(owner: *mut c_void) -> i32;
        fn CloseClipboard() -> i32;
        fn IsClipboardFormatAvailable(format: u32) -> i32;
        fn GetClipboardData(format: u32) -> *mut c_void;
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn DragQueryFileW(drop: *mut c_void, index: u32, path: *mut u16, capacity: u32) -> u32;
    }

    struct Clipboard;
    impl Drop for Clipboard {
        fn drop(&mut self) {
            // SAFETY: constructed only after opening on this thread, closed exactly once.
            unsafe { CloseClipboard() };
        }
    }

    pub(crate) fn read() -> Result<Vec<String>, String> {
        // SAFETY: a null window associates the open clipboard with the current task.
        if unsafe { OpenClipboard(null_mut()) } == 0 {
            return Err(format!(
                "Could not open clipboard: {}",
                std::io::Error::last_os_error()
            ));
        }
        let _open = Clipboard;
        // SAFETY: CF_HDROP is a predefined clipboard format; the handle is borrowed until close.
        let drop = unsafe {
            if IsClipboardFormatAvailable(CF_HDROP) == 0 {
                return Ok(Vec::new());
            }
            GetClipboardData(CF_HDROP)
        };
        if drop.is_null() {
            return Err("Could not read the clipboard file list".into());
        }
        // SAFETY: for a live HDROP, UINT_MAX requests the file count, no output buffer needed.
        let count = unsafe { DragQueryFileW(drop, u32::MAX, null_mut(), 0) };
        super::read_list(count as usize, |index| {
            // SAFETY: index is below the queried count; null asks for UTF-16 code units, sans NUL.
            let len = unsafe { DragQueryFileW(drop, index as u32, null_mut(), 0) };
            // Bound the clipboard-controlled allocation to the Windows extended path limit.
            if len == 0 || len >= 32_768 {
                return Err("Invalid path in clipboard file list".into());
            }
            let mut wide = vec![0u16; len as usize + 1];
            // SAFETY: capacity includes the trailing NUL; the buffer and HDROP remain live.
            let copied = unsafe { DragQueryFileW(drop, index as u32, wide.as_mut_ptr(), len + 1) };
            if copied != len {
                return Err("Could not read a path from the clipboard".into());
            }
            String::from_utf16(&wide[..len as usize])
                .map_err(|_| "Clipboard file path is not valid Unicode".into())
        })
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn read() -> Result<Vec<String>, String> {
    use objc2::ClassType;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{
        NSPasteboard, NSPasteboardTypeFileURL, NSPasteboardURLReadingFileURLsOnlyKey,
    };
    use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSURL};

    let board = NSPasteboard::generalPasteboard();
    // SAFETY: these are AppKit's constant type and option names.
    let (file_url, only_files) = unsafe {
        (
            NSPasteboardTypeFileURL,
            NSPasteboardURLReadingFileURLsOnlyKey,
        )
    };
    if board
        .availableTypeFromArray(&NSArray::from_slice(&[file_url]))
        .is_none()
    {
        return Ok(Vec::new());
    }
    let classes = NSArray::from_slice(&[NSURL::class()]);
    let yes = NSNumber::numberWithBool(true);
    let options = NSDictionary::<_, AnyObject>::from_slices(&[only_files], &[&yes]);
    // SAFETY: NSURL implements NSPasteboardReading; this NSURL option expects an NSNumber boolean.
    let urls = unsafe { board.readObjectsForClasses_options(&classes, Some(&options)) }
        .ok_or("Could not read the clipboard file list")?;
    read_list(urls.count(), |index| {
        let url = urls
            .objectAtIndex(index)
            .downcast::<NSURL>()
            .map_err(|_| "Invalid clipboard file URL")?;
        url.to_file_path()
            .and_then(|p| p.into_os_string().into_string().ok())
            .ok_or_else(|| "Invalid clipboard file path".into())
    })
}

/// Linux file managers put copied files on the clipboard as a `text/uri-list` target (not as
/// plain text). Read that target through the session's clipboard tool; none installed, or no
/// file list on the clipboard, is simply "no files".
#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) fn read() -> Result<Vec<String>, String> {
    use std::process::Command;
    use std::time::Duration;
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
    let mut tools: Vec<(&str, &[&str])> = vec![
        ("wl-paste", &["--no-newline", "--type", "text/uri-list"]),
        ("xclip", &["-selection", "clipboard", "-t", "text/uri-list", "-o"]),
    ];
    if !wayland {
        tools.reverse();
    }
    for (tool, args) in tools {
        let out = crate::launcher::run_with_timeout(Command::new(tool).args(args), Duration::from_secs(2));
        if let Some(out) = out {
            let paths = uri_list_paths(&String::from_utf8_lossy(&out));
            return read_list(paths.len(), |index| Ok(paths[index].clone()));
        }
    }
    Ok(Vec::new())
}

/// The local absolute paths in a `text/uri-list` (RFC 2483): `file://` URIs with an empty or
/// `localhost` host, percent-decoded. Comments, other schemes and remote hosts are skipped.
#[cfg(any(test, not(any(windows, target_os = "macos"))))]
fn uri_list_paths(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("file://")?;
            let path = rest.strip_prefix("localhost").filter(|p| p.starts_with('/')).unwrap_or(rest);
            if !path.starts_with('/') {
                return None;
            }
            let bytes = path.as_bytes();
            let mut out = Vec::with_capacity(bytes.len());
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'%' {
                    let hex = path.get(i + 1..i + 3)?;
                    out.push(u8::from_str_radix(hex, 16).ok()?);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            String::from_utf8(out).ok().filter(|p| !p.contains('\0'))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::read_list;
    #[test]
    fn uri_lists_yield_only_local_file_paths() {
        let text = "# copied\r\nfile:///home/a/My%20File.png\r\nfile://localhost/tmp/x\nfile://server/share/y\nhttps://example.com/z\nfile:///bad%zz\n/plain/text\n";
        assert_eq!(super::uri_list_paths(text), ["/home/a/My File.png", "/tmp/x"]);
    }


    #[test]
    fn excess_files_are_rejected_before_reading_any_paths() {
        let limit = super::super::MAX_IMAGES;
        assert_eq!(
            read_list(limit, |i| Ok(i.to_string())).unwrap().len(),
            limit
        );
        assert!(
            read_list(limit + 1, |_| panic!("oversized list was accessed"))
                .unwrap_err()
                .contains("at most")
        );
    }
}
