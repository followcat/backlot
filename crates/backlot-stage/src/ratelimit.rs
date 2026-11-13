//! The rate-limit taxonomy (S12d): a terminal whose status is
//! `error`, or a waiting screen whose text names a limit, is rate
//! limited — its own attention reason, counted separately from
//! `stalled` and rework. While the limit holds the stage waits the
//! parsed reset out, then sends `continue`; three auto-retries
//! and the episode escalates to a person.

use crate::reconcile::LiveTerminal;

/// The attention reason.
pub const RATE_LIMITED: &str = "rate limited";
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
    "upgrade your plan",
    "try again at",
    "try again in",
    "retry after",
    "resets at",
    "resets in",
    "reset at",
    "reset in",
];

/// Whether the terminal is rate limited now — and how long the
/// wait should be before a `continue`. `status.state == "error"`
/// from a reporting core is the primary signal (the error an
/// agent surfaces after a burned turn is its limit); a `waiting`
/// or unknown-status screen carrying limit text is the fallback
/// for a core that reports nothing. `None` — not limited.
/// `now_ms` is the injected epoch ms the `at HH:MM UTC` parses
/// against.
pub(crate) fn limited(term: &LiveTerminal, screen: &str, now_ms: u64) -> Option<u64> {
    if let Some(status) = term.status.as_ref().filter(|s| s.state == "error") {
        let msg = status.msg.to_lowercase();
        let wait = parse_wait(&msg, now_ms).unwrap_or_else(|| reset_ms(screen, now_ms));
        return Some(wait.clamp(RETRY_MARGIN_MS, 3_600_000));
    }
    let reporting = term.status.is_some();
    if !reporting && limit_text(screen) {
        return Some(reset_ms(screen, now_ms));
    }
    None
}

/// Whether the screen carries a limit line — checked whole, so a
/// `usage limit` inside prose still counts.
fn limit_text(screen: &str) -> bool {
    let lower = screen.to_lowercase();
    LIMIT_WORDS.iter().any(|w| lower.contains(w))
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
    parse_at(text, now_ms).or_else(|| parse_in(text))
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
mod tests {
    use super::*;

    /// A fixed `now_ms` at 12:00:00 UTC — the `at HH:MM UTC` cases
    /// read their waits off it.
    const NOON: u64 = 86_400_000 * 20_000 + 12 * 3_600_000;

    #[test]
    fn reset_words_parse() {
        assert_eq!(
            reset_ms("rate limit reached, try again in 90s", NOON),
            90_000
        );
        assert_eq!(reset_ms("usage limit, resets in 5m", NOON), 300_000);
        assert_eq!(reset_ms("retry after 2h", NOON), 3_600_000);
        // Nothing parseable → the default minute.
        assert_eq!(reset_ms("Upgrade your plan to continue", NOON), 60_000);
        // A week never parks the loop.
        assert_eq!(reset_ms("resets in 48h", NOON), 3_600_000);
    }

    #[test]
    fn the_absolute_reset_wins_over_the_relative() {
        // Devin's real banner carries both forms; the `at` clause
        // is preferred — and it must not hang the parser (S12d
        // review: the old one spun on `54 minutes` forever).
        let banner = "your limit will reset in 54 minutes (at 12:06 utc).";
        assert_eq!(reset_ms(banner, NOON), 6 * 60_000);
        assert_eq!(
            reset_ms("your limit will reset in 54 minutes", NOON),
            54 * 60_000
        );
        assert_eq!(reset_ms("try again in 45 seconds", NOON), 45_000);
        // Two hours out clamps to the parking limit.
        assert_eq!(reset_ms("try again in 2 hours", NOON), 3_600_000);
        // A bare clock with no UTC marker is not a parse — it could
        // be local time; the relative form answers instead.
        assert_eq!(
            reset_ms("your limit resets at 12:06 — try again in 9 minutes", NOON),
            9 * 60_000
        );
        // A clock already past wraps to tomorrow — clamped to the
        // hour, never a day.
        assert_eq!(reset_ms("your limit resets at 11:00 utc", NOON), 3_600_000);
    }

    #[test]
    fn only_the_latest_limit_lines_clause_counts() {
        // Stacked banners in scrollback: the freshest one's reset
        // wins — the stale `54` above must not answer.
        let screen = "usage limit, resets in 54 minutes\nusage limit, resets in 51 minutes\n";
        assert_eq!(reset_ms(screen, NOON), 51 * 60_000);
    }

    #[test]
    fn the_parser_never_hangs_and_never_panics() {
        // The reproduced killers (S12d review): digits followed by
        // a non-unit spun the `continue` loop forever; a non-ascii
        // or empty tail panicked on `&tail[1..]`.
        for text in [
            "logged in ✓ ready",
            "signed in ",
            " in ",
            "in",
            "your limit will reset in 54 minutes (at 12:06 utc).",
        ] {
            let _ = parse_wait(text, NOON);
        }
        // Every char-boundary suffix of a unicode-heavy screen.
        let screen = "─ ❭ ✓ ⠋ usage limit: resets in 54 minutes (at 12:06 utc) ─";
        for (i, _) in screen.char_indices() {
            let _ = reset_ms(&screen[i..], NOON);
        }
        // A unit lookalike is not a unit: `months` parses nothing,
        // and the line still answers the default through reset_ms.
        assert_eq!(reset_ms("come back in 5 months", NOON), 60_000);
    }

    #[test]
    fn limited_reads_the_structured_error_first() {
        use hc_proto::terminal::Status;
        let err = |msg: &str| LiveTerminal {
            status: Some(Status {
                state: "error".into(),
                msg: msg.into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            limited(&err("429, try again in 30s"), "", NOON),
            Some(30_000)
        );
        // An error with no reset text falls back to the screen's.
        assert_eq!(
            limited(&err("denied"), "usage limit resets in 2m", NOON),
            Some(120_000)
        );
        // A silent core: waiting screen text carries it.
        assert_eq!(
            limited(
                &LiveTerminal::default(),
                "rate limit reached, try again in 10s",
                NOON
            ),
            Some(10_000)
        );
        // Working with an ordinary screen is not limited.
        let working = LiveTerminal {
            status: Some(Status {
                state: "working".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(limited(&working, "progress", NOON), None);
        // A reporting core's `error` wins even over clean text.
        assert_eq!(limited(&err(""), "everything is fine", NOON), Some(60_000));
    }

    #[test]
    fn the_retry_budget_escalates() {
        let pace = RatePace {
            retries: 0,
            due_ms: 100,
        };
        assert!(matches!(step(pace, true, 99), Step::Wait));
        assert!(matches!(step(pace, true, 100), Step::Send));
        assert!(matches!(step(pace, false, 9_999), Step::Escalate));
    }
}
