//! The agent's action batches: one `input` call carries a list of actions (type text, key combos,
//! clicks, double-clicks, drags, scrolls, pointer moves, waits) that run in order on the screen
//! the agent last saw, so a model turn does a whole task step instead of one key.
//!
//! [`plan`] checks the whole batch before anything is sent (an invalid action sends nothing) and
//! turns it into [`Step`]s: low-level input for guests that take a stream, or whole clicks and
//! presses for a guest that takes only those. Every step leaves nothing held except inside a drag,
//! whose button the runner lets go of if the batch stops early.

use std::time::Duration;

use serde::Deserialize;

use super::input::{BUTTON_LEFT, BUTTON_MIDDLE, BUTTON_RIGHT, Gesture, Low, MAX_WHEEL, Modifier, Unsupported, qcode_for};

/// Most actions in one call.
pub const MAX_ACTIONS: usize = 64;
/// Most characters typed in one call, over all `type` actions.
pub const MAX_TEXT: usize = 4000;
/// Longest single `wait`, and all waits in one call together.
pub const MAX_WAIT: Duration = Duration::from_secs(5);
pub const MAX_TOTAL_WAIT: Duration = Duration::from_secs(15);
/// Characters sent per input burst while typing, and the pause between bursts, so a guest's
/// keyboard buffer is not overrun.
const TYPE_CHUNK: usize = 24;
const TYPE_PAUSE: Duration = Duration::from_millis(12);
/// Pause between the clicks of a multi-click and the segments of a drag.
const GESTURE_PAUSE: Duration = Duration::from_millis(30);
const DRAG_SEGMENTS: u32 = 8;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Button {
    #[default]
    Left,
    Middle,
    Right,
}

impl Button {
    fn bit(self) -> u8 {
        match self {
            Self::Left => BUTTON_LEFT,
            Self::Middle => BUTTON_MIDDLE,
            Self::Right => BUTTON_RIGHT,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Middle => "middle",
            Self::Right => "right",
        }
    }
}

/// One agent action, as the wire carries it (`kind` plus fields; camelCase).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// `count` 2 is a double-click, 3 a triple-click.
    Click {
        x: u32,
        y: u32,
        #[serde(default)]
        button: Button,
        #[serde(default = "one")]
        count: u8,
    },
    DoubleClick {
        x: u32,
        y: u32,
    },
    /// Move the pointer without pressing (hover menus, tooltips).
    Move {
        x: u32,
        y: u32,
    },
    /// Press at (x, y), move to (toX, toY), let go.
    #[serde(rename_all = "camelCase")]
    Drag {
        x: u32,
        y: u32,
        to_x: u32,
        to_y: u32,
        #[serde(default)]
        button: Button,
    },
    /// `dy` notches at (x, y); positive scrolls down.
    Scroll {
        x: u32,
        y: u32,
        dy: i32,
    },
    /// A key or combination: `keys` like `"ctrl+c"`, `"Enter"`, `"alt+F4"`, `"shift+Tab"`; or the
    /// older `code` (a `KeyboardEvent.code`) with `modifiers`. `repeat` presses it several times.
    Press {
        #[serde(default)]
        keys: Option<String>,
        #[serde(default)]
        code: Option<String>,
        #[serde(default)]
        modifiers: Vec<Modifier>,
        #[serde(default = "one")]
        repeat: u8,
    },
    /// Text typed on a US keyboard layout: printable ASCII, `\n` (Enter) and `\t` (Tab).
    Type {
        text: String,
    },
    /// Pause, for the guest to react (a page load, a menu opening). At most 5 s.
    Wait {
        ms: u64,
    },
}

fn one() -> u8 {
    1
}

/// One thing the runner does, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Low-level input for a stream guest. `buttons_after` is what the pointer holds once sent.
    Low { steps: Vec<Low>, buttons_after: u8 },
    /// Whole gestures, for a guest that takes only clicks and presses.
    Whole(Vec<Gesture>),
    Wait(Duration),
}

/// A batch ready to run: its steps and, per step, the index of the action it came from.
#[derive(Debug)]
pub struct Plan {
    pub steps: Vec<(usize, Step)>,
    pub actions: usize,
}

