// Derived from herdr src/raw_input.rs (Apache-2.0).
use super::*;

// -------------------------------------------------------------------
// parse_terminal_key_sequence: the reference's full key tables.
// -------------------------------------------------------------------

/// `parse_terminal_key_sequence`: kitty `CSI u` first, then the legacy
/// table (control chars, special sequences, xterm-modified, SS3,
/// Alt-prefixed keys, modifyOtherKeys).
pub fn parse_terminal_key_sequence(data: &str) -> Option<TerminalKey> {
    if let Some(key) = parse_kitty_key_sequence(data) {
        return Some(key);
    }
    if let Some(key) = parse_modify_other_keys_sequence(data) {
        return Some(key);
    }
    parse_legacy_key_sequence(data)
}

pub(super) fn tk(key: Key, mods: Mods, action: Action) -> TerminalKey {
    TerminalKey::new(KeyInput { key, mods, action })
}

pub(super) fn parse_modify_other_keys_sequence(data: &str) -> Option<TerminalKey> {
    let body = data.strip_prefix("\x1b[27;")?.strip_suffix('~')?;
    // Exactly `27 ; <mods> ; <codepoint> ~` — extra fields reject.
    let (modifier_part, codepoint_part) = body.split_once(';')?;
    let modifier = modifier_part.parse::<u8>().ok()?.checked_sub(1)?;
    let codepoint = codepoint_part.parse::<u32>().ok()?;
    let key = kitty_codepoint_to_key(codepoint)?;
    Some(tk(key, key_modifiers_from_u8(modifier), Action::Press))
}

pub(super) fn parse_legacy_key_sequence(data: &str) -> Option<TerminalKey> {
    if let Some(key) = parse_legacy_special_sequence(data) {
        return Some(key);
    }

    match data {
        "\x1b\x08" => Some(tk(Key::Backspace, Mods::ALT, Action::Press)),
        "\x1b\r" => Some(tk(Key::Enter, Mods::ALT, Action::Press)),
        "\x1b\t" => Some(tk(Key::Tab, Mods::ALT, Action::Press)),
        "\r" => Some(tk(Key::Enter, Mods::NONE, Action::Press)),
        "\t" => Some(tk(Key::Tab, Mods::NONE, Action::Press)),
        "\x1b" => Some(tk(Key::Escape, Mods::NONE, Action::Press)),
        "\x1b\x7f" => Some(tk(Key::Backspace, Mods::ALT, Action::Press)),
        "\x7f" => Some(tk(Key::Backspace, Mods::NONE, Action::Press)),
        _ if data.starts_with('\x1b') => {
            let rest = data.strip_prefix('\x1b')?;
            if let Some(key) = parse_legacy_key_sequence(rest) {
                let mut input = key.input;
                input.mods = input.mods.union(Mods::ALT);
                return Some(TerminalKey::new(input));
            }
            None
        }
        _ if data.chars().count() == 1 => {
            let ch = data.chars().next().unwrap();
            if let Some(ctrl_key) = parse_legacy_ctrl_char(ch) {
                return Some(ctrl_key);
            }
            // An uppercase letter reports Shift — the reference keeps it
            // so configured `shift+X` shortcuts match a typed capital.
            let mods = if ch.is_ascii_uppercase() {
                Mods::SHIFT
            } else {
                Mods::NONE
            };
            Some(tk(Key::Char(ch), mods, Action::Press))
        }
        _ => None,
    }
}

pub(super) fn parse_legacy_ctrl_char(ch: char) -> Option<TerminalKey> {
    match ch as u32 {
        0 => Some(tk(Key::Char(' '), Mods::CTRL, Action::Press)),
        1..=26 => Some(tk(
            Key::Char(char::from_u32(96 + ch as u32).unwrap()),
            Mods::CTRL,
            Action::Press,
        )),
        27 => Some(tk(Key::Char('['), Mods::CTRL, Action::Press)),
        28 => Some(tk(Key::Char('\\'), Mods::CTRL, Action::Press)),
        29 => Some(tk(Key::Char(']'), Mods::CTRL, Action::Press)),
        30 => Some(tk(Key::Char('^'), Mods::CTRL, Action::Press)),
        31 => Some(tk(Key::Char('_'), Mods::CTRL, Action::Press)),
        _ => None,
    }
}

