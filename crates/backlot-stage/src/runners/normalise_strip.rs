//! Token, duration, clock, and separator stripping helpers shared by
//! [`super::normalise`].

/// Strip bracketed `[HH:MM(:SS)?]` always, and bare status clocks
/// `HH:MM(:SS)?( AM|PM)?` when `bare_clocks` is true.
pub(crate) fn strip_clocks(s: &str, bare_clocks: bool) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'['
            && let Some(end) = match_clock_at(&s[i + 1..], false)
            && s[i + 1 + end..].starts_with(']')
        {
            i += 1 + end + 1;
            continue;
        }
        if bare_clocks
            && is_boundary_before(s, i)
            && let Some(len) = match_clock_at(&s[i..], true)
        {
            i += len;
            continue;
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn match_clock_at(s: &str, allow_ampm: bool) -> Option<usize> {
    let (hh_len, hh) = take_digits(s, 1, 2)?;
    let rest = s.get(hh_len..)?.strip_prefix(':')?;
    let (mm_len, mm) = take_digits(rest, 2, 2)?;
    if mm_len != 2 || hh > 23 || mm > 59 {
        return None;
    }
    let mut len = hh_len + 1 + mm_len;
    if let Some(after_colon) = s[len..].strip_prefix(':') {
        let (ss_len, ss) = take_digits(after_colon, 2, 2)?;
        if ss_len != 2 || ss > 59 {
            return None;
        }
        len += 1 + ss_len;
    }
    if allow_ampm {
        let ws = leading_ws(&s[len..]);
        let tail = &s[len + ws..];
        if tail.len() >= 2 {
            let ampm = &tail[..2];
            if ampm.eq_ignore_ascii_case("am") || ampm.eq_ignore_ascii_case("pm") {
                let after = &tail[2..];
                if after
                    .chars()
                    .next()
                    .is_none_or(|c| !c.is_alphanumeric() && c != '_')
                    && (1..=12).contains(&hh)
                {
                    len += ws + 2;
                }
            }
        }
    }
    let next = s[len..].chars().next();
    if next.is_some_and(|c| c.is_ascii_alphanumeric() || c == ':' || c == '_') {
        return None;
    }
    Some(len)
}

/// Strip arrow counters (`↑ 1.2k tokens`, `↓ 219`) and `<N> tokens`
/// counters (`147 tokens`, `1,234 tokens`, `1.2k tokens`).
pub(crate) fn strip_token_counters(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let tail = &s[i..];
        if (tail.starts_with('↑') || tail.starts_with('↓'))
            && let Some(len) = match_arrow_counter_at(tail)
        {
            i += len;
            continue;
        }
        if is_boundary_before(s, i)
            && let Some(len) = match_token_count_at(tail)
        {
            i += len;
            continue;
        }
        let ch = tail.chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn match_arrow_counter_at(s: &str) -> Option<usize> {
    let arrow = s.chars().next()?;
    if !matches!(arrow, '↑' | '↓') {
        return None;
    }
    let a_len = arrow.len_utf8();
    let ws = leading_ws(&s[a_len..]);
    let num_len = match_compact_number(&s[a_len + ws..])?;
    let mut len = a_len + ws + num_len;
    let ws2 = leading_ws(&s[len..]);
    if let Some(word_len) = match_tokens_word(&s[len + ws2..]) {
        len += ws2 + word_len;
    }
    let next = s[len..].chars().next();
    if next.is_some_and(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    Some(len)
}

pub(crate) fn match_token_count_at(s: &str) -> Option<usize> {
    let num_len = match_compact_number(s)?;
    let ws = leading_ws(&s[num_len..]);
    if ws == 0 {
        return None;
    }
    let word_len = match_tokens_word(&s[num_len + ws..])?;
    let len = num_len + ws + word_len;
    let next = s[len..].chars().next();
    if next.is_some_and(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    Some(len)
}

fn match_compact_number(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.first().is_none_or(|b| !b.is_ascii_digit()) {
        return None;
    }
    let mut i = 0;
    while i < bytes.len()
        && (bytes[i].is_ascii_digit()
            || ((bytes[i] == b'.' || bytes[i] == b',')
                && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)))
    {
        i += 1;
    }
    if bytes
        .get(i)
        .is_some_and(|b| matches!(b, b'k' | b'K' | b'm' | b'M'))
    {
        i += 1;
    }
    Some(i)
}

fn match_tokens_word(s: &str) -> Option<usize> {
    if s.get(..6).is_some_and(|p| p.eq_ignore_ascii_case("tokens")) {
        return Some(6);
    }
    if s.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("token")) {
        return Some(5);
    }
    None
}

/// Strip word-bounded elapsed durations (`12s`, `1.2s`, `1m 20s`, `3m`,
/// `1 second`, `2 minutes`).
pub(crate) fn strip_durations(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if is_boundary_before(s, i)
            && let Some(len) = match_duration_at(&s[i..])
        {
            i += len;
            continue;
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

pub(crate) fn match_duration_at(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.first().is_none_or(|b| !b.is_ascii_digit()) {
        return None;
    }
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if bytes.get(i) == Some(&b'.') && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }
    let unit_len = match bytes.get(i) {
        Some(b'h' | b'm' | b's') => {
            let next = s[i + 1..].chars().next();
            if next.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                return None;
            }
            1
        }
        _ => {
            let ws = leading_ws(&s[i..]);
            if ws == 0 {
                return None;
            }
            let word: String = s[i + ws..]
                .chars()
                .take_while(|c| c.is_ascii_alphabetic())
                .collect();
            let w_len = match word.to_ascii_lowercase().as_str() {
                "s" | "sec" | "secs" | "second" | "seconds" | "m" | "min" | "mins" | "minute"
                | "minutes" | "h" | "hr" | "hrs" | "hour" | "hours" => word.len(),
                _ => return None,
            };
            let next = s[i + ws + w_len..].chars().next();
            if next.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                return None;
            }
            ws + w_len
        }
    };
    Some(i + unit_len)
}

fn take_digits(s: &str, min: usize, max: usize) -> Option<(usize, u32)> {
    let len = s
        .as_bytes()
        .iter()
        .take(max)
        .take_while(|b| b.is_ascii_digit())
        .count();
    if len < min {
        return None;
    }
    let val = s[..len].parse().ok()?;
    Some((len, val))
}

pub(crate) fn leading_ws(s: &str) -> usize {
    s.bytes().take_while(|b| b.is_ascii_whitespace()).count()
}

fn is_boundary_before(s: &str, idx: usize) -> bool {
    s[..idx]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_alphanumeric() && !matches!(c, '_' | '.' | ':'))
}

/// Collapse whitespace runs and empty separator fragments left after stripping
/// animated tokens.
pub(crate) fn clean_separators(s: &str) -> String {
    let collapsed: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut parts: Vec<&str> = Vec::new();
    for tok in collapsed.split(' ') {
        if tok.is_empty() {
            continue;
        }
        let is_sep = matches!(tok, "·" | "•" | ",");
        if is_sep {
            if let Some(&prev) = parts.last()
                && matches!(prev, "·" | "•" | "," | "(" | "[")
            {
                continue;
            }
            if parts.is_empty() {
                continue;
            }
        }
        if matches!(tok, ")" | "]") {
            while parts.last().is_some_and(|p| matches!(*p, "·" | "•" | ",")) {
                parts.pop();
            }
        }
        parts.push(tok);
    }
    while parts.last().is_some_and(|p| matches!(*p, "·" | "•" | ",")) {
        parts.pop();
    }
    let joined = parts.join(" ");
    joined
        .replace("( )", "")
        .replace("()", "")
        .replace("[ ]", "")
        .replace("[]", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
