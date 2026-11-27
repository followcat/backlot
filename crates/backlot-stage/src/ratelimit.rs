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

use backlot_model::summary::{QuotaWindow, Work};

use crate::reconcile::LiveTerminal;

#[path = "ratelimit_parse.rs"]
mod parse;

pub(crate) use parse::format_reset_local;
#[cfg(test)]
pub(crate) use parse::parse_wait;

/// Maximum delay between auto-retry probes (clamped retry cadence only).
pub(crate) const MAX_WAIT_MS: u64 = 3_600_000;

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
    /// Clamped delay for the stage's next auto-retry probe (capped at 1 h).
    pub retry_wait_ms: u64,
    /// Unclamped absolute Unix epoch ms of the parsed reset time, when known.
    pub reset_at_ms: Option<u64>,
    /// Classified quota horizon (`Short | Long | Unknown`).
    pub window: QuotaWindow,
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

/// Whether the terminal is rate limited now — and both its clamped retry delay
/// and unclamped reset timestamp when parsed.
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
                let delta =
                    parse::parse_wait(&msg, now_ms).or_else(|| parse_screen_delta(screen, now_ms));
                let escalates = long_limit(&msg) || long_limit(tail);
                return Some(LimitHit {
                    retry_wait_ms: delta
                        .unwrap_or(DEFAULT_WAIT_MS)
                        .clamp(RETRY_MARGIN_MS, MAX_WAIT_MS),
                    reset_at_ms: delta.map(|d| now_ms.saturating_add(d)),
                    window: classify_window(escalates, &[&msg, tail], delta),
                    escalate_now: escalates,
                    auto_continue: auto_continue(tail),
                });
            }
            if limit_text(tail) {
                return Some(hit_screen(tail, screen, now_ms));
            }
            return None;
        }
    }
    if work != Work::Working && limit_text(tail) {
        return Some(hit_screen(tail, screen, now_ms));
    }
    None
}

fn hit_screen(tail: &str, screen: &str, now_ms: u64) -> LimitHit {
    let delta = parse_screen_delta(screen, now_ms);
    let escalates = long_limit(tail);
    LimitHit {
        retry_wait_ms: delta
            .unwrap_or(DEFAULT_WAIT_MS)
            .clamp(RETRY_MARGIN_MS, MAX_WAIT_MS),
        reset_at_ms: delta.map(|d| now_ms.saturating_add(d)),
        window: classify_window(escalates, &[tail], delta),
        escalate_now: escalates,
        auto_continue: auto_continue(tail),
    }
}

#[cfg(test)]
fn hit(text: &str, delta_ms: Option<u64>, now_ms: u64) -> LimitHit {
    let escalates = long_limit(text);
    LimitHit {
        retry_wait_ms: delta_ms
            .unwrap_or(DEFAULT_WAIT_MS)
            .clamp(RETRY_MARGIN_MS, MAX_WAIT_MS),
        reset_at_ms: delta_ms.map(|d| now_ms.saturating_add(d)),
        window: classify_window(escalates, &[text], delta_ms),
        escalate_now: escalates,
        auto_continue: auto_continue(text),
    }
}

fn classify_window(escalates: bool, texts: &[&str], delta: Option<u64>) -> QuotaWindow {
    if escalates || delta.is_some_and(|d| d > parse::LONG_WINDOW_MS) {
        return QuotaWindow::Long;
    }
    if delta.is_some() || texts.iter().any(|t| short_limit_hint(t)) {
        return QuotaWindow::Short;
    }
    QuotaWindow::Unknown
}

fn short_limit_hint(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("session limit")
        || lower.contains("5-hour")
        || lower.contains("5 hour")
        || lower.contains("5h ")
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

/// Unclamped parsed wait delta from the freshest limit line on `text`.
fn parse_screen_delta(text: &str, now_ms: u64) -> Option<u64> {
    let lower = text.to_lowercase();
    let region = last_limit_line(&lower).unwrap_or(&lower);
    parse::parse_wait(region, now_ms)
}

/// Whether the limit hit's parsed reset time is anchored to a calendar
/// or wall-clock timestamp rather than a floating relative offset.
pub(crate) fn anchored_reset(term: &LiveTerminal, screen: &str, now_ms: u64) -> bool {
    let tail = fresh_tail(screen);
    let screen_anchored = {
        let lower = screen.to_lowercase();
        let region = last_limit_line(&lower).unwrap_or(&lower);
        parse::has_anchored_reset(region, now_ms)
    };
    if let Some(status) = term.status.as_ref()
        && status.state == "error"
    {
        let msg = status.msg.to_lowercase();
        if limit_text(&msg) {
            return parse::has_anchored_reset(&msg, now_ms) || screen_anchored;
        }
        if limit_text(tail) {
            return screen_anchored;
        }
        return false;
    }
    screen_anchored
}

/// Clamped auto-retry wait (for tests exercising the default/clamp fallback).
#[cfg(test)]
fn reset_ms(text: &str, now_ms: u64) -> u64 {
    parse_screen_delta(text, now_ms)
        .unwrap_or(DEFAULT_WAIT_MS)
        .clamp(RETRY_MARGIN_MS, MAX_WAIT_MS)
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

/// One role's retry exchange: `continue`s already sent, when the next
/// probe is due (clamped at 1 h), and the unclamped reset time if known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RatePace {
    pub retries: u8,
    pub due_ms: u64,
    pub reset_at_ms: Option<u64>,
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

/// Millisecond tolerance when comparing `reset_at_ms` across polls for
/// the same clock/calendar window (`localtime_r` truncates sub-second ms).
const SAME_RESET_SLOP_MS: u64 = 1_000;
const DAY_MS: u64 = 86_400_000;

/// How an observed hit compares to an existing reset timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowMatch {
    Same,
    Next,
    Fallback,
}

/// Compare a new limit hit's reset timestamp against an existing one.
pub(crate) fn classify_hit_window(
    hit_reset: Option<u64>,
    open_reset: Option<u64>,
    anchored: bool,
    auto_continue: bool,
    now_ms: u64,
) -> WindowMatch {
    let (Some(new), Some(old)) = (hit_reset, open_reset) else {
        return WindowMatch::Fallback;
    };
    if !anchored {
        // Relative resets (e.g. `in 30s`) drift with `now_ms`; a much
        // later reset while the current window has not elapsed starts a
        // new window, while polling the same banner uses existing pace.
        if now_ms < old && new > old.saturating_add(60_000) {
            return WindowMatch::Next;
        }
        return WindowMatch::Fallback;
    }
    if new <= old.saturating_add(SAME_RESET_SLOP_MS) {
        return WindowMatch::Same;
    }
    // An untouched `HH:MM` clock banner wraps +24h once `old` passes on
    // an auto-continuing banner; keep falling through so reset-passed
    // handling can escalate rather than opening tomorrow's window.
    let day_wrap = new.saturating_sub(old).abs_diff(DAY_MS) <= SAME_RESET_SLOP_MS;
    if auto_continue && now_ms >= old && day_wrap {
        return WindowMatch::Fallback;
    }
    WindowMatch::Next
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
