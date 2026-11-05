// Derived from herdr src/input/parse.rs and src/raw_input.rs (Apache-2.0).
//! Event-level decoding for framed input chunks: the reference's
//! `parse_terminal_key_sequence` (legacy + kitty + xterm-modified +
//! modifyOtherKeys tables), SGR/default mouse, host OSC/CSI replies,
//! and `extract_one_event`, which classifies one complete sequence.
//! The byte framer in `raw_input` owns timing; this file is pure.

use hc_vt::{Action, Key, KeyInput, Mods, Rgb};

use crate::app::UiEvent;
use crate::app::input::TerminalKey;
use crate::host::HostReply;
use crate::raw_frames::*;

/// What one complete sequence is.
#[derive(Debug, Clone)]
pub enum Decoded {
    /// User input — keys, text commits, paste, focus, mouse.
    Ui(UiEvent),
    /// A terminal reply routed to host-facts state.
    Reply(HostReply),
}

/// Classifies the chunks the byte framer released — the reference's
/// `events_from_framed_chunks`: a lone `[ESC]` is Escape, an
/// `[ESC, intro]` pair released as a key is Alt+key, anything else
/// goes through `extract_one_event`.
pub fn events_from_framed_chunks(chunks: Vec<Vec<u8>>) -> Vec<Decoded> {
    chunks
        .into_iter()
        .filter_map(|chunk| {
            if chunk.as_slice() == [ESC] {
                return Some(Decoded::Ui(UiEvent::Key(
                    TerminalKey::new(KeyInput {
                        key: Key::Escape,
                        mods: Mods::NONE,
                        action: Action::Press,
                    })
                    .with_vt_bytes(chunk),
                )));
            }
            // The byte framer released this introducer as a key on its own.
            if is_alt_key_introducer(&chunk) {
                let text = std::str::from_utf8(&chunk).ok()?;
                return parse_terminal_key_sequence(text)
                    .map(|key| Decoded::Ui(UiEvent::Key(key.with_vt_bytes(chunk.clone()))));
            }
            extract_one_event(&chunk).map(|(event, _)| event)
        })
        .collect()
}

/// One complete sequence → one event, mirroring the reference's
/// `extract_one_event` classification order: bracketed paste, escape
/// sequences (mouse → replies → focus → scheme → cell size → SGR
/// mouse → keys → unsupported), then a single UTF-8 char key.
pub(crate) fn extract_one_event(buffer: &[u8]) -> Option<(Decoded, usize)> {
    if buffer.is_empty() {
        return None;
    }

    if buffer.starts_with(BRACKETED_PASTE_START) {
        let end = find_subsequence(buffer, BRACKETED_PASTE_END)?;
        let content = std::str::from_utf8(&buffer[BRACKETED_PASTE_START.len()..end]).ok()?;
        return Some((
            Decoded::Ui(UiEvent::Paste(content.as_bytes().to_vec())),
            end + BRACKETED_PASTE_END.len(),
        ));
    }

    if buffer[0] == ESC {
        let seq_len = complete_escape_sequence_len(buffer)?;
        if buffer[..seq_len].starts_with(b"\x1b[M") {
            let event = parse_default_mouse(&buffer[..seq_len])
                .map(|m| Decoded::Ui(UiEvent::Mouse(m)))
                .unwrap_or(Decoded::Ui(UiEvent::Unsupported));
            return Some((event, seq_len));
        }
        let seq = std::str::from_utf8(&buffer[..seq_len]).ok()?;

        if let Some(reply) = parse_default_color_response(seq) {
            return Some((Decoded::Reply(reply), seq_len));
        }
        if let Some((index, color)) = parse_palette_color_response(seq) {
            return Some((Decoded::Reply(HostReply::Palette(index, color)), seq_len));
        }

        match seq {
            "\x1b[I" => return Some((Decoded::Ui(UiEvent::Focus(true)), seq_len)),
            "\x1b[O" => return Some((Decoded::Ui(UiEvent::Focus(false)), seq_len)),
            _ => {}
        }

        if let Some(appearance) = parse_host_color_scheme_report(&buffer[..seq_len]) {
            return Some((Decoded::Reply(appearance), seq_len));
        }

        if let Some((width_px, height_px)) = parse_host_cell_size_report(&buffer[..seq_len]) {
            return Some((
                Decoded::Reply(HostReply::CellSize {
                    width_px,
                    height_px,
                }),
                seq_len,
            ));
        }

        if let Some(mouse) = parse_sgr_mouse(seq) {
            return Some((Decoded::Ui(UiEvent::Mouse(mouse)), seq_len));
        }

        if let Some(reply) = parse_host_capability_reply(seq) {
            return Some((Decoded::Reply(reply), seq_len));
        }

        if let Some(key) = parse_terminal_key_sequence(seq) {
            return Some((
                Decoded::Ui(UiEvent::Key(key.with_vt_bytes(buffer[..seq_len].to_vec()))),
                seq_len,
            ));
        }

        // Unknown escape sequence: dropped (the framer's discard
        // machinery bounds any tail).
        return Some((Decoded::Ui(UiEvent::Unsupported), seq_len));
    }

    let consumed = first_complete_utf8_char_len(buffer)?;
    let text = std::str::from_utf8(&buffer[..consumed]).ok()?;
    let key = parse_terminal_key_sequence(text)?
        .with_text_commit()
        .with_vt_bytes(buffer[..consumed].to_vec());
    Some((Decoded::Ui(UiEvent::Key(key)), consumed))
}

