//! Instant session previews: the saved conversation of a session shown while its engine opens it.
//!
//! Opening a session waits for an engine to `resume` it and replay its history. Meanwhile the
//! window shows the last copy Desktop kept on this computer, then the copy the running engine
//! reads with `session_messages` (no resume, no new engine). The preview is read-only: the
//! snapshot stays `history_loading` until the engine has opened the session.
//!
//! Copies live in Desktop's private folder, one file per session under a folder per project,
//! and hold the engine's message records as JSON. They are bounded in size and count and are
//! removed when the session is deleted permanently.

use crate::engine::Update;
use crate::model::Model;
use dial_client::{Event, Message};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

/// Larger conversations are not kept; they still preview from the running engine.
const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
/// Kept copies per project; the least recently written go first.
const MAX_FILES: usize = 200;

/// The preview on screen, if any.
pub struct Shown {
    pub root: String,
    pub id: String,
    pub model: Model,
}

/// Kept copies and the preview on screen.
#[derive(Default)]
pub struct Previews {
    /// `None` keeps nothing on disk (tests, or no per-user folder).
    dir: Option<PathBuf>,
    shown: Option<Shown>,
}

impl Previews {
    pub fn new(app_dir: Option<PathBuf>) -> Self {
        Self { dir: app_dir.map(|dir| dir.join("session-previews")), shown: None }
    }

    /// Start showing session `id` of project `root` from the kept copy, when there is one.
    pub fn begin(&mut self, root: &str, id: &str) {
        let messages = self.load(root, id);
        self.shown = Some(Shown { root: root.to_owned(), id: id.to_owned(), model: preview_model(messages.as_deref().unwrap_or_default()) });
    }

    /// The engine read session `id`: keep the copy and show it if that session is being opened.
    pub fn update(&mut self, root: &str, id: &str, messages: &[Value]) {
        if crate::model::has_compaction_truncation(messages) {
            return;
        }
        self.save(root, id, messages);
        if let Some(shown) = self.shown.as_mut().filter(|s| s.root == root && s.id == id) {
            shown.model = preview_model(messages);
        }
    }

    /// The preview for session `id` of `root` while it is being opened; `None` clears any other.
    pub fn shown(&mut self, root: &str, opening: Option<&str>) -> Option<&Shown> {
        if self.shown.as_ref().is_some_and(|s| s.root != root || Some(s.id.as_str()) != opening) {
            self.shown = None;
        }
        self.shown.as_ref().filter(|s| !s.model.conversation.entries().is_empty())
    }

    /// Remove the kept copies of sessions deleted permanently.
    pub fn forget(&mut self, root: &str, ids: &[String]) {
        for id in ids {
            if let Some(file) = self.file(root, id) {
                let _ = fs::remove_file(file);
            }
        }
        if self.shown.as_ref().is_some_and(|s| s.root == root && ids.contains(&s.id)) {
            self.shown = None;
        }
    }

    fn file(&self, root: &str, id: &str) -> Option<PathBuf> {
        let safe = !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        Some(self.dir.as_ref()?.join(format!("{:016x}", fnv1a(root.as_bytes()))).join(format!("{id}.json"))).filter(|_| safe)
    }

    fn load(&self, root: &str, id: &str) -> Option<Vec<Value>> {
        let bytes = fs::read(self.file(root, id)?).ok()?;
        let saved: Value = serde_json::from_slice(&bytes).ok()?;
        // A different project folder with the same hash must not show another project's session.
        (saved["root"] == root && saved["session_id"] == id)
            .then(|| saved["messages"].as_array().cloned())
            .flatten()
            .filter(|messages| !crate::model::has_compaction_truncation(messages))
    }

    fn save(&self, root: &str, id: &str, messages: &[Value]) {
        let Some(file) = self.file(root, id) else { return };
        let Ok(bytes) = serde_json::to_vec(&json!({"root": root, "session_id": id, "messages": messages})) else { return };
        if bytes.len() > MAX_FILE_BYTES {
            let _ = fs::remove_file(&file);
            return;
        }
        let Some(dir) = file.parent() else { return };
        if create_private_dir(dir).is_err() {
            return;
        }
        let tmp = file.with_extension("tmp");
        if write_private(&tmp, &bytes).is_ok() && fs::rename(&tmp, &file).is_err() {
            let _ = fs::remove_file(&tmp);
        }
        prune(dir);
    }
}

/// A scratch model holding `messages` as a resumed session's replay would.
pub fn preview_model(messages: &[Value]) -> Model {
    let mut model = Model::default();
    for message in messages {
        model.apply(Update::Message(Message::Event(Event { kind: "replay_message".into(), body: json!({ "message": message }) })));
    }
    model
}

/// Stable across runs and versions (unlike `DefaultHasher`), so a project keeps its folder.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, b| (hash ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

fn prune(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut files: Vec<_> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    if files.len() <= MAX_FILES {
        return;
    }
    files.sort();
    for (_, path) in &files[..files.len() - MAX_FILES] {
        let _ = fs::remove_file(path);
    }
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(bytes)
}

#[cfg(test)]
mod tests;
