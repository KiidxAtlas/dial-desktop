//! QEMU-style qcode (from `input::qcode_for`) to X11 keysym, the key names RFB's KeyEvent uses.

/// The keysym for a qcode, or an error when the key has no mapping.
pub(super) fn keysym(qcode: &str) -> Result<u32, String> {
    if let [byte] = qcode.as_bytes()
        && (byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return Ok(u32::from(*byte));
    }
    if let Some(n) = qcode.strip_prefix('f').and_then(|n| n.parse::<u32>().ok())
        && (1..=12).contains(&n)
    {
        return Ok(0xffbe + n - 1);
    }
    Ok(match qcode {
        "ret" => 0xff0d,
        "esc" => 0xff1b,
        "backspace" => 0xff08,
        "tab" => 0xff09,
        "spc" => 0x20,
        "minus" => 0x2d,
        "equal" => 0x3d,
        "bracket_left" => 0x5b,
        "bracket_right" => 0x5d,
        "backslash" => 0x5c,
        "semicolon" => 0x3b,
        "apostrophe" => 0x27,
        "grave_accent" => 0x60,
        "comma" => 0x2c,
        "dot" => 0x2e,
        "slash" => 0x2f,
        "up" => 0xff52,
        "down" => 0xff54,
        "left" => 0xff51,
        "right" => 0xff53,
        "home" => 0xff50,
        "end" => 0xff57,
        "pgup" => 0xff55,
        "pgdn" => 0xff56,
        "delete" => 0xffff,
        "insert" => 0xff63,
        "shift" => 0xffe1,
        "shift_r" => 0xffe2,
        "ctrl" => 0xffe3,
        "ctrl_r" => 0xffe4,
        "caps_lock" => 0xffe5,
        "alt" => 0xffe9,
        "alt_r" => 0xffea,
        "meta_l" => 0xffeb,
        "meta_r" => 0xffec,
        _ => return Err("unsupported physical key".into()),
    })
}
