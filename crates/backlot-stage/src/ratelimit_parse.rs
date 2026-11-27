//! Reset-time parsing and local display formatting for rate-limit banners.

#[path = "ratelimit_cal.rs"]
mod cal;

pub(crate) use cal::format_reset_local;
use cal::{
    ClockSpec, DAY_MS, next_local_clock_delta, next_month_day_delta, next_utc_clock_delta,
    next_weekday_delta,
};

/// Hours threshold above which a parsed reset classifies as a long quota window.
pub(crate) const LONG_WINDOW_MS: u64 = 12 * 3_600_000;

const WEEKDAY_NAMES: &[(&str, i32)] = &[
    ("sunday", 0),
    ("sun", 0),
    ("monday", 1),
    ("mon", 1),
    ("tuesday", 2),
    ("tues", 2),
    ("tue", 2),
    ("wednesday", 3),
    ("wed", 3),
    ("thursday", 4),
    ("thurs", 4),
    ("thur", 4),
    ("thu", 4),
    ("friday", 5),
    ("fri", 5),
    ("saturday", 6),
    ("sat", 6),
];

const MONTH_NAMES: &[(&str, u32)] = &[
    ("january", 1),
    ("jan", 1),
    ("february", 2),
    ("feb", 2),
    ("march", 3),
    ("mar", 3),
    ("april", 4),
    ("apr", 4),
    ("may", 5),
    ("june", 6),
    ("jun", 6),
    ("july", 7),
    ("jul", 7),
    ("august", 8),
    ("aug", 8),
    ("september", 9),
    ("sept", 9),
    ("sep", 9),
    ("october", 10),
    ("oct", 10),
    ("november", 11),
    ("nov", 11),
    ("december", 12),
    ("dec", 12),
];

/// Unclamped wait duration (ms from `now_ms`) parsed from the freshest reset
/// clause: explicit calendar date+time first, then weekday+time, then `at HH:MM UTC`,
/// then local `am`/`pm` reset clocks, and finally relative durations (`in 5m`).
pub(crate) fn parse_wait(text: &str, now_ms: u64) -> Option<u64> {
    parse_month_day_time(text, now_ms)
        .or_else(|| parse_weekday_time(text, now_ms))
        .or_else(|| parse_at(text, now_ms))
        .or_else(|| parse_local_ampm(text, now_ms))
        .or_else(|| parse_in(text))
}

fn word_boundary_before(s: &str, byte_idx: usize) -> bool {
    s[..byte_idx]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_ascii_alphanumeric())
}

fn word_boundary_after(s: &str) -> bool {
    s.chars().next().is_none_or(|c| !c.is_ascii_alphanumeric())
}

/// Parse `<H>[:<MM>][ ]?(am|pm)[ utc]` or `<HH>:<MM> utc` at the start of `s`.
/// Rejects bare `HH:MM` without `am`/`pm` or `utc` unless the caller knows it's UTC.
fn parse_explicit_clock(s: &str) -> Option<(ClockSpec, &str)> {
    let s = s.trim_start();
    let (hh, rest) = front_digits(s, 1, 2)?;
    let (mm, rest, has_colon) = if let Some(after_colon) = rest.strip_prefix(':') {
        let (mm, rest) = front_digits(after_colon, 2, 2)?;
        if mm > 59 {
            return None;
        }
        (mm, rest, true)
    } else {
        (0, rest, false)
    };
    let after_num = rest.trim_start();
    if let Some(pm_flag) = after_num
        .strip_prefix("pm")
        .map(|r| (true, r))
        .or_else(|| after_num.strip_prefix("am").map(|r| (false, r)))
    {
        let (pm, after_meridiem) = pm_flag;
        if !(1..=12).contains(&hh) || !word_boundary_after(after_meridiem) {
            return None;
        }
        let hour24 = (hh % 12) + u64::from(pm) * 12;
        let (is_utc, tail) = take_optional_utc(after_meridiem);
        return Some((
            ClockSpec {
                hour24,
                minute: mm,
                is_utc,
            },
            tail,
        ));
    }
    if has_colon && hh <= 23 {
        let (is_utc, tail) = take_optional_utc(rest);
        if is_utc {
            return Some((
                ClockSpec {
                    hour24: hh,
                    minute: mm,
                    is_utc: true,
                },
                tail,
            ));
        }
    }
    None
}

fn take_optional_utc(s: &str) -> (bool, &str) {
    let trimmed = s.trim_start();
    if let Some(after) = trimmed.strip_prefix("utc")
        && word_boundary_after(after)
    {
        return (true, after);
    }
    (false, s)
}

fn strip_separator_and_at(mut s: &str) -> &str {
    s = s.trim_start();
    if let Some(after_comma) = s.strip_prefix(',') {
        s = after_comma.trim_start();
    }
    if let Some(after_at) = s.strip_prefix("at")
        && word_boundary_after(after_at)
    {
        s = after_at.trim_start();
    }
    s
}

