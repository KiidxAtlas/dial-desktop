//! Voice mode is driven through the real [`Model`], the way engine updates reach it.
use super::*;
use crate::engine::Update;
use crate::model::Model;
use dial_client::{Entry, Event, Message, Ready, RpcError};
use serde_json::json;

fn connected() -> Model {
    let mut model = Model::starting();
    model.apply(Update::Connected(Box::new(Ready {
        session_id: "s1".into(),
        engine_version: Some("0.3.11".into()),
        model: "m".into(),
        supported_protocol_versions: vec![1, 2],
        methods: vec![],
        capabilities: vec![],
        raw: json!({"event": "ready", "session_id": "s1", "seq": 1}),
    })));
    model
}

fn id(n: u64) -> RequestId {
    Event {
        kind: "x".into(),
        body: json!({"request_id": n}),
    }
    .request_id()
    .unwrap()
}

fn requested(model: &mut Model, command: Command, n: u64) -> RequestId {
    let id = id(n);
    model.apply(Update::Requested { command, id });
    id
}

fn event(model: &mut Model, body: Value) -> Vec<Command> {
    model.apply(Update::Message(Message::Event(Event {
        kind: body["event"].as_str().unwrap().to_owned(),
        body,
    })))
}

fn respond(
    model: &mut Model,
    id: RequestId,
    method: &str,
    result: Result<Value, RpcError>,
) -> Vec<Command> {
    model.apply(Update::Message(Message::Response {
        id,
        method: method.into(),
        result,
    }))
}

fn failure(code: Option<&str>, message: &str) -> Result<Value, RpcError> {
    Err(RpcError {
        code: code.map(str::to_owned),
        message: message.into(),
    })
}

/// Voice mode started and listening on request `n`.
fn listening(model: &mut Model, n: u64) -> RequestId {
    let start = model.start_voice().unwrap().unwrap();
    assert_eq!(
        start,
        Command::LiveStart {
            instructions: Some(SPOKEN_STYLE.into())
        }
    );
    let started = requested(model, start, n);
    assert_eq!(model.voice.phase, Phase::Starting);
    let follow = respond(
        model,
        started,
        "live_start",
        Ok(json!({"live": true, "muted": false})),
    );
    assert_eq!(follow, vec![Command::LiveListen]);
    let listen = requested(model, Command::LiveListen, n + 1);
    assert_eq!(model.voice.phase, Phase::Listening);
    listen
}

#[test]
fn a_spoken_turn_is_shown_answered_read_aloud_then_listened_for_again() {
    let mut model = connected();
    let listen = listening(&mut model, 2);
    event(
        &mut model,
        json!({"event": "live_transcript", "text": "what time is it", "request_id": 3}),
    );
    assert_eq!(model.voice.phase, Phase::Thinking);
    assert_eq!(model.voice.heard.as_deref(), Some("what time is it"));
    assert!(model.conversation.turn_running());
    assert!(
        matches!(model.conversation.entries(), [Entry::User(u)] if u.text == "what time is it")
    );
    // The engine starts reading before it answers the request.
    event(
        &mut model,
        json!({"event": "live_speech", "speaking": true}),
    );
    assert_eq!(model.voice.phase, Phase::Speaking);
    let follow = respond(
        &mut model,
        listen,
        "live_listen",
        Ok(json!({"text": "Noon.", "outcome": "completed"})),
    );
    assert!(!model.conversation.turn_running());
    assert!(
        !follow.contains(&Command::LiveListen),
        "still reading: {follow:?}"
    );
    assert_eq!(model.voice.phase, Phase::Speaking);
    // Listening again only once the read-out ends, so the reply is not recorded as speech.
    assert_eq!(
        event(
            &mut model,
            json!({"event": "live_speech", "speaking": false})
        ),
        vec![Command::LiveListen]
    );
}

#[test]
fn a_reply_that_finished_reading_before_the_answer_listens_when_the_answer_arrives() {
    let mut model = connected();
    let listen = listening(&mut model, 2);
    event(
        &mut model,
        json!({"event": "live_transcript", "text": "hi"}),
    );
    event(
        &mut model,
        json!({"event": "live_speech", "speaking": true}),
    );
    assert!(
        event(
            &mut model,
            json!({"event": "live_speech", "speaking": false})
        )
        .is_empty()
    );
    let follow = respond(
        &mut model,
        listen,
        "live_listen",
        Ok(json!({"text": "Hello"})),
    );
    assert!(follow.contains(&Command::LiveListen));
}

