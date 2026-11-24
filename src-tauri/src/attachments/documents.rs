//! Non-image files travel as references to private workspace copies, not base64 in the RPC frame.
use super::{MAX_IMAGE_BYTES, MAX_IMAGES, STANDARD};
use base64::Engine as _;
use serde::Deserialize;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use tempfile::TempDir;

#[derive(Deserialize)]
pub(crate) struct Document {
    name: String,
    path: Option<String>,
    data: Option<String>,
}

/// Names appear in a model-facing path. Never interpret clipboard names as paths or markup.
fn copy_name(index: usize, name: &str) -> String {
    let leaf = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean = |s: &str, max: usize| -> String {
        s.chars()
            .take(max)
            .map(|c| {
                if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    };
    let (stem, ext) = leaf.rsplit_once('.').unwrap_or((leaf, ""));
    let stem = clean(stem, 40);
    let ext = clean(ext, 12);
    format!(
        "{}-{}{}{}",
        index + 1,
        if stem.is_empty() { "file" } else { &stem },
        if ext.is_empty() { "" } else { "." },
        ext
    )
    .trim_end_matches('.')
    .to_owned()
}

fn copy(doc: Document, dest: &Path) -> Result<(), String> {
    let too_large = || format!("{} exceeds the 64 MiB attachment limit", doc.name);
    let save = |e: io::Error| format!("Could not attach {}: {e}", doc.name);
    let mut output = File::create_new(dest).map_err(save)?;
    match (doc.path, doc.data) {
        (Some(path), None) => {
            let path = Path::new(&path);
            super::validate_path(path)?;
            let file = File::open(path).map_err(save)?;
            let meta = file.metadata().map_err(save)?;
            if !meta.is_file() {
                return Err(format!("Not a regular file: {}", doc.name));
            }
            if meta.len() > MAX_IMAGE_BYTES
                || io::copy(&mut file.take(MAX_IMAGE_BYTES + 1), &mut output).map_err(save)?
                    > MAX_IMAGE_BYTES
            {
                return Err(too_large());
            }
        }
        (None, Some(data)) => {
            if data.len() > (MAX_IMAGE_BYTES as usize).div_ceil(3) * 4 {
                return Err(too_large());
            }
            let bytes = STANDARD
                .decode(data)
                .map_err(|_| format!("Could not decode {}", doc.name))?;
            if bytes.len() > MAX_IMAGE_BYTES as usize {
                return Err(too_large());
            }
            output.write_all(&bytes).map_err(save)?;
        }
        _ => return Err("An attachment must have exactly one source".into()),
    }
    Ok(())
}

/// A fresh directory cannot collide with an existing attachment or a project-supplied symlink.
/// The caller keeps it only once the prompt is submitted. Until then, errors roll back the batch.
pub(crate) fn stage(
    root: &Path,
    text: String,
    docs: Vec<Document>,
) -> Result<(String, Option<TempDir>), String> {
    if docs.is_empty() {
        return Ok((text, None));
    }
    if docs.len() > MAX_IMAGES {
        return Err(format!("Attach at most {MAX_IMAGES} files to one prompt"));
    }
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    let folder = builder
        .prefix(".dial-attachments-")
        .tempdir_in(root)
        .map_err(|e| format!("Could not store attachments in the open project: {e}"))?;
    // Keep private inputs out of ordinary `git add .`, without editing the user's ignore rules.
    File::create_new(folder.path().join(".gitignore"))
        .and_then(|mut f| f.write_all(b"*\n"))
        .map_err(|e| e.to_string())?;
    let relative = folder.path().file_name().unwrap().to_string_lossy();
    let mut prompt = text;
    if !prompt.is_empty() {
        prompt.push_str("\n\n");
    }
    prompt.push_str("Attached files (read these workspace copies as needed):");
    for (i, doc) in docs.into_iter().enumerate() {
        let name = copy_name(i, &doc.name);
        copy(doc, &folder.path().join(&name))?;
        prompt.push_str(&format!("\n- `{relative}/{name}`"));
    }
    Ok((prompt, Some(folder)))
}

#[cfg(test)]
mod tests;
