//! The rate-limit taxonomy (S12d): limit text observed — in an
//! `error` status's message or in the fresh tail of a non-working
//! screen — makes a terminal rate limited, its own attention
//! reason, counted separately from `stalled` and rework. While
//! the limit holds the stage waits the parsed reset out, then
//! sends `continue`; three auto-retries and the episode escalates
//! to a person. An explicit weekly or monthly quota is a person's
//! at once — no `continue` shortens a multi-day limit. A bare
//! `error` with no limit text observed is [`AGENT_ERROR`], never
//! an auto-`continue` — observed, never assumed.

use backlot_model::summary::Work;

use crate::reconcile::LiveTerminal;

/// The rate-limit attention reason.
pub const RATE_LIMITED: &str = "rate limited";
/// A reporting core's `error` status with no limit text anywhere —
/// its own attention, answered by the director or the user.
pub const AGENT_ERROR: &str = "agent error";
/// Auto-retries before the episode is a person's.
pub const MAX_RETRIES: u8 = 3;
/// A `continue` goes out this long after the parsed reset — the
/// clock and the agent's need not agree by less than this.
pub const RETRY_MARGIN_MS: u64 = 5_000;
/// No reset text parsed: retry after a minute.
pub const DEFAULT_WAIT_MS: u64 = 60_000;
/// The resume text sent after a reset.
pub const CONTINUE: &str = "continue";

/// Vocabulary measured on cursor 2.x and common to the runners:
/// a limit line names the limit, a reset, or the way out.
const LIMIT_WORDS: &[&str] = &[
    "rate limit",
    "rate-limit",
    "too many requests",
    "usage limit",
    "hit your limit",
    "session limit",
    "weekly limit",
    "monthly limit",
    "upgrade your plan",
    "try again at",
    "try again in",
    "retry after",
    "resets at",
    "resets in",
    "reset at",
    "reset in",
];

/// Words naming a multi-day quota: the reset is hours or days away,
/// so the short-window retry loop would only burn its budget. The
/// episode is a person's the moment this text is observed.
const LONG_LIMIT_WORDS: &[&str] = &["weekly limit", "monthly limit"];

/// One observed limit and what the stage should do about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LimitHit {
    /// The wait the reset line asked for, in milliseconds.
    pub wait_ms: u64,
    /// The text names a weekly or monthly quota: raise it for a
    /// person at once instead of auto-`continue`ing.
    pub escalate_now: bool,
    /// The runner says it continues by itself after the reset.
    pub auto_continue: bool,
}

/// Claude's weekly-limit four-choice dialog: options 1–2 wait;
/// options 3–4 spend money. The stage must never answer it.
pub(crate) fn weekly_choice_dialog(screen: &str) -> bool {
    let lower = screen.to_lowercase();
    lower.contains("weekly limit")
        && lower.contains("upgrade")
        && (lower.contains("usage credits") || lower.contains("credits"))
        && lower.contains("wait")
}

/// Whether the terminal reports `error` — regardless of whether
/// observed limit text makes it this module's episode.
pub(crate) fn errored(term: &LiveTerminal) -> bool {
    term.status.as_ref().is_some_and(|s| s.state == "error")
}

/// Whether the terminal is rate limited now — and how long the
/// wait should be before a `continue`. Observed, never assumed:
/// a reporting core's `error` counts only when limit text shows —
/// in `status.msg`, else in the screen's fresh tail. A reporting
/// core in a non-working state (`done`, `idle`, `waiting`) is read
/// like a silent core: its screen's fresh tail is the evidence.
/// A `working` terminal is never limited — its limit words are text
/// the agent printed (fixture output, an edit to this file), not a
/// state it is in. A silent core's non-working screen is the same
/// fallback. A bare `error` is [`AGENT_ERROR`]'s attention instead.
/// `None` — not limited. `now_ms` is the injected epoch ms the
/// `at HH:MM UTC` parses against.
pub(crate) fn limited(
    term: &LiveTerminal,
    screen: &str,
    work: Work,
    now_ms: u64,
) -> Option<LimitHit> {
    let tail = fresh_tail(screen);
    if let Some(status) = term.status.as_ref() {
        if status.state == "working" {
            return None;
        }
        if status.state == "error" {
            let msg = status.msg.to_lowercase();
            if limit_text(&msg) {
                let wait = parse_wait(&msg, now_ms).unwrap_or_else(|| reset_ms(screen, now_ms));
                return Some(LimitHit {
                    wait_ms: wait.clamp(RETRY_MARGIN_MS, 3_600_000),
                    escalate_now: long_limit(&msg) || long_limit(tail),
                    auto_continue: auto_continue(tail),
                });
            }
            if limit_text(tail) {
                return Some(hit(tail, reset_ms(screen, now_ms)));
            }
            return None;
        }
    }
    if work != Work::Working && limit_text(tail) {
        return Some(hit(tail, reset_ms(screen, now_ms)));
    }
    None
}

