use super::*;

fn parse(json: &str) -> Vec<Action> {
    serde_json::from_str(json).expect("valid actions")
}

fn keys(step: &Step) -> Vec<(&'static str, bool)> {
    match step {
        Step::Low { steps, .. } => steps
            .iter()
            .filter_map(|s| match s {
                Low::Key { qcode, down, .. } => Some((*qcode, *down)),
                _ => None,
            })
            .collect(),
        other => panic!("not low: {other:?}"),
    }
}

#[test]
fn typing_shifts_capitals_and_symbols_and_sends_enter() {
    let plan = plan(&parse(r#"[{"kind":"type","text":"Hi!\n"}]"#), 100, 100, false).unwrap();
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(
        keys(&plan.steps[0].1),
        vec![
            ("shift", true),
            ("h", true),
            ("h", false),
            ("shift", false),
            ("i", true),
            ("i", false),
            ("shift", true),
            ("1", true),
            ("1", false),
            ("shift", false),
            ("ret", true),
            ("ret", false),
        ]
    );
}

#[test]
fn long_text_is_sent_in_paced_bursts() {
    let text = "a".repeat(TYPE_CHUNK * 2 + 1);
    let actions = vec![Action::Type { text }];
    let plan = plan(&actions, 10, 10, false).unwrap();
    let kinds: Vec<&str> = plan
        .steps
        .iter()
        .map(|(_, s)| match s {
            Step::Low { .. } => "low",
            Step::Wait(_) => "wait",
            Step::Whole(_) => "whole",
        })
        .collect();
    assert_eq!(kinds, ["low", "wait", "low", "wait", "low"]);
}

#[test]
fn combos_parse_names_symbols_and_plus() {
    assert_eq!(parse_combo("ctrl+c").unwrap(), ("KeyC".into(), vec![Modifier::Control]));
    assert_eq!(
        parse_combo("Ctrl+Shift+T").unwrap(),
        ("KeyT".into(), vec![Modifier::Control, Modifier::Shift])
    );
    assert_eq!(parse_combo("alt+F4").unwrap(), ("F4".into(), vec![Modifier::Alt]));
    assert_eq!(parse_combo("Enter").unwrap(), ("Enter".into(), vec![]));
    assert_eq!(parse_combo("cmd+space").unwrap(), ("Space".into(), vec![Modifier::Meta]));
    assert_eq!(parse_combo("ctrl++").unwrap(), ("Equal".into(), vec![Modifier::Control]));
    assert_eq!(parse_combo("?").unwrap(), ("Slash".into(), vec![Modifier::Shift]));
    assert_eq!(parse_combo("KeyA").unwrap(), ("KeyA".into(), vec![]));
    assert!(parse_combo("hyper+a").is_err());
    assert!(parse_combo("ctrl+").is_err());
    assert!(parse_combo("ctrl+nokey").is_err());
}

#[test]
fn double_click_presses_twice_at_the_point() {
    let plan = plan(&parse(r#"[{"kind":"double_click","x":5,"y":6}]"#), 10, 10, false).unwrap();
    let Step::Low { steps, buttons_after } = &plan.steps[0].1 else { panic!() };
    assert_eq!(*buttons_after, 0);
    let downs = steps
        .iter()
        .filter(|s| matches!(s, Low::Mouse { buttons: 1, prev: 0, x: 5, y: 6 }))
        .count();
    assert_eq!(downs, 2);
}

#[test]
fn drag_holds_the_button_until_the_end() {
    let plan = plan(&parse(r#"[{"kind":"drag","x":0,"y":0,"toX":80,"toY":40}]"#), 100, 100, false).unwrap();
    let lows: Vec<(&Vec<Low>, u8)> = plan
        .steps
        .iter()
        .filter_map(|(_, s)| match s {
            Step::Low { steps, buttons_after } => Some((steps, *buttons_after)),
            _ => None,
        })
        .collect();
    assert_eq!(lows.first().unwrap().1, BUTTON_LEFT);
    assert_eq!(lows.last().unwrap().1, 0);
    assert_eq!(
        lows.last().unwrap().0.last(),
        Some(&Low::Mouse { x: 80, y: 40, buttons: 0, prev: BUTTON_LEFT })
    );
}

#[test]
fn big_scrolls_are_split_into_wheel_events() {
    let plan = plan(&parse(r#"[{"kind":"scroll","x":1,"y":1,"dy":-25}]"#), 10, 10, false).unwrap();
    let Step::Low { steps, .. } = &plan.steps[0].1 else { panic!() };
    let notches: Vec<i32> = steps
        .iter()
        .filter_map(|s| match s {
            Low::Wheel { dy, .. } => Some(*dy),
            _ => None,
        })
        .collect();
    assert_eq!(notches, [-10, -10, -5]);
}

#[test]
fn a_bad_action_anywhere_refuses_the_whole_batch() {
    let actions = parse(
        r#"[{"kind":"type","text":"ok"},{"kind":"click","x":500,"y":1},{"kind":"press","keys":"Enter"}]"#,
    );
    let err = plan(&actions, 100, 100, false).unwrap_err();
    assert!(err.0.starts_with("action 2"), "{}", err.0);
    assert!(plan(&parse(r#"[{"kind":"type","text":"é"}]"#), 10, 10, false).is_err());
    assert!(plan(&parse(r#"[{"kind":"wait","ms":6000}]"#), 10, 10, false).is_err());
    assert!(plan(&[], 10, 10, false).is_err());
    assert!(serde_json::from_str::<Vec<Action>>(r#"[{"kind":"click","x":1,"y":1,"oops":1}]"#).is_err());
}

#[test]
fn whole_gesture_guests_get_clicks_and_presses_only() {
    let plan = plan(
        &parse(r#"[{"kind":"click","x":1,"y":2,"count":2},{"kind":"type","text":"A"},{"kind":"press","keys":"cmd+q"}]"#),
        10,
        10,
        true,
    )
    .unwrap();
    let wholes: Vec<&Vec<Gesture>> = plan
        .steps
        .iter()
        .filter_map(|(_, s)| match s {
            Step::Whole(g) => Some(g),
            _ => None,
        })
        .collect();
    assert_eq!(wholes[0].len(), 2);
    assert_eq!(wholes[1], &vec![Gesture::Press { code: "KeyA".into(), modifiers: vec![Modifier::Shift] }]);
    assert_eq!(wholes[2], &vec![Gesture::Press { code: "KeyQ".into(), modifiers: vec![Modifier::Meta] }]);
    assert!(super::plan(&parse(r#"[{"kind":"scroll","x":1,"y":1,"dy":1}]"#), 10, 10, true).is_err());
}

#[test]
fn every_typeable_character_maps_to_a_known_key() {
    for c in (32u8..127).map(char::from).chain(['\n', '\t']) {
        let (code, _) = char_key(c).unwrap_or_else(|| panic!("{c:?}"));
        assert!(qcode_for(code).is_some(), "{c:?} -> {code}");
    }
}
