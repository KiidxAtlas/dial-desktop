use super::super::Controller;
use super::Ledger;

fn agent_with_token(ledger: &mut Ledger, token: u128) -> u128 {
    ledger.resume();
    ledger
        .issue_token(token)
        .expect("the agent holds control, so it gets a token")
}

#[test]
fn an_agent_without_control_gets_no_token_and_cannot_act() {
    let mut ledger = Ledger::default();
    assert_eq!(ledger.controller(), Controller::User);
    assert_eq!(ledger.issue_token(7), None);
    let error = ledger.spend_agent_token(0, 7).unwrap_err();
    assert_eq!(error.code, "not_controller");
}

#[test]
fn a_token_is_single_use() {
    let mut ledger = Ledger::default();
    let token = agent_with_token(&mut ledger, 42);
    ledger.spend_agent_token(ledger.seq(), token).unwrap();
    let again = ledger.spend_agent_token(ledger.seq(), token).unwrap_err();
    assert_eq!(again.code, "stale_token");
}

#[test]
fn a_wrong_token_is_refused_and_does_not_spend_the_right_one() {
    let mut ledger = Ledger::default();
    let token = agent_with_token(&mut ledger, 42);
    assert_eq!(
        ledger.spend_agent_token(0, 41).unwrap_err().code,
        "stale_token"
    );
    ledger.spend_agent_token(0, token).unwrap();
}

#[test]
fn a_redraw_of_the_same_size_keeps_the_token_for_the_frame_it_was_issued_on() {
    // A blinking cursor records frames faster than the agent can answer: the input still lands.
    let mut ledger = Ledger::default();
    let token = agent_with_token(&mut ledger, 42);
    let seen = ledger.seq();
    ledger.advance_frame();
    ledger.advance_frame();
    ledger.spend_agent_token(seen, token).unwrap();
}

#[test]
fn a_redraw_does_not_let_the_token_name_a_frame_it_was_not_issued_on() {
    let mut ledger = Ledger::default();
    let token = agent_with_token(&mut ledger, 42);
    let newer = ledger.advance_frame();
    assert_eq!(
        ledger.spend_agent_token(newer, token).unwrap_err().code,
        "stale_frame"
    );
}

#[test]
fn a_resize_invalidates_the_token() {
    let mut ledger = Ledger::default();
    let token = agent_with_token(&mut ledger, 42);
    ledger.advance_frame();
    ledger.screen_resized();
    assert_eq!(
        ledger.spend_agent_token(0, token).unwrap_err().code,
        "stale_token"
    );
}

#[test]
fn a_newer_agent_frame_replaces_the_token() {
    let mut ledger = Ledger::default();
    let old = agent_with_token(&mut ledger, 42);
    ledger.advance_frame();
    let new = ledger.issue_token(43).unwrap();
    assert_eq!(
        ledger.spend_agent_token(0, old).unwrap_err().code,
        "stale_token"
    );
    ledger.spend_agent_token(ledger.seq(), new).unwrap();
}

#[test]
fn an_old_token_expires() {
    let mut ledger = Ledger::default();
    let token = agent_with_token(&mut ledger, 42);
    ledger.age_token(super::TOKEN_TTL + std::time::Duration::from_secs(1));
    assert_eq!(
        ledger.spend_agent_token(0, token).unwrap_err().code,
        "stale_token"
    );
    // An expired token is gone, not merely refused once.
    ledger.age_token(std::time::Duration::ZERO);
    assert_eq!(
        ledger.spend_agent_token(0, token).unwrap_err().code,
        "stale_token"
    );
}

#[test]
fn an_agent_token_for_an_old_frame_number_is_stale_frame() {
    let mut ledger = Ledger::default();
    agent_with_token(&mut ledger, 42);
    ledger.advance_frame();
    ledger.issue_token(43).unwrap();
    assert_eq!(
        ledger.spend_agent_token(0, 43).unwrap_err().code,
        "stale_frame"
    );
}

#[test]
fn a_human_takeover_invalidates_the_token_and_blocks_the_agent_until_resume() {
    let mut ledger = Ledger::default();
    let token = agent_with_token(&mut ledger, 42);
    ledger.take_over();
    assert_eq!(
        ledger.spend_agent_token(0, token).unwrap_err().code,
        "taken_over"
    );
    assert_eq!(
        ledger.issue_token(44),
        None,
        "no token while the human has control"
    );

    ledger.resume();
    assert_eq!(
        ledger.spend_agent_token(0, token).unwrap_err().code,
        "stale_token"
    );
    let fresh = ledger
        .issue_token(45)
        .expect("resume gives the agent a fresh token");
    ledger.spend_agent_token(0, fresh).unwrap();
}

#[test]
fn ui_input_may_name_any_shown_frame_but_not_a_future_one() {
    let mut ledger = Ledger::default();
    ledger.check_ui_frame(0).unwrap();
    ledger.advance_frame();
    // The screen moved on while the key was in flight: the keystroke still lands.
    ledger.check_ui_frame(0).unwrap();
    ledger.check_ui_frame(1).unwrap();
    assert_eq!(ledger.check_ui_frame(2).unwrap_err().code, "stale_frame");
}

#[test]
fn an_agent_launch_starts_with_the_agent_in_control() {
    let mut ledger = Ledger::agent_launch();
    assert_eq!(ledger.controller(), Controller::Agent);
    assert!(!ledger.taken_over());
    let token = ledger
        .issue_token(9)
        .expect("the agent launched the run, so it holds control");
    ledger.spend_agent_token(0, token).unwrap();
}

#[test]
fn a_takeover_of_an_agent_launch_is_undone_only_by_resume() {
    let mut ledger = Ledger::agent_launch();
    ledger.take_over();
    assert!(ledger.taken_over());
    assert_eq!(ledger.issue_token(10), None);
    ledger.resume();
    assert!(!ledger.taken_over());
}

#[test]
fn an_agent_batch_continues_only_until_the_user_acts_or_turns_access_off() {
    let mut ledger = Ledger::default();
    ledger.resume();
    let epoch = ledger.user_epoch();
    assert!(ledger.agent_may_continue(epoch));
    // A redraw does not interrupt the batch.
    ledger.advance_frame();
    assert!(ledger.agent_may_continue(epoch));
    // The user's input does, even though access stays on.
    ledger.user_acted();
    assert!(!ledger.agent_may_continue(epoch));
    let epoch = ledger.user_epoch();
    assert!(ledger.agent_may_continue(epoch));
    // Access off does too, and stays off for that epoch after access returns.
    ledger.take_over();
    assert!(!ledger.agent_may_continue(epoch));
    ledger.resume();
    assert!(!ledger.agent_may_continue(epoch));
}