/// Check and translate a batch for a `width` x `height` screen. `whole` is a guest that takes only
/// whole clicks and presses; actions it cannot do are refused up front.
pub fn plan(actions: &[Action], width: u32, height: u32, whole: bool) -> Result<Plan, Unsupported> {
    if width == 0 || height == 0 {
        return Err(Unsupported("no guest frame is available".into()));
    }
    if actions.is_empty() {
        return Err(Unsupported("send at least one action".into()));
    }
    if actions.len() > MAX_ACTIONS {
        return Err(Unsupported(format!("at most {MAX_ACTIONS} actions per call")));
    }
    let text: usize = actions
        .iter()
        .map(|a| match a {
            Action::Type { text } => text.chars().count(),
            _ => 0,
        })
        .sum();
    if text > MAX_TEXT {
        return Err(Unsupported(format!("at most {MAX_TEXT} characters typed per call")));
    }
    let mut waited = Duration::ZERO;
    let mut out = Vec::new();
    let inside = |i: usize, x: u32, y: u32| {
        if x < width && y < height {
            Ok(())
        } else {
            Err(Unsupported(format!(
                "action {} is outside the {width}x{height} screen",
                i + 1
            )))
        }
    };
    let refuse_whole = |i: usize, what: &str| {
        Unsupported(format!("action {}: this guest takes only clicks, presses and typing, not {what}", i + 1))
    };
    for (i, action) in actions.iter().enumerate() {
        let at = |e: Unsupported| Unsupported(format!("action {}: {}", i + 1, e.0));
        let click = match action {
            Action::Click { x, y, button, count } => Some((*x, *y, *button, *count)),
            Action::DoubleClick { x, y } => Some((*x, *y, Button::Left, 2)),
            _ => None,
        };
        if let Some((x, y, button, count)) = click {
            inside(i, x, y)?;
            if !(1..=3).contains(&count) {
                return Err(at(Unsupported("count must be 1, 2 or 3".into())));
            }
            if whole {
                let click = Gesture::Click { x, y, button: button.name().into() };
                out.push((i, Step::Whole((0..count).map(|_| click.clone()).collect())));
                continue;
            }
            let bit = button.bit();
            let mut steps = vec![mouse(x, y, 0, 0)];
            for _ in 0..count {
                steps.push(mouse(x, y, bit, 0));
                steps.push(mouse(x, y, 0, bit));
            }
            out.push((i, Step::Low { steps, buttons_after: 0 }));
            continue;
        }
        match action {
            Action::Click { .. } | Action::DoubleClick { .. } => {}
            Action::Move { x, y } => {
                inside(i, *x, *y)?;
                if whole {
                    return Err(refuse_whole(i, "pointer moves"));
                }
                out.push((i, Step::Low { steps: vec![mouse(*x, *y, 0, 0)], buttons_after: 0 }));
            }
            Action::Drag { x, y, to_x, to_y, button } => {
                inside(i, *x, *y)?;
                inside(i, *to_x, *to_y)?;
                if whole {
                    return Err(refuse_whole(i, "drags"));
                }
                let bit = button.bit();
                out.push((i, Step::Low { steps: vec![mouse(*x, *y, 0, 0), mouse(*x, *y, bit, 0)], buttons_after: bit }));
                for n in 1..=DRAG_SEGMENTS {
                    let px = lerp(*x, *to_x, n, DRAG_SEGMENTS);
                    let py = lerp(*y, *to_y, n, DRAG_SEGMENTS);
                    out.push((i, Step::Wait(GESTURE_PAUSE)));
                    out.push((i, Step::Low { steps: vec![mouse(px, py, bit, bit)], buttons_after: bit }));
                }
                out.push((i, Step::Wait(GESTURE_PAUSE)));
                out.push((i, Step::Low { steps: vec![mouse(*to_x, *to_y, 0, bit)], buttons_after: 0 }));
            }
            Action::Scroll { x, y, dy } => {
                inside(i, *x, *y)?;
                if whole {
                    return Err(refuse_whole(i, "scrolling"));
                }
                if *dy == 0 || dy.unsigned_abs() > 100 {
                    return Err(at(Unsupported("dy must be between -100 and 100, not 0".into())));
                }
                let mut steps = vec![mouse(*x, *y, 0, 0)];
                let mut left = *dy;
                while left != 0 {
                    let part = left.clamp(-MAX_WHEEL, MAX_WHEEL);
                    steps.push(Low::Wheel { x: *x, y: *y, buttons: 0, dy: part });
                    left -= part;
                }
                out.push((i, Step::Low { steps, buttons_after: 0 }));
            }
            Action::Press { keys, code, modifiers, repeat } => {
                let (key, mods) = match (keys, code) {
                    (Some(keys), None) => parse_combo(keys).map_err(at)?,
                    (None, Some(code)) => {
                        let code = code.clone();
                        qcode_for(&code).ok_or_else(|| at(Unsupported(format!("unknown key code {code}"))))?;
                        (code, modifiers.clone())
                    }
                    _ => return Err(at(Unsupported("press needs keys (like \"ctrl+c\") or code".into()))),
                };
                if !(1..=50).contains(repeat) {
                    return Err(at(Unsupported("repeat must be 1 to 50".into())));
                }
                let mut mods_unique = Vec::new();
                for m in mods {
                    if !mods_unique.contains(&m) {
                        mods_unique.push(m);
                    }
                }
                let presses: Vec<(String, Vec<Modifier>)> =
                    (0..*repeat).map(|_| (key.clone(), mods_unique.clone())).collect();
                out.push((i, key_step(&presses, whole)));
            }
            Action::Type { text } => {
                let mut presses = Vec::new();
                for c in text.chars() {
                    let (code, shift) = char_key(c).ok_or_else(|| {
                        at(Unsupported(format!(
                            "{c:?} cannot be typed: only printable ASCII, newline and tab (US layout)"
                        )))
                    })?;
                    presses.push((code.to_owned(), if shift { vec![Modifier::Shift] } else { Vec::new() }));
                }
                for (n, chunk) in presses.chunks(TYPE_CHUNK).enumerate() {
                    if n > 0 {
                        out.push((i, Step::Wait(TYPE_PAUSE)));
                    }
                    out.push((i, key_step(chunk, whole)));
                }
            }
            Action::Wait { ms } => {
                let wait = Duration::from_millis(*ms);
                if wait > MAX_WAIT {
                    return Err(at(Unsupported("a wait is at most 5000 ms".into())));
                }
                waited += wait;
                if waited > MAX_TOTAL_WAIT {
                    return Err(at(Unsupported("waits total at most 15000 ms per call".into())));
                }
                out.push((i, Step::Wait(wait)));
            }
        }
    }
    Ok(Plan { steps: out, actions: actions.len() })
}

