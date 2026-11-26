//! Types: collections the user saves things into, each one its own small web app.
//!
//! A type is a [`TypeDef`] (a name, an icon, the shape of its items), the app that shows them (a
//! whole HTML document the agent writes, in `app.html`), its saved [`Item`]s and a JSON `state`
//! the app keeps for itself (settings, game progress). There are no built-in layouts: a recipe
//! type becomes a cookbook, a weather type a live forecast, because the agent builds that app.
//! The UI runs it in a sandboxed frame from the `dialtype:` scheme ([`custom_page`]), with the
//! `window.dial` runtime (`types/runtime.js`) injected; the agent designs, edits, opens, inspects
//! and exercises it through the `types_*` tools (`agent_browser`, `types::live`).
//!
//! Types live in the app's folder, or in a project's `.dial/types`: `<id>/type.json`,
//! `<id>/app.html`, `<id>/items.json`, `<id>/state.json` and `<id>/assets/`. Every write is
//! atomic and every input, from the UI or the agent, goes through [`normalize`] or
//! [`sanitize_fields`].

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::collections::HashSet;
use std::sync::{Mutex, MutexGuard, PoisonError};
use tauri::{AppHandle, Emitter, State};

pub mod live;
#[cfg(test)]
mod tests;

pub const VERSION: u32 = 2;
/// List invalidation (`null`), a changed id, or an agent build step with its progress.
pub const CHANGED: &str = "types-changed";
const MAX_FIELDS: usize = 32;
const MAX_ITEMS: usize = 5_000;
const MAX_LIST: usize = 200;
const MAX_SHORT: usize = 2_000;
const MAX_LONG: usize = 40_000;
/// A free-form item (a type with no fields) keeps at most this many keys.
const MAX_FREE_KEYS: usize = 64;
/// The app's HTML document.
pub const MAX_APP: usize = 1024 * 1024;
/// The app's own JSON state.
pub const MAX_STATE: usize = 512 * 1024;
const MAX_IMAGE: u64 = 25 * 1024 * 1024;
const MAX_TYPES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    #[default]
    Text,
    Longtext,
    Markdown,
    List,
    Steps,
    Image,
    Images,
    Time,
    Date,
    Datetime,
    Number,
    Url,
    Tags,
    Rating,
    Toggle,
    /// Any JSON value the app needs (coordinates, a nested structure), at most 40 KB.
    Json,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub kind: FieldKind,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TypeDef {
    pub version: u32,
    pub id: String,
    pub name: String,
    pub description: String,
    /// An emoji or a short glyph.
    pub icon: String,
    /// `#rgb` or `#rrggbb`, for the sidebar; empty follows the app's theme.
    pub accent: String,
    pub created: i64,
    pub updated: i64,
    /// The keys items use, each in its kind's shape. None: items keep any keys (free-form).
    pub fields: Vec<Field>,
    /// The app: a whole HTML document, kept in `app.html` beside `type.json`. Read from a legacy
    /// `customHtml` member once, then moved there.
    #[serde(skip_serializing, alias = "customHtml", alias = "custom_html")]
    pub html: String,
    /// The type's own conversation per project (workspace root to session id): where its app is
    /// built and where saves to it are added. These sessions stay out of the session list.
    pub sessions: std::collections::BTreeMap<String, String>,
}

