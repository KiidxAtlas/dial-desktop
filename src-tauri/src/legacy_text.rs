//! The one place the desktop reads legacy Type framing back out of stored user text.
//!
//! Older engines and Dial Desktop builds stored a Type conversation's prompt with its `[[type:ID]]`
//! line and `<type-context>` briefing in the text. The engine marks every user message it admits
//! from a person with `dial_structured_text`, so only unmarked messages are read here; a literal
//! `[[type:…]]` the user typed in a structured message is shown exactly as typed. A prefix counts as
//! framing only when it is complete and well formed, matching the engine's own split.

use serde_json::{Value, json};

const STRUCTURED_TEXT_KEY: &str = "dial_structured_text";
const LEGACY_OPEN: &str = "[[type:";
const CONTEXT_OPEN: &str = "<type-context>";
const CONTEXT_CLOSE: &str = "</type-context>";

/// Show a stored user message's own words: legacy framing comes off its text. Other messages, and
/// messages the engine marked as structured, are left exactly as stored.
pub fn project_user_message(message: &mut Value) {
    if message["role"] != "user" || message[STRUCTURED_TEXT_KEY] == true {
        return;
    }
    match &mut message["content"] {
        Value::String(text) => {
            if let Some(body) = strip(text) {
                *text = body;
            }
        }
        Value::Array(parts) => {
            if let Some(part) = parts.iter_mut().find(|part| part["type"] == "text") {
                if let Some(body) = part["text"].as_str().and_then(strip) {
                    part["text"] = json!(body);
                }
            }
        }
        _ => {}
    }
}

/// The user's words when `text` begins with a complete legacy framing; `None` otherwise. The tag is
/// a line of its own, and a `<type-context>` block must be complete lines, as the engine reads it.
fn strip(text: &str) -> Option<String> {
    let (id, rest) = text.strip_prefix(LEGACY_OPEN)?.split_once("]]")?;
    if !valid_id(id) {
        return None;
    }
    let rest = rest.strip_prefix('\n')?;
    if rest.starts_with(CONTEXT_OPEN) {
        let end = rest.find(CONTEXT_CLOSE)? + CONTEXT_CLOSE.len();
        return Some(rest[end..].strip_prefix('\n')?.to_owned());
    }
    Some(rest.to_owned())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
mod tests;
