//! The settings page: the engine's settings reference (`settings_schema`) joined with the saved
//! values (`get_settings`) into typed controls the UI renders without knowing the schema.
//!
//! The reference declares each setting with a `kind` and JSON-Schema-like `constraints`; values
//! come back as leaf keys (`providers.openai.base_url`), so table settings are folded back
//! together here. Credentials are never shown or round-tripped: a redacted value is read-only.

use crate::model::SavedSetting;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::HashMap;

#[cfg(test)]
mod tests;

/// Everything the settings page shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    /// Tabs in display order ("Common" first).
    pub tabs: Vec<String>,
    pub settings: Vec<SettingView>,
}

/// One setting row.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingView {
    pub key: String,
    pub label: String,
    pub description: String,
    pub tab: String,
    pub section: String,
    pub control: Control,
    /// The effective value: saved, or the default.
    pub value: Value,
    pub default: Value,
    /// The value differs from the default.
    pub modified: bool,
    /// Holds a credential: shown as hidden and not editable here.
    pub redacted: bool,
    /// Takes effect when the engine restarts rather than on apply.
    pub restart: bool,
}

/// How a setting is edited.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Control {
    Toggle,
    /// One of fixed options.
    Select {
        options: Vec<Choice>,
    },
    /// A number; `auto` adds an "Automatic" choice whose value is the string `auto`.
    Number {
        min: Option<f64>,
        max: Option<f64>,
        step: Option<f64>,
        integer: bool,
        auto: bool,
        /// What the number counts (`s`, `bytes`, `tokens`, `%`…), when known.
        unit: Option<String>,
    },
    /// Free text, optionally with suggestions; `hint` says what the text names, so the page can
    /// offer a picker (a model, a file) instead of a bare text box.
    Text {
        suggestions: Vec<Choice>,
        hint: TextHint,
    },
    /// A list of strings.
    List,
    /// A table edited as JSON.
    Table,
}

/// What a text setting holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TextHint {
    Plain,
    /// A model id from the engine's model list.
    Model,
    /// A provider name from the engine's model list.
    Provider,
    /// An executable file.
    Program,
    /// Any file.
    File,
    /// A folder.
    Folder,
    /// A web address.
    Url,
    /// The name of an environment variable (never its value).
    EnvVar,
    /// An audio input device by name.
    Microphone,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Choice {
    pub value: String,
    pub label: String,
    /// Picker identity when several providers expose the same saved value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Other settings saved with this choice: an image model's endpoint and protocol.
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub with: Map<String, Value>,
}

/// The terminal's built-in color themes (`dial-ui` `BUILTINS`); a custom name stays possible.
const TERMINAL_THEMES: &[&str] = &[
    "dial",
    "dial-dark",
    "dark",
    "light",
    "mono",
    "gruvbox",
    "nord",
    "paper",
    "dark-one",
    "light-one",
    "dark-github",
    "dark-nord",
    "dark-ocean",
    "dark-forest",
    "obsidian",
    "graphite",
    "porcelain",
];

/// Image models the engine documents (`dial-llm` catalog `documented_image_generation` and the
/// image config); any other id the endpoint accepts can still be typed.
const IMAGE_MODELS: &[(&str, &str)] = &[
    ("gpt-image-1.5", "GPT Image 1.5 · OpenAI"),
    ("gpt-image-2", "GPT Image 2 · OpenAI"),
    ("gpt-image-1", "GPT Image 1 · OpenAI"),
    ("gpt-image-1-mini", "GPT Image 1 mini · OpenAI"),
    ("dall-e-3", "DALL·E 3 · OpenAI"),
    ("grok-imagine-image-2.0", "Grok Imagine 2.0 · xAI"),
    ("gemini-3-pro-image-preview", "Gemini 3 Pro Image · Google"),
    (
        "gemini-3.1-flash-image-preview",
        "Gemini 3.1 Flash Image · Google",
    ),
    ("gemini-2.5-flash-image", "Gemini 2.5 Flash Image · Google"),
    ("imagen-4.0-generate-001", "Imagen 4 · Google"),
    ("flux-2-pro", "FLUX.2 Pro · Black Forest Labs"),
    ("flux-pro-1.1", "FLUX 1.1 Pro · Black Forest Labs"),
    ("flux-kontext-pro", "FLUX Kontext Pro · Black Forest Labs"),
    ("flux-schnell", "FLUX Schnell"),
];

/// Web search backends `web.search_provider` names in its description.
const SEARCH_PROVIDERS: &[(&str, &str)] = &[
    ("auto", "Automatic"),
    ("brave", "Brave"),
    ("tavily", "Tavily"),
    ("exa", "Exa"),
    ("searxng", "SearXNG"),
    ("duckduckgo", "DuckDuckGo"),
];

