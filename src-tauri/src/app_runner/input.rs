//! Input translation (UI and agent events to QMP commands) and frames (raw RGBA pixels from a
//! PPM screendump or a BGRA/BGRX framebuffer; PNG only when the agent asks for a screenshot).
//!
//! Pointer coordinates are frame pixels with the origin at the top left; they are scaled to the
//! virtio tablet's 0..32767 range. Keys are sent by physical key code (`KeyboardEvent.code`) mapped
//! to QEMU's qcode names; text layout is the guest's choice.
//!
//! Two kinds of gesture exist. `click` and `press` are complete in one call: the agent may send only
//! these. The UI streams `pointer`, `wheel`, `key` and `release` like a remote desktop viewer (moves,
//! drags, held keys). [`Held`] records what the stream holds down, so `release` (and a stop) lets go
//! of everything; it never outlives the run.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};

/// The tablet's absolute axis range (QEMU's virtio-tablet).
const ABS_MAX: u64 = 32767;

/// A refusal of one input, with the code the UI shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported(pub String);

/// One QMP command to run, in order.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub name: &'static str,
    pub arguments: Value,
}

/// One gesture. `click` and `press` are atomic and neither client can leave anything held with them;
/// the rest are the UI's stream and are refused for the agent.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Gesture {
    Click {
        x: u32,
        y: u32,
        button: String,
    },
    Press {
        code: String,
        #[serde(default)]
        modifiers: Vec<Modifier>,
    },
    /// UI only: the pointer at frame pixel (x, y) with the DOM `buttons` mask held
    /// (1 left, 2 right, 4 middle). Moves, presses, releases and drags are all this.
    Pointer { x: u32, y: u32, buttons: u8 },
    /// UI only: `dy` wheel notches at (x, y); positive scrolls down.
    Wheel { x: u32, y: u32, dy: i32 },
    /// UI only: one physical key goes down (again, for autorepeat) or up.
    Key { code: String, down: bool },
    /// UI only: let go of every key and button the UI holds.
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Modifier {
    Shift,
    Control,
    Alt,
    Meta,
}

impl Modifier {
    fn qcode(self) -> &'static str {
        match self {
            Self::Shift => "shift",
            Self::Control => "ctrl",
            Self::Alt => "alt",
            Self::Meta => "meta_l",
        }
    }
}

impl Gesture {
    /// Click and press complete in one call; the agent may send only these.
    pub fn is_atomic(&self) -> bool {
        matches!(self, Self::Click { .. } | Self::Press { .. })
    }

    pub fn command(&self, width: u32, height: u32) -> Result<Command, Unsupported> {
        if width == 0 || height == 0 {
            return Err(Unsupported("no guest frame is available".into()));
        }
        let events = match self {
            Self::Click { x, y, button } if *x < width && *y < height => {
                let button = qemu_button(button)?;
                vec![
                    abs(*x, width, "x"),
                    abs(*y, height, "y"),
                    btn(button, true),
                    btn(button, false),
                ]
            }
            Self::Click { .. } => {
                return Err(Unsupported("click is outside the guest frame".into()));
            }
            Self::Press { code, modifiers } => {
                let key = qcode_for(code)
                    .ok_or_else(|| Unsupported("unsupported physical key".into()))?;
                if modifiers.len() > 4
                    || modifiers
                        .iter()
                        .enumerate()
                        .any(|(i, m)| modifiers[..i].contains(m))
                {
                    return Err(Unsupported("duplicate key modifiers".into()));
                }
                let mods: Vec<_> = modifiers
                    .iter()
                    .map(|m| m.qcode())
                    .filter(|&m| m != key)
                    .collect();
                mods.iter()
                    .map(|m| key_event(m, true))
                    .chain([key_event(key, true), key_event(key, false)])
                    .chain(mods.iter().rev().map(|m| key_event(m, false)))
                    .collect()
            }
            _ => return Err(Unsupported("this input needs the live screen".into())),
        };
        Ok(send_events(events))
    }
}

fn send_events(events: Vec<Value>) -> Command {
    Command {
        name: "input-send-event",
        arguments: json!({ "events": events }),
    }
}

fn abs(value: u32, size: u32, axis: &str) -> Value {
    let span = u64::from(size.saturating_sub(1).max(1));
    let scaled = (u64::from(value) * ABS_MAX + span / 2) / span;
    json!({ "type": "abs", "data": { "axis": axis, "value": scaled } })
}

fn btn(button: &str, down: bool) -> Value {
    json!({ "type": "btn", "data": { "down": down, "button": button } })
}

fn key_event(qcode: &str, down: bool) -> Value {
    json!({ "type": "key", "data": { "down": down, "key": { "type": "qcode", "data": qcode } } })
}

