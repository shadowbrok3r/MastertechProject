//! ASCII and chord names to USB HID keyboard usage codes (usage page 0x07).

pub const MOD_CTRL: u8 = 0x01;
pub const MOD_SHIFT: u8 = 0x02;
pub const MOD_ALT: u8 = 0x04;
pub const MOD_GUI: u8 = 0x08;

/// One key press: modifier bitmask and a usage code, or `None` if unmappable.
pub type Chord = (u8, u8);

/// Maps one character to a modifier + usage code.
pub fn ascii_to_key(c: char) -> Option<Chord> {
    let key = |u: u8| Some((0, u));
    let shift = |u: u8| Some((MOD_SHIFT, u));
    match c {
        'a'..='z' => key(0x04 + (c as u8 - b'a')),
        'A'..='Z' => shift(0x04 + (c as u8 - b'A')),
        '1'..='9' => key(0x1E + (c as u8 - b'1')),
        '0' => key(0x27),
        '!' => shift(0x1E),
        '@' => shift(0x1F),
        '#' => shift(0x20),
        '$' => shift(0x21),
        '%' => shift(0x22),
        '^' => shift(0x23),
        '&' => shift(0x24),
        '*' => shift(0x25),
        '(' => shift(0x26),
        ')' => shift(0x27),
        ' ' => key(0x2C),
        '\n' => key(0x28),
        '\r' => key(0x28),
        '\t' => key(0x2B),
        '-' => key(0x2D),
        '_' => shift(0x2D),
        '=' => key(0x2E),
        '+' => shift(0x2E),
        '[' => key(0x2F),
        '{' => shift(0x2F),
        ']' => key(0x30),
        '}' => shift(0x30),
        '\\' => key(0x31),
        '|' => shift(0x31),
        ';' => key(0x33),
        ':' => shift(0x33),
        '\'' => key(0x34),
        '"' => shift(0x34),
        '`' => key(0x35),
        '~' => shift(0x35),
        ',' => key(0x36),
        '<' => shift(0x36),
        '.' => key(0x37),
        '>' => shift(0x37),
        '/' => key(0x38),
        '?' => shift(0x38),
        _ => None,
    }
}

/// Usage code for a named non-character key, case-insensitive.
fn named_key(name: &str) -> Option<u8> {
    let n = name.to_ascii_lowercase();
    let u = match n.as_str() {
        "enter" | "return" => 0x28,
        "esc" | "escape" => 0x29,
        "backspace" | "bksp" => 0x2A,
        "tab" => 0x2B,
        "space" => 0x2C,
        "capslock" | "caps" => 0x39,
        "del" | "delete" => 0x4C,
        "ins" | "insert" => 0x49,
        "home" => 0x4A,
        "end" => 0x4D,
        "pageup" | "pgup" => 0x4B,
        "pagedown" | "pgdn" => 0x4E,
        "right" => 0x4F,
        "left" => 0x50,
        "down" => 0x51,
        "up" => 0x52,
        "printscreen" | "prtsc" => 0x46,
        "f1" => 0x3A,
        "f2" => 0x3B,
        "f3" => 0x3C,
        "f4" => 0x3D,
        "f5" => 0x3E,
        "f6" => 0x3F,
        "f7" => 0x40,
        "f8" => 0x41,
        "f9" => 0x42,
        "f10" => 0x43,
        "f11" => 0x44,
        "f12" => 0x45,
        _ => return None,
    };
    Some(u)
}

/// Modifier bit for a modifier token, case-insensitive.
fn modifier(name: &str) -> Option<u8> {
    match name.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some(MOD_CTRL),
        "shift" => Some(MOD_SHIFT),
        "alt" | "option" => Some(MOD_ALT),
        "gui" | "win" | "windows" | "cmd" | "meta" | "super" => Some(MOD_GUI),
        _ => None,
    }
}

/// Parses a `+`-joined chord like `ctrl+alt+del` or `shift+F2` into modifiers + key.
pub fn parse_chord(chord: &str) -> Option<Chord> {
    let parts: Vec<&str> = chord.split('+').map(str::trim).filter(|s| !s.is_empty()).collect();
    let (key_tok, mod_toks) = parts.split_last()?;

    let mut mods = 0u8;
    for tok in mod_toks {
        mods |= modifier(tok)?;
    }

    if let Some(u) = named_key(key_tok) {
        return Some((mods, u));
    }
    if key_tok.chars().count() == 1 {
        let (kmods, u) = ascii_to_key(key_tok.chars().next()?)?;
        return Some((mods | kmods, u));
    }
    // A lone modifier chord (e.g. "shift") presses no key.
    if mod_toks.is_empty() {
        if let Some(m) = modifier(key_tok) {
            return Some((m, 0));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_and_case() {
        assert_eq!(ascii_to_key('a'), Some((0, 0x04)));
        assert_eq!(ascii_to_key('z'), Some((0, 0x1D)));
        assert_eq!(ascii_to_key('A'), Some((MOD_SHIFT, 0x04)));
    }

    #[test]
    fn digits_and_symbols() {
        assert_eq!(ascii_to_key('1'), Some((0, 0x1E)));
        assert_eq!(ascii_to_key('0'), Some((0, 0x27)));
        assert_eq!(ascii_to_key('!'), Some((MOD_SHIFT, 0x1E)));
        assert_eq!(ascii_to_key(')'), Some((MOD_SHIFT, 0x27)));
        assert_eq!(ascii_to_key(' '), Some((0, 0x2C)));
        assert_eq!(ascii_to_key('\n'), Some((0, 0x28)));
    }

    #[test]
    fn chords_combine_modifiers() {
        assert_eq!(parse_chord("ctrl+alt+del"), Some((MOD_CTRL | MOD_ALT, 0x4C)));
        assert_eq!(parse_chord("F2"), Some((0, 0x3B)));
        assert_eq!(parse_chord("ctrl+c"), Some((MOD_CTRL, 0x06)));
        assert_eq!(parse_chord("win+r"), Some((MOD_GUI, 0x15)));
        assert_eq!(parse_chord("shift+tab"), Some((MOD_SHIFT, 0x2B)));
    }

    #[test]
    fn shifted_letter_in_chord_keeps_both_modifiers() {
        assert_eq!(parse_chord("ctrl+A"), Some((MOD_CTRL | MOD_SHIFT, 0x04)));
    }

    #[test]
    fn lone_modifier_and_bad_input() {
        assert_eq!(parse_chord("shift"), Some((MOD_SHIFT, 0)));
        assert_eq!(parse_chord("nope"), None);
        assert_eq!(parse_chord(""), None);
    }
}
