//! Files attached to a prompt. Images use the engine's vision input; other files are read from workspace copies.
//!
//! A file chosen in the picker or dropped on the window stays a path: the engine reads it when the
//! turn starts. The UI still needs to show it, so [`read_image`] returns a small data URL preview
//! of the same bytes. Pasted images have no file and travel as base64 (`inline_images`).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::Serialize;
use std::path::Path;

pub(crate) mod clipboard_paths;
pub(crate) mod documents;

#[cfg(test)]
mod tests;

/// Match the engine's source limit; it bounds the model-facing encoding separately.
pub const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
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

/// Check a selected attachment before showing a ready chip. Recheck on submission too:
/// files may move or grow between the picker and Send.
pub fn validate_path(path: &Path) -> Result<(), String> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let meta = std::fs::metadata(path).map_err(|e| format!("Could not read {name}: {e}"))?;
    if !path.is_absolute() || !meta.is_file() {
        return Err(format!("Select a regular file, not a directory or device: {name}"));
    }
    if meta.len() > MAX_IMAGE_BYTES {
        return Err(format!("{name} exceeds the 64 MiB attachment limit"));
    }
    Ok(())
}

/// Check and preview the image at `path`.
pub fn read_image(path: &Path) -> Result<ImageFile, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let mime =
        image_mime(path).ok_or_else(|| format!("{name} is not a PNG, JPEG, GIF or WebP image"))?;
    validate_path(path)?;
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
    for image in images {
        let path = Path::new(image);
        if image_mime(path).is_none() {
            return Err(format!("{image} is not a PNG, JPEG, GIF or WebP image"));
        }
        validate_path(path)?;
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
        "Pasted images can be at most {} MiB",
        MAX_IMAGE_BYTES / (1024 * 1024)
    )
}

/// Pasted images older than this are removed the next time one is saved.
const PASTED_KEEP: std::time::Duration = std::time::Duration::from_secs(7 * 24 * 60 * 60);

/// The image file extension for `bytes`, from its signature; `None` for anything else.
pub fn sniff_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// Save image bytes (base64) to `path`, a place the user picked in the save dialog. Writes a
/// temporary file beside it first, so a failed save never leaves half an image behind.
pub fn save_image(path: &Path, data: &str) -> Result<(), String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    image_mime(path).ok_or_else(|| format!("{name} needs a .png, .jpg, .gif or .webp name"))?;
    let data = data.trim();
    if data.len() > (MAX_IMAGE_BYTES as usize).div_ceil(3) * 4 {
        return Err(format!(
            "Images can be at most {} MiB",
            MAX_IMAGE_BYTES / (1024 * 1024)
        ));
    }
    let bytes = STANDARD
        .decode(data)
        .map_err(|_| "The image could not be read".to_owned())?;
    let kind = sniff_extension(&bytes).ok_or("The image could not be read")?;
    let wanted = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let wanted = if wanted == "jpeg" { "jpg" } else { wanted.as_str() };
    if wanted != kind {
        return Err(format!(
            "This image is a {} file; save it with a .{kind} name",
            kind.to_ascii_uppercase()
        ));
    }
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::Builder::new()
        .prefix(".dial-save-")
        .tempfile_in(dir)
        .map_err(|e| format!("Could not save {name}: {e}"))?;
    std::io::Write::write_all(&mut file, &bytes).map_err(|e| format!("Could not save {name}: {e}"))?;
    file.persist(path)
        .map_err(|e| format!("Could not save {name}: {}", e.error))?;
    Ok(())
}

/// Turn pasted images (base64, already [`check`]ed) into private files in `dir` and return their
/// paths, so the prompt sends them by path like chosen files.
///
/// The engine's RPC refuses a request over 1 MiB, and a pasted screenshot as base64 is usually
/// larger, so inline images made such prompts fail. Each file is created with a random name and
/// owner-only permissions; files older than a week are pruned here.
pub fn save_pasted(dir: &Path, inline_images: &[String]) -> Result<Vec<String>, String> {
    if inline_images.is_empty() {
        return Ok(Vec::new());
    }
    create_private_dir(dir).map_err(|e| format!("Could not save the pasted image: {e}"))?;
    prune(dir, PASTED_KEEP);
    inline_images
        .iter()
        .map(|data| {
            let bytes = STANDARD
                .decode(data.trim())
                .map_err(|_| "A pasted image could not be read".to_owned())?;
            let ext = sniff_extension(&bytes)
                .ok_or("A pasted image is not a PNG, JPEG, GIF or WebP image")?;
            let mut file = tempfile::Builder::new()
                .prefix("pasted-")
                .suffix(&format!(".{ext}"))
                .tempfile_in(dir)
                .map_err(|e| format!("Could not save the pasted image: {e}"))?;
            std::io::Write::write_all(&mut file, &bytes)
                .map_err(|e| format!("Could not save the pasted image: {e}"))?;
            let (_, path) = file
                .keep()
                .map_err(|e| format!("Could not save the pasted image: {e}"))?;
            Ok(path.to_string_lossy().into_owned())
        })
        .collect()
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Remove this folder's `pasted-*` files last changed more than `keep` ago. Best effort.
fn prune(dir: &Path, keep: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > keep);
        let ours = entry.file_name().to_string_lossy().starts_with("pasted-");
        if old && ours {
            let _ = std::fs::remove_file(entry.path());
        }
    }
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