pub(super) fn parse_legacy_special_sequence(data: &str) -> Option<TerminalKey> {
    let none = Mods::NONE;
    let key = |k: Key| tk(k, none, Action::Press);
    match data {
        "\x1b\x1b[A" => Some(tk(Key::ArrowUp, Mods::ALT, Action::Press)),
        "\x1b\x1b[B" => Some(tk(Key::ArrowDown, Mods::ALT, Action::Press)),
        "\x1b\x1b[C" => Some(tk(Key::ArrowRight, Mods::ALT, Action::Press)),
        "\x1b\x1b[D" => Some(tk(Key::ArrowLeft, Mods::ALT, Action::Press)),
        "\x1b[A" | "\x1bOA" => Some(key(Key::ArrowUp)),
        "\x1b[B" | "\x1bOB" => Some(key(Key::ArrowDown)),
        "\x1b[C" | "\x1bOC" => Some(key(Key::ArrowRight)),
        "\x1b[D" | "\x1bOD" => Some(key(Key::ArrowLeft)),
        "\x1b[H" | "\x1bOH" | "\x1b[1~" | "\x1b[7~" => Some(key(Key::Home)),
        "\x1b[F" | "\x1bOF" | "\x1b[4~" | "\x1b[8~" => Some(key(Key::End)),
        "\x1b[5~" => Some(key(Key::PageUp)),
        "\x1b[6~" => Some(key(Key::PageDown)),
        "\x1b[2~" => Some(key(Key::Insert)),
        "\x1b[3~" => Some(key(Key::Delete)),
        "\x1bOp" => Some(key(Key::Char('0'))),
        "\x1bOq" => Some(key(Key::Char('1'))),
        "\x1bOr" => Some(key(Key::Char('2'))),
        "\x1bOs" => Some(key(Key::Char('3'))),
        "\x1bOt" => Some(key(Key::Char('4'))),
        "\x1bOu" => Some(key(Key::Char('5'))),
        "\x1bOv" => Some(key(Key::Char('6'))),
        "\x1bOw" => Some(key(Key::Char('7'))),
        "\x1bOx" => Some(key(Key::Char('8'))),
        "\x1bOy" => Some(key(Key::Char('9'))),
        "\x1bOn" => Some(key(Key::Char('.'))),
        "\x1bOl" => Some(key(Key::Char(','))),
        "\x1bOm" => Some(key(Key::Char('-'))),
        "\x1bOk" => Some(key(Key::Char('+'))),
        "\x1bOj" => Some(key(Key::Char('*'))),
        "\x1bOo" => Some(key(Key::Char('/'))),
        "\x1bOM" => Some(key(Key::Enter)),
        "\x1bOP" | "\x1b[11~" | "\x1b[P" => Some(key(Key::F(1))),
        "\x1bOQ" | "\x1b[12~" | "\x1b[Q" => Some(key(Key::F(2))),
        "\x1bOR" | "\x1b[13~" => Some(key(Key::F(3))),
        "\x1bOS" | "\x1b[14~" | "\x1b[S" => Some(key(Key::F(4))),
        "\x1b[15~" => Some(key(Key::F(5))),
        "\x1b[17~" => Some(key(Key::F(6))),
        "\x1b[18~" => Some(key(Key::F(7))),
        "\x1b[19~" => Some(key(Key::F(8))),
        "\x1b[20~" => Some(key(Key::F(9))),
        "\x1b[21~" => Some(key(Key::F(10))),
        "\x1b[23~" => Some(key(Key::F(11))),
        "\x1b[24~" => Some(key(Key::F(12))),
        "\x1b[Z" => Some(tk(Key::Tab, Mods::SHIFT, Action::Press)),
        _ => parse_xterm_modified_special_sequence(data),
    }
}

pub(super) fn parse_xterm_modified_special_sequence(data: &str) -> Option<TerminalKey> {
    let body = data.strip_prefix("\x1b[")?;

    // `\x1b[1;<mods>[:<event>]<final>` modified keys.
    if let Some(body) = body.strip_prefix("1;") {
        for suffix_char in ['A', 'B', 'C', 'D', 'H', 'F', 'P', 'Q', 'R', 'S'] {
            if let Some(modifier_and_event) = body.strip_suffix(suffix_char) {
                let (modifier_part, event_type) = split_modifier_and_event(modifier_and_event);
                let code = match suffix_char {
                    'A' => Key::ArrowUp,
                    'B' => Key::ArrowDown,
                    'C' => Key::ArrowRight,
                    'D' => Key::ArrowLeft,
                    'H' => Key::Home,
                    'F' => Key::End,
                    'P' => Key::F(1),
                    'Q' => Key::F(2),
                    'R' => Key::F(3),
                    'S' => Key::F(4),
                    _ => unreachable!(),
                };
                return Some(tk(
                    code,
                    strict_modifier(modifier_part)?,
                    kitty_event_type(event_type)?,
                ));
            }
        }
    }

    let tilde_body = body.strip_suffix('~')?;
    let mut parts = tilde_body.splitn(2, ';');
    let code_part = parts.next()?;
    let code = match code_part {
        "2" => Key::Insert,
        "3" => Key::Delete,
        "5" => Key::PageUp,
        "6" => Key::PageDown,
        "11" => Key::F(1),
        "12" => Key::F(2),
        "13" => Key::F(3),
        "14" => Key::F(4),
        "15" => Key::F(5),
        "17" => Key::F(6),
        "18" => Key::F(7),
        "19" => Key::F(8),
        "20" => Key::F(9),
        "21" => Key::F(10),
        "23" => Key::F(11),
        "24" => Key::F(12),
        _ => return None,
    };
    let (modifier_part, event_type) = split_modifier_and_event(parts.next().unwrap_or("1"));
    Some(tk(
        code,
        strict_modifier(modifier_part)?,
        kitty_event_type(event_type)?,
    ))
}

