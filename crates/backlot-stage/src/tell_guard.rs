//! Guard preventing studio roles from sending unflagged questions to `user`.
//!
//! When a studio role (`producer`, `director`, `writer`, `actor-<task>-c<n>`)
//! runs `backlot tell user` with text containing `?` or `？` outside code
//! spans, fenced code blocks, and URL query strings, it must choose `--ask`
//! (wait for an answer) or `--note` (explicit plain information).

use std::fs;
use std::path::Path;

use crate::cli::{Caller, Fail};

pub(crate) const REFUSAL_MSG: &str = "this reads as a question to the person: add --ask so the studio waits for their answer, or --note to send it as information";

/// Whether `role` is a studio role subject to the `tell user` question guard.
pub(crate) fn is_studio_role(role: &str) -> bool {
    matches!(role, "producer" | "director" | "writer")
        || backlot_model::state::actor_task(role).is_some()
}

/// Refuse an unflagged `tell user` from a studio CLI caller when the body looks like a question.
pub(crate) fn check_tell_user(caller: &Caller, file: &Path) -> Result<(), Fail> {
    if caller.via.as_deref() == Some("doc") || !is_studio_role(&caller.role) {
        return Ok(());
    }
    let body =
        fs::read_to_string(file).map_err(|e| Fail::refused(format!("cannot read message: {e}")))?;
    if contains_question(&body) {
        return Err(Fail::usage(REFUSAL_MSG));
    }
    Ok(())
}

/// Whether `text` contains a `?` or `？` outside fenced code blocks, inline code spans, and URL query strings.
pub(crate) fn contains_question(text: &str) -> bool {
    let mut fence: Option<(char, usize)> = None;
    let mut para = String::new();

    for line in text.lines() {
        if let Some((ch, len)) = fence {
            if is_closing_fence(line, ch, len) {
                fence = None;
            }
            continue;
        }
        if let Some((ch, len)) = opening_fence(line) {
            if !para.is_empty() {
                if paragraph_has_question(&para) {
                    return true;
                }
                para.clear();
            }
            fence = Some((ch, len));
            continue;
        }
        if line.trim().is_empty() {
            if !para.is_empty() {
                if paragraph_has_question(&para) {
                    return true;
                }
                para.clear();
            }
            continue;
        }
        if !para.is_empty() {
            para.push('\n');
        }
        para.push_str(line);
    }

    !para.is_empty() && paragraph_has_question(&para)
}

fn opening_fence(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start_matches([' ', '\t']);
    let ch = trimmed.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let len = trimmed.bytes().take_while(|&b| b as char == ch).count();
    if len < 3 {
        return None;
    }
    let info = &trimmed[len..];
    if ch == '`' && info.contains('`') {
        return None;
    }
    Some((ch, len))
}

fn is_closing_fence(line: &str, ch: char, open_len: usize) -> bool {
    let trimmed = line.trim_start_matches([' ', '\t']);
    let len = trimmed.bytes().take_while(|&b| b as char == ch).count();
    len >= open_len && trimmed[len..].trim_end().is_empty()
}

fn paragraph_has_question(para: &str) -> bool {
    let bytes = para.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_punctuation() {
            i += 2;
            continue;
        }
        if bytes[i] == b'`' {
            let run = count_backticks(bytes, i);
            if let Some(end) = find_closing_backticks(bytes, i + run, run) {
                i = end;
            } else {
                // Unmatched backtick run is literal text in CommonMark; keep scanning inside.
                i += run;
            }
            continue;
        }
        if bytes[i] == b'?' {
            if !is_url_query(&para[i + 1..]) {
                return true;
            }
            i += 1;
            continue;
        }
        let ch = para[i..].chars().next().expect("valid utf-8 boundary");
        if ch == '？' {
            return true;
        }
        i += ch.len_utf8();
    }
    false
}

fn count_backticks(bytes: &[u8], start: usize) -> usize {
    bytes[start..].iter().take_while(|&&b| b == b'`').count()
}

fn find_closing_backticks(bytes: &[u8], mut i: usize, target: usize) -> Option<usize> {
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let run = count_backticks(bytes, i);
            if run == target {
                return Some(i + run);
            }
            i += run;
        } else {
            i += 1;
        }
    }
    None
}

fn is_url_query(after_q: &str) -> bool {
    let bytes = after_q.as_bytes();
    let mut key_len = 0;
    while key_len < bytes.len() && is_url_key_byte(bytes[key_len]) {
        key_len += 1;
    }
    key_len > 0 && bytes.get(key_len) == Some(&b'=')
}

fn is_url_key_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'[' | b']' | b'%')
}
