//! Reset-time parsing and local display formatting for rate-limit banners.

/// Hours threshold above which a parsed reset classifies as a long quota window.
pub(crate) const LONG_WINDOW_MS: u64 = 12 * 3_600_000;

const DAY_MS: u64 = 86_400_000;

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

/// Format an absolute reset epoch ms for terminal/attention display: `HH:MM` within
/// 24 h, or `Mon HH:MM` (local day + clock) when more than 24 h away.
pub(crate) fn format_reset_local(reset_at_ms: u64, now_ms: u64) -> String {
    let timestamp = (reset_at_ms / 1000).min(i64::MAX as u64) as libc::time_t;
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&timestamp, &mut local) }.is_null() {
        return "the reset".into();
    }
    if reset_at_ms.saturating_sub(now_ms) > DAY_MS {
        const WDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        if let Some(wday) = usize::try_from(local.tm_wday)
            .ok()
            .and_then(|i| WDAYS.get(i))
        {
            return format!("{wday} {:02}:{:02}", local.tm_hour, local.tm_min);
        }
    }
    format!("{:02}:{:02}", local.tm_hour, local.tm_min)
}

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

#[derive(Clone, Copy)]
struct ClockSpec {
    hour24: u64,
    minute: u64,
    is_utc: bool,
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

fn next_utc_clock_delta(hh: u64, mm: u64, now_ms: u64) -> Option<u64> {
    let target = hh * 3_600_000 + mm * 60_000;
    Some((target + DAY_MS - now_ms % DAY_MS) % DAY_MS)
}

fn next_local_clock_delta(hh: u64, mm: u64, now_ms: u64) -> Option<u64> {
    let timestamp = (now_ms / 1000).min(i64::MAX as u64) as libc::time_t;
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&timestamp, &mut local) }.is_null() {
        return None;
    }
    let now = (local.tm_hour as u64) * 3_600_000
        + (local.tm_min as u64) * 60_000
        + (local.tm_sec as u64) * 1000;
    let target = hh * 3_600_000 + mm * 60_000;
    Some((target + DAY_MS - now) % DAY_MS)
}

fn next_weekday_delta(target_wday: i32, clock: ClockSpec, now_ms: u64) -> Option<u64> {
    if clock.is_utc {
        let days = (now_ms / DAY_MS) as i64;
        let cur_wday = (days + 4).rem_euclid(7) as i32;
        let mut ahead = (target_wday - cur_wday).rem_euclid(7) as i64;
        let mut target_ms =
            ((days + ahead) as u64) * DAY_MS + clock.hour24 * 3_600_000 + clock.minute * 60_000;
        if target_ms < now_ms {
            ahead += 7;
            target_ms =
                ((days + ahead) as u64) * DAY_MS + clock.hour24 * 3_600_000 + clock.minute * 60_000;
        }
        return Some(target_ms.saturating_sub(now_ms));
    }
    let now_sec = (now_ms / 1000).min(i64::MAX as u64) as libc::time_t;
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&now_sec, &mut local) }.is_null() {
        return None;
    }
    let ahead = (target_wday - local.tm_wday).rem_euclid(7);
    for extra_days in [0, 7] {
        if let Some(target_sec) = mktime_local(
            local.tm_year,
            local.tm_mon,
            local.tm_mday + ahead + extra_days,
            clock.hour24 as i32,
            clock.minute as i32,
        ) && target_sec >= now_sec
        {
            return Some((target_sec - now_sec) as u64 * 1000);
        }
    }
    None
}

fn next_month_day_delta(month: u32, day: u32, clock: ClockSpec, now_ms: u64) -> Option<u64> {
    if clock.is_utc {
        let now_days = (now_ms / DAY_MS) as i64;
        let (now_year, _, _) = civil_from_days(now_days);
        for year in now_year..=now_year + 8 {
            if day > days_in_month(year, month) {
                continue;
            }
            let days = days_from_civil(year, month, day);
            if days < 0 {
                continue;
            }
            let target_ms =
                (days as u64) * DAY_MS + clock.hour24 * 3_600_000 + clock.minute * 60_000;
            if target_ms >= now_ms {
                return Some(target_ms.saturating_sub(now_ms));
            }
        }
        return None;
    }
    let now_sec = (now_ms / 1000).min(i64::MAX as u64) as libc::time_t;
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&now_sec, &mut local) }.is_null() {
        return None;
    }
    let mon0 = (month - 1) as i32;
    let mday = day as i32;
    for year in local.tm_year..=local.tm_year + 8 {
        if let Some(target_sec) =
            mktime_exact_month_day(year, mon0, mday, clock.hour24 as i32, clock.minute as i32)
            && target_sec >= now_sec
        {
            return Some((target_sec - now_sec) as u64 * 1000);
        }
    }
    None
}

fn mktime_local(
    tm_year: i32,
    tm_mon: i32,
    tm_mday: i32,
    tm_hour: i32,
    tm_min: i32,
) -> Option<libc::time_t> {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = tm_year;
    tm.tm_mon = tm_mon;
    tm.tm_mday = tm_mday;
    tm.tm_hour = tm_hour;
    tm.tm_min = tm_min;
    tm.tm_sec = 0;
    tm.tm_isdst = -1;
    let sec = unsafe { libc::mktime(&mut tm) };
    (sec >= 0).then_some(sec)
}

fn mktime_exact_month_day(
    tm_year: i32,
    tm_mon: i32,
    tm_mday: i32,
    tm_hour: i32,
    tm_min: i32,
) -> Option<libc::time_t> {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = tm_year;
    tm.tm_mon = tm_mon;
    tm.tm_mday = tm_mday;
    tm.tm_hour = tm_hour;
    tm.tm_min = tm_min;
    tm.tm_sec = 0;
    tm.tm_isdst = -1;
    let sec = unsafe { libc::mktime(&mut tm) };
    (sec >= 0 && tm.tm_mon == tm_mon && tm.tm_mday == tm_mday && tm.tm_year == tm_year)
        .then_some(sec)
}

fn is_leap_year(y: i64) -> bool {
    y.rem_euclid(4) == 0 && (y.rem_euclid(100) != 0 || y.rem_euclid(400) == 0)
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(y) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Howard Hinnant civil-from-days algorithm (UTC, `1970-01-01` -> `0`).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = y - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400) as u64;
    let mp = u64::from(m) + if m > 2 { 0 } else { 12 } - 3;
    let doy = (153 * mp + 2) / 5 + u64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + (doe as i64) - 719_468
}

/// Howard Hinnant days-from-civil inverse.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (y + i64::from(m <= 2), m, d)
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