/// Build the page from the reference and the saved values.
pub fn view(schema: &[Value], saved: &HashMap<String, SavedSetting>) -> SettingsView {
    let mut tabs: Vec<String> = Vec::new();
    let mut settings = Vec::new();
    for entry in schema {
        let Some(key) = entry["path"].as_str().filter(|k| !k.is_empty()) else {
            continue;
        };
        let text = |name: &str| entry[name].as_str().unwrap_or_default().to_owned();
        let tab = Some(text("tab"))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "Advanced".to_owned());
        if !tabs.contains(&tab) {
            tabs.push(tab.clone());
        }
        let control = control(entry);
        let default = entry["default"].clone();
        let (value, redacted) = match &control {
            Control::Table => table_value(key, saved),
            _ => match saved.get(key) {
                Some(s) => (s.value.clone(), s.redacted),
                None => (Value::Null, false),
            },
        };
        let value = if value.is_null() {
            default.clone()
        } else {
            value
        };
        // A number's label may name its unit, which the control shows instead.
        let (label, label_unit) = if matches!(control, Control::Number { .. }) {
            split_unit(&text("label"))
        } else {
            (text("label"), None)
        };
        settings.push(SettingView {
            key: key.to_owned(),
            label: Some(label)
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| key.to_owned()),
            description: text("description"),
            section: text("section"),
            modified: !redacted && !same(&value, &default),
            tab,
            control: with_unit(control, label_unit.or_else(|| key_unit(key))),
            value,
            default,
            redacted,
            restart: entry["restart"].as_bool().unwrap_or_default(),
        });
    }
    // "Common" leads; the rest keep the reference's order.
    if let Some(i) = tabs.iter().position(|t| t == "Common") {
        let common = tabs.remove(i);
        tabs.insert(0, common);
    }
    SettingsView { tabs, settings }
}

/// The control for one reference entry, from its kind and constraints.
fn control(entry: &Value) -> Control {
    let kind = entry["kind"].as_str().unwrap_or_default();
    let key = entry["path"].as_str().unwrap_or_default();
    let c = &entry["constraints"];
    let name = kind
        .split(|ch: char| !ch.is_alphanumeric())
        .next()
        .unwrap_or_default();
    match name {
        "Bool" => Control::Toggle,
        "AutoBool" => Control::Select {
            options: vec![
                choice("auto", "Automatic"),
                choice("on", "On"),
                choice("off", "Off"),
            ],
        },
        "Enum" => Control::Select {
            options: choices(&c["labels"], &c["enum"]),
        },
        "Int" | "Float" | "AutoInt" | "AutoFloat" => {
            // `Auto*` kinds wrap the numeric bounds in `anyOf` beside `{const: "auto"}`.
            let bounds = c["anyOf"]
                .as_array()
                .and_then(|any| {
                    any.iter()
                        .find(|b| b.get("minimum").is_some() || b.get("maximum").is_some())
                })
                .unwrap_or(c);
            Control::Number {
                min: bounds["minimum"].as_f64(),
                max: bounds["maximum"].as_f64(),
                step: c["step"].as_f64().or_else(|| bounds["step"].as_f64()),
                integer: name.ends_with("Int"),
                auto: name.starts_with("Auto"),
                unit: None,
            }
        }
        "StringList" => Control::List,
        "Toml" | "Table" => Control::Table,
        // Fixed lists are dropdowns; a value set elsewhere that is not listed still shows.
        "Theme" => {
            let auto = (key == "ui.theme").then(|| choice("auto", "Match the terminal"));
            Control::Select {
                options: auto
                    .into_iter()
                    .chain(TERMINAL_THEMES.iter().map(|t| choice(t, t)))
                    .collect(),
            }
        }
        _ if key == "web.search_provider" => Control::Select {
            options: SEARCH_PROVIDERS.iter().map(|(v, l)| choice(v, l)).collect(),
        },
        // Image models are not chat models: offer the ones the engine documents, by provider.
        _ if key == "image.model" => Control::Text {
            suggestions: IMAGE_MODELS.iter().map(|(v, l)| choice(v, l)).collect(),
            hint: TextHint::Plain,
        },
        _ => Control::Text {
            suggestions: choices(&c["suggestions"], &Value::Null),
            hint: text_hint(key),
        },
    }
}

/// What a free-text setting names, from its key.
fn text_hint(key: &str) -> TextHint {
    let leaf = key.rsplit('.').next().unwrap_or(key);
    match leaf {
        "model" | "draft_model" => TextHint::Model,
        "provider" => TextHint::Provider,
        "python" | "node" | "worker" | "program" => TextHint::Program,
        "idadir" => TextHint::Folder,
        "progress_file" => TextHint::File,
        "input_device" => TextHint::Microphone,
        _ if leaf.ends_with("_env") => TextHint::EnvVar,
        _ if leaf.ends_with("endpoint") || leaf.ends_with("_url") => TextHint::Url,
        _ => TextHint::Plain,
    }
}

/// `("Worker timeout", Some("s"))` from `"Worker timeout (s)"`.
fn split_unit(label: &str) -> (String, Option<String>) {
    if let Some(open) = label.rfind(" (")
        && label.ends_with(')')
    {
        let unit = &label[open + 2..label.len() - 1];
        let unit = match unit {
            "seconds" => "s",
            "minutes" => "min",
            other => other,
        };
        if !unit.is_empty() && unit.len() <= 8 && !unit.contains(' ') {
            return (label[..open].to_owned(), Some(unit.to_owned()));
        }
    }
    (label.to_owned(), None)
}