/// DOM `buttons` bits Dial passes on.
pub const BUTTON_LEFT: u8 = 1;
pub const BUTTON_RIGHT: u8 = 2;
pub const BUTTON_MIDDLE: u8 = 4;
const BUTTONS: u8 = BUTTON_LEFT | BUTTON_RIGHT | BUTTON_MIDDLE;
/// Most wheel notches in one event, and most keys held at once.
pub const MAX_WHEEL: i32 = 10;
const MAX_HELD_KEYS: usize = 8;

/// One low-level input step, in frame pixels, for a backend to send as is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Low {
    /// The pointer at (x, y) with `buttons` held; `prev` is the mask before this step.
    Mouse { x: u32, y: u32, buttons: u8, prev: u8 },
    /// `dy` notches at (x, y), positive down, with `buttons` still held.
    Wheel { x: u32, y: u32, buttons: u8, dy: i32 },
    Key { code: String, qcode: &'static str, down: bool },
}

/// What the UI's stream holds down in the guest. Lives in the run's channel.
#[derive(Debug, Default)]
pub struct Held {
    buttons: u8,
    x: u32,
    y: u32,
    keys: Vec<(String, &'static str)>,
}

impl Held {
    pub fn any(&self) -> bool {
        self.buttons != 0 || !self.keys.is_empty()
    }

    /// Translate one stream gesture into steps and record what it holds. A refusal changes nothing.
    pub fn apply(&mut self, gesture: &Gesture, width: u32, height: u32) -> Result<Vec<Low>, Unsupported> {
        if width == 0 || height == 0 {
            return Err(Unsupported("no guest frame is available".into()));
        }
        // A drag may leave the frame while the pointer is captured: pin it to the edge.
        let pin = |x: u32, y: u32| (x.min(width - 1), y.min(height - 1));
        match gesture {
            Gesture::Pointer { x, y, buttons } => {
                let (x, y) = pin(*x, *y);
                let buttons = buttons & BUTTONS;
                let step = Low::Mouse { x, y, buttons, prev: self.buttons };
                (self.x, self.y, self.buttons) = (x, y, buttons);
                Ok(vec![step])
            }
            Gesture::Wheel { x, y, dy } => {
                let (x, y) = pin(*x, *y);
                (self.x, self.y) = (x, y);
                let dy = (*dy).clamp(-MAX_WHEEL, MAX_WHEEL);
                Ok(if dy == 0 {
                    Vec::new()
                } else {
                    vec![Low::Wheel { x, y, buttons: self.buttons, dy }]
                })
            }
            Gesture::Key { code, down } => {
                let qcode =
                    qcode_for(code).ok_or_else(|| Unsupported("unsupported physical key".into()))?;
                let held = self.keys.iter().position(|(c, _)| c == code);
                match (down, held) {
                    (true, None) if self.keys.len() >= MAX_HELD_KEYS => {
                        Err(Unsupported("too many keys are held".into()))
                    }
                    (true, None) => {
                        self.keys.push((code.clone(), qcode));
                        Ok(vec![Low::Key { code: code.clone(), qcode, down: true }])
                    }
                    // Autorepeat: the key is already down; the guest repeats it on its own.
                    (true, Some(_)) => Ok(Vec::new()),
                    (false, Some(i)) => {
                        self.keys.remove(i);
                        Ok(vec![Low::Key { code: code.clone(), qcode, down: false }])
                    }
                    // An up for a key this stream never pressed (it went down outside the screen).
                    (false, None) => Ok(Vec::new()),
                }
            }
            Gesture::Release => Ok(self.release()),
            Gesture::Click { .. } | Gesture::Press { .. } => {
                Err(Unsupported("not a live-screen input".into()))
            }
        }
    }

    /// Let go of everything: keys newest first, then the buttons where the pointer last was.
    pub fn release(&mut self) -> Vec<Low> {
        let mut out: Vec<Low> = self
            .keys
            .drain(..)
            .rev()
            .map(|(code, qcode)| Low::Key { code, qcode, down: false })
            .collect();
        if self.buttons != 0 {
            out.push(Low::Mouse { x: self.x, y: self.y, buttons: 0, prev: self.buttons });
            self.buttons = 0;
        }
        out
    }