fn mouse(x: u32, y: u32, buttons: u8, prev: u8) -> Low {
    Low::Mouse { x, y, buttons, prev }
}

fn lerp(from: u32, to: u32, n: u32, of: u32) -> u32 {
    let (from, to) = (i64::from(from), i64::from(to));
    (from + (to - from) * i64::from(n) / i64::from(of)) as u32
}

/// Presses (key code, modifiers) as one step: modifiers down, key down and up, modifiers up.
fn key_step(presses: &[(String, Vec<Modifier>)], whole: bool) -> Step {
    if whole {
        return Step::Whole(
            presses
                .iter()
                .map(|(code, modifiers)| Gesture::Press { code: code.clone(), modifiers: modifiers.clone() })
                .collect(),
        );
    }
    let mut steps = Vec::new();
    for (code, modifiers) in presses {
        let key = qcode_for(code).expect("checked when planned");
        let mods: Vec<(&str, &'static str)> = modifiers
            .iter()
            .map(|m| modifier_key(*m))
            .filter(|(_, q)| *q != key)
            .collect();
        for (c, q) in &mods {
            steps.push(Low::Key { code: (*c).into(), qcode: q, down: true });
        }
        steps.push(Low::Key { code: code.clone(), qcode: key, down: true });
        steps.push(Low::Key { code: code.clone(), qcode: key, down: false });
        for (c, q) in mods.iter().rev() {
            steps.push(Low::Key { code: (*c).into(), qcode: q, down: false });
        }
    }
    Step::Low { steps, buttons_after: 0 }
}

fn modifier_key(m: Modifier) -> (&'static str, &'static str) {
    match m {
        Modifier::Shift => ("ShiftLeft", "shift"),
        Modifier::Control => ("ControlLeft", "ctrl"),
        Modifier::Alt => ("AltLeft", "alt"),
        Modifier::Meta => ("MetaLeft", "meta_l"),
    }
}

/// `"ctrl+shift+t"` → (`KeyT`, [Control, Shift]). Names are case-insensitive; the last part is
/// the key: a name (`enter`, `esc`, `tab`, `space`, `backspace`, `delete`, arrows `up`/`down`/
/// `left`/`right`, `home`, `end`, `pageup`, `pagedown`, `insert`, `f1`–`f12`), one character, or
/// a `KeyboardEvent.code` such as `KeyA`.
pub fn parse_combo(combo: &str) -> Result<(String, Vec<Modifier>), Unsupported> {
    let combo = combo.trim();
    // The plus key itself: "+" or "ctrl++".
    let (mods, key): (Vec<&str>, &str) = if combo == "+" {
        (Vec::new(), "+")
    } else if let Some(head) = combo.strip_suffix("++") {
        (head.split('+').map(str::trim).collect(), "+")
    } else {
        let mut parts: Vec<&str> = combo.split('+').map(str::trim).collect();
        let key = parts.pop().unwrap_or_default();
        (parts, key)
    };
    if key.is_empty() {
        return Err(Unsupported(format!("no key in {combo:?}")));
    }
    let mut modifiers = Vec::new();
    for m in mods {
        modifiers.push(match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Modifier::Control,
            "shift" => Modifier::Shift,
            "alt" | "option" | "opt" => Modifier::Alt,
            "meta" | "cmd" | "command" | "super" | "win" | "windows" => Modifier::Meta,
            other => return Err(Unsupported(format!("unknown modifier {other:?} in {combo:?}"))),
        });
    }
    let code = key_code(key).ok_or_else(|| Unsupported(format!("unknown key {key:?} in {combo:?}")))?;
    if let (Some(shift), true) = (char_shift(key), modifiers.is_empty()) {
        // A shifted symbol on its own ("?") is typed with Shift.
        if shift {
            modifiers.push(Modifier::Shift);
        }
    }
    Ok((code, modifiers))
}