/// The unit a numeric key's suffix names (`retry_delay_secs` → `s`).
fn key_unit(key: &str) -> Option<String> {
    let suffix = key.rsplit(['.', '_']).next()?;
    let unit = match suffix {
        "secs" | "seconds" => "s",
        "ms" => "ms",
        "minutes" | "mins" => "min",
        "hours" => "hours",
        "days" => "days",
        "bytes" => "bytes",
        "tokens" => "tokens",
        "percent" | "pct" => "%",
        "chars" => "chars",
        "lines" => "lines",
        "turns" => "turns",
        "usd" => "USD",
        _ => return None,
    };
    Some(unit.to_owned())
}

fn with_unit(control: Control, found: Option<String>) -> Control {
    match control {
        Control::Number {
            min,
            max,
            step,
            integer,
            auto,
            ..
        } => Control::Number {
            min,
            max,
            step,
            integer,
            auto,
            unit: found,
        },
        other => other,
    }
}

/// Options from `[[value, label], ...]` pairs, or from plain values.
fn choices(labelled: &Value, plain: &Value) -> Vec<Choice> {
    if let Some(pairs) = labelled.as_array().filter(|p| !p.is_empty()) {
        return pairs
            .iter()
            .filter_map(|pair| {
                let value = pair[0].as_str()?;
                Some(choice(value, pair[1].as_str().unwrap_or(value)))
            })
            .collect();
    }
    plain
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|v| choice(v, v))
        .collect()
}

fn choice(value: &str, label: &str) -> Choice {
    Choice {
        value: value.to_owned(),
        label: label.to_owned(),
        id: None,
        with: Map::new(),
    }
}

/// Offer the engine's image models (`list_models` with `purpose: "image"`) for `image.model`
/// instead of the built-in list. Each choice also saves the endpoint and image protocol the model
/// needs, so picking a Gemini or OpenRouter model does not leave the OpenAI endpoint in place.
/// No rows (an engine without the listing) keeps the built-in list.
pub fn offer_image_models(view: &mut SettingsView, rows: &[Value]) {
    let options: Vec<Choice> = rows
        .iter()
        .filter_map(|row| {
            let model = row["model_id"].as_str().filter(|m| !m.is_empty())?;
            let label = row["label"]
                .as_str()
                .filter(|l| !l.is_empty())
                .unwrap_or(model);
            let provider = row["provider"].as_str().unwrap_or_default();
            let mut with = Map::new();
            if let Some(url) = row["base_url"].as_str().filter(|u| !u.is_empty()) {
                with.insert("image.base_url".into(), Value::from(url));
            }
            if let Some(api) = row["image_api"].as_str().filter(|a| !a.is_empty()) {
                with.insert("image.api".into(), Value::from(api));
            }
            Some(Choice {
                value: model.to_owned(),
                id: Some(format!("\0{}", serde_json::json!([provider, model, &with]))),
                label: format!("{label} · {provider}"),
                with,
            })
        })
        .collect();
    if options.is_empty() {
        return;
    }
    if let Some(setting) = view.settings.iter_mut().find(|s| s.key == "image.model") {
        setting.control = Control::Text { suggestions: options, hint: TextHint::Plain };
    }
}

/// A table setting's value, folded from its saved leaf keys; redacted when any leaf is.
fn table_value(key: &str, saved: &HashMap<String, SavedSetting>) -> (Value, bool) {
    if let Some(whole) = saved.get(key) {
        return (whole.value.clone(), whole.redacted);
    }
    let prefix = format!("{key}.");
    let mut leaves: Vec<(&String, &SavedSetting)> = saved
        .iter()
        .filter(|(k, _)| k.starts_with(&prefix))
        .collect();
    if leaves.is_empty() {
        return (Value::Null, false);
    }
    leaves.sort_by(|a, b| a.0.cmp(b.0));
    let mut table = Map::new();
    let mut redacted = false;
    for (leaf, setting) in leaves {
        redacted |= setting.redacted;
        insert(&mut table, &leaf[prefix.len()..], setting.value.clone());
    }
    (Value::Object(table), redacted)
}

/// Put `value` at dotted `path` inside `table`, creating intermediate tables.
fn insert(table: &mut Map<String, Value>, path: &str, value: Value) {
    match path.split_once('.') {
        None => {
            table.insert(path.to_owned(), value);
        }
        Some((head, rest)) => {
            let child = table
                .entry(head.to_owned())
                .or_insert_with(|| Value::Object(Map::new()));
            if !child.is_object() {
                *child = Value::Object(Map::new());
            }
            if let Value::Object(child) = child {
                insert(child, rest, value);
            }
        }
    }
}

/// Equal values, treating `1` and `1.0` alike (TOML keeps integers and floats apart).
fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}