/// `OSC 10;?`/`OSC 11;?` replies — the reference
/// `parse_default_color_response`.
fn parse_default_color_response(seq: &str) -> Option<HostReply> {
    let body = seq
        .strip_prefix("\x1b]")?
        .strip_suffix("\x1b\\")
        .or_else(|| seq.strip_prefix("\x1b]")?.strip_suffix('\u{7}'))?;
    let (cmd, value) = body.split_once(';')?;
    let color = crate::host::parse_rgb_color(value)?;
    match cmd {
        "10" => Some(HostReply::Foreground(color)),
        "11" => Some(HostReply::Background(color)),
        _ => None,
    }
}

/// `OSC 4;idx;?` reply — the reference `parse_palette_color_response`.
fn parse_palette_color_response(seq: &str) -> Option<(u8, Rgb)> {
    let body = seq
        .strip_prefix("\x1b]")?
        .strip_suffix("\x1b\\")
        .or_else(|| seq.strip_prefix("\x1b]")?.strip_suffix('\u{7}'))?;
    let rest = body.strip_prefix("4;")?;
    let (index, value) = rest.split_once(';')?;
    let index: u8 = index.parse().ok()?;
    let color = crate::host::parse_rgb_color(value)?;
    Some((index, color))
}

fn parse_host_color_scheme_report(buffer: &[u8]) -> Option<HostReply> {
    match buffer {
        GHOSTTY_COLOR_SCHEME_DARK_REPORT => Some(HostReply::Dark),
        GHOSTTY_COLOR_SCHEME_LIGHT_REPORT => Some(HostReply::Light),
        _ => None,
    }
}

/// `CSI ? flags u` kitty-keyboard support reply and `CSI ? … c`
/// primary device attributes — the reference's
/// `consume_host_keyboard_probe_responses` flag/DA cases. The `?`
/// prefix keeps them apart from kitty key reports (`CSI num;mod u`).
fn parse_host_capability_reply(seq: &str) -> Option<HostReply> {
    let body = seq.strip_prefix("\x1b[?")?;
    match seq.as_bytes().last()? {
        b'u' => {
            let flags = body.strip_suffix('u')?;
            if flags.is_empty() || !flags.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            flags.parse::<u16>().ok().map(HostReply::KeyboardFlags)
        }
        b'c' => {
            let attrs = body.strip_suffix('c')?;
            (!attrs.is_empty() && attrs.bytes().all(|b| b.is_ascii_digit() || b == b';'))
                .then_some(HostReply::PrimaryDeviceAttributes)
        }
        // DECRPM `CSI ? 1016 ; Ps $ y` — SGR pixel mouse status:
        // 2 (recognized, reset) is the only supported answer; 1/3
        // (still set after our explicit reset) and 0/4 are all
        // `false`, matching the reference's probe mapping.
        b'y' => {
            let report = body.strip_suffix("$y")?;
            let mut fields = report.split(';');
            (fields.next()? == "1016").then_some(())?;
            let status = fields.next()?.parse::<u8>().ok()?;
            (fields.next().is_none() && status <= 4)
                .then_some(HostReply::SgrPixelMouse(status == 2))
        }
        _ => None,
    }
}

/// XTWINOPS `CSI 6 ; height ; width t` — reply orders height first.
fn parse_host_cell_size_report(buffer: &[u8]) -> Option<(u32, u32)> {
    let body = buffer.strip_prefix(b"\x1b[")?.strip_suffix(b"t")?;
    let text = std::str::from_utf8(body).ok()?;
    let mut params = text.split(';');
    if params.next()? != "6" {
        return None;
    }
    let height_px = params.next()?.parse::<u32>().ok()?;
    let width_px = params.next()?.parse::<u32>().ok()?;
    if params.next().is_some() || width_px == 0 || height_px == 0 {
        return None;
    }
    Some((width_px, height_px))
}

#[path = "raw_events_keys.rs"]
mod keys;
pub use keys::parse_terminal_key_sequence;