/// `;mods` on a modified special key: strict `1 + bitmask` — `0` and
/// values over a byte's range reject like the reference `parse::<u8>
/// .checked_sub(1)`.
pub(super) fn strict_modifier(value: &str) -> Option<Mods> {
    let modifier = value.parse::<u8>().ok()?.checked_sub(1)?;
    Some(key_modifiers_from_u8(modifier))
}

/// `;mods[:event]` → modifiers + event type.
pub(super) fn split_modifier_and_event(input: &str) -> (&str, Option<&str>) {
    match input.split_once(':') {
        Some((modifier, event)) if !modifier.is_empty() => (modifier, Some(event)),
        _ => (input, None),
    }
}

pub(super) fn kitty_event_type(value: Option<&str>) -> Option<Action> {
    match value.unwrap_or("1") {
        "1" => Some(Action::Press),
        "2" => Some(Action::Repeat),
        "3" => Some(Action::Release),
        _ => None,
    }
}

/// `CSI <cp>[:shifted[:base]] ; <mods>[:ev] ; [text] u`.
pub(super) fn parse_kitty_key_sequence(data: &str) -> Option<TerminalKey> {
    let body = data.strip_prefix("\x1b[")?.strip_suffix('u')?;

    let mut fields = body.split(';');
    let key_part = fields.next()?;
    let modifier_part = fields
        .next()
        .filter(|field| !field.is_empty())
        .unwrap_or("1");
    let associated_text = fields.next();
    if fields.next().is_some() {
        return None;
    }

    let (modifier_text, event_type) = split_modifier_and_event(modifier_part);
    let modifier = modifier_text.parse::<u8>().ok()?.checked_sub(1)?;

    let mut key_fields = key_part.split(':');
    let codepoint: u32 = key_fields.next()?.parse().ok()?;
    let mut shifted_codepoint = key_fields
        .next()
        .filter(|field| !field.is_empty())
        .and_then(|field| field.parse::<u32>().ok());
    let base_layout_codepoint = key_fields
        .next()
        .filter(|field| !field.is_empty())
        .and_then(|field| field.parse::<u32>().ok());

    let mut code = kitty_codepoint_to_key(codepoint)?;
    // Control-code text only ever matches Enter/Backspace/Tab/Esc, never a
    // character key, so it is checked against the reported key.
    let associated_text = match associated_text {
        Some(value) => match parse_kitty_associated_text(value) {
            Some(text) => Some(text),
            None if matching_control_associated_text(value, code) => None,
            None => return None,
        },
        None => None,
    };
    // Ctrl chords on a non-Latin layout are shortcuts for the physical
    // key the host names in the base-layout field (Ctrl+C). Only what
    // Ghostty maps is rewritten: Ctrl alone on a control-byte key.
    let mut layout_key = None;
    if let (Key::Char(ch), Some(base)) = (code, base_layout_codepoint.and_then(char::from_u32)) {
        let shifted = shifted_codepoint.is_some_and(|shifted| shifted != codepoint);
        if key_modifiers_from_u8(modifier) == Mods::CTRL
            && !shifted
            && is_non_latin_script(ch)
            && ghostty_maps_ctrl_to_physical_key(base)
            && associated_text.is_none()
        {
            layout_key = Some(ch);
            code = Key::Char(base);
            // The shifted alternate belongs to the layout character.
            shifted_codepoint = None;
        }
    }
    let action = kitty_event_type(event_type)?;
    let mut modifiers = key_modifiers_from_u8(modifier);
    // Kitty permits the shifted alternate only while Shift is active.
    if matches!(code, Key::Char(_))
        && shifted_codepoint
            .is_some_and(|shifted| shifted != codepoint && char::from_u32(shifted).is_some())
    {
        modifiers = modifiers.union(Mods::SHIFT);
    }

    let mut key = tk(code, modifiers, action);
    key.layout_key = layout_key;
    key.shifted_codepoint = shifted_codepoint.and_then(char::from_u32);
    key.text_commit = if action == Action::Release {
        None
    } else {
        associated_text
    };
    Some(key)
}