impl TypeDef {
    pub fn has_app(&self) -> bool {
        !self.html.trim().is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Source {
    pub text: String,
    #[serde(alias = "session_id")]
    pub session_id: Option<String>,
    pub workspace: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Item {
    pub id: String,
    pub created: i64,
    pub updated: i64,
    pub fields: Map<String, Value>,
    pub done: bool,
    pub source: Option<Source>,
}

/// A change to one item; absent members stay as they are.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ItemPatch {
    /// Merged over the item's fields; a `null` member clears that field.
    pub fields: Option<Map<String, Value>>,
    pub done: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub description: String,
    pub accent: String,
    pub count: usize,
    pub updated: i64,
    /// The agent has built its app.
    pub has_app: bool,
    /// Where the type is kept.
    pub scope: Scope,
    /// The ids of the type's own sessions (see [`TypeDef::sessions`]).
    pub sessions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Full {
    pub def: TypeDef,
    pub has_app: bool,
    /// Changes exactly when the app's HTML does: the frame reloads on it, and the agent's tools
    /// wait for the frame to show it.
    pub app_version: String,
    pub items: Vec<Item>,
    /// The app's own saved state (`{}` until it saves some).
    pub state: Value,
    /// The folder item images are in; an image field holds a file name in it.
    pub assets: String,
}

/// A short fingerprint of an app's HTML.
pub fn app_version(html: &str) -> String {
    Sha256::digest(html.as_bytes())
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// A field key: lowercase letters, digits and `_`, from a key or a label.
pub fn slug_key(text: &str) -> String {
    let mut out = String::new();
    for c in text.trim().chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let out = out.trim_end_matches('_');
    clip(out, 40)
}

fn valid_color(text: &str) -> bool {
    let hex = text.strip_prefix('#').unwrap_or("");
    matches!(hex.len(), 3 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// An asset file name this store wrote: a hash and an image extension.
pub fn valid_asset(name: &str) -> bool {
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    (16..=64).contains(&stem.len())
        && stem.chars().all(|c| c.is_ascii_hexdigit())
        && matches!(ext, "png" | "jpg" | "jpeg" | "gif" | "webp")
}

/// Check and tidy a type from the UI or the agent. Keeps `id` and `created`.
pub fn normalize(mut def: TypeDef, now: i64) -> Result<TypeDef, String> {
    def.version = VERSION;
    def.name = clip(&def.name, 60);
    if def.name.is_empty() {
        return Err("A type needs a name".into());
    }
    def.description = clip(&def.description, 400);
    def.icon = def.icon.trim().chars().take(4).collect();
    if def.icon.is_empty() {
        def.icon = "✦".into();
    }
    if def.created == 0 {
        def.created = now;
    }
    def.updated = now;
    def.accent = def.accent.trim().to_owned();
    if !valid_color(&def.accent) {
        def.accent.clear();
    }

    let mut fields: Vec<Field> = Vec::new();
    for mut field in std::mem::take(&mut def.fields) {
        field.label = clip(&field.label, 60);
        let key = slug_key(if field.key.trim().is_empty() {
            &field.label
        } else {
            &field.key
        });
        if key.is_empty() || fields.iter().any(|f| f.key == key) {
            continue;
        }
        if field.label.is_empty() {
            field.label = key.replace('_', " ");
        }
        field.key = key;
        fields.push(field);
        if fields.len() == MAX_FIELDS {
            break;
        }
    }
    def.fields = fields;

    def.sessions
        .retain(|root, id| !root.is_empty() && root.len() <= 4096 && !id.is_empty() && id.len() <= 200);
    if def.sessions.len() > 64 {
        return Err("This type already has conversations in 64 projects".into());
    }
    if def.html.len() > MAX_APP {
        return Err(format!("A type's app holds at most {} KB of HTML", MAX_APP / 1024));
    }
    Ok(def)
}

fn as_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// A list from an array, or from lines of text with their bullets and numbers removed.
fn as_list(value: &Value) -> Vec<String> {
    let raw: Vec<String> = match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|v| {
                as_text(v).or_else(|| {
                    // `{"text": ..}` or `{"name": ..}`: common shapes for a structured line.
                    v.as_object().and_then(|o| {
                        ["text", "name", "title", "label", "value"]
                            .iter()
                            .find_map(|k| o.get(*k).and_then(as_text))
                    })
                })
            })
            .collect(),
        Value::String(s) => s
            .lines()
            .map(|l| {
                l.trim()
                    .trim_start_matches(['-', '*', '•'])
                    .trim_start_matches(|c: char| c.is_ascii_digit())
                    .trim_start_matches(['.', ')'])
                    .trim()
                    .to_owned()
            })
            .collect(),
        _ => Vec::new(),
    };
    raw.iter()
        .map(|s| clip(s, MAX_SHORT))
        .filter(|s| !s.is_empty())
        .take(MAX_LIST)
        .collect()
}

/// Keep only the type's fields, each in its kind's shape. Images must already be assets. A type
/// without fields keeps any keys, each value at most [`MAX_LONG`] bytes of JSON.
pub fn sanitize_fields(def: &TypeDef, fields: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    if def.fields.is_empty() {
        for (key, value) in fields {
            let key = slug_key(key);
            if key.is_empty() || value.is_null() || out.contains_key(&key) {
                continue;
            }
            if serde_json::to_string(value).map_or(true, |s| s.len() > MAX_LONG) {
                continue;
            }
            out.insert(key, value.clone());
            if out.len() == MAX_FREE_KEYS {
                break;
            }
        }
        return out;
    }
    for field in &def.fields {
        let Some(value) = fields.get(&field.key).filter(|v| !v.is_null()) else {
            continue;
        };
        let clean = match field.kind {
            FieldKind::Text
            | FieldKind::Url
            | FieldKind::Time
            | FieldKind::Date
            | FieldKind::Datetime => as_text(value).map(|s| Value::String(clip(&s, MAX_SHORT))),
            FieldKind::Longtext | FieldKind::Markdown => {
                as_text(value).map(|s| Value::String(clip(&s, MAX_LONG)))
            }
            FieldKind::List | FieldKind::Steps | FieldKind::Tags => {
                Some(as_list(value).into_iter().map(Value::String).collect())
            }
            FieldKind::Image => {
                let first = match value {
                    Value::Array(list) => list.first().and_then(Value::as_str),
                    other => other.as_str(),
                };
                first
                    .map(str::trim)
                    .filter(|n| valid_asset(n))
                    .map(|n| Value::String(n.to_owned()))
            }
            FieldKind::Images => Some(
                as_list(value)
                    .into_iter()
                    .filter(|n| valid_asset(n))
                    .map(Value::String)
                    .collect(),
            ),
            FieldKind::Number => value
                .as_f64()
                .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number),
            FieldKind::Rating => value
                .as_f64()
                .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
                .map(|n| Value::from(n.round().clamp(0.0, 5.0) as u8)),
            FieldKind::Toggle => value
                .as_bool()
                .or_else(|| value.as_str().map(|s| matches!(s, "true" | "yes" | "1")))
                .map(Value::Bool),
            FieldKind::Json => serde_json::to_string(value)
                .ok()
                .filter(|s| s.len() <= MAX_LONG)
                .map(|_| value.clone()),
        };
        match clean {
            Some(Value::String(s)) if s.is_empty() => {}
            Some(Value::Array(a)) if a.is_empty() => {}
            Some(v) => {
                out.insert(field.key.clone(), v);
            }
            None => {}
        }
    }
    out
}

/// What an image file is, from its first bytes.
fn image_ext(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        Some("gif")
    } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("No folder")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let partial = path.with_extension("partial");
    std::fs::write(&partial, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&partial, path).map_err(|e| e.to_string())
}

fn random_suffix() -> String {
    crate::agent_browser::new_token()[..8].to_owned()
}

/// Write a type's files: `type.json`, and its app in `app.html` (removed when it has none).
fn write_def_in(dir: &Path, def: &TypeDef) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(def).map_err(|e| e.to_string())?;
    write_atomic(&dir.join("type.json"), &bytes)?;
    let app = dir.join("app.html");
    if def.has_app() {
        write_atomic(&app, def.html.as_bytes())
    } else {
        match std::fs::remove_file(&app) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

/// Where a type is kept: with the app (every project sees it) or in one project's folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    #[default]
    App,
    Project,
}

/// The setting for where new types are kept, and what it comes to right now.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    /// What the user chose in Settings.
    pub save_in: Scope,
    /// The open project's folder for types, when a project is open.
    pub project: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Prefs {
    save_in: Scope,
}

#[derive(Debug, Default)]
struct Place {
    /// The open project's root.
    project: Option<PathBuf>,
    save_in: Scope,
}

/// Where a project keeps its types, so they travel with it.
fn project_types(root: &Path) -> PathBuf {
    root.join(".dial").join("types")
}

/// The saved types, owned by the app. Types live in the app's folder, shared by every project, or
/// in the open project's `.dial/types`; the setting only decides where new ones go. Both places
/// are always listed, so changing it never hides a type.
pub struct Store {
    root: Option<PathBuf>,
    prefs: Option<PathBuf>,
    place: Mutex<Place>,
    lock: Mutex<()>,
}

impl Store {
    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            root,
            prefs: None,
            place: Mutex::new(Place::default()),
            lock: Mutex::new(()),
        }
    }