fn char_shift(key: &str) -> Option<bool> {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if !c.is_ascii_alphabetic() => char_key(c).map(|(_, s)| s),
        _ => None,
    }
}

fn key_code(key: &str) -> Option<String> {
    if qcode_for(key).is_some() {
        return Some(key.to_owned());
    }
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return char_key(c.to_ascii_lowercase()).map(|(code, _)| code.to_owned());
    }
    let lower = key.to_ascii_lowercase();
    let named = match lower.as_str() {
        "enter" | "return" => "Enter",
        "esc" | "escape" => "Escape",
        "tab" => "Tab",
        "space" | "spacebar" => "Space",
        "backspace" => "Backspace",
        "delete" | "del" => "Delete",
        "insert" | "ins" => "Insert",
        "up" | "arrowup" => "ArrowUp",
        "down" | "arrowdown" => "ArrowDown",
        "left" | "arrowleft" => "ArrowLeft",
        "right" | "arrowright" => "ArrowRight",
        "home" => "Home",
        "end" => "End",
        "pageup" | "pgup" => "PageUp",
        "pagedown" | "pgdn" => "PageDown",
        "capslock" => "CapsLock",
        "plus" => "Equal",
        "minus" => "Minus",
        _ => {
            if let Some(n) = lower.strip_prefix('f')
                && let Ok(n) = n.parse::<u8>()
                && (1..=12).contains(&n)
            {
                return Some(format!("F{n}"));
            }
            return None;
        }
    };
    Some(named.to_owned())
}

/// A character on a US keyboard: its physical key and whether Shift is held.
pub fn char_key(c: char) -> Option<(&'static str, bool)> {
    const LETTERS: [&str; 26] = [
        "KeyA", "KeyB", "KeyC", "KeyD", "KeyE", "KeyF", "KeyG", "KeyH", "KeyI", "KeyJ", "KeyK", "KeyL", "KeyM",
        "KeyN", "KeyO", "KeyP", "KeyQ", "KeyR", "KeyS", "KeyT", "KeyU", "KeyV", "KeyW", "KeyX", "KeyY", "KeyZ",
    ];
    const DIGITS: [&str; 10] =
        ["Digit0", "Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "Digit6", "Digit7", "Digit8", "Digit9"];
    Some(match c {
        'a'..='z' => (LETTERS[(c as u8 - b'a') as usize], false),
        'A'..='Z' => (LETTERS[(c as u8 - b'A') as usize], true),
        '0'..='9' => (DIGITS[(c as u8 - b'0') as usize], false),
        ')' => ("Digit0", true),
        '!' => ("Digit1", true),
        '@' => ("Digit2", true),
        '#' => ("Digit3", true),
        '$' => ("Digit4", true),
        '%' => ("Digit5", true),
        '^' => ("Digit6", true),
        '&' => ("Digit7", true),
        '*' => ("Digit8", true),
        '(' => ("Digit9", true),
        ' ' => ("Space", false),
        '\n' => ("Enter", false),
        '\t' => ("Tab", false),
        '-' => ("Minus", false),
        '_' => ("Minus", true),
        '=' => ("Equal", false),
        '+' => ("Equal", true),
        '[' => ("BracketLeft", false),
        '{' => ("BracketLeft", true),
        ']' => ("BracketRight", false),
        '}' => ("BracketRight", true),
        '\\' => ("Backslash", false),
        '|' => ("Backslash", true),
        ';' => ("Semicolon", false),
        ':' => ("Semicolon", true),
        '\'' => ("Quote", false),
        '"' => ("Quote", true),
        '`' => ("Backquote", false),
        '~' => ("Backquote", true),
        ',' => ("Comma", false),
        '<' => ("Comma", true),
        '.' => ("Period", false),
        '>' => ("Period", true),
        '/' => ("Slash", false),
        '?' => ("Slash", true),
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