    /// The modifiers among the held keys, for a guest that takes only whole presses.
    fn modifiers(&self) -> Vec<Modifier> {
        let mut out = Vec::new();
        for (_, qcode) in &self.keys {
            let m = match *qcode {
                "shift" | "shift_r" => Modifier::Shift,
                "ctrl" | "ctrl_r" => Modifier::Control,
                "alt" | "alt_r" => Modifier::Alt,
                "meta_l" | "meta_r" => Modifier::Meta,
                _ => continue,
            };
            if !out.contains(&m) {
                out.push(m);
            }
        }
        out
    }
}

fn is_modifier(qcode: &str) -> bool {
    matches!(qcode, "shift" | "shift_r" | "ctrl" | "ctrl_r" | "alt" | "alt_r" | "meta_l" | "meta_r")
}

/// Each DOM button bit and its QEMU name.
const BUTTON_NAMES: [(u8, &str); 3] =
    [(BUTTON_LEFT, "left"), (BUTTON_RIGHT, "right"), (BUTTON_MIDDLE, "middle")];

/// Steps as gestures for a guest that takes only whole ones (the virtual Mac's companion): a click
/// when a button is let go, a press when a non-modifier key goes down with the modifiers then held.
/// Moves, the wheel and key-ups send nothing. `held` is the state after the steps.
pub fn atomic(steps: &[Low], held: &Held) -> Vec<Gesture> {
    let mut out = Vec::new();
    for step in steps {
        match step {
            Low::Mouse { x, y, buttons, prev } => {
                for (bit, name) in BUTTON_NAMES {
                    if prev & bit != 0 && buttons & bit == 0 {
                        out.push(Gesture::Click { x: *x, y: *y, button: name.into() });
                    }
                }
            }
            Low::Key { code, qcode, down: true } if !is_modifier(qcode) => {
                out.push(Gesture::Press { code: code.clone(), modifiers: held.modifiers() });
            }
            _ => {}
        }
    }
    out
}

/// Steps as one QMP `input-send-event`, or `None` when there is nothing to send.
pub fn stream_command(steps: &[Low], width: u32, height: u32) -> Option<Command> {
    let mut events = Vec::new();
    for step in steps {
        match step {
            Low::Mouse { x, y, buttons, prev } => {
                events.push(abs(*x, width, "x"));
                events.push(abs(*y, height, "y"));
                for (bit, name) in BUTTON_NAMES {
                    if (prev ^ buttons) & bit != 0 {
                        events.push(btn(name, buttons & bit != 0));
                    }
                }
            }
            Low::Wheel { x, y, dy, .. } => {
                events.push(abs(*x, width, "x"));
                events.push(abs(*y, height, "y"));
                let name = if *dy > 0 { "wheel-down" } else { "wheel-up" };
                for _ in 0..dy.unsigned_abs() {
                    events.push(btn(name, true));
                    events.push(btn(name, false));
                }
            }
            Low::Key { qcode, down, .. } => events.push(key_event(qcode, *down)),
        }
    }
    (!events.is_empty()).then(|| send_events(events))
}

fn qemu_button(button: &str) -> Result<&'static str, Unsupported> {
    match button {
        "left" => Ok("left"),
        "right" => Ok("right"),
        "middle" => Ok("middle"),
        other => Err(Unsupported(format!(
            "the mouse button {other} is not supported"
        ))),
    }
}

/// `KeyboardEvent.code` to QEMU's qcode.
pub fn qcode_for(code: &str) -> Option<&'static str> {
    const LETTERS: [&str; 26] = [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r",
        "s", "t", "u", "v", "w", "x", "y", "z",
    ];
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    const FUNCTION: [&str; 12] = [
        "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
    ];
    if let Some(letter) = code.strip_prefix("Key") {
        let mut chars = letter.chars();
        if let (Some(c), None) = (chars.next(), chars.next())
            && c.is_ascii_uppercase()
        {
            return Some(LETTERS[(c as u8 - b'A') as usize]);
        }
        return None;
    }
    if let Some(digit) = code.strip_prefix("Digit") {
        let mut chars = digit.chars();
        if let (Some(c), None) = (chars.next(), chars.next())
            && c.is_ascii_digit()
        {
            return Some(DIGITS[(c as u8 - b'0') as usize]);
        }
        return None;
    }
    if let Some(n) = code.strip_prefix('F')
        && let Ok(n) = n.parse::<usize>()
        && (1..=12).contains(&n)
    {
        return Some(FUNCTION[n - 1]);
    }
    Some(match code {
        "Enter" | "NumpadEnter" => "ret",
        "Escape" => "esc",
        "Backspace" => "backspace",
        "Tab" => "tab",
        "Space" => "spc",
        "Minus" => "minus",
        "Equal" => "equal",
        "BracketLeft" => "bracket_left",
        "BracketRight" => "bracket_right",
        "Backslash" => "backslash",
        "Semicolon" => "semicolon",
        "Quote" => "apostrophe",
        "Backquote" => "grave_accent",
        "Comma" => "comma",
        "Period" => "dot",
        "Slash" => "slash",
        "ArrowUp" => "up",
        "ArrowDown" => "down",
        "ArrowLeft" => "left",
        "ArrowRight" => "right",
        "Home" => "home",
        "End" => "end",
        "PageUp" => "pgup",
        "PageDown" => "pgdn",
        "Delete" => "delete",
        "Insert" => "insert",
        "ShiftLeft" => "shift",
        "ShiftRight" => "shift_r",
        "ControlLeft" => "ctrl",
        "ControlRight" => "ctrl_r",
        "AltLeft" => "alt",
        "AltRight" => "alt_r",
        "MetaLeft" => "meta_l",
        "MetaRight" => "meta_r",
        "CapsLock" => "caps_lock",
        _ => return None,
    })
}

