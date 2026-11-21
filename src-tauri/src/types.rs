//! Types: collections the user saves things into, each drawn as its own small app.
//!
//! A type is a blueprint ([`TypeDef`]: the shape of one item, the layout that draws the items and
//! a look) plus its saved [`Item`]s. The UI draws the blueprint with built-in layouts (a book, a
//! checklist, a board, a wheel, a calendar, cards, a table) or, for a `custom` layout, the blueprint's own
//! HTML in a sandboxed frame. The agent designs blueprints and saves items through the
//! `types_*` tools (`agent_browser`); the user saves answers from the conversation.
//!
//! Types belong to the user, not to a project: they live in the app's folder,
//! `types/<id>/type.json`, `types/<id>/items.json` and `types/<id>/assets/`. Every write is atomic
//! and every input, from the UI or the agent, goes through [`normalize`] or [`sanitize_fields`].

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};

#[cfg(test)]
mod tests;

pub const VERSION: u32 = 1;
/// The event the UI reloads on, with the changed type's id (or `null` for the list).
pub const CHANGED: &str = "types-changed";
const MAX_FIELDS: usize = 24;
const MAX_ITEMS: usize = 5_000;
const MAX_LIST: usize = 200;
const MAX_SHORT: usize = 2_000;
const MAX_LONG: usize = 40_000;
const MAX_CUSTOM: usize = 256 * 1024;
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
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub kind: FieldKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    Book,
    Checklist,
    Board,
    Wheel,
    Calendar,
    #[default]
    Cards,
    Table,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct View {
    pub layout: Layout,
    #[serde(alias = "title_field")]
    pub title_field: Option<String>,
    #[serde(alias = "subtitle_field")]
    pub subtitle_field: Option<String>,
    #[serde(alias = "image_field")]
    pub image_field: Option<String>,
    #[serde(alias = "time_field")]
    pub time_field: Option<String>,
    /// Items with a time can remind the user before it (a checklist's bell).
    pub reminders: bool,
    /// Minutes before an item's time to remind; `0` is "at the time".
    #[serde(alias = "lead_minutes")]
    pub lead_minutes: Vec<u32>,
    /// Checked items uncheck the next day: a routine, not a one-off list.
    #[serde(alias = "daily_reset")]
    pub daily_reset: bool,
    /// A line under the title: a book's cover, a board's header.
    pub tagline: String,
    /// A layout's variant: for a book `cookbook`, `journal` or `guide` (each its own pages);
    /// empty lets the UI pick one from the fields.
    pub style: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Font {
    #[default]
    Sans,
    Serif,
    Mono,
    Hand,
    Display,
    Rounded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Texture {
    #[default]
    None,
    Paper,
    Linen,
    Grid,
    Dots,
    Noise,
}

/// Colours are `#rgb` or `#rrggbb`; an empty one follows the app's theme.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub accent: String,
    /// The page colour.
    pub paper: String,
    /// The text colour on the page.
    pub ink: String,
    pub font: Font,
    pub texture: Texture,
    pub radius: u8,
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
    pub created: i64,
    pub updated: i64,
    pub fields: Vec<Field>,
    pub view: View,
    pub theme: Theme,
    /// A `custom` layout's page: HTML with inline CSS and JS, run in a sandboxed frame that
    /// receives the items by `postMessage` (see `CustomView.vue`).
    #[serde(alias = "custom_html")]
    pub custom_html: String,
    /// The agent has chosen this type's layout, fields and look; a type made from the sidebar
    /// with only a name has not, and the first save designs it.
    pub designed: bool,
    /// The type's own conversation in each project (workspace root to session id): where it is
    /// designed and where saves to it are added. These sessions stay out of the session list.
    pub sessions: std::collections::BTreeMap<String, String>,
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
    /// The local day (`YYYY-MM-DD`) it was checked, for a daily reset.
    pub done_on: Option<String>,
    /// Remind about this item before its time.
    pub remind: bool,
    pub source: Option<Source>,
}

/// A change to one item; absent members stay as they are.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ItemPatch {
    /// Merged over the item's fields; a `null` member clears that field.
    pub fields: Option<Map<String, Value>>,
    pub done: Option<bool>,
    #[serde(alias = "done_on")]
    pub done_on: Option<String>,
    pub remind: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub description: String,
    pub layout: Layout,
    pub accent: String,
    pub count: usize,
    pub updated: i64,
    pub designed: bool,
    /// The ids of the type's own sessions (see [`TypeDef::sessions`]).
    pub sessions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Full {
    pub def: TypeDef,
    pub items: Vec<Item>,
    /// The folder item images are in; an image field holds a file name in it.
    pub assets: String,
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

/// Fields a layout starts with when the blueprint names none.
fn default_fields(layout: Layout) -> Vec<Field> {
    let f = |key: &str, label: &str, kind| Field {
        key: key.into(),
        label: label.into(),
        kind,
    };
    match layout {
        Layout::Book => vec![
            f("title", "Title", FieldKind::Text),
            f("image", "Image", FieldKind::Image),
            f("body", "Page", FieldKind::Markdown),
        ],
        Layout::Checklist => vec![
            f("title", "Task", FieldKind::Text),
            f("time", "Time", FieldKind::Time),
            f("notes", "Notes", FieldKind::Longtext),
        ],
        Layout::Board => vec![
            f("image", "Image", FieldKind::Image),
            f("title", "Caption", FieldKind::Text),
        ],
        Layout::Wheel => vec![
            f("title", "Option", FieldKind::Text),
            f("notes", "Notes", FieldKind::Longtext),
        ],
        Layout::Calendar => vec![
            f("title", "Title", FieldKind::Text),
            f("when", "When", FieldKind::Datetime),
            f("notes", "Notes", FieldKind::Longtext),
        ],
        Layout::Cards | Layout::Table | Layout::Custom => vec![
            f("title", "Title", FieldKind::Text),
            f("body", "Notes", FieldKind::Markdown),
        ],
    }
}

/// Check and tidy a blueprint from the UI or the agent. Keeps `id` and `created`.
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
    if fields.is_empty() {
        fields = default_fields(def.view.layout);
    }
    // A calendar places items by a date: name one, from a date-like text field if need be.
    if def.view.layout == Layout::Calendar
        && !fields
            .iter()
            .any(|f| matches!(f.kind, FieldKind::Date | FieldKind::Datetime))
    {
        let named = def
            .view
            .time_field
            .as_deref()
            .map(slug_key)
            .filter(|k| fields.iter().any(|f| &f.key == k));
        let guess = named.or_else(|| {
            ["when", "date", "day", "start", "starts", "time", "at"]
                .iter()
                .find(|k| {
                    fields.iter().any(|f| {
                        f.key == **k && !matches!(f.kind, FieldKind::Image | FieldKind::Images)
                    })
                })
                .map(|k| (*k).to_owned())
        });
        match guess.and_then(|k| fields.iter().position(|f| f.key == k)) {
            Some(at) => fields[at].kind = FieldKind::Datetime,
            None => {
                if fields.len() == MAX_FIELDS {
                    fields.pop();
                }
                fields.push(Field {
                    key: "when".into(),
                    label: "When".into(),
                    kind: FieldKind::Datetime,
                });
            }
        }
    }
    def.fields = fields;

    let has = |key: &Option<String>, kinds: &[FieldKind]| {
        key.as_ref().map(|k| slug_key(k)).filter(|k| {
            def.fields
                .iter()
                .any(|f| &f.key == k && (kinds.is_empty() || kinds.contains(&f.kind)))
        })
    };
    let first = |kinds: &[FieldKind]| {
        def.fields
            .iter()
            .find(|f| kinds.contains(&f.kind))
            .map(|f| f.key.clone())
    };
    let titles = [FieldKind::Text, FieldKind::Url];
    let images = [FieldKind::Image, FieldKind::Images];
    let times = [FieldKind::Time, FieldKind::Datetime, FieldKind::Date];
    let dates = [FieldKind::Datetime, FieldKind::Date];
    def.view.title_field = has(&def.view.title_field, &[]).or_else(|| first(&titles));
    def.view.subtitle_field = has(&def.view.subtitle_field, &[]);
    def.view.image_field = has(&def.view.image_field, &images).or_else(|| first(&images));
    def.view.time_field = if def.view.layout == Layout::Calendar {
        has(&def.view.time_field, &dates).or_else(|| first(&dates))
    } else {
        has(&def.view.time_field, &times).or_else(|| first(&times))
    };
    def.view.tagline = clip(&def.view.tagline, 160);
    def.view.style = match def.view.style.trim().to_ascii_lowercase().as_str() {
        s @ ("cookbook" | "journal" | "guide") if def.view.layout == Layout::Book => s.to_owned(),
        _ => String::new(),
    };
    let mut leads: Vec<u32> = def
        .view
        .lead_minutes
        .iter()
        .map(|m| (*m).min(1_440))
        .collect();
    leads.sort_unstable_by(|a, b| b.cmp(a));
    leads.dedup();
    leads.truncate(4);
    if leads.is_empty() {
        leads = vec![15, 0];
    }
    def.view.lead_minutes = leads;

    for color in [
        &mut def.theme.accent,
        &mut def.theme.paper,
        &mut def.theme.ink,
    ] {
        *color = color.trim().to_owned();
        if !valid_color(color) {
            color.clear();
        }
    }
    def.theme.radius = def.theme.radius.min(28);
    def.sessions
        .retain(|root, id| !root.is_empty() && root.len() <= 4096 && !id.is_empty() && id.len() <= 200);
    while def.sessions.len() > 64 {
        let first = def.sessions.keys().next().cloned().unwrap_or_default();
        def.sessions.remove(&first);
    }

    if def.custom_html.len() > MAX_CUSTOM {
        return Err(format!(
            "A custom view holds at most {} KB of HTML",
            MAX_CUSTOM / 1024
        ));
    }
    if def.view.layout == Layout::Custom && def.custom_html.trim().is_empty() {
        return Err("A custom layout needs its customHtml page".into());
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

/// Keep only the blueprint's fields, each in its kind's shape. Images must already be assets.
pub fn sanitize_fields(def: &TypeDef, fields: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
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

/// The saved types, owned by the app.
pub struct Store {
    root: Option<PathBuf>,
    lock: Mutex<()>,
}

impl Store {
    pub fn new(root: Option<PathBuf>) -> Self {
        Self {
            root,
            lock: Mutex::new(()),
        }
    }

    /// The store in the app's per-user folder.
    pub fn for_home(home: Option<&Path>) -> Self {
        Self::new(crate::launcher::app_dir(home).map(|d| d.join("types")))
    }

    fn root(&self) -> Result<&Path, String> {
        self.root
            .as_deref()
            .ok_or_else(|| "No folder for saved types (no home folder)".into())
    }

    fn dir(&self, id: &str) -> Result<PathBuf, String> {
        if !valid_id(id) {
            return Err(format!("No type `{id}`"));
        }
        Ok(self.root()?.join(id))
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn read_def(&self, id: &str) -> Result<TypeDef, String> {
        let path = self.dir(id)?.join("type.json");
        let bytes = std::fs::read(&path).map_err(|_| format!("No type `{id}`"))?;
        serde_json::from_slice(&bytes).map_err(|e| format!("Type `{id}` is damaged: {e}"))
    }

    fn write_def(&self, def: &TypeDef) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(def).map_err(|e| e.to_string())?;
        write_atomic(&self.dir(&def.id)?.join("type.json"), &bytes)
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

    pub fn list(&self) -> Result<Vec<Summary>, String> {
        let _guard = self.guard();
        let Ok(entries) = std::fs::read_dir(self.root()?) else {
            return Ok(Vec::new());
        };
        let mut out: Vec<Summary> = entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|id| valid_id(id))
            .filter_map(|id| {
                let def = self.read_def(&id).ok()?;
                let count = self.read_items(&id).map(|i| i.len()).unwrap_or(0);
                Some(Summary {
                    id: def.id,
                    name: def.name,
                    icon: def.icon,
                    description: def.description,
                    layout: def.view.layout,
                    accent: def.theme.accent,
                    count,
                    updated: def.updated,
                    designed: def.designed,
                    sessions: def.sessions.into_values().collect(),
                })
            })
            .collect();
        out.sort_by_key(|s| s.name.to_lowercase());
        Ok(out)
    }

    pub fn get(&self, id: &str) -> Result<Full, String> {
        let _guard = self.guard();
        Ok(Full {
            def: self.read_def(id)?,
            items: self.read_items(id)?,
            assets: self.dir(id)?.join("assets").to_string_lossy().into_owned(),
        })
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
        let root = self.root()?;
        let count = std::fs::read_dir(root).map(|d| d.count()).unwrap_or(0);
        if count >= MAX_TYPES {
            return Err(format!("At most {MAX_TYPES} types"));
        }
        let mut base: String = slug_key(&def.name).replace('_', "-");
        if base.is_empty() {
            base = "type".into();
        }
        base.truncate(40);
        def.id = format!("{}-{}", base.trim_end_matches('-'), random_suffix());
        self.write_def(&def)?;
        self.write_items(&def.id, &[])?;
        Ok(def)
    }

    /// Merge `patch` (members of a blueprint) over type `id`; its items stay.
    pub fn update(&self, id: &str, patch: &Value) -> Result<TypeDef, String> {
        let _guard = self.guard();
        let current = self.read_def(id)?;
        let mut merged = serde_json::to_value(&current).map_err(|e| e.to_string())?;
        let (Value::Object(base), Value::Object(patch)) = (&mut merged, patch) else {
            return Err("A change to a type is an object".into());
        };
        for (key, value) in patch {
            let key = match key.as_str() {
                "custom_html" => "customHtml",
                other => other,
            };
            if matches!(key, "id" | "created" | "updated" | "version") {
                continue;
            }
            match (base.get_mut(key), value) {
                (Some(Value::Object(inner)), Value::Object(more))
                    if !matches!(key, "fields" | "sessions") =>
                {
                    for (k, v) in more {
                        inner.insert(k.clone(), v.clone());
                    }
                }
                _ => {
                    base.insert(key.to_owned(), value.clone());
                }
            }
        }
        let mut def: TypeDef =
            serde_json::from_value(merged).map_err(|e| format!("Not a type: {e}"))?;
        def.id = current.id;
        def.created = current.created;
        let def = normalize(def, crate::time::now())?;
        self.write_def(&def)?;
        Ok(def)
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        let _guard = self.guard();
        let dir = self.dir(id)?;
        if !dir.join("type.json").is_file() {
            return Err(format!("No type `{id}`"));
        }
        std::fs::remove_dir_all(dir).map_err(|e| e.to_string())
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
                let path = Path::new(text.strip_prefix("file://").unwrap_or(text));
                if path.is_absolute() {
                    match self.import_image(&def.id, path) {
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

    /// Save a new item; image fields may hold absolute paths, which are imported.
    /// Returns the item and anything that could not be kept.
    pub fn add_item(
        &self,
        id: &str,
        mut fields: Map<String, Value>,
        source: Option<Source>,
    ) -> Result<(Item, Vec<String>), String> {
        let _guard = self.guard();
        let def = self.read_def(id)?;
        let problems = self.import_field_images(&def, &mut fields);
        let mut items = self.read_items(id)?;
        if items.len() >= MAX_ITEMS {
            return Err(format!("A type holds at most {MAX_ITEMS} items"));
        }
        let fields = sanitize_fields(&def, &fields);
        if fields.is_empty() {
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
            remind: def.view.reminders,
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
        let _guard = self.guard();
        let def = self.read_def(id)?;
        let mut items = self.read_items(id)?;
        let item = items
            .iter_mut()
            .find(|i| i.id == item_id)
            .ok_or_else(|| format!("No item `{item_id}`"))?;
        if let Some(mut more) = patch.fields {
            self.import_field_images(&def, &mut more);
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
            item.done_on = done.then(|| clip(patch.done_on.as_deref().unwrap_or(""), 10));
        }
        if let Some(remind) = patch.remind {
            item.remind = remind;
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

#[tauri::command]
pub fn types_list(store: State<'_, Store>) -> Result<Vec<Summary>, String> {
    store.list()
}

#[tauri::command]
pub fn types_get(store: State<'_, Store>, id: String) -> Result<Full, String> {
    store.get(&id)
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
    let def = store.update(&id, &patch)?;
    changed(&app, Some(&id));
    Ok(def)
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
) -> Result<Item, String> {
    let (item, _) = store.add_item(&id, fields, source)?;
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
) -> Result<Item, String> {
    let item = store.update_item(&id, &item_id, patch)?;
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

// The agent's side: the `types_*` MCP tools.

/// How the blueprint format reads to the agent; shared by the tool descriptions.
pub const BLUEPRINT_GUIDE: &str = "A Type is a saved collection the user opens as its own little app. \
Blueprint: {name, description, icon (one emoji), fields: [{key, label, kind}], view: {layout, \
titleField, subtitleField, imageField, timeField, reminders, leadMinutes, dailyReset, tagline}, \
theme: {accent, paper, ink (#hex colours), font, texture, radius 0-28}, customHtml}. \
Field kinds: text, longtext, markdown, list, steps, image, images, time (HH:MM), date (YYYY-MM-DD), \
datetime (YYYY-MM-DD HH:MM), number, url, tags, rating (0-5), toggle. \
Layouts: book (one item per page, flipped like a real book: recipes, journals, guides; \
view.style picks its pages: cookbook for recipes, journal for dated entries, guide for \
chapters), \
checklist (tick off items with times; reminders:true adds a remind-me bell that notifies \
leadMinutes before each time; dailyReset:true unticks every day: routines), board (a masonry \
image wall: moodboards, inspiration), wheel (a spinner that picks one item at random: deciders), \
calendar (a month grid with an agenda, items placed by their date or datetime timeField: \
events, appointments, plans, deadlines; reminders:true adds the bell), \
cards (a grid of cards), table (rows and columns), custom (your own page in customHtml). \
Fonts: sans, serif, mono, hand, display, rounded. Textures: none, paper, linen, grid, dots, noise. \
Pick the layout, fields and look that make the collection feel like the real thing. \
customHtml is a full HTML document with inline CSS and JS only (no network, no external files); \
it receives window message {type:'dial-type', def, items, images} where images maps asset names to \
data URLs, and may post {type:'dial-type-update', itemId, fields|done} back to change an item. \
Prefer a built-in layout unless none fits. \
A user message that starts with a line `[[type:ID]]` was written inside that Type's own view: the \
user is looking at the Type, the conversation is its own, and \"it\" or \"this\" means the Type. \
Make the change they ask for with types_update (layout, fields, look) and the item tools, check \
with types_items, and keep replies to a short sentence. When the Type is not designed yet, design \
it from its name and what is being saved: pick a layout, fields, icon and theme that make it feel \
like the real thing. To save something into it, fill the Type's fields from the content (split a \
list into several items when the layout is a checklist or wheel) with types_add_item. When the \
Type has an image field and the item has no picture of its own, make one for it with your image \
generation tool when you have one (the dish itself for a recipe, never a generic stand-in) and \
pass its absolute path; otherwise leave the field empty and the Type draws its own illustration.";

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

fn describe(def: &TypeDef, count: usize) -> String {
    let fields: Vec<String> = def
        .fields
        .iter()
        .map(|f| {
            format!(
                "{} ({})",
                f.key,
                serde_json::to_value(f.kind)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default()
            )
        })
        .collect();
    let layout = serde_json::to_value(def.view.layout)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    let design = if def.designed {
        ""
    } else {
        " NOT DESIGNED YET: choose its layout, fields, icon and theme with types_update."
    };
    format!(
        "{} {} [id {}] — {layout} layout, {count} item(s). Fields: {}.{design}",
        def.icon,
        def.name,
        def.id,
        fields.join(", ")
    )
}

/// Run one `types_*` tool for the agent.
pub fn agent_call(
    app: &AppHandle,
    store: &Store,
    tool: &str,
    args: &Value,
) -> Result<String, String> {
    let args = args_object(args)?;
    match tool {
        "types_list" => {
            let list = store.list()?;
            if list.is_empty() {
                return Ok("The user has no types yet.".into());
            }
            let mut lines = Vec::new();
            for s in list {
                let def = store.get(&s.id)?.def;
                lines.push(describe(&def, s.count));
            }
            Ok(lines.join("\n"))
        }
        "types_create" => {
            let blueprint = args.get("blueprint").unwrap_or(&Value::Null);
            let blueprint = if blueprint.is_object() {
                blueprint.clone()
            } else {
                Value::Object(args.clone())
            };
            let mut def: TypeDef =
                serde_json::from_value(blueprint).map_err(|e| format!("Not a blueprint: {e}"))?;
            def.designed = true;
            def.sessions.clear();
            let def = store.create(def)?;
            changed(app, Some(&def.id));
            Ok(format!(
                "Created {}. The user sees it under Types in the sidebar. Save items with types_add_item.",
                describe(&def, 0)
            ))
        }
        "types_update" => {
            let id = store.find(arg_str(args, "type")?)?;
            let mut patch = args
                .get("changes")
                .filter(|v| v.is_object())
                .cloned()
                .ok_or("`changes` (an object of blueprint members) is required")?;
            if let Value::Object(members) = &mut patch {
                // The UI owns which sessions belong to the type.
                members.remove("sessions");
                members.insert("designed".into(), Value::Bool(true));
            }
            let def = store.update(&id, &patch)?;
            changed(app, Some(&id));
            let count = store.get(&id)?.items.len();
            Ok(format!("Updated {}", describe(&def, count)))
        }
        "types_add_item" => {
            let id = store.find(arg_str(args, "type")?)?;
            let items: Vec<Map<String, Value>> = match (args.get("items"), args.get("fields")) {
                (Some(Value::Array(list)), _) => {
                    list.iter().filter_map(|v| v.as_object().cloned()).collect()
                }
                (_, Some(Value::Object(fields))) => vec![fields.clone()],
                _ => {
                    return Err(
                        "`fields` (an object) or `items` (a list of them) is required".into(),
                    );
                }
            };
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
            let name = store.get(&id)?.def.name;
            let mut out = format!("Saved {saved} item(s) to {name}.");
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
            let mut out = describe(&full.def, full.items.len());
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
                ..ItemPatch::default()
            };
            store.update_item(&id, item_id, patch)?;
            changed(app, Some(&id));
            Ok(format!("Updated item {item_id}."))
        }
        other => Err(format!("Unknown tool `{other}`")),
    }
}

/// The `types_*` tools, in the MCP tool-list shape.
pub fn tools() -> Vec<Value> {
    use serde_json::json;
    let tool = |name: &str, title: &str, read_only: bool, description: String, schema: Value| {
        json!({
            "name": name,
            "title": title,
            "description": description,
            "inputSchema": schema,
            "annotations": {"title": title, "readOnlyHint": read_only, "openWorldHint": false}
        })
    };
    let which = json!({"type": "string", "description": "The type's name or id"});
    vec![
        tool(
            "types_list",
            "List the user's Types",
            true,
            "List the user's Types (saved collections in Dial Desktop) with their fields. Check it \
             before creating a type or when the user asks to save something somewhere."
                .into(),
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "types_create",
            "Create a Type",
            false,
            format!(
                "Create a Type when the user asks for a place to keep things (a cookbook, a \
                 routine, a moodboard, a decider). {BLUEPRINT_GUIDE}"
            ),
            json!({"type": "object", "properties": {"blueprint": {"type": "object", "description": "The blueprint"}}, "required": ["blueprint"]}),
        ),
        tool(
            "types_update",
            "Change a Type's design",
            false,
            format!(
                "Restyle or reshape a Type; its items stay. `changes` holds the blueprint members to \
                 replace (view and theme merge member by member; fields replace the list). {BLUEPRINT_GUIDE}"
            ),
            json!({"type": "object", "properties": {"type": which, "changes": {"type": "object"}}, "required": ["type", "changes"]}),
        ),
        tool(
            "types_add_item",
            "Save to a Type",
            false,
            "Save one item (`fields`) or several (`items`) to a Type, filling its fields by key \
             from what the user wants to keep: lists as arrays of strings, steps in order, \
             images as absolute local file paths (generated or downloaded images). `source` is \
             the original text, kept with the item."
                .into(),
            json!({"type": "object", "properties": {
                "type": which,
                "fields": {"type": "object"},
                "items": {"type": "array", "items": {"type": "object"}},
                "source": {"type": "string"}
            }, "required": ["type"]}),
        ),
        tool(
            "types_items",
            "Read a Type's items",
            true,
            "Read what is saved in a Type, with item ids.".into(),
            json!({"type": "object", "properties": {"type": which}, "required": ["type"]}),
        ),
        tool(
            "types_update_item",
            "Change a saved item",
            false,
            "Change fields of a saved item (by id from types_items) or tick it done.".into(),
            json!({"type": "object", "properties": {
                "type": which,
                "item": {"type": "string"},
                "fields": {"type": "object"},
                "done": {"type": "boolean"}
            }, "required": ["type", "item"]}),
        ),
    ]
}

/// The scheme a `custom` layout's page loads from, in a sandboxed frame of the types view.
pub const CUSTOM_SCHEME: &str = "dialtype";
/// A custom page runs only its own inline code: no network, no other files, no forms.
pub const CUSTOM_CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; \
style-src 'unsafe-inline'; img-src data: blob:; font-src data:; media-src data: blob:; \
form-action 'none'; base-uri 'none'; frame-ancestors *";

/// Answer `dialtype://localhost/<id>`: type `id`'s custom page under [`CUSTOM_CSP`].
pub fn custom_page(store: &Store, path: &str) -> tauri::http::Response<Vec<u8>> {
    let id = path
        .trim_start_matches('/')
        .split(['/', '?'])
        .next()
        .unwrap_or("");
    let page = store
        .get(id)
        .ok()
        .filter(|full| full.def.view.layout == Layout::Custom)
        .map(|full| full.def.custom_html);
    let (status, body) = match page {
        Some(html) => (200, html.into_bytes()),
        None => (404, b"No custom page".to_vec()),
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
