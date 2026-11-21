//! The agent's wire format for the `app_desktop` tool. The names here are the public contract:
//!
//! - `state` `{ guestTarget? }` → the run state for the workspace and guest target (`status`,
//!   `guestTarget`, `run`, …).
//! - `start` `{ guestTarget?, vmId? }` → the new run's info (`runId`, `status`, `controller`,
//!   `origin`, `guestTarget`). A registered `vm` requires the exact one-use allowance shown in
//!   `state`; other targets open an unshared private guest. An open project run blocks start.
//!
//! `guestTarget` is one of `default`, `linux`, `windows`, `macos`, `vm`. Absent means the host's preferred
//! target. Explicit `default` always means the unlabelled disk. The target selects which guest
//! boots; it does not verify the guest's operating system.
//! - `frame` `{ runId }` → `{ runId, frameSeq, token, controller, width, height }` as the reply text,
//!   with the screen as the image. `token` is the string to send with the next input, or null when
//!   the agent does not hold control.
//! - `input` `{ runId, frameSeq, token, actions }` → runs the batch `actions` (see
//!   [`super::actions::Action`]) in order on the frame `frameSeq`, then replies like `frame` with
//!   the screen after it. `token` is a string, spent by this call. The older single `event` (a
//!   click or a press) is still accepted as a batch of one.
//!
//! The token is a 32-digit lowercase hex string, never a number, so it survives every JSON client.

use serde::Serialize;
use serde_json::{Value, json};

use super::Controller;
use super::actions::Action;
use super::input::Gesture;
use super::runtime::{GuestTarget, preferred_guest_target};

/// A parsed `input` call: the batch to run on frame `frame_seq`.
#[derive(Debug)]
pub struct InputRequest {
    pub run_id: String,
    pub frame_seq: u64,
    pub token: u128,
    pub actions: Vec<Action>,
}

/// The `runId` of a `frame` or `input` call. Required and exact: the runner never substitutes
/// another run.
pub fn run_id_of(args: &Value) -> Result<String, String> {
    args.get("runId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "this action needs runId, the run id from state or frame".to_owned())
}

/// Parse an `input` call's arguments.
pub fn parse_input(args: &Value) -> Result<InputRequest, String> {
    let run_id = run_id_of(args)?;
    let frame_seq = args
        .get("frameSeq")
        .and_then(Value::as_u64)
        .ok_or("input needs frameSeq, the frame the event was chosen on")?;
    let token = args
        .get("token")
        .and_then(Value::as_str)
        .and_then(parse_token)
        .ok_or("input needs token, the string from the latest frame")?;
    let actions = match (args.get("actions"), args.get("event")) {
        (Some(list), None) => serde_json::from_value::<Vec<Action>>(list.clone())
            .map_err(|e| format!("the actions are not valid: {e}"))?,
        (None, Some(event)) => vec![legacy_event(event)?],
        (Some(_), Some(_)) => return Err("send actions or event, not both".into()),
        (None, None) => return Err("input needs actions (a list), or a single event".into()),
    };
    Ok(InputRequest {
        run_id,
        frame_seq,
        token,
        actions,
    })
}

/// The older single `event` (a click or a press) as one action.
fn legacy_event(event: &Value) -> Result<Action, String> {
    let gesture = serde_json::from_value::<Gesture>(event.clone())
        .map_err(|_| "the event is not a valid gesture".to_owned())?;
    // Live input (held keys and buttons, moves) is the user's; the agent sends whole gestures.
    match gesture {
        Gesture::Click { x, y, button } => {
            let button = serde_json::from_value(json!(button))
                .map_err(|_| "the mouse button must be left, middle or right".to_owned())?;
            Ok(Action::Click { x, y, button, count: 1 })
        }
        Gesture::Press { code, modifiers } => Ok(Action::Press {
            keys: None,
            code: Some(code),
            modifiers,
            repeat: 1,
        }),
        _ => Err("the event must be a click or a press".into()),
    }
}

/// The optional `guestTarget` of `state` and `start`; absent or null is the host's preferred guest.
pub fn guest_target_of(args: &Value) -> Result<GuestTarget, String> {
    match args.get("guestTarget") {
        None | Some(Value::Null) => Ok(preferred_guest_target()),
        Some(value) => serde_json::from_value(value.clone())
            .map_err(|_| "guestTarget must be default, linux, windows, macos or vm".to_owned()),
    }
}

/// A token as the wire carries it: 32 lowercase hex digits.
pub fn format_token(token: u128) -> String {
    format!("{token:032x}")
}

/// The string the wire carries back as a token; anything else is refused.
pub fn parse_token(text: &str) -> Option<u128> {
    if text.len() != 32
        || !text
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return None;
    }
    u128::from_str_radix(text, 16).ok()
}

/// The reply text of a `frame` call.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameReply {
    pub run_id: String,
    pub frame_seq: u64,
    pub token: Option<String>,
    pub controller: Controller,
    pub width: u32,
    pub height: u32,
}

impl FrameReply {
    pub fn to_text(&self) -> String {
        json!(self).to_string()
    }
}

#[cfg(test)]
mod tests;