/// One screen picture: RGBA pixels (alpha 255), row-major from the top left. Shared, never copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

impl Frame {
    /// From 3-byte RGB pixels.
    pub fn from_rgb(width: u32, height: u32, rgb: &[u8]) -> Result<Frame, String> {
        check_size(width, height, rgb.len(), 3)?;
        let mut rgba = Vec::with_capacity(rgb.len() / 3 * 4);
        for &[r, g, b] in rgb.as_chunks::<3>().0 {
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
        Ok(Frame { width, height, rgba: rgba.into() })
    }

    /// From 4-byte little-endian 32-bit pixels, blue first (VNC's and VirtualBox's BGRA/BGRX).
    pub fn from_bgrx(width: u32, height: u32, bgrx: &[u8]) -> Result<Frame, String> {
        check_size(width, height, bgrx.len(), 4)?;
        let mut rgba = Vec::with_capacity(bgrx.len());
        for &[b, g, r, _] in bgrx.as_chunks::<4>().0 {
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
        Ok(Frame { width, height, rgba: rgba.into() })
    }

    /// The picture as a PNG, for the agent's screenshot.
    pub fn png(&self) -> Result<Vec<u8>, String> {
        let mut rgb = Vec::with_capacity(self.rgba.len() / 4 * 3);
        for &[r, g, b, _] in self.rgba.as_chunks::<4>().0 {
            rgb.extend_from_slice(&[r, g, b]);
        }
        let mut png = Vec::new();
        let mut encoder = png::Encoder::new(&mut png, self.width, self.height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fastest);
        let mut writer = encoder
            .write_header()
            .map_err(|error| format!("could not encode the screen: {error}"))?;
        writer
            .write_image_data(&rgb)
            .map_err(|error| format!("could not encode the screen: {error}"))?;
        writer
            .finish()
            .map_err(|error| format!("could not encode the screen: {error}"))?;
        Ok(png)
    }
}

/// Largest picture accepted from any guest, in pixels.
pub const MAX_FRAME_PIXELS: u64 = 4096 * 4096;

fn check_size(width: u32, height: u32, len: usize, bytes_per_pixel: u64) -> Result<(), String> {
    let pixels = u64::from(width) * u64::from(height);
    if width == 0 || height == 0 || pixels > MAX_FRAME_PIXELS {
        return Err("the guest screen has an unsupported size".into());
    }
    if len as u64 != pixels * bytes_per_pixel {
        return Err("incorrect screen image length".into());
    }
    Ok(())
}

/// Convert a binary P6 screendump.
pub fn frame_from_ppm(ppm: &[u8]) -> Result<Frame, String> {
    let (width, height, data) = parse_ppm(ppm)?;
    Frame::from_rgb(width, height, data)
}

fn parse_ppm(ppm: &[u8]) -> Result<(u32, u32, &[u8]), String> {
    let mut pos = 0usize;
    let mut token = || -> Result<String, String> {
        loop {
            while pos < ppm.len() && ppm[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if pos < ppm.len() && ppm[pos] == b'#' {
                while pos < ppm.len() && ppm[pos] != b'\n' {
                    pos += 1;
                }
                continue;
            }
            break;
        }
        let start = pos;
        while pos < ppm.len() && !ppm[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if start == pos {
            return Err("the screen image is truncated".into());
        }
        Ok(String::from_utf8_lossy(&ppm[start..pos]).into_owned())
    };
    if token()? != "P6" {
        return Err("the screen image is not a binary PPM".into());
    }
    let width: u32 = token()?
        .parse()
        .map_err(|_| "bad screen width".to_owned())?;
    let height: u32 = token()?
        .parse()
        .map_err(|_| "bad screen height".to_owned())?;
    let maxval: u32 = token()?
        .parse()
        .map_err(|_| "bad screen depth".to_owned())?;
    if maxval != 255 || width == 0 || height == 0 {
        return Err("the screen image has an unsupported format".into());
    }
    // Exactly one whitespace byte separates the header from the pixels.
    pos += 1;
    let expected = u64::from(width) * u64::from(height);
    if expected > 4096 * 4096 {
        return Err("the guest screen exceeds the pixel limit".into());
    }
    let data = ppm
        .get(pos..)
        .filter(|p| p.len() == expected as usize * 3)
        .ok_or_else(|| "incorrect screen image length".to_owned())?;
    Ok((width, height, data))
}