#[test]
fn hearing_the_same_words_twice_in_a_row_stops_that_turn_and_pauses() {
    let mut model = connected();
    let listen = listening(&mut model, 2);
    event(
        &mut model,
        json!({"event": "live_transcript", "text": "Hello, hello."}),
    );
    event(
        &mut model,
        json!({"event": "live_speech", "speaking": true}),
    );
    respond(
        &mut model,
        listen,
        "live_listen",
        Ok(json!({"text": "Hi!"})),
    );
    let again = event(
        &mut model,
        json!({"event": "live_speech", "speaking": false}),
    );
    assert_eq!(again, vec![Command::LiveListen]);
    let listen = requested(&mut model, Command::LiveListen, 4);
    // Noise recognized as the same phrase again: that turn is stopped, not answered.
    let follow = event(
        &mut model,
        json!({"event": "live_transcript", "text": "hello hello"}),
    );
    assert_eq!(follow, vec![Command::Abort]);
    assert_eq!(model.voice.phase, Phase::Paused);
    let follow = respond(
        &mut model,
        listen,
        "live_listen",
        failure(Some("aborted"), "turn aborted"),
    );
    assert!(
        !follow.contains(&Command::LiveListen),
        "no new listen: {follow:?}"
    );
    assert_eq!(model.voice.phase, Phase::Paused);
    assert!(
        model
            .voice
            .problem
            .as_deref()
            .unwrap()
            .starts_with("Heard the same thing twice")
    );
    // Different words go on as usual.
    assert_eq!(model.voice.tap(false), Some(Command::LiveListen));
}

#[test]
fn muted_replies_listen_again_as_soon_as_the_turn_ends() {
    let mut model = connected();
    let listen = listening(&mut model, 2);
    assert_eq!(model.voice.mute(true), Some(Command::LiveMute(true)));
    event(
        &mut model,
        json!({"event": "live_transcript", "text": "hi"}),
    );
    let follow = respond(
        &mut model,
        listen,
        "live_listen",
        Ok(json!({"text": "Hello"})),
    );
    assert!(follow.contains(&Command::LiveListen));
}

#[test]
fn a_recording_without_speech_pauses_with_a_hint_and_adds_no_error_row() {
    let mut model = connected();
    let listen = listening(&mut model, 2);
    let follow = respond(
        &mut model,
        listen,
        "live_listen",
        failure(
            None,
            "no speech detected in audio from cpal; check the input device",
        ),
    );
    assert!(follow.is_empty());
    assert_eq!(model.voice.phase, Phase::Paused);
    assert_eq!(
        model.voice.problem.as_deref(),
        Some("Didn't catch that. Tap to talk again")
    );
    assert!(model.conversation.entries().is_empty());
    // Tapping talks again.
    assert_eq!(model.voice_tap(), Some(Command::LiveListen));
}

#[test]
fn dictation_turned_off_says_where_to_turn_it_on() {
    let mut model = connected();
    let listen = listening(&mut model, 2);
    respond(
        &mut model,
        listen,
        "live_listen",
        failure(
            None,
            "speech recognition is disabled (set voice.stt.enabled = true)",
        ),
    );
    assert!(
        model
            .voice
            .problem
            .as_deref()
            .unwrap()
            .contains("Settings → Voice")
    );
}

#[test]
fn ending_while_recording_cancels_it_quietly() {
    let mut model = connected();
    let listen = listening(&mut model, 2);
    assert_eq!(model.voice.stop(), vec![Command::Abort, Command::LiveStop]);
    assert_eq!(model.voice.phase, Phase::Off);
    let follow = respond(
        &mut model,
        listen,
        "live_listen",
        failure(None, "speech input was cancelled"),
    );
    assert!(follow.is_empty());
    assert!(
        model.conversation.entries().is_empty(),
        "no error row for a stop"
    );
    assert_eq!(model.voice.phase, Phase::Off);
    assert!(model.toast.is_none());
}

#[test]
fn tapping_while_listening_stops_and_pauses_without_a_problem() {
    let mut model = connected();
    let listen = listening(&mut model, 2);
    assert_eq!(model.voice_tap(), Some(Command::Abort));
    respond(
        &mut model,
        listen,
        "live_listen",
        failure(Some("aborted"), "aborted"),
    );
    assert_eq!(model.voice.phase, Phase::Paused);
    assert_eq!(model.voice.problem, None);
}

#[test]
fn the_previous_reply_being_cut_off_does_not_trigger_another_listen() {
    let mut model = connected();
    let first = listening(&mut model, 2);
    event(
        &mut model,
        json!({"event": "live_transcript", "text": "one"}),
    );
    respond(
        &mut model,
        first,
        "live_listen",
        Ok(json!({"text": "A long answer"})),
    );
    event(
        &mut model,
        json!({"event": "live_speech", "speaking": true}),
    );
    // The user talks over the reply: the engine stops reading it.
    assert_eq!(model.voice_tap(), Some(Command::LiveListen));
    let second = requested(&mut model, Command::LiveListen, 9);
    assert!(
        event(
            &mut model,
            json!({"event": "live_speech", "speaking": false, "cancelled": true})
        )
        .is_empty()
    );
    event(
        &mut model,
        json!({"event": "live_transcript", "text": "two"}),
    );
    // The second reply is unmuted and not yet read, so its answer must not listen right away.
    let follow = respond(&mut model, second, "live_listen", Ok(json!({"text": "B"})));
    assert!(!follow.contains(&Command::LiveListen), "{follow:?}");
    assert_eq!(model.voice.phase, Phase::Speaking);
}

