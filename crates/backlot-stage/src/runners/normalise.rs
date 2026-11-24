//! Per-runner screen normalisation (D4): strips animated spinner
//! glyphs, elapsed timers, `Thought for Ns, N tokens` headers, token
//! counters, and status-line clocks before comparing screens for stall
//! detection. The send gate keeps using the raw screen.

use super::normalise_strip::{
    clean_separators, leading_ws, match_duration_at, match_token_count_at, strip_clocks,
    strip_durations, strip_token_counters,
};
use super::work;

/// Normalise `screen` for `runner` by stripping its animated parts and
/// collapsing blank/whitespace noise so a spinner-only redraw compares
/// equal across samples.
pub(crate) fn normalise_screen(runner: &str, screen: &str) -> String {
    let lines: Vec<&str> = screen.lines().collect();
    let non_blank_total = lines.iter().filter(|l| !l.trim().is_empty()).count();
    let mut non_blank_seen = 0usize;
    let mut out = Vec::new();
    for raw in lines {
        let trimmed = raw.trim();
        let in_bottom = if !trimmed.is_empty() {
            non_blank_seen += 1;
            non_blank_total.saturating_sub(non_blank_seen) < 4
        } else {
            false
        };
        let norm = normalise_line(runner, raw, in_bottom);
        if !norm.is_empty() {
            out.push(norm);
        }
    }
    out.join("\n")
}

fn normalise_line(runner: &str, line: &str, in_bottom: bool) -> String {
    let status = in_bottom || is_status_line(line);
    match runner {
        "claude" => {
            if work::claude_spinner_line(line) {
                return if line.contains("esc to interrupt") {
                    "<spinner> esc to interrupt".into()
                } else {
                    "<spinner>".into()
                };
            }
            strip_common(line, status, true)
        }
        "cmd" => {
            if line.contains("esc to interrupt") {
                return "esc to interrupt".into();
            }
            strip_common(line, status, true)
        }
        "cursor" | "agy" | "cline" => strip_common(line, status, false),
        _ => strip_common(line, status, true),
    }
}

fn is_status_line(line: &str) -> bool {
    let l = line.trim();
    l.contains("esc to")
        || l.contains("esc interrupt")
        || l.contains("ctrl+c")
        || l.contains("task(s)")
        || l.contains("accept-edits")
        || l.contains("tokens")
        || l.contains("done ")
        || l.starts_with("● [")
}

fn strip_common(line: &str, status: bool, claude_glyphs: bool) -> String {
    let mut s = strip_braille(line);
    if claude_glyphs {
        s = strip_spinner_glyphs(&s);
    }
    s = strip_thought_phrases(&s);
    s = strip_clocks(&s, status);
    s = strip_token_counters(&s);
    s = strip_durations(&s);
    clean_separators(&s)
}

/// Remove all Unicode Braille Patterns (`U+2800..=U+28FF`).
fn strip_braille(s: &str) -> String {
    s.chars()
        .filter(|c| !('\u{2800}'..='\u{28ff}').contains(c))
        .collect()
}

/// Remove Claude/cmd spinner glyphs: `✻✶✳✢✽` and `◇◆` anywhere, and
/// leading `·` or `*` when it begins a trimmed line followed by space
/// or end-of-line.
fn strip_spinner_glyphs(s: &str) -> String {
    let trimmed = s.trim_start();
    let after_lead = match trimmed.chars().next() {
        Some('·' | '*') if trimmed.chars().nth(1).is_none_or(char::is_whitespace) => {
            &trimmed[trimmed.chars().next().map_or(0, char::len_utf8)..]
        }
        _ => s,
    };
    after_lead
        .chars()
        .filter(|c| !matches!(c, '✻' | '✶' | '✳' | '✢' | '✽' | '◇' | '◆'))
        .collect()
}

/// Strip `for <dur>(, <N> tokens)?` on `Thought for …` / `Worked for …`
/// / `Churned for …` summary headers.
fn strip_thought_phrases(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while !rest.is_empty() {
        let matched = ["Thought for ", "Worked for ", "Churned for "]
            .iter()
            .find_map(|kw| rest.find(kw).map(|pos| (pos, *kw)));
        let Some((pos, kw)) = matched else {
            out.push_str(rest);
            break;
        };
        let prefix_end = pos + kw.len();
        let after = &rest[prefix_end..];
        if let Some(consumed) = match_thought_tail(after) {
            out.push_str(&rest[..pos + kw.len() - " for ".len()]);
            rest = &after[consumed..];
        } else {
            out.push_str(&rest[..prefix_end]);
            rest = after;
        }
    }
    out
}

fn match_thought_tail(after: &str) -> Option<usize> {
    let mut idx = 0;
    let mut any_dur = false;
    loop {
        let ws = leading_ws(&after[idx..]);
        if any_dur && ws == 0 {
            break;
        }
        let d_len = match_duration_at(&after[idx + ws..])?;
        idx += ws + d_len;
        any_dur = true;
        let next_ws = leading_ws(&after[idx..]);
        if next_ws == 0 || match_duration_at(&after[idx + next_ws..]).is_none() {
            break;
        }
    }
    if !any_dur {
        return None;
    }
    let rem = &after[idx..];
    if let Some(after_comma) = rem.strip_prefix(',') {
        let ws = leading_ws(after_comma);
        if let Some(tok_len) = match_token_count_at(&after_comma[ws..]) {
            idx += 1 + ws + tok_len;
        }
    }
    Some(idx)
}
