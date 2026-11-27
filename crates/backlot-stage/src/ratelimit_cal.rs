//! Calendar arithmetic and local wall-clock reset formatting for rate limits.

pub(crate) const DAY_MS: u64 = 86_400_000;

#[derive(Clone, Copy)]
pub(crate) struct ClockSpec {
    pub hour24: u64,
    pub minute: u64,
    pub is_utc: bool,
}

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

pub(crate) fn next_utc_clock_delta(hh: u64, mm: u64, now_ms: u64) -> Option<u64> {
    let target = hh * 3_600_000 + mm * 60_000;
    Some((target + DAY_MS - now_ms % DAY_MS) % DAY_MS)
}

pub(crate) fn next_local_clock_delta(hh: u64, mm: u64, now_ms: u64) -> Option<u64> {
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

pub(crate) fn next_weekday_delta(target_wday: i32, clock: ClockSpec, now_ms: u64) -> Option<u64> {
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

pub(crate) fn next_month_day_delta(
    month: u32,
    day: u32,
    clock: ClockSpec,
    now_ms: u64,
) -> Option<u64> {
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