#[test]
fn a_failed_start_turns_voice_off_and_says_why() {
    let mut model = connected();
    let start = model.start_voice().unwrap().unwrap();
    let id = requested(&mut model, start, 2);
    let follow = respond(
        &mut model,
        id,
        "live_start",
        failure(None, "invalid live voice name"),
    );
    assert!(follow.is_empty());
    assert_eq!(model.voice.phase, Phase::Off);
    assert!(
        model
            .toast
            .as_deref()
            .unwrap()
            .contains("invalid live voice name")
    );
}

#[test]
fn a_user_set_style_is_not_overridden_and_a_running_turn_must_finish_first() {
    let mut model = connected();
    model.settings = Some(
        [(
            "voice.live.instructions".to_owned(),
            crate::model::SavedSetting {
                value: json!("Talk like a pirate"),
                redacted: false,
            },
        )]
        .into(),
    );
    assert_eq!(
        model.start_voice().unwrap(),
        Some(Command::LiveStart { instructions: None })
    );
    let mut busy = connected();
    requested(
        &mut busy,
        Command::Send {
            text: "work".into(),
            images: vec![],
            inline_images: vec![],
        },
        2,
    );
    assert!(busy.start_voice().is_err());
}

#[test]
fn a_disconnect_turns_voice_off() {
    let mut model = connected();
    listening(&mut model, 2);
    model.apply(Update::Message(Message::Disconnected {
        outstanding: vec![],
    }));
    assert_eq!(model.voice.phase, Phase::Off);
}

#[test]
fn the_level_meter_follows_the_microphone_only_while_listening() {
    let mut model = connected();
    listening(&mut model, 2);
    event(
        &mut model,
        json!({"event": "live_level", "level": 0.25, "speech": true}),
    );
    assert_eq!((model.voice.level, model.voice.hearing), (100, true));
    event(
        &mut model,
        json!({"event": "live_level", "level": 0.0, "speech": false}),
    );
    assert_eq!((model.voice.level, model.voice.hearing), (0, false));
    event(
        &mut model,
        json!({"event": "live_level", "level": 0.09, "speech": true}),
    );
    assert_eq!(model.voice.level, 60);
    // Once heard, the meter rests until the next recording.
    event(
        &mut model,
        json!({"event": "live_transcript", "text": "hi"}),
    );
    assert_eq!((model.voice.level, model.voice.hearing), (0, false));
    event(
        &mut model,
        json!({"event": "live_level", "level": 0.2, "speech": true}),
    );
    assert_eq!(model.voice.level, 0);
}

#[test]
fn a_misheard_request_can_be_cancelled_while_dial_works_on_it() {
    let mut model = connected();
    let listen = listening(&mut model, 2);
    assert_eq!(model.voice_cancel(), None, "nothing heard yet");
    event(
        &mut model,
        json!({"event": "live_transcript", "text": "delete everything"}),
    );
    assert_eq!(model.voice_cancel(), Some(Command::Abort));
    assert_eq!(model.voice.phase, Phase::Paused);
    let follow = respond(
        &mut model,
        listen,
        "live_listen",
        Ok(json!({"interrupted": true})),
    );
    assert!(!follow.contains(&Command::LiveListen), "{follow:?}");
    assert_eq!(model.voice.phase, Phase::Paused);
    assert_eq!(model.voice.problem, None);
    assert_eq!(model.voice.tap(false), Some(Command::LiveListen));
}

#[test]
fn read_aloud_outside_voice_mode_is_tracked_and_can_be_stopped() {
    let mut model = connected();
    assert_eq!(
        model.read_aloud(Some("Hello there".into())),
        Ok(Command::LiveSpeak(Some("Hello there".into())))
    );
    assert_eq!(
        model.read_aloud(Some("  ".into())),
        Ok(Command::LiveSpeak(None))
    );
    assert!(
        event(
            &mut model,
            json!({"event": "live_speech", "speaking": true})
        )
        .is_empty()
    );
    assert!(model.voice.reading);
    assert_eq!(
        model.voice.phase,
        Phase::Off,
        "reading does not start voice mode"
    );
    assert!(
        event(
            &mut model,
            json!({"event": "live_speech", "speaking": false})
        )
        .is_empty()
    );
    assert!(!model.voice.reading);
}
