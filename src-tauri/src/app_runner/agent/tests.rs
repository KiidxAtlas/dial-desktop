use serde_json::json;

use super::super::Controller;
use super::super::actions::{Action, Button};
use super::super::runtime::{GuestTarget, preferred_guest_target};
use super::{FrameReply, format_token, guest_target_of, parse_input, parse_token, run_id_of};

fn valid_args() -> serde_json::Value {
    json!({
        "runId": "run-7",
        "frameSeq": 3,
        "token": "0123456789abcdef0123456789abcdef",
        "event": { "kind": "click", "x": 4, "y": 5, "button": "left" }
    })
}

#[test]
fn a_complete_input_call_parses_to_its_exact_run_frame_token_and_event() {
    let request = parse_input(&valid_args()).unwrap();
    assert_eq!(request.run_id, "run-7");
    assert_eq!(request.frame_seq, 3);
    assert_eq!(
        format_token(request.token),
        "0123456789abcdef0123456789abcdef"
    );
    assert_eq!(
        request.actions,
        vec![Action::Click { x: 4, y: 5, button: Button::Left, count: 1 }]
    );
}

#[test]
fn a_batch_of_actions_parses_in_order() {
    let mut args = valid_args();
    let object = args.as_object_mut().unwrap();
    object.remove("event");
    object.insert(
        "actions".into(),
        json!([
            {"kind": "click", "x": 1, "y": 2},
            {"kind": "type", "text": "root\n"},
            {"kind": "press", "keys": "ctrl+c"},
            {"kind": "wait", "ms": 200}
        ]),
    );
    let request = parse_input(&args).unwrap();
    assert_eq!(request.actions.len(), 4);
    assert_eq!(request.actions[1], Action::Type { text: "root\n".into() });
    // Both forms at once are refused rather than guessed.
    args["event"] = json!({ "kind": "click", "x": 4, "y": 5, "button": "left" });
    assert!(parse_input(&args).is_err());
}

#[test]
fn each_required_field_is_named_when_missing() {
    let cases = [
        ("runId", "runId"),
        ("frameSeq", "frameSeq"),
        ("token", "token"),
        ("event", "event"),
    ];
    for (field, expected) in cases {
        let mut args = valid_args();
        args.as_object_mut().unwrap().remove(field);
        let error = parse_input(&args).unwrap_err();
        assert!(error.contains(expected), "{field}: {error}");
    }
}

#[test]
fn the_old_wire_names_are_not_accepted() {
    // The previous names (`seq`, `gesture`) must not silently work: the event is required under
    // its public name.
    let mut args = valid_args();
    let object = args.as_object_mut().unwrap();
    object.remove("event");
    object.insert(
        "gesture".into(),
        json!({ "kind": "click", "x": 1, "y": 1, "button": "left" }),
    );
    assert!(parse_input(&args).is_err());
    let mut args = valid_args();
    let object = args.as_object_mut().unwrap();
    object.remove("frameSeq");
    object.insert("seq".into(), json!(3));
    assert!(parse_input(&args).is_err());
}

#[test]
fn a_token_must_be_a_string_of_32_lowercase_hex_digits() {
    let bad = [
        json!(12345),
        json!("0123456789ABCDEF0123456789ABCDEF"),
        json!("0123"),
        json!("zz23456789abcdef0123456789abcdef"),
        json!(""),
    ];
    for token in bad {
        let mut args = valid_args();
        args["token"] = token.clone();
        assert!(parse_input(&args).is_err(), "{token} must be refused");
    }
}

#[test]
fn a_token_round_trips_through_its_string_form() {
    let token = 0x00ff_u128 << 64 | 0x1234;
    let text = format_token(token);
    assert_eq!(text.len(), 32);
    assert_eq!(parse_token(&text), Some(token));
}

#[test]
fn a_run_id_is_required_and_never_empty() {
    assert!(run_id_of(&json!({})).is_err());
    assert!(run_id_of(&json!({ "runId": "" })).is_err());
    assert_eq!(run_id_of(&json!({ "runId": "run-1" })).unwrap(), "run-1");
}

#[test]
fn a_frame_reply_carries_run_frame_and_a_string_token() {
    let reply = FrameReply {
        run_id: "run-7".into(),
        frame_seq: 9,
        token: Some(format_token(42)),
        controller: Controller::Agent,
        width: 800,
        height: 600,
    };
    let value: serde_json::Value = serde_json::from_str(&reply.to_text()).unwrap();
    assert_eq!(value["runId"], "run-7");
    assert_eq!(value["frameSeq"], 9);
    assert!(
        value["token"].is_string(),
        "the token is a string, never a number"
    );
    assert_eq!(value["controller"], "agent");
}

#[test]
fn a_frame_reply_without_control_has_a_null_token() {
    let reply = FrameReply {
        run_id: "run-7".into(),
        frame_seq: 1,
        token: None,
        controller: Controller::User,
        width: 1,
        height: 1,
    };
    let value: serde_json::Value = serde_json::from_str(&reply.to_text()).unwrap();
    assert!(value["token"].is_null());
}

#[test]
fn guest_target_is_optional_and_only_known_wire_names_are_accepted() {
    let table = [
        (json!({}), Ok(preferred_guest_target())),
        (json!({"guestTarget": null}), Ok(preferred_guest_target())),
        (json!({"guestTarget": "default"}), Ok(GuestTarget::Default)),
        (json!({"guestTarget": "linux"}), Ok(GuestTarget::Linux)),
        (json!({"guestTarget": "windows"}), Ok(GuestTarget::Windows)),
        (json!({"guestTarget": "macos"}), Ok(GuestTarget::Macos)),
        (json!({"guestTarget": "vm"}), Ok(GuestTarget::Vm)),
        (json!({"guestTarget": "Linux"}), Err(())),
        (json!({"guestTarget": 1}), Err(())),
        (json!({"guestTarget": "arch"}), Err(())),
    ];
    for (args, expected) in table {
        assert_eq!(
            guest_target_of(&args).map_err(|_| ()),
            expected,
            "arguments {args}"
        );
    }
}