/// One limit hit: the wait plus whether the evidence names a
/// weekly or monthly quota.
fn hit(text: &str, wait_ms: u64) -> LimitHit {
    LimitHit {
        wait_ms,
        escalate_now: long_limit(text),
        auto_continue: auto_continue(text),
    }
}

/// Whether the runner says it continues by itself after the reset.
fn auto_continue(text: &str) -> bool {
    text.to_lowercase().contains("continuing automatically at")
}

/// Whether `text` names a multi-day quota.
fn long_limit(text: &str) -> bool {
    let lower = text.to_lowercase();
    LONG_LIMIT_WORDS.iter().any(|w| lower.contains(w))
}

/// How much of a screen still counts as *fresh* for the taxonomy:
/// the last ~12 non-blank lines, the region a live banner
/// occupies above the input box. Anything deeper is scrollback,
/// and scrollback keeps stale banners — t30 stacked 54/53/51
/// -minute ones — that must not drive the reason or the clock.
const FRESH_NONBLANK: usize = 12;

/// The screen's fresh tail — up to [`FRESH_NONBLANK`] non-blank
/// lines off the bottom. Shared with the connection-error pass so
/// both read exactly the same window.
pub(crate) fn fresh_tail(screen: &str) -> &str {
    let mut tail = 0usize;
    let mut nonblank = 0usize;
    for line in screen.rsplit('\n') {
        if nonblank == FRESH_NONBLANK {
            break;
        }
        tail += line.len() + 1;
        if !line.trim().is_empty() {
            nonblank += 1;
        }
    }
    &screen[screen.len() - tail.min(screen.len())..]
}

/// cline's own rate-limit wording is **unmeasured** (S14a section 7:
/// ClinePass has 5 h, weekly and monthly limits, none hit). This
/// placeholder is empty on purpose and matches nothing: no word is
/// guessed, so no user-visible text claims a limit nobody saw. When a
/// real limit screen is captured, add its words here — `limit_text`
/// already reads this list beside the shared vocabulary.
pub(crate) const CLINE_LIMIT_WORDS: &[&str] = &[];

/// Whether the screen carries a limit line — checked whole, so a
/// `usage limit` inside prose still counts.
fn limit_text(screen: &str) -> bool {
    let lower = screen.to_lowercase();
    LIMIT_WORDS.iter().any(|w| lower.contains(w))
        || CLINE_LIMIT_WORDS.iter().any(|w| lower.contains(w))
}

/// The wait a reset line asks for. The LAST limit line owns the
/// answer — screens keep earlier banners in scrollback (t30 showed
/// 54/53/51-minute banners stacked), and a stale banner's clause
/// must never win. Inside that region the absolute `at HH:MM UTC`
/// is preferred over the `in <n>`/`after <n>` relatives — Devin
/// prints both, and the clock is the fresher fact when they
/// disagree. No parse → the default minute; above an hour clamps
/// to it — the retry loop is not a parking spot.
fn reset_ms(text: &str, now_ms: u64) -> u64 {
    let lower = text.to_lowercase();
    let region = last_limit_line(&lower).unwrap_or(&lower);
    parse_wait(region, now_ms)
        .unwrap_or(DEFAULT_WAIT_MS)
        .clamp(RETRY_MARGIN_MS, 3_600_000)
}

/// The text from its last limit line on — the freshest banner's
/// own reset clause.
fn last_limit_line(text: &str) -> Option<&str> {
    let mut found = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        if LIMIT_WORDS.iter().any(|w| line.contains(w)) {
            found = Some(offset);
        }
        offset += line.len();
    }
    found.map(|at| &text[at..])
}

/// The wait one region asks for: `at HH:MM UTC` first, then the
/// relative forms.
fn parse_wait(text: &str, now_ms: u64) -> Option<u64> {
    parse_at(text, now_ms)
        .or_else(|| parse_local_ampm(text, now_ms))
        .or_else(|| parse_in(text))
}