/// Match `<month> <day>[,| at]? <clock> [utc]`, e.g. `nov 21, 4pm` or `nov 30 at 12:00 utc`.
fn parse_month_day_time(text: &str, now_ms: u64) -> Option<u64> {
    let lower = text.to_lowercase();
    for (idx, _) in lower.char_indices() {
        if !word_boundary_before(&lower, idx) {
            continue;
        }
        let slice = &lower[idx..];
        let Some((month, after_month)) = match_prefix_table(slice, MONTH_NAMES) else {
            continue;
        };
        if !after_month.starts_with(char::is_whitespace) {
            continue;
        }
        let after_ws = after_month.trim_start();
        let Some((day, after_day)) = front_digits(after_ws, 1, 2) else {
            continue;
        };
        if !(1..=31).contains(&day) || !word_boundary_after(after_day) {
            continue;
        }
        let clock_tail = strip_separator_and_at(after_day);
        if clock_tail.len() == after_day.len() && !after_day.starts_with(',') {
            continue;
        }
        let Some((clock, _)) = parse_explicit_clock(clock_tail) else {
            continue;
        };
        if let Some(delta) = next_month_day_delta(month, day as u32, clock, now_ms) {
            return Some(delta);
        }
    }
    None
}

/// Match `<weekday>[,| at]? <clock> [utc]`, e.g. `mon 9am` or `monday at 9:00 am`.
fn parse_weekday_time(text: &str, now_ms: u64) -> Option<u64> {
    let lower = text.to_lowercase();
    for (idx, _) in lower.char_indices() {
        if !word_boundary_before(&lower, idx) {
            continue;
        }
        let slice = &lower[idx..];
        let Some((wday, after_wday)) = match_prefix_table(slice, WEEKDAY_NAMES) else {
            continue;
        };
        if !word_boundary_after(after_wday) {
            continue;
        }
        let clock_tail = strip_separator_and_at(after_wday);
        if clock_tail.len() == after_wday.len() {
            continue;
        }
        let Some((clock, _)) = parse_explicit_clock(clock_tail) else {
            continue;
        };
        if let Some(delta) = next_weekday_delta(wday, clock, now_ms) {
            return Some(delta);
        }
    }
    None
}

fn match_prefix_table<'a, T: Copy>(s: &'a str, table: &[(&str, T)]) -> Option<(T, &'a str)> {
    for &(name, val) in table {
        if let Some(rest) = s.strip_prefix(name)
            && word_boundary_after(rest)
        {
            return Some((val, rest));
        }
    }
    None
}

/// Claude's local `12:10pm` / `2:10pm` / `9am` reset clock -> ms until it next strikes.
fn parse_local_ampm(text: &str, now_ms: u64) -> Option<u64> {
    let lower = text.to_lowercase();
    for needle in [
        "resets ",
        "reset at ",
        "try again at ",
        "continuing automatically at ",
    ] {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(needle) {
            let start = from + pos + needle.len();
            let tail = lower[start..].trim_start();
            if let Some((clock, _)) = parse_explicit_clock(tail) {
                let delta = if clock.is_utc {
                    next_utc_clock_delta(clock.hour24, clock.minute, now_ms)
                } else {
                    next_local_clock_delta(clock.hour24, clock.minute, now_ms)
                };
                if let Some(ms) = delta {
                    return Some(ms);
                }
            }
            from = start;
        }
    }
    None
}

/// The first `at HH:MM UTC` in the region -> ms until that clock next strikes.
fn parse_at(text: &str, now_ms: u64) -> Option<u64> {
    let mut from = 0;
    while let Some(at) = text[from..].find("at") {
        let at = from + at;
        let bounded = word_boundary_before(text, at) && word_boundary_after(&text[at + 2..]);
        if bounded && let Some(ms) = clock_wait(&text[at + 2..], now_ms) {
            return Some(ms);
        }
        from = at + 2;
    }
    None
}

fn clock_wait(s: &str, now_ms: u64) -> Option<u64> {
    let s = s.trim_start();
    let (hh, s) = front_digits(s, 1, 2)?;
    let s = s.strip_prefix(':')?;
    let (mm, s) = front_digits(s, 2, 2)?;
    let (is_utc, _) = take_optional_utc(s);
    if !is_utc || hh > 23 || mm > 59 {
        return None;
    }
    next_utc_clock_delta(hh, mm, now_ms)
}

/// `in <n> <unit>` or `after <n> <unit>` -> ms.
fn parse_in(text: &str) -> Option<u64> {
    for anchor in [" in ", " after "] {
        let mut rest = text;
        while let Some(at) = rest.find(anchor) {
            let tail = &rest[at + anchor.len()..];
            if let Some(ms) = rel_wait(tail) {
                return Some(ms);
            }
            rest = tail;
        }
    }
    None
}

fn rel_wait(tail: &str) -> Option<u64> {
    let (n, rest) = front_digits(tail, 1, 20)?;
    let word: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    let mult = match word.as_str() {
        "s" | "sec" | "secs" | "second" | "seconds" => 1_000,
        "m" | "min" | "mins" | "minute" | "minutes" => 60_000,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3_600_000,
        "d" | "day" | "days" => DAY_MS,
        _ => return None,
    };
    Some(n.saturating_mul(mult))
}

fn front_digits(s: &str, min: usize, max: usize) -> Option<(u64, &str)> {
    let width = s
        .chars()
        .take(max)
        .take_while(|c| c.is_ascii_digit())
        .count();
    if width < min {
        return None;
    }
    let (digits, rest) = s.split_at(width);
    Some((digits.parse().unwrap_or(u64::MAX), rest))
}
