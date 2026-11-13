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
pub(crate) fn limited(term: &LiveTerminal, screen: &str) -> Option<u64> {
    if let Some(status) = term.status.as_ref().filter(|s| s.state == "error") {
        let wait = parse_wait(&status.msg.to_lowercase())
            .map(|w| w.clamp(RETRY_MARGIN_MS, 3_600_000))
            .unwrap_or_else(|| reset_ms(screen));
        return Some(wait);
    }
    let reporting = term.status.is_some();
    if !reporting && limit_text(screen) {
        return Some(reset_ms(screen));
    }
    None
}

/// Whether the screen carries a limit line — checked whole, so a
/// `usage limit` inside prose still counts.
fn limit_text(screen: &str) -> bool {
    let lower = screen.to_lowercase();
    LIMIT_WORDS.iter().any(|w| lower.contains(w))
}

/// The wait a reset line asks for: `…in <n>s`, `…in <n>m`,
/// `…after <n>s`/`m`, `…at HH:MM` (same-day). No parse → the
/// default minute. A parsed wait above an hour clamps to it —
/// the retry loop is not a parking spot.
fn reset_ms(text: &str) -> u64 {
    let lower = text.to_lowercase();
    let found = parse_wait(&lower).unwrap_or(DEFAULT_WAIT_MS);
    found.clamp(RETRY_MARGIN_MS, 3_600_000)
}

/// `in 90s` / `in 5m` / `after 30s` / `after 2m` → ms.
fn parse_wait(text: &str) -> Option<u64> {
    for anchor in [" in ", " after "] {
        let mut rest = text;
        while let Some(at) = rest.find(anchor) {
            let tail = &rest[at + anchor.len()..];
            let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = digits.parse::<u64>() {
                let unit = tail[digits.len()..].chars().next().unwrap_or(' ');
                let mult = match unit {
                    's' => 1_000,
                    'm' => 60_000,
                    'h' => 3_600_000,
                    _ => continue,
                };
                return Some(n * mult);
            }
            rest = &tail[1..];
        }
    }
    None
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

    #[test]
    fn reset_words_parse() {
        assert_eq!(reset_ms("rate limit reached, try again in 90s"), 90_000);
        assert_eq!(reset_ms("usage limit, resets in 5m"), 300_000);
        assert_eq!(reset_ms("retry after 2h"), 3_600_000);
        // Nothing parseable → the default minute.
        assert_eq!(reset_ms("Upgrade your plan to continue"), 60_000);
        // A week never parks the loop.
        assert_eq!(reset_ms("resets in 48h"), 3_600_000);
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
        assert_eq!(limited(&err("429, try again in 30s"), ""), Some(30_000));
        // An error with no reset text falls back to the screen's.
        assert_eq!(
            limited(&err("denied"), "usage limit resets in 2m"),
            Some(120_000)
        );
        // A silent core: waiting screen text carries it.
        assert_eq!(
            limited(
                &LiveTerminal::default(),
                "rate limit reached, try again in 10s"
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
        assert_eq!(limited(&working, "progress"), None);
        // A reporting core's `error` wins even over clean text.
        assert_eq!(limited(&err(""), "everything is fine"), Some(60_000));
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