/// Physical keys Ghostty sends a control byte for when Ctrl is held on
/// a non-Latin layout. Ctrl+I/M/[ are left out on purpose (fixterms).
pub(super) fn ghostty_maps_ctrl_to_physical_key(base: char) -> bool {
    matches!(base, 'a'..='z' | '0'..='9' | '/' | '\\' | ']') && !matches!(base, 'i' | 'm')
}

/// Scripts whose layouts put their own letters on the US letter keys.
pub(super) fn is_non_latin_script(ch: char) -> bool {
    matches!(
        u32::from(ch),
        0x0370..=0x03FF | 0x0400..=0x052F | 0x0530..=0x058F | 0x0590..=0x05FF
            | 0x0600..=0x06FF | 0x0700..=0x07FF | 0x08A0..=0x08FF | 0x0900..=0x0DFF
            | 0x0E00..=0x0EFF | 0x0F00..=0x0FFF | 0x1000..=0x109F | 0x10A0..=0x10FF
            | 0x1100..=0x11FF | 0x1200..=0x18FF | 0x1C80..=0x1C8F | 0x1C90..=0x1CBF
            | 0x1F00..=0x1FFF | 0x2DE0..=0x2DFF | 0x3040..=0x30FF | 0x3100..=0x318F
            | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xA640..=0xA69F | 0xAB70..=0xABBF
            | 0xAC00..=0xD7AF
    )
}

/// Associated text is DECIMAL Unicode scalars joined by `:`; a control
/// scalar rejects the whole field (the caller then checks the
/// matching-control exception).
pub(super) fn parse_kitty_associated_text(value: &str) -> Option<String> {
    let mut text = String::new();
    for codepoint in value.split(':') {
        let ch = char::from_u32(codepoint.parse::<u32>().ok()?)?;
        if ch.is_control() {
            return None;
        }
        text.push(ch);
    }
    (!text.is_empty()).then_some(text)
}

/// WezTerm attaches the matching legacy control code in report-all mode
/// even though the spec forbids it: keep the key, drop the field.
pub(super) fn matching_control_associated_text(value: &str, code: Key) -> bool {
    matches!(
        (code, value),
        (Key::Enter, "13") | (Key::Backspace, "8") | (Key::Tab, "9") | (Key::Escape, "27")
    )
}

/// The modifier parameter decoded as bits (already minus 1).
pub(super) fn key_modifiers_from_u8(modifier: u8) -> Mods {
    let mut mods = Mods::NONE;
    if modifier & 0b1 != 0 {
        mods = mods.union(Mods::SHIFT);
    }
    if modifier & 0b10 != 0 {
        mods = mods.union(Mods::ALT);
    }
    if modifier & 0b100 != 0 {
        mods = mods.union(Mods::CTRL);
    }
    if modifier & 0b1000 != 0 {
        mods = mods.union(Mods::SUPER);
    }
    if modifier & 0b1_0000 != 0 {
        mods = mods.union(Mods::HYPER);
    }
    if modifier & 0b10_0000 != 0 {
        mods = mods.union(Mods::META);
    }
    mods
}

/// Kitty codepoint → `Key` — the reference `kitty_codepoint_to_keycode`
/// table (modifier/lock keys decode to nothing for this UI).
pub(super) fn kitty_codepoint_to_key(cp: u32) -> Option<Key> {
    Some(match cp {
        8 | 127 => Key::Backspace,
        9 => Key::Tab,
        13 | 57414 => Key::Enter,
        27 => Key::Escape,
        32 => Key::Char(' '),
        57364..=57375 => Key::F((cp - 57364 + 1) as u8),
        57376..=57398 => Key::F((cp - 57376 + 13) as u8),
        57399..=57408 => Key::Char(char::from_u32('0' as u32 + (cp - 57399)).unwrap()),
        57409 => Key::Char('.'),
        57410 => Key::Char('/'),
        57411 => Key::Char('*'),
        57412 => Key::Char('-'),
        57413 => Key::Char('+'),
        57415 => Key::Char('='),
        57416 => Key::Char(','),
        57417 => Key::ArrowLeft,
        57418 => Key::ArrowRight,
        57419 => Key::ArrowUp,
        57420 => Key::ArrowDown,
        57421 => Key::PageUp,
        57422 => Key::PageDown,
        57423 => Key::Home,
        57424 => Key::End,
        57425 => Key::Insert,
        57426 => Key::Delete,
        57358..=57363 | 57427..=57440 => Key::Functional(cp),
        57441..=57454 => return None,
        value => return char::from_u32(value).map(Key::Char),
    })
}