    /// The store in the app's per-user folder, with the saved choice of where new types go.
    pub fn for_home(home: Option<&Path>) -> Self {
        let app = crate::launcher::app_dir(home);
        let prefs = app.as_ref().map(|d| d.join("types-location.json"));
        let saved: Prefs = prefs
            .as_deref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            root: app.map(|d| d.join("types")),
            prefs,
            place: Mutex::new(Place {
                project: None,
                save_in: saved.save_in,
            }),
            lock: Mutex::new(()),
        }
    }

    fn place(&self) -> MutexGuard<'_, Place> {
        self.place.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The saved scope on disk when a prefs file exists, else the in-memory setting.
    /// Re-reading keeps multiple open windows aligned when the user changes Settings → Types.
    fn saved_scope(&self) -> Scope {
        if let Some(path) = &self.prefs
            && let Ok(bytes) = std::fs::read(path)
            && let Ok(saved) = serde_json::from_slice::<Prefs>(&bytes)
        {
            self.place().save_in = saved.save_in;
            return saved.save_in;
        }
        self.place().save_in
    }

    /// The project the user has open (`None` for none): its types are listed with the app's.
    pub fn set_project(&self, project: Option<&Path>) {
        self.place().project = project.map(Path::to_path_buf);
    }

    pub fn location(&self) -> Location {
        let save_in = self.saved_scope();
        let place = self.place();
        Location {
            save_in,
            project: place
                .project
                .as_deref()
                .map(|p| project_types(p).to_string_lossy().into_owned()),
        }
    }

    /// Choose where new types are kept, and remember it.
    pub fn set_save_in(&self, scope: Scope) -> Result<(), String> {
        if let Some(path) = &self.prefs {
            let bytes = serde_json::to_vec_pretty(&Prefs { save_in: scope })
                .map_err(|e| e.to_string())?;
            write_atomic(path, &bytes)?;
        }
        self.place().save_in = scope;
        Ok(())
    }

    /// The folders types are read from; the project's comes first and wins a clash of ids.
    fn roots(&self) -> Vec<(Scope, PathBuf)> {
        let mut out = Vec::new();
        if let Some(project) = &self.place().project {
            out.push((Scope::Project, project_types(project)));
        }
        if let Some(root) = &self.root {
            out.push((Scope::App, root.clone()));
        }
        out
    }

    /// Where a new type goes: the chosen place, or the other when that one is not available (the
    /// project's with no project open).
    fn new_root(&self) -> Result<(Scope, PathBuf), String> {
        let want = self.saved_scope();
        let roots = self.roots();
        roots
            .iter()
            .find(|(scope, _)| *scope == want)
            .or_else(|| roots.first())
            .cloned()
            .ok_or_else(|| "No folder for saved types (no home folder)".into())
    }

    /// Where type `id` is: the place it exists in, else the app's folder.
    fn locate(&self, id: &str) -> Result<(Scope, PathBuf), String> {
        if !valid_id(id) {
            return Err(format!("No type `{id}`"));
        }
        let roots = self.roots();
        roots
            .iter()
            .map(|(scope, root)| (*scope, root.join(id)))
            .find(|(_, dir)| dir.join("type.json").is_file())
            .or_else(|| roots.last().map(|(scope, root)| (*scope, root.join(id))))
            .ok_or_else(|| "No folder for saved types (no home folder)".into())
    }

    fn dir(&self, id: &str) -> Result<PathBuf, String> {
        self.locate(id).map(|(_, dir)| dir)
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }


    /// Type `id`'s blueprint and, when `with_app`, its app's HTML.
    fn read_def_with(&self, id: &str, with_app: bool) -> Result<(TypeDef, bool), String> {
        let dir = self.dir(id)?;
        let bytes = std::fs::read(dir.join("type.json")).map_err(|_| format!("No type `{id}`"))?;
        let mut def: TypeDef =
            serde_json::from_slice(&bytes).map_err(|e| format!("Type `{id}` is damaged: {e}"))?;
        let app = dir.join("app.html");
        let has_file = std::fs::metadata(&app).is_ok_and(|m| m.is_file() && m.len() > 0);
        if with_app && has_file {
            def.html = std::fs::read_to_string(&app).map_err(|e| e.to_string())?;
        }
        let has_app = has_file || def.has_app();
        Ok((def, has_app))
    }

    fn read_def(&self, id: &str) -> Result<TypeDef, String> {
        self.read_def_with(id, true).map(|(def, _)| def)
    }

    fn write_def(&self, def: &TypeDef) -> Result<(), String> {
        write_def_in(&self.dir(&def.id)?, def)
    }

    fn read_items(&self, id: &str) -> Result<Vec<Item>, String> {
        match std::fs::read(self.dir(id)?.join("items.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| format!("The items of `{id}` are damaged: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn write_items(&self, id: &str, items: &[Item]) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(items).map_err(|e| e.to_string())?;
        write_atomic(&self.dir(id)?.join("items.json"), &bytes)
    }

    fn read_state(&self, id: &str) -> Value {
        self.dir(id)
            .ok()
            .and_then(|dir| std::fs::read(dir.join("state.json")).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| Value::Object(Map::new()))
    }

    pub fn list(&self) -> Result<Vec<Summary>, String> {
        let _guard = self.guard();
        let roots = self.roots();
        if roots.is_empty() {
            return Err("No folder for saved types (no home folder)".into());
        }
        let mut seen = HashSet::new();
        let mut out: Vec<Summary> = Vec::new();
        for (scope, root) in roots {
            let Ok(entries) = std::fs::read_dir(&root) else {
                continue;
            };
            for id in entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|id| valid_id(id))
            {
                if !seen.insert(id.clone()) {
                    continue;
                }
                let Ok((def, has_app)) = self.read_def_with(&id, false) else {
                    continue;
                };
                let count = self.read_items(&id).map(|i| i.len()).unwrap_or(0);
                out.push(Summary {
                    id: def.id,
                    name: def.name,
                    icon: def.icon,
                    description: def.description,
                    accent: def.accent,
                    count,
                    updated: def.updated,
                    has_app,
                    scope,
                    sessions: def.sessions.into_values().collect(),
                });
            }
        }
        out.sort_by_key(|s| s.name.to_lowercase());
        Ok(out)
    }

    /// Everything but the app's HTML, which the `dialtype:` scheme serves.
    pub fn get(&self, id: &str) -> Result<Full, String> {
        let _guard = self.guard();
        let (mut def, has_app) = self.read_def_with(id, true)?;
        let app_version = app_version(&def.html);
        def.html.clear();
        Ok(Full {
            def,
            has_app,
            app_version,
            items: self.read_items(id)?,
            state: self.read_state(id),
            assets: self.dir(id)?.join("assets").to_string_lossy().into_owned(),
        })
    }

    /// Type `id`'s app, its HTML document; empty when it has none yet.
    pub fn app_html(&self, id: &str) -> Result<String, String> {
        let _guard = self.guard();
        self.read_def(id).map(|def| def.html)
    }

    /// The id of the type named `which` (its id, or its name in any case).
    pub fn find(&self, which: &str) -> Result<String, String> {
        let which = which.trim();
        let list = self.list()?;
        list.iter()
            .find(|s| s.id == which)
            .or_else(|| list.iter().find(|s| s.name.eq_ignore_ascii_case(which)))
            .map(|s| s.id.clone())
            .ok_or_else(|| {
                let names: Vec<&str> = list.iter().map(|s| s.name.as_str()).collect();
                if names.is_empty() {
                    format!("No type `{which}`; there are no types yet")
                } else {
                    format!("No type `{which}`; the types are: {}", names.join(", "))
                }
            })
    }

    pub fn create(&self, def: TypeDef) -> Result<TypeDef, String> {
        let now = crate::time::now();
        let mut def = normalize(def, now)?;
        def.created = now;
        let _guard = self.guard();
        let (_, root) = self.new_root()?;
        let count = std::fs::read_dir(&root).map(|d| d.count()).unwrap_or(0);
        if count >= MAX_TYPES {
            return Err(format!("At most {MAX_TYPES} types"));
        }
        let mut base: String = slug_key(&def.name).replace('_', "-");
        if base.is_empty() {
            base = "type".into();
        }
        base.truncate(40);
        def.id = format!("{}-{}", base.trim_end_matches('-'), random_suffix());
        write_def_in(&root.join(&def.id), &def)?;
        self.write_items(&def.id, &[])?;
        Ok(def)
    }

    /// Replace members of type `id` (`name`, `description`, `icon`, `accent`, `fields`, `html`,
    /// `sessions`); its items and state stay.
    pub fn update(&self, id: &str, patch: &Value) -> Result<TypeDef, String> {
        let _guard = self.guard();
        let current = self.read_def(id)?;
        let mut merged = serde_json::to_value(&current).map_err(|e| e.to_string())?;
        let (Value::Object(base), Value::Object(patch)) = (&mut merged, patch) else {
            return Err("A change to a type is an object".into());
        };
        let mut html = None;
        for (key, value) in patch {
            match key.as_str() {
                "id" | "created" | "updated" | "version" => {}
                "html" | "customHtml" | "custom_html" => {
                    html = Some(value.as_str().ok_or("`html` is a string")?.to_owned());
                }
                other => {
                    base.insert(other.to_owned(), value.clone());
                }
            }
        }
        let mut def: TypeDef =
            serde_json::from_value(merged).map_err(|e| format!("Not a type: {e}"))?;
        def.id = current.id;
        def.created = current.created;
        def.html = html.unwrap_or(current.html);
        let def = normalize(def, crate::time::now())?;
        self.write_def(&def)?;
        Ok(def)
    }

    fn claim_session(&self, id: &str, root: String, session: String) -> Result<(), String> {
        let _guard = self.guard();
        let mut def = self.read_def(id)?;
        if def.sessions.get(&root).is_some_and(|old| old != &session) {
            return Err("This type already has a conversation here; reopen it".into());
        }
        def.sessions.insert(root, session);
        self.write_def(&normalize(def, crate::time::now())?)
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        let _guard = self.guard();
        let dir = self.dir(id)?;
        if !dir.join("type.json").is_file() {
            return Err(format!("No type `{id}`"));
        }
        std::fs::remove_dir_all(dir).map_err(|e| e.to_string())
    }

    /// Replace the app's own saved state.
    pub fn set_state(&self, id: &str, state: &Value) -> Result<(), String> {
        let _guard = self.guard();
        self.read_def_with(id, false)?;
        let bytes = serde_json::to_vec(state).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_STATE {
            return Err(format!("A type's state holds at most {} KB", MAX_STATE / 1024));
        }
        write_atomic(&self.dir(id)?.join("state.json"), &bytes)
    }

    /// Copy the image at `path` into type `id`'s assets; returns its file name there.
    pub fn import_image(&self, id: &str, path: &Path) -> Result<String, String> {
        let meta =
            std::fs::metadata(path).map_err(|_| format!("No image at {}", path.display()))?;
        if !meta.is_file() || meta.len() > MAX_IMAGE {
            return Err(format!("{} is not an image under 25 MB", path.display()));
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        self.store_image(id, &bytes)
            .map_err(|_| format!("{} is not a PNG, JPEG, GIF or WebP image", path.display()))
    }

    fn store_image(&self, id: &str, bytes: &[u8]) -> Result<String, String> {
        let ext = image_ext(bytes).ok_or("Not an image")?;
        let hash: String = Sha256::digest(bytes)
            .iter()
            .take(16)
            .map(|b| format!("{b:02x}"))
            .collect();
        let name = format!("{hash}.{ext}");
        let target = self.dir(id)?.join("assets").join(&name);
        if !target.is_file() {
            write_atomic(&target, bytes)?;
        }
        Ok(name)
    }

    /// Replace absolute image paths in image fields with imported assets.
    fn import_field_images(&self, def: &TypeDef, fields: &mut Map<String, Value>) -> Vec<String> {
        let mut problems = Vec::new();
        for field in &def.fields {
            if !matches!(field.kind, FieldKind::Image | FieldKind::Images) {
                continue;
            }
            let Some(value) = fields.get_mut(&field.key) else {
                continue;
            };
            let mut convert = |v: &mut Value| {
                let Some(text) = v.as_str() else { return };
                if valid_asset(text) {
                    return;
                }
                let path = if text.get(..7).is_some_and(|p| p.eq_ignore_ascii_case("file://")) {
                    tauri::Url::parse(text).ok().and_then(|url| url.to_file_path().ok())
                } else {
                    Some(PathBuf::from(text))
                };
                if let Some(path) = path.filter(|p| p.is_absolute()) {
                    match self.import_image(&def.id, &path) {
                        Ok(name) => *v = Value::String(name),
                        Err(e) => problems.push(e),
                    }
                } else {
                    problems.push(format!(
                        "`{text}` is not an absolute path to a local image; web images must be downloaded first"
                    ));
                }
            };
            match value {
                Value::Array(items) => items.iter_mut().for_each(&mut convert),
                other => convert(other),
            }
        }
        problems
    }

    /// Resolve `path` only if it is an existing asset image file inside one of this store's
    /// `<root>/<valid-id>/assets/<valid-asset>` folders (never following symlinks outside).
    pub fn resolve_asset(&self, path: &Path) -> Result<PathBuf, String> {
        let real = path
            .canonicalize()
            .map_err(|_| "That type image is no longer available".to_owned())?;
        if !real.is_file() {
            return Err("That type image is not a file".into());
        }
        for (_, root) in self.roots() {
            let Ok(root_real) = root.canonicalize() else {
                continue;
            };
            let Ok(rel) = real.strip_prefix(&root_real) else {
                continue;
            };
            let parts: Vec<&str> = rel
                .components()
                .filter_map(|c| c.as_os_str().to_str())
                .collect();
            if parts.len() == 3 && valid_id(parts[0]) && parts[1] == "assets" && valid_asset(parts[2]) {
                return Ok(real);
            }
        }
        Err("That path is not a saved type image".into())
    }

    /// Save a new item; image fields may hold absolute paths, which are imported.
    /// Returns the item and anything that could not be kept.
    pub fn add_item(
        &self,
        id: &str,
        fields: Map<String, Value>,
        source: Option<Source>,
    ) -> Result<(Item, Vec<String>), String> {
        self.add_item_with(id, fields, source, true)
    }

    /// Save a new item; when `allow_import` is false (the type's own sandboxed app), local image
    /// paths are never read from disk and only existing asset names are kept.
    pub fn add_item_with(
        &self,
        id: &str,
        mut fields: Map<String, Value>,
        source: Option<Source>,
        allow_import: bool,
    ) -> Result<(Item, Vec<String>), String> {
        let _guard = self.guard();
        let (def, _) = self.read_def_with(id, false)?;
        let problems = if allow_import {
            self.import_field_images(&def, &mut fields)
        } else {
            Vec::new()
        };
        let mut items = self.read_items(id)?;
        if items.len() >= MAX_ITEMS {
            return Err(format!("A type holds at most {MAX_ITEMS} items"));
        }
        let fields = sanitize_fields(&def, &fields);
        if fields.is_empty() {
            if def.fields.is_empty() {
                return Err("Nothing to save: `fields` is empty".into());
            }
            return Err(format!(
                "Nothing to save: give at least one of the fields {}",
                def.fields
                    .iter()
                    .map(|f| format!("`{}` ({:?})", f.key, f.kind).to_lowercase())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let now = crate::time::now();
        let item = Item {
            id: format!("{now:x}-{}", &random_suffix()[..6]),
            created: now,
            updated: now,
            fields,
            source: source.map(|s| Source {
                text: clip(&s.text, MAX_LONG),
                ..s
            }),
            ..Item::default()
        };
        items.push(item.clone());
        self.write_items(id, &items)?;
        Ok((item, problems))
    }

    pub fn update_item(&self, id: &str, item_id: &str, patch: ItemPatch) -> Result<Item, String> {
        self.update_item_with(id, item_id, patch, true)
    }

    pub fn update_item_with(
        &self,
        id: &str,
        item_id: &str,
        patch: ItemPatch,
        allow_import: bool,
    ) -> Result<Item, String> {
        let _guard = self.guard();
        let (def, _) = self.read_def_with(id, false)?;
        let mut items = self.read_items(id)?;
        let item = items
            .iter_mut()
            .find(|i| i.id == item_id)
            .ok_or_else(|| format!("No item `{item_id}`"))?;
        if let Some(mut more) = patch.fields {
            if allow_import {
                let problems = self.import_field_images(&def, &mut more);
                if !problems.is_empty() {
                    return Err(problems.join("; "));
                }
            }
            let mut merged = item.fields.clone();
            for (key, value) in more {
                if value.is_null() {
                    merged.remove(&key);
                } else {
                    merged.insert(key, value);
                }
            }
            item.fields = sanitize_fields(&def, &merged);
        }
        if let Some(done) = patch.done {
            item.done = done;
        }
        item.updated = crate::time::now();
        let item = item.clone();
        self.write_items(id, &items)?;
        Ok(item)
    }

    pub fn remove_item(&self, id: &str, item_id: &str) -> Result<(), String> {
        let _guard = self.guard();
        let mut items = self.read_items(id)?;
        let before = items.len();
        items.retain(|i| i.id != item_id);
        if items.len() == before {
            return Err(format!("No item `{item_id}`"));
        }
        self.write_items(id, &items)
    }

    /// Put item `item_id` at position `to` (clamped).
    pub fn move_item(&self, id: &str, item_id: &str, to: usize) -> Result<(), String> {
        let _guard = self.guard();
        let mut items = self.read_items(id)?;
        let from = items
            .iter()
            .position(|i| i.id == item_id)
            .ok_or_else(|| format!("No item `{item_id}`"))?;
        let item = items.remove(from);
        items.insert(to.min(items.len()), item);
        self.write_items(id, &items)
    }
}

fn changed(app: &AppHandle, id: Option<&str>) {
    let _ = app.emit(CHANGED, id);
}

/// An agent build step on type `id`, for the UI's progress while a type gets its first app:
/// `fields` (their labels), `app` (its size in KB), `saved` (items saved) or `checked`.
pub fn progress(app: &AppHandle, id: &str, choices: Value) {
    let _ = app.emit(CHANGED, serde_json::json!({"id": id, "choices": choices}));
}

/// The build steps a type change commits: only members the patch actually set.
fn build_step(def: &TypeDef, patch: &Value) -> Value {
    let mut choices = Map::new();
    if patch.get("fields").is_some() {
        choices.insert(
            "fields".into(),
            Value::from(def.fields.iter().map(|f| f.label.clone()).collect::<Vec<_>>()),
        );
    }
    if ["html", "customHtml", "custom_html"].iter().any(|k| patch.get(*k).is_some()) && def.has_app() {
        choices.insert("app".into(), Value::from(def.html.len().div_ceil(1024)));
    }
    Value::Object(choices)
}

#[tauri::command]
pub fn types_list(store: State<'_, Store>) -> Result<Vec<Summary>, String> {
    store.list()
}

#[tauri::command]
pub fn types_get(store: State<'_, Store>, id: String) -> Result<Full, String> {
    store.get(&id)
}

/// Where new types are kept, and the open project's folder for them.
#[tauri::command]
pub fn types_location(store: State<'_, Store>) -> Location {
    store.location()
}

/// Choose where new types are kept: with the app, or in the open project.
#[tauri::command]
pub fn types_set_location(store: State<'_, Store>, save_in: Scope) -> Result<Location, String> {
    store.set_save_in(save_in)?;
    Ok(store.location())
}

/// The project the user opened: its types join the list, and new ones go there when chosen.
pub fn follow_project(app: &AppHandle, store: &Store, project: &Path) {
    store.set_project(Some(project));
    changed(app, None);
}

#[tauri::command]
pub fn types_create(
    app: AppHandle,
    store: State<'_, Store>,
    def: TypeDef,
) -> Result<TypeDef, String> {
    let def = store.create(def)?;
    changed(&app, Some(&def.id));
    Ok(def)
}

#[tauri::command]
pub fn types_update(
    app: AppHandle,
    store: State<'_, Store>,
    id: String,
    patch: Value,
) -> Result<TypeDef, String> {
    let mut def = store.update(&id, &patch)?;
    changed(&app, Some(&id));
    def.html.clear();
    Ok(def)
}

#[tauri::command]
pub fn types_claim_session(app: AppHandle, store: State<'_, Store>, id: String, root: String, session: String) -> Result<(), String> {
    store.claim_session(&id, root, session)?;
    changed(&app, Some(&id));
    Ok(())
}

#[tauri::command]
pub fn types_delete(app: AppHandle, store: State<'_, Store>, id: String) -> Result<(), String> {
    store.delete(&id)?;
    changed(&app, None);
    Ok(())
}

#[tauri::command]
pub fn types_add_item(
    app: AppHandle,
    store: State<'_, Store>,
    id: String,
    fields: Map<String, Value>,
    source: Option<Source>,
    allow_import: Option<bool>,
) -> Result<Item, String> {
    let (item, _) = store.add_item_with(&id, fields, source, allow_import.unwrap_or(true))?;
    changed(&app, Some(&id));
    Ok(item)
}

#[tauri::command]
pub fn types_update_item(
    app: AppHandle,
    store: State<'_, Store>,
    id: String,
    item_id: String,
    patch: ItemPatch,
    allow_import: Option<bool>,
) -> Result<Item, String> {
    let item = store.update_item_with(&id, &item_id, patch, allow_import.unwrap_or(true))?;
    changed(&app, Some(&id));
    Ok(item)
}

#[tauri::command]
pub fn types_remove_item(
    app: AppHandle,
    store: State<'_, Store>,
    id: String,
    item_id: String,
) -> Result<(), String> {
    store.remove_item(&id, &item_id)?;
    changed(&app, Some(&id));
    Ok(())
}

#[tauri::command]
pub fn types_move_item(
    app: AppHandle,
    store: State<'_, Store>,
    id: String,
    item_id: String,
    to: usize,
) -> Result<(), String> {
    store.move_item(&id, &item_id, to)?;
    changed(&app, Some(&id));
    Ok(())
}

/// The app saves its own state (`dial.setState`).
#[tauri::command]
pub fn types_set_state(
    app: AppHandle,
    store: State<'_, Store>,
    id: String,
    state: Value,
) -> Result<(), String> {
    // No change event: the app already holds this state, and a reload of it would re-render the
    // app from its own write.
    let _ = app;
    store.set_state(&id, &state)
}

#[tauri::command]
pub fn types_import_image(
    store: State<'_, Store>,
    id: String,
    path: String,
) -> Result<String, String> {
    store.import_image(&id, Path::new(&path))
}

/// Save pasted or generated bytes (base64) as an asset.
#[tauri::command]
pub fn types_import_image_data(
    store: State<'_, Store>,
    id: String,
    data: String,
) -> Result<String, String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|_| "Not base64 image data".to_owned())?;
    if bytes.len() as u64 > MAX_IMAGE {
        return Err("Images are at most 25 MB".into());
    }
    store.store_image(&id, &bytes)
}

/// Open a saved type asset image in the system's default viewer.
#[tauri::command]
pub async fn types_open_asset(
    webview: tauri::Webview,
    store: State<'_, Store>,
    path: String,
) -> Result<(), String> {
    if webview.label() != "main" {
        return Err("Type images are only available in the app UI".into());
    }
    let resolved = store.resolve_asset(Path::new(&path))?;
    if crate::attachments::image_mime(&resolved).is_none() {
        return Err("That file is not an image".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        tauri_plugin_opener::open_path(&resolved, None::<&str>)
            .map_err(|e| format!("Could not open image: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}


// The agent's side: the `types_*` MCP tools.

/// How to build a Type's app: the `types_guide` tool, and the briefing of a Type with no app yet.
pub const APP_GUIDE: &str = include_str!("types/guide.md");

fn args_object(args: &Value) -> Result<&Map<String, Value>, String> {
    args.as_object()
        .ok_or_else(|| "Arguments must be an object".into())
}

fn arg_str<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("`{key}` is required"))
}

fn kind_name(kind: FieldKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn describe_with(def: &TypeDef, has_app: bool, count: usize) -> String {
    let fields: Vec<String> = def
        .fields
        .iter()
        .map(|f| format!("{} ({})", f.key, kind_name(f.kind)))
        .collect();
    let fields = if fields.is_empty() {
        "free-form (any keys)".to_owned()
    } else {
        fields.join(", ")
    };
    let app = if has_app {
        if def.html.is_empty() {
            "has its app".to_owned()
        } else {
            format!("app {} KB", def.html.len().div_ceil(1024))
        }
    } else {
        "NO APP YET: build it (types_guide, then types_update with html)".to_owned()
    };
    format!(
        "{} {} [id {}] — {app}; {count} item(s). Fields: {fields}.",
        def.icon, def.name, def.id
    )
}

fn describe(def: &TypeDef, count: usize) -> String {
    describe_with(def, def.has_app(), count)
}

/// Marks the Type's own briefing in a prompt, so the UI can leave it out of the transcript.
pub const CONTEXT_OPEN: &str = "<type-context>";
const CONTEXT_CLOSE: &str = "</type-context>";

/// What the agent is told about the Type a conversation belongs to.
fn context(def: &TypeDef, count: usize, scope: Scope) -> String {
    let purpose = if def.description.trim().is_empty() {
        "not described yet; infer it from the name, the items and the user".to_owned()
    } else {
        def.description.trim().to_owned()
    };
    let kept = match scope {
        Scope::App => "in the app, shared by every project",
        Scope::Project => "in this project's .dial/types folder",
    };
    let task = if def.has_app() {
        "Change the app with types_edit (read it first with types_source) or rewrite it with \
         types_update; call types_guide if you need the window.dial API or the quality bar."
            .to_owned()
    } else {
        format!("It has no app yet: build it now, following this guide.\n{APP_GUIDE}")
    };
    format!(
        "{CONTEXT_OPEN}\n\
         The user is inside their Type “{} {}” (id {}), which Dial Desktop shows as its own small web \
         app that you build and keep improving. This conversation is about that Type, not about the \
         project's code. Purpose: {purpose}.\n\
         Now: {}\n\
         {task}\n\
         Always look at your work: after every visual change take types_screenshot (and use types_run \
         to click through interactions and read console errors), then fix what is off before you \
         finish. Kept {kept}; the types_* tools own its files, so never edit them directly. You may use \
         the web and image generation when the app needs them; leave the project's files alone unless \
         the user names one. Keep replies to a sentence or two.\n\
         {CONTEXT_CLOSE}",
        def.icon,
        def.name,
        def.id,
        describe(def, count)
    )
}

/// The scope a prompt written in Type `id` carries to an engine. `modern` is whether the engine takes
/// `type_context`. `None` sends nothing: an engine that predates the field, for a prompt in no Type.
pub fn type_scope(store: &Store, id: Option<&str>, modern: bool) -> Option<crate::engine::TypeScope> {
    let Some((id, briefing)) = id.and_then(|id| framing(store, id)) else {
        return modern.then_some(crate::engine::TypeScope::Plain);
    };
    let context = crate::engine::TypeContext { id, briefing };
    Some(if modern {
        crate::engine::TypeScope::Typed(context)
    } else {
        crate::engine::TypeScope::Framed(context)
    })
}

/// The text an engine without `type_context` reads: the Type's `[[type:ID]]` line, its briefing, then the user's text.
pub fn legacy_prompt(context: &crate::engine::TypeContext, text: &str) -> String {
    format!("[[type:{}]]\n{}\n{text}", context.id, context.briefing)
}

/// The Type's id and its briefing, when the Type is saved.
fn framing(store: &Store, id: &str) -> Option<(String, String)> {
    let def = {
        let _guard = store.guard();
        store.read_def(id).ok()?
    };
    let count = store.get(id).ok()?.items.len();
    let scope = store.locate(id).map(|(scope, _)| scope).unwrap_or_default();
    Some((id.to_owned(), context(&def, count, scope)))
}

/// The app's HTML from `html` or from the file at `htmlFile` (an absolute path), if given.
fn html_arg(args: &Map<String, Value>) -> Result<Option<String>, String> {
    if let Some(path) = args.get("htmlFile").and_then(Value::as_str) {
        let path = Path::new(path.trim());
        if !path.is_absolute() {
            return Err("`htmlFile` must be an absolute path".into());
        }
        let meta = std::fs::metadata(path).map_err(|_| format!("No file at {}", path.display()))?;
        if !meta.is_file() || meta.len() as usize > MAX_APP {
            return Err(format!("{} is not a file under {} KB", path.display(), MAX_APP / 1024));
        }
        return std::fs::read_to_string(path).map(Some).map_err(|e| e.to_string());
    }
    Ok(args.get("html").and_then(Value::as_str).map(str::to_owned))
}

/// Apply exact find/replace `edits` to `html`, all or none.
pub fn apply_edits(html: &str, edits: &[Value]) -> Result<String, String> {
    if edits.is_empty() {
        return Err("`edits` is empty".into());
    }
    let mut out = html.to_owned();
    for (n, edit) in edits.iter().enumerate() {
        let n = n + 1;
        let find = edit.get("find").and_then(Value::as_str).filter(|f| !f.is_empty())
            .ok_or_else(|| format!("Edit {n} needs a non-empty `find`"))?;
        let replace = edit.get("replace").and_then(Value::as_str)
            .ok_or_else(|| format!("Edit {n} needs `replace`"))?;
        let all = edit.get("all").and_then(Value::as_bool).unwrap_or(false);
        let found = out.matches(find).count();
        if found == 0 {
            return Err(format!("Edit {n}: `find` does not occur in the app (nothing was changed). Read it with types_source and copy the text exactly."));
        }
        if found > 1 && !all {
            return Err(format!("Edit {n}: `find` occurs {found} times (nothing was changed); add surrounding text to make it unique, or set `all`."));
        }
        out = out.replace(find, replace);
    }
    Ok(out)
}

fn item_list(args: &Map<String, Value>) -> Result<Vec<Map<String, Value>>, String> {
    match (args.get("items"), args.get("fields")) {
        (Some(Value::Array(list)), _) => Ok(list.iter().filter_map(|v| v.as_object().cloned()).collect()),
        (_, Some(Value::Object(fields))) => Ok(vec![fields.clone()]),
        _ => Err("`fields` (an object) or `items` (a list of them) is required".into()),
    }
}

/// Run one `types_*` tool for the agent (`types_screenshot` is `agent_browser`'s).
pub fn agent_call(
    app: &AppHandle,
    store: &Store,
    tool: &str,
    args: &Value,
) -> Result<String, String> {
    let args = args_object(args)?;
    match tool {
        "types_guide" => Ok(APP_GUIDE.to_owned()),
        "types_list" => {
            let list = store.list()?;
            if list.is_empty() {
                return Ok("The user has no types yet.".into());
            }
            let mut lines = Vec::new();
            for s in list {
                let def = store.get(&s.id)?.def;
                lines.push(describe_with(&def, s.has_app, s.count));
            }
            Ok(lines.join("\n"))
        }
        "types_create" => {
            let source = match args.get("blueprint") {
                Some(Value::Object(blueprint)) => blueprint,
                _ => args,
            };
            let mut members = source.clone();
            for key in ["htmlFile", "blueprint", "sessions", "type"] {
                members.remove(key);
            }
            let mut def: TypeDef = serde_json::from_value(Value::Object(members))
                .map_err(|e| format!("Not a type: {e}"))?;
            if let Some(html) = html_arg(source)? {
                def.html = html;
            }
            def.sessions.clear();
            let def = store.create(def)?;
            changed(app, Some(&def.id));
            Ok(format!(
                "Created {}. It shows under Types in the sidebar; types_open shows it to the user.",
                describe(&def, 0)
            ))
        }
        "types_update" => {
            let id = store.find(arg_str(args, "type")?)?;
            let mut patch = match args.get("changes") {
                Some(Value::Object(changes)) => changes.clone(),
                None => Map::new(),
                Some(_) => return Err("`changes` is an object of type members".into()),
            };
            // The UI owns which sessions belong to the type.
            patch.remove("sessions");
            if let Some(html) = html_arg(args)? {
                patch.insert("html".into(), Value::String(html));
            }
            if patch.is_empty() {
                return Err("Nothing to change: give `changes` or `htmlFile`".into());
            }
            let patch = Value::Object(patch);
            let def = store.update(&id, &patch)?;
            progress(app, &id, build_step(&def, &patch));
            let count = store.get(&id)?.items.len();
            Ok(format!("Updated {}. Look at it with types_screenshot.", describe(&def, count)))
        }
        "types_edit" => {
            let id = store.find(arg_str(args, "type")?)?;
            let edits = args.get("edits").and_then(Value::as_array).ok_or("`edits` (a list of {find, replace}) is required")?;
            let current = store.app_html(&id)?;
            if current.trim().is_empty() {
                return Err("This type has no app yet; write one with types_update".into());
            }
            let html = apply_edits(&current, edits)?;
            let patch = serde_json::json!({"html": html});
            let def = store.update(&id, &patch)?;
            progress(app, &id, build_step(&def, &patch));
            Ok(format!(
                "Applied {} edit(s); the app is {} KB. Look at it with types_screenshot.",
                edits.len(),
                def.html.len().div_ceil(1024)
            ))
        }
        "types_source" => {
            let id = store.find(arg_str(args, "type")?)?;
            let html = store.app_html(&id)?;
            if html.trim().is_empty() {
                return Ok("This type has no app yet.".into());
            }
            let lines: Vec<&str> = html.lines().collect();
            let line = |key: &str| args.get(key).and_then(Value::as_u64).map(|n| n as usize);
            let start = line("startLine").unwrap_or(1).max(1);
            let end = line("endLine").unwrap_or(lines.len()).min(lines.len());
            if start > end {
                return Err(format!("The app has {} lines", lines.len()));
            }
            let mut text = String::new();
            let mut last = start - 1;
            for l in &lines[start - 1..end] {
                if text.len() + l.len() > 60_000 {
                    break;
                }
                text.push_str(l);
                text.push('\n');
                last += 1;
            }
            let more = if last < end {
                format!("\n[Stopped at line {last}; read on with startLine {}.]", last + 1)
            } else {
                String::new()
            };
            Ok(format!("app.html, lines {start}–{last} of {}:\n{text}{more}", lines.len()))
        }
        "types_open" => {
            let id = store.find(arg_str(args, "type")?)?;
            live::open(app, &id)
        }
        "types_run" => {
            let id = store.find(arg_str(args, "type")?)?;
            live::run(app, &id, arg_str(args, "script")?)
        }
        "types_add_item" => {
            let id = store.find(arg_str(args, "type")?)?;
            let items = item_list(args)?;
            let source = args
                .get("source")
                .and_then(Value::as_str)
                .map(|text| Source {
                    text: text.to_owned(),
                    ..Source::default()
                });
            let mut saved = 0;
            let mut notes = Vec::new();
            for fields in items {
                match store.add_item(&id, fields, source.clone()) {
                    Ok((_, problems)) => {
                        saved += 1;
                        notes.extend(problems);
                    }
                    Err(e) => notes.push(e),
                }
            }
            changed(app, Some(&id));
            let full = store.get(&id)?;
            if saved > 0 {
                progress(app, &id, serde_json::json!({"saved": full.items.len()}));
            }
            let mut out = format!("Saved {saved} item(s) to {}.", full.def.name);
            if !notes.is_empty() {
                out.push_str("\nNot kept: ");
                out.push_str(&notes.join("; "));
            }
            if saved == 0 {
                return Err(out);
            }
            Ok(out)
        }
        "types_items" => {
            let id = store.find(arg_str(args, "type")?)?;
            let full = store.get(&id)?;
            let mut out = describe_with(&full.def, full.has_app, full.items.len());
            for item in full.items.iter().take(200) {
                out.push_str(&format!(
                    "\n- [{}]{} {}",
                    item.id,
                    if item.done { " (done)" } else { "" },
                    serde_json::to_string(&item.fields).unwrap_or_default()
                ));
            }
            Ok(clip(&out, 30_000))
        }
        "types_update_item" => {
            let id = store.find(arg_str(args, "type")?)?;
            let item_id = arg_str(args, "item")?;
            let patch = ItemPatch {
                fields: args.get("fields").and_then(Value::as_object).cloned(),
                done: args.get("done").and_then(Value::as_bool),
            };
            store.update_item(&id, item_id, patch)?;
            changed(app, Some(&id));
            Ok(format!("Updated item {item_id}."))
        }
        "types_remove_item" => {
            let id = store.find(arg_str(args, "type")?)?;
            let item_id = arg_str(args, "item")?;
            store.remove_item(&id, item_id)?;
            changed(app, Some(&id));
            Ok(format!("Removed item {item_id}."))
        }
        "types_state" => {
            let id = store.find(arg_str(args, "type")?)?;
            if let Some(state) = args.get("state") {
                store.set_state(&id, state)?;
                changed(app, Some(&id));
                return Ok("Replaced the app's state.".into());
            }
            Ok(clip(&serde_json::to_string_pretty(&store.get(&id)?.state).unwrap_or_default(), 30_000))
        }
        other => Err(format!("Unknown tool `{other}`")),
    }
}


/// The `types_*` tools, in the MCP tool-list shape.
pub fn tools() -> Vec<Value> {
    use serde_json::json;
    let tool = |name: &str, title: &str, read_only: bool, description: &str, schema: Value| {
        json!({
            "name": name,
            "title": title,
            "description": description,
            "inputSchema": schema,
            "annotations": {"title": title, "readOnlyHint": read_only, "openWorldHint": false}
        })
    };
    let which = || json!({"type": "string", "description": "The type's name or id"});
    let fields = json!({"type": "array", "description": "The keys items use: [{key, label, kind}]. Kinds: text, longtext, markdown, list, steps, image, images, time, date, datetime, number, url, tags, rating, toggle, json. Omit for free-form items.", "items": {"type": "object"}});
    let html = json!({"type": "string", "description": "The whole app: one HTML document"});
    let html_file = json!({"type": "string", "description": "Instead of html: absolute path of an .html file you wrote"});
    vec![
        tool(
            "types_guide",
            "How to build a Type's app",
            true,
            "The guide to building a Type's app: the window.dial runtime (items, state, images), what \
             the sandbox allows (https APIs and CDNs), the quality bar and the build-and-check loop. \
             Read it before building or substantially changing a Type's app.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "types_list",
            "List the user's Types",
            true,
            "List the user's Types: things they keep (recipes, places, plans, scores) that Dial Desktop \
             opens as their own small web apps. Check it before creating a type or when the user asks \
             to save something somewhere.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "types_create",
            "Create a Type",
            false,
            "Create a Type when the user wants a place to keep or use something (a cookbook, a \
             weather app for their cities, a reading list, a game). Give it a name, a one-line \
             description, an emoji icon, an accent (#hex), its fields and, ideally, its app (html or \
             htmlFile; see types_guide). Then save items with types_add_item, show it with types_open \
             and check it with types_screenshot.",
            json!({"type": "object", "properties": {
                "name": {"type": "string"}, "description": {"type": "string"}, "icon": {"type": "string"},
                "accent": {"type": "string"}, "fields": fields.clone(), "html": html.clone(), "htmlFile": html_file.clone()
            }, "required": ["name"]}),
        ),
        tool(
            "types_update",
            "Change a Type or replace its app",
            false,
            "Change a Type: `changes` holds the members to replace (name, description, icon, accent, \
             fields, html); items and state stay. Pass htmlFile to load the app from a file you wrote. \
             For small changes to the app prefer types_edit. Take types_screenshot afterwards.",
            json!({"type": "object", "properties": {
                "type": which(),
                "changes": {"type": "object", "properties": {"name": {"type": "string"}, "description": {"type": "string"}, "icon": {"type": "string"}, "accent": {"type": "string"}, "fields": fields, "html": html}},
                "htmlFile": html_file
            }, "required": ["type"]}),
        ),
        tool(
            "types_edit",
            "Edit a Type's app",
            false,
            "Edit a Type's app in place with exact find/replace pairs, applied in order, all or \
             none. Each `find` must occur exactly once unless `all` is true. Read the app first with \
             types_source; take types_screenshot afterwards.",
            json!({"type": "object", "properties": {
                "type": which(),
                "edits": {"type": "array", "items": {"type": "object", "properties": {
                    "find": {"type": "string"}, "replace": {"type": "string"}, "all": {"type": "boolean"}
                }, "required": ["find", "replace"]}}
            }, "required": ["type", "edits"]}),
        ),
        tool(
            "types_source",
            "Read a Type's app",
            true,
            "Read a Type's app (its HTML), optionally only lines startLine to endLine.",
            json!({"type": "object", "properties": {
                "type": which(), "startLine": {"type": "integer"}, "endLine": {"type": "integer"}
            }, "required": ["type"]}),
        ),
        tool(
            "types_open",
            "Show a Type",
            false,
            "Open a Type's app in Dial Desktop, in place of the conversation, and wait until it has \
             loaded, so types_screenshot and types_run can see it. The user's conversations are \
             inside a Type already when they talk to it; use this for one created from elsewhere.",
            json!({"type": "object", "properties": {"type": which()}, "required": ["type"]}),
        ),
        tool(
            "types_screenshot",
            "Look at a Type's app",
            false,
            "A PNG of the Type's app as the user sees it (macOS), with the console errors and \
             warnings it logged since it loaded. The Type must be open (types_open). Use it after \
             every visual change and fix what looks wrong.",
            json!({"type": "object", "properties": {"type": which()}, "required": ["type"]}),
        ),
        tool(
            "types_run",
            "Try a Type's app",
            false,
            "Run JavaScript inside the open Type's app and return its result as JSON, with console \
             output logged meanwhile. `script` is an async function body (use return); `dial` and the \
             DOM are in scope. Use it to click buttons, fill inputs, read the DOM, check layout sizes \
             or test the app's logic; take types_screenshot after visible changes. The Type must be \
             open (types_open).",
            json!({"type": "object", "properties": {"type": which(), "script": {"type": "string"}}, "required": ["type", "script"]}),
        ),
        tool(
            "types_add_item",
            "Save to a Type",
            false,
            "Save one item (`fields`) or several (`items`) to a Type, by field key: lists as arrays of \
             strings, steps in order, images as absolute local paths (an attachment's path, or a \
             generated or downloaded image), which become asset names. `source` holds the original \
             text. The open app updates by itself.",
            json!({"type": "object", "properties": {
                "type": which(),
                "fields": {"type": "object"},
                "items": {"type": "array", "items": {"type": "object"}},
                "source": {"type": "string"}
            }, "required": ["type"]}),
        ),
        tool(
            "types_items",
            "Read a Type's items",
            true,
            "Read what is saved in a Type, with item ids.",
            json!({"type": "object", "properties": {"type": which()}, "required": ["type"]}),
        ),
        tool(
            "types_update_item",
            "Change a saved item",
            false,
            "Change fields of a saved item (by id from types_items; a null field clears it) or mark it done.",
            json!({"type": "object", "properties": {
                "type": which(),
                "item": {"type": "string"},
                "fields": {"type": "object"},
                "done": {"type": "boolean"}
            }, "required": ["type", "item"]}),
        ),
        tool(
            "types_remove_item",
            "Remove a saved item",
            false,
            "Remove a saved item (by id from types_items).",
            json!({"type": "object", "properties": {"type": which(), "item": {"type": "string"}}, "required": ["type", "item"]}),
        ),
        tool(
            "types_state",
            "Read or replace a Type app's state",
            false,
            "Read the JSON state a Type's app keeps for itself (dial.state: settings, progress, \
             caches), or replace it with `state`.",
            json!({"type": "object", "properties": {"type": which(), "state": {}}, "required": ["type"]}),
        ),
    ]
}

/// The scheme a type's app loads from, in a sandboxed frame of the types view.
pub const CUSTOM_SCHEME: &str = "dialtype";
/// A type's app runs its own code and may use the web over https (APIs with CORS, CDNs, fonts),
/// in a frame without same-origin access: it cannot reach the app, its files or its IPC.
pub const CUSTOM_CSP: &str = "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval' https: blob:; \
style-src 'unsafe-inline' https:; img-src data: blob: https:; font-src data: https:; \
media-src data: blob: https:; connect-src https: wss:; worker-src blob:; \
form-action 'none'; base-uri 'none'; frame-ancestors *";

/// The `window.dial` runtime injected into every app.
const RUNTIME: &str = include_str!("types/runtime.js");

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
    let page = store.app_html(id).ok().filter(|html| !html.trim().is_empty());
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
