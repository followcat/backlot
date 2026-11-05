// Derived from herdr src/pane/input.rs (Apache-2.0).
//! Builds the libghostty key event the Ghostty app would build from an
//! OS key event: physical key, modifiers, the layout text, the unshifted
//! codepoint and Shift marked consumed when it produced the text.
//! Hyper/Meta have no Ghostty equivalent and are dropped, not typed as
//! bare keys; legacy panes keep the classic shell exceptions.

use hc_vt::{Key, Mods, VtKeyEvent};

use crate::app::input::TerminalKey;

/// `ghostty_key_event_from_terminal_key`: TerminalKey → encoder event.
/// Returns `None` for chords the encoder cannot represent (Hyper/Meta).
pub(super) fn key_event_from_terminal_key(
    key: &TerminalKey,
    legacy_pane: bool,
) -> Option<VtKeyEvent> {
    if key.input.mods.intersects(Mods::HYPER.union(Mods::META)) {
        return None;
    }
    let mut event = VtKeyEvent {
        action: key.input.action,
        mods: key.input.mods,
        ..VtKeyEvent::default()
    };
    let mut mods = key.input.mods;
    match key.input.key {
        Key::Char(c) => {
            // An uppercase letter's key is its lowercase letter on every
            // layout. Nothing else is inferred: Kitty reports already
            // name the unshifted key.
            let base = unshifted_letter(c);
            if base != c {
                mods = mods.union(Mods::SHIFT);
            }
            event.key = Key::Char(base);
            event.unshifted_codepoint = Some(base as u32);
            let shifted = mods.contains(Mods::SHIFT);
            if let Some(text) = key_text(key, base, shifted) {
                // Text the host reported with Shift held already includes
                // Shift (Shift+7 = "/" on a German layout).
                let reported_shifted_text = key.text_commit.is_some() && text != " ";
                // A non-letter given with Shift and no reported shifted
                // character is the produced character itself.
                let given_shifted_char =
                    key.shifted_codepoint.is_none() && !base.is_alphabetic() && text != " ";
                if shifted
                    && (!text.starts_with(base) || reported_shifted_text || given_shifted_char)
                {
                    event.consumed_mods = Mods::SHIFT;
                }
                // Legacy Alt prefixes the produced text: Alt+Shift+ö is
                // ESC Ö — the text is the key's real output.
                if legacy_pane
                    && mods.contains(Mods::ALT)
                    && !mods.contains(Mods::CTRL)
                    && let Some(produced) = single_char(&text).filter(|c| !c.is_ascii())
                {
                    event.unshifted_codepoint = Some(produced as u32);
                }
                event.utf8 = Some(text);
            }
        }
        Key::BackTab => {
            // Ghostty represents backtab as Tab with Shift.
            mods = mods.union(Mods::SHIFT);
            event.key = Key::Tab;
        }
        code => event.key = code,
    }
    event.mods = mods;
    Some(event)
}

/// `legacy_shell_key`: panes that negotiated no keyboard protocol keep
/// the classic chords — libghostty still encodes the result.
pub(super) fn legacy_shell_key(key: &TerminalKey) -> TerminalKey {
    let alt = key.input.mods.intersection(Mods::ALT);
    let chord = key.input.mods.difference(Mods::ALT);
    let classic = match (key.input.key, chord) {
        (Key::Enter, chord) if !chord.is_empty() => Key::Enter,
        (Key::Char('m'), Mods::CTRL) => Key::Enter,
        (Key::Char('i'), Mods::CTRL) | (Key::Tab, Mods::CTRL) => Key::Tab,
        (Key::Char('['), Mods::CTRL) => Key::Escape,
        // Ghostty reports Ctrl+Shift+letter as CSI u; every terminal
        // sends the plain control byte. Ctrl+I/M then hit the rows above.
        (Key::Char(c), chord)
            if chord.difference(Mods::SHIFT) == Mods::CTRL
                && c.is_ascii_alphabetic()
                && (chord.contains(Mods::SHIFT) || c.is_ascii_uppercase()) =>
        {
            let mut lowered = TerminalKey::new(hc_vt::KeyInput {
                key: Key::Char(c.to_ascii_lowercase()),
                mods: Mods::CTRL.union(alt),
                action: key.input.action,
            });
            lowered.vt_bytes = key.vt_bytes.clone();
            return legacy_shell_key(&lowered);
        }
        _ => return key.clone(),
    };
    let mut rewritten = TerminalKey::new(hc_vt::KeyInput {
        key: classic,
        mods: alt,
        action: key.input.action,
    });
    rewritten.vt_bytes = key.vt_bytes.clone();
    rewritten
}

/// `legacy_super_chord`: legacy encoding has no Super — these chords are
/// not forwarded (typing the bare key turns Cmd+C into "c").
pub(super) fn legacy_super_chord(key: &TerminalKey) -> bool {
    key.input.mods.contains(Mods::SUPER)
}

fn single_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

/// Text the key produced on the user's layout, before Ctrl/Alt
/// transformations — the reference `key_text`.
fn key_text(key: &TerminalKey, base: char, shifted: bool) -> Option<String> {
    if let Some(text) = key
        .text_commit
        .as_ref()
        .filter(|text| !text.is_empty() && !text.chars().any(char::is_control))
    {
        return Some(text.clone());
    }
    // Without a reported shifted character, only a letter's Shift is known.
    let produced = match key.shifted_codepoint {
        Some(shifted_char) if shifted => shifted_char,
        _ if shifted => shifted_letter(base),
        _ => base,
    };
    (!produced.is_control()).then(|| produced.to_string())
}

fn shifted_letter(c: char) -> char {
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(upper), None) if c.is_lowercase() => upper,
        _ => c,
    }
}

fn unshifted_letter(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(lower), None) if c.is_uppercase() => lower,
        _ => c,
    }
}

/// Encode accepted pending input against this exact attach's live modes.
pub(super) fn encode_pending(vt: &hc_vt::Vt, item: &super::Pending) -> Result<Vec<u8>, String> {
    use super::Pending;
    match item {
        Pending::Control(_) => Err("Viewer control was not routed through its FIFO".into()),
        Pending::Key(k) => {
            let legacy_pane = vt.keyboard_negotiated_nothing();
            if legacy_pane && legacy_super_chord(k) {
                Ok(Vec::new()) // legacy has no Super: no bytes
            } else {
                let k = if legacy_pane {
                    legacy_shell_key(k)
                } else {
                    k.clone()
                };
                match key_event_from_terminal_key(&k, legacy_pane) {
                    Some(event) => vt.key_event(&event).map_err(|e| e.to_string()),
                    None => Ok(Vec::new()), // Hyper/Meta: not sendable
                }
            }
        }
        Pending::Paste(t) => vt.paste(t).map_err(|e| e.to_string()),
        Pending::Mouse(_, mouse) => vt.mouse(*mouse).map_err(|e| e.to_string()),
        Pending::Encoded(b, ..) | Pending::Prepared(b, _) => Ok(b.clone()),
    }
}