/// Claude's local `12:10pm`/`1:05am` reset clock → ms until it next
/// strikes. Compared in local wall-clock time, so no timezone table is needed.
fn parse_local_ampm(text: &str, now_ms: u64) -> Option<u64> {
    let lower = text.to_lowercase();
    for needle in ["resets ", "continuing automatically at "] {
        let Some(at) = lower.find(needle) else {
            continue;
        };
        let tail = lower[at + needle.len()..].trim_start();
        let clock = (|| {
            let (hh, tail) = front_digits(tail, 1, 2)?;
            let tail = tail.strip_prefix(':')?;
            let (mm, tail) = front_digits(tail, 2, 2)?;
            let pm = if tail.starts_with("pm") {
                true
            } else if tail.starts_with("am") {
                false
            } else {
                return None;
            };
            ((1..=12).contains(&hh) && mm <= 59).then_some((hh, mm, pm))
        })();
        let Some((hh, mm, pm)) = clock else {
            continue;
        };
        let hour = (hh % 12) + u64::from(pm) * 12;
        let timestamp = (now_ms / 1000).min(i64::MAX as u64) as libc::time_t;
        let mut local: libc::tm = unsafe { std::mem::zeroed() };
        if unsafe { libc::localtime_r(&timestamp, &mut local) }.is_null() {
            return None;
        }
        let now = (local.tm_hour as u64) * 3_600_000
            + (local.tm_min as u64) * 60_000
            + (local.tm_sec as u64) * 1000;
        let target = hour * 3_600_000 + mm * 60_000;
        return Some((target + 86_400_000 - now) % 86_400_000);
    }
    None
}

/// The first `at HH:MM UTC` in the region → ms until that clock
/// next strikes. The `at` is word-bounded (`status` is not a
/// match) and the UTC word is required — a bare `at HH:MM` could
/// be local time and parses nothing.
fn parse_at(text: &str, now_ms: u64) -> Option<u64> {
    let mut from = 0;
    while let Some(at) = text[from..].find("at") {
        let at = from + at;
        let bounded = text[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric())
            && text[at + 2..]
                .chars()
                .next()
                .is_some_and(|c| !c.is_ascii_alphanumeric());
        if bounded && let Some(ms) = clock_wait(&text[at + 2..], now_ms) {
            return Some(ms);
        }
        from = at + 2;
    }
    None
}

/// ` HH:MM UTC` at the front of `s` — one or two hour digits, a
/// colon, two minute digits, the UTC word — → ms until that clock
/// next strikes against the injected `now_ms`.
fn clock_wait(s: &str, now_ms: u64) -> Option<u64> {
    let s = s.trim_start();
    let (hh, s) = front_digits(s, 1, 2)?;
    let s = s.strip_prefix(':')?;
    let (mm, s) = front_digits(s, 2, 2)?;
    let s = s.trim_start();
    let utc = s.strip_prefix("utc")?;
    if utc
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
    {
        return None;
    }
    if hh > 23 || mm > 59 {
        return None;
    }
    let day_ms = 86_400_000;
    let target = hh * 3_600_000 + mm * 60_000;
    Some((target + day_ms - now_ms % day_ms) % day_ms)
}

/// `in <n> <unit>` or `after <n> <unit>` → ms. The unit is a word
/// (`s`/`sec`/`seconds`, `m`/`min`/`minutes`, `h`/`hr`/`hours`, …)
/// after optional space, matched whole — `5 months` parses nothing.
/// A failed occurrence advances past its anchor, never by a fixed
/// byte — the old `continue` spun on the same `rest` forever, and
/// `&tail[1..]` panicked off a char boundary.
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

/// `<n><ws?><unit-word>` at the front of `tail` → ms.
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
        _ => return None,
    };
    Some(n.saturating_mul(mult))
}

/// `min..=max` leading ascii digits → `(value, rest)`; an overflow
/// saturates, and the clamp keeps the loop unparked anyway.
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

/// One role's retry exchange: `continue`s already sent and when
/// the next is due.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RatePace {
    pub retries: u8,
    pub due_ms: u64,
}

/// What one sample of a still-limited role should do — pure so
/// the injected clock tests it. `due` is the wait from now.
pub(crate) enum Step {
    /// Wait for `due_ms` to pass.
    Wait,
    /// The reset ran out — send `continue`.
    Send,
    /// The budget is spent — the open stage-owned attention
    /// resolves and a `needs_user` one raises.
    Escalate,
}

/// The pace decision for a still-limited role with an open
/// stage-owned attention: under the budget and past `due` it is
/// time to send; at the budget it escalates; otherwise it waits.
pub(crate) fn step(pace: RatePace, retries_left: bool, now_ms: u64) -> Step {
    if !retries_left {
        return Step::Escalate;
    }
    if now_ms >= pace.due_ms {
        return Step::Send;
    }
    Step::Wait
}

#[cfg(test)]
#[path = "ratelimit_tests.rs"]
mod tests;
