//! Images attached to a prompt.
//!
//! A file chosen in the picker or dropped on the window stays a path: the engine reads it when the
//! turn starts. The UI still needs to show it, so [`read_image`] returns a small data URL preview
//! of the same bytes. Pasted images have no file and travel as base64 (`inline_images`).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;
use std::path::Path;

#[cfg(test)]
mod tests;

/// Larger files are refused before reading; model providers reject them anyway.
pub const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
/// Attachments per prompt.
pub const MAX_IMAGES: usize = 10;

/// A readable image file, ready to show and to send by path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageFile {
    pub path: String,
    pub name: String,
    /// `data:` URL of the file, for the thumbnail.
    pub url: String,
}

/// The MIME type of an image file, from its extension; `None` for anything else.
pub fn image_mime(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    })
}

/// Check and preview the image at `path`.
pub fn read_image(path: &Path) -> Result<ImageFile, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let mime =
        image_mime(path).ok_or_else(|| format!("{name} is not a PNG, JPEG, GIF or WebP image"))?;
    let size = std::fs::metadata(path)
        .map_err(|error| format!("Could not read {name}: {error}"))?
        .len();
    if size > MAX_IMAGE_BYTES {
        return Err(format!(
            "{name} is {} MB; images can be at most {} MB",
            size.div_ceil(1024 * 1024),
            MAX_IMAGE_BYTES / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(path).map_err(|error| format!("Could not read {name}: {error}"))?;
    Ok(ImageFile {
        path: path.to_string_lossy().into_owned(),
        name,
        url: format!("data:{mime};base64,{}", STANDARD.encode(bytes)),
    })
}

/// Check a prompt's attachments: paths of supported images and valid base64 for pasted ones.
pub fn check(images: &[String], inline_images: &[String]) -> Result<(), String> {
    if images.len() + inline_images.len() > MAX_IMAGES {
        return Err(format!("Attach at most {MAX_IMAGES} images to one prompt"));
    }
    if let Some(path) = images.iter().find(|p| image_mime(Path::new(p)).is_none()) {
        return Err(format!("{path} is not a PNG, JPEG, GIF or WebP image"));
    }
    for data in inline_images {
        let data = data.trim();
        if data.len() > (MAX_IMAGE_BYTES as usize).div_ceil(3) * 4 {
            return Err(pasted_image_too_large());
        }
        let bytes = STANDARD
            .decode(data)
            .map_err(|_| "A pasted image could not be read")?;
        if bytes.is_empty() {
            return Err("A pasted image could not be read".into());
        }
        if bytes.len() > MAX_IMAGE_BYTES as usize {
            return Err(pasted_image_too_large());
        }
    }
    Ok(())
}

fn pasted_image_too_large() -> String {
    format!(
        "Pasted images can be at most {} MB",
        MAX_IMAGE_BYTES / (1024 * 1024)
    )
}

/// Read only image data, called on macOS when the composer receives a paste without a DOM file.
#[cfg(target_os = "macos")]
pub fn clipboard_image() -> Result<Option<String>, String> {
    use objc2_app_kit::{
        NSBitmapImageFileType, NSBitmapImageRep, NSPasteboard, NSPasteboardTypePNG,
        NSPasteboardTypeTIFF,
    };
    use objc2_foundation::NSDictionary;

    let board = NSPasteboard::generalPasteboard();
    // SAFETY: these are AppKit's constant pasteboard type names.
    let (png, tiff) = unsafe { (NSPasteboardTypePNG, NSPasteboardTypeTIFF) };
    let data = if let Some(data) = board.dataForType(png) {
        data
    } else if let Some(data) = board.dataForType(tiff) {
        if data.len() as u64 > MAX_IMAGE_BYTES {
            return Err(pasted_image_too_large());
        }
        let rep = NSBitmapImageRep::imageRepWithData(&data).ok_or("Unreadable clipboard image")?;
        // SAFETY: an empty options dictionary is valid for PNG encoding.
        unsafe {
            rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
        }
        .ok_or("Could not encode the clipboard image")?
    } else {
        return Ok(None);
    };
    if data.len() as u64 > MAX_IMAGE_BYTES {
        return Err(pasted_image_too_large());
    }
    if data.is_empty() {
        return Err("Unreadable clipboard image".into());
    }
    Ok(Some(STANDARD.encode(data.to_vec())))
}
