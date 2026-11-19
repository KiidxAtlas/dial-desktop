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
    },
    /// Free text, optionally with suggestions.
    Text {
        suggestions: Vec<Choice>,
    },
    /// A list of strings.
    List,
    /// A table edited as JSON.
    Table,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

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
        settings.push(SettingView {
            key: key.to_owned(),
            label: Some(text("label"))
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| key.to_owned()),
            description: text("description"),
            section: text("section"),
            modified: !redacted && !same(&value, &default),
            tab,
            control,
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
            }
        }
        "StringList" => Control::List,
        "Toml" | "Table" => Control::Table,
        _ => Control::Text {
            suggestions: choices(&c["suggestions"], &Value::Null),
        },
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
