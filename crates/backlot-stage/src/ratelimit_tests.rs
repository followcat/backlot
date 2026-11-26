//! The rate-limit taxonomy's unit tests: the reset parsers, the
//! fresh-tail window and the detection rules per reporting-core
//! state, including the weekly/monthly quota escalation.

use super::*;
use hc_proto::terminal::Status;

/// A fixed `now_ms` at 12:00:00 UTC — the `at HH:MM UTC` cases
/// read their waits off it.
const NOON: u64 = 86_400_000 * 20_000 + 12 * 3_600_000;

/// A short-window hit with this wait.
fn wait(ms: u64) -> Option<LimitHit> {
    Some(LimitHit {
        wait_ms: ms,
        escalate_now: false,
        auto_continue: false,
    })
}

fn err(msg: &str) -> LiveTerminal {
    LiveTerminal {
        status: Some(Status {
            state: "error".into(),
            msg: msg.into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn reporting(state: &str) -> LiveTerminal {
    LiveTerminal {
        status: Some(Status {
            state: state.into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

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
fn a_non_clock_resets_phrase_falls_through_to_auto_continue() {
    // `resets soon` must not end the search: the later explicit
    // auto-continuation clock is the wait the runner promised.
    let timestamp = (NOON / 1000).min(i64::MAX as u64) as libc::time_t;
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    assert!(!unsafe { libc::localtime_r(&timestamp, &mut local) }.is_null());
    let total = (local.tm_hour as u64 * 60 + local.tm_min as u64 + 2) % (24 * 60);
    let hour24 = total / 60;
    let minute = total % 60;
    let hour12 = if hour24.is_multiple_of(12) {
        12
    } else {
        hour24 % 12
    };
    let suffix = if hour24 >= 12 { "pm" } else { "am" };
    let screen = format!(
        "session limit reached · resets soon\nContinuing automatically at {hour12}:{minute:02}{suffix}"
    );
    let wait = parse_wait(&screen, NOON).expect("the auto-continue clock parses");
    assert!((60_001..=120_000).contains(&wait), "{wait}");
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
fn an_error_is_limited_only_when_limit_text_is_observed() {
    assert!(errored(&err("denied")));
    // Limit text in the status message — the wait parses off
    // the message itself.
    assert_eq!(
        limited(&err("429, try again in 30s"), "", Work::Waiting, NOON),
        wait(30_000)
    );
    // Limit text only on the screen — the latest block carries
    // the observed evidence and the wait.
    assert_eq!(
        limited(
            &err("denied"),
            "usage limit resets in 2m",
            Work::Waiting,
            NOON
        ),
        wait(120_000)
    );
    // A bare error with clean text everywhere is NOT a limit —
    // it is `agent error`'s, and no `continue` ever leaves.
    assert_eq!(
        limited(&err(""), "everything is fine", Work::Waiting, NOON),
        None
    );
    assert_eq!(
        limited(&err("denied"), "all clean", Work::Waiting, NOON),
        None
    );
    // A silent core: a non-working screen's limit line carries
    // it — but a working screen is only *printing* the words
    // (fixture output, an edit to this file), never limited.
    let silent = LiveTerminal::default();
    assert!(!errored(&silent));
    assert_eq!(
        limited(
            &silent,
            "rate limit reached, try again in 10s",
            Work::Waiting,
            NOON
        ),
        wait(10_000)
    );
    assert_eq!(
        limited(
            &silent,
            "rate limit reached, try again in 90s",
            Work::Working,
            NOON
        ),
        None
    );
    // Working with an ordinary screen is not limited.
    assert_eq!(
        limited(&reporting("working"), "progress", Work::Working, NOON),
        None
    );
}

#[test]
fn a_reporting_core_at_a_non_working_state_reads_its_screen() {
    // Devin reports `done` while its rate limit holds; the banner
    // is the only evidence, and the reset parses off it.
    let devin = "Reached free model rate limit for free model, \
                 your limit will reset in 54 minutes (at 12:06 utc)";
    for state in ["done", "idle", "waiting"] {
        assert_eq!(
            limited(&reporting(state), devin, Work::Waiting, NOON),
            wait(6 * 60_000),
            "{state} missed Devin's limit"
        );
    }
    // A working terminal may be printing the words: never a hit.
    for work in [Work::Working, Work::Waiting] {
        assert_eq!(limited(&reporting("working"), devin, work, NOON), None);
    }
    // A stale banner in scrollback is not the fresh tail's.
    let stale = format!(
        "{devin}\n{}",
        "a fresh output line\n".repeat(FRESH_NONBLANK + 1)
    );
    assert_eq!(
        limited(&reporting("done"), &stale, Work::Waiting, NOON),
        None
    );
}

#[test]
fn a_weekly_or_monthly_quota_escalates_at_once() {
    // Claude Code's measured screens: both are recognized, and a
    // weekly/monthly reset — days away — is nobody's retry loop.
    let weekly = "You've hit your weekly limit · resets Nov 21, 4pm";
    assert_eq!(
        limited(&reporting("waiting"), weekly, Work::Waiting, NOON),
        Some(LimitHit {
            wait_ms: DEFAULT_WAIT_MS,
            escalate_now: true,
            auto_continue: false,
        })
    );
    let monthly = "You've hit your monthly limit · resets Dec 1, 4pm";
    let hit = limited(&reporting("idle"), monthly, Work::Waiting, NOON).unwrap();
    assert!(hit.escalate_now);

    // A session limit is short-window: recognized, but retried.
    let session = "You've hit your session limit · resets in 2h";
    let hit = limited(&reporting("done"), session, Work::Waiting, NOON).unwrap();
    assert!(!hit.escalate_now);
    assert_eq!(hit.wait_ms, 3_600_000);

    // The same wording in an error status's message escalates too.
    let hit = limited(
        &err("You've hit your weekly limit"),
        "",
        Work::Waiting,
        NOON,
    )
    .unwrap();
    assert!(hit.escalate_now);
}

#[test]
fn only_the_fresh_tail_counts_as_screen_evidence() {
    // A stale banner above the fresh tail is scrollback, not
    // evidence — the plain error is `agent error`'s.
    let stale = format!(
        "usage limit, resets in 5m\n{}",
        "a fresh output line\n".repeat(FRESH_NONBLANK + 1)
    );
    assert_eq!(
        limited(&err("transport died"), &stale, Work::Waiting, NOON),
        None
    );
    // The same banner inside the tail is observed.
    let fresh = "usage limit, resets in 5m\na fresh output line\n";
    assert_eq!(
        limited(&err("transport died"), fresh, Work::Waiting, NOON),
        wait(300_000)
    );
    // And a silent core reads the tail the same way.
    assert_eq!(
        limited(&LiveTerminal::default(), &stale, Work::Waiting, NOON),
        None
    );
    assert_eq!(
        limited(&LiveTerminal::default(), fresh, Work::Waiting, NOON),
        wait(300_000)
    );
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

/// Claude's 12-hour reset clock and explicit auto-continue line parse.
#[test]
fn parses_claude_local_reset_and_auto_continue() {
    let text = "Session limit reached · resets 12:10pm\nContinuing automatically at 12:10pm";
    let now = 1_700_000_000_000u64;
    let hit = hit(text, reset_ms(text, now));
    assert!(hit.auto_continue);
    assert!(hit.wait_ms <= 86_400_000);
    assert_eq!(
        parse_wait("resets 12:10pm", now),
        parse_wait("Continuing automatically at 12:10pm", now)
    );
}

/// Claude's weekly-limit picker is limit evidence, not a question the stage answers.
#[test]
fn recognizes_the_weekly_limit_paid_choice_dialog() {
    let screen = include_str!("../tests/fixtures/claude/weekly-limit.txt");
    assert!(weekly_choice_dialog(screen));
    assert!(limit_text(screen));
    assert!(long_limit(screen));
}
