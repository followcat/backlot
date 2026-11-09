//! `work_state` table tests: each runner's blocked and working
//! markers from its real screen, blocked beating working, and
//! anything unmatched reading `Waiting`.

use backlot_model::summary::Work;

use super::work_state;

/// Claude Code's permission prompt — the working line can still be
/// on screen above it, so blocked must win.
#[test]
fn claude_blocked_beats_a_working_line() {
    let working = "✻ Orbiting… (12s · ↑ 1.2k tokens · esc to interrupt)\n";
    assert_eq!(work_state("claude", working), Work::Working);
    let blocked = "✻ Orbiting… (12s · ↑ 1.2k tokens · esc to interrupt)\n\n Do you want to proceed?\n ❯ 1. Yes\n   2. Yes, and don't ask again for npm test commands\n";
    assert_eq!(work_state("claude", blocked), Work::Blocked);
    // The question without the picker cursor is not the prompt.
    assert_eq!(
        work_state("claude", "Do you want to proceed?\n  1. Yes\n"),
        Work::Waiting
    );
    // Nor a cursor alone.
    assert_eq!(work_state("claude", "❯ 1. Yes\n"), Work::Waiting);
}

/// The folder-trust dialog is blocked too — the same screen the
/// watch's trust exchange answers.
#[test]
fn claude_trust_dialog_is_blocked() {
    let trust = "Quick safety check\nDo you trust the files in this folder?\n\n❯ No, exit\n  Yes, I trust this folder\n";
    assert_eq!(work_state("claude", trust), Work::Blocked);
}

/// Claude 2.1.319's running turn is a spinner glyph and a `…`
/// gerund, with no `esc to interrupt` on screen; the finished line
/// keeps the glyph but drops the `…`, and a transcript bullet is
/// not a turn — both stay waiting.
#[test]
fn claude_spinner_line_is_working() {
    assert_eq!(
        work_state("claude", "· Schlepping… (17s · ↓ 95 tokens)\n"),
        Work::Working
    );
    assert_eq!(work_state("claude", "· Schlepping…\n"), Work::Working);
    assert_eq!(
        work_state("claude", "✻ Churned for 2s · done 3:07 PM\n"),
        Work::Waiting
    );
    assert_eq!(
        work_state("claude", "· a bullet in the transcript\n"),
        Work::Waiting
    );
    // A permission prompt on a spinner screen is still blocked.
    let screen = "· Schlepping… (17s · ↓ 95 tokens)\n\n Do you want to proceed?\n ❯ 1. Yes\n";
    assert_eq!(work_state("claude", screen), Work::Blocked);
}

/// Codex's `Would you like to` + `› 1.` picker.
#[test]
fn codex_working_and_blocked() {
    assert_eq!(
        work_state("codex", "Thinking · 2s (esc to interrupt)\n"),
        Work::Working
    );
    let blocked = "Would you like to run the following command?\n› 1. Yes, proceed (y)\n  2. Yes, and don't ask again for this command (p)\n";
    assert_eq!(work_state("codex", blocked), Work::Blocked);
    assert_eq!(
        work_state("codex", "Would you like to run this?\n  1. yes\n"),
        Work::Waiting
    );
}

/// Devin's `Allow … Once` / `Deny` pair; working is any
/// `to interrupt` line.
#[test]
fn devin_working_and_blocked() {
    assert_eq!(
        work_state("devin", "Thinking · 2s (esc twice to interrupt)\n"),
        Work::Working
    );
    let blocked = "Allow ← ◼◼◼ → Once     Exec(flock)\n  Deny\n";
    assert_eq!(work_state("devin", blocked), Work::Blocked);
    // Only half the pair is not a prompt.
    assert_eq!(work_state("devin", "  Deny\n"), Work::Waiting);
    assert_eq!(
        work_state("devin", "Allow ← ◼◼◼ → Once     Exec(flock)\n"),
        Work::Waiting
    );
}

/// OpenCode's permission card. Its working marker is unmeasured —
/// the test pins the documented guess.
#[test]
fn opencode_blocked() {
    let blocked = "△ Permission required\n Allow once   Always allow   Reject\n";
    assert_eq!(work_state("opencode", blocked), Work::Blocked);
    assert_eq!(work_state("opencode", "esc interrupt\n"), Work::Working);
    assert_eq!(
        work_state("opencode", "Permission required\n"),
        Work::Waiting
    );
}

/// The fake runner's latest `fake-work:` line is its state.
#[test]
fn fake_reads_its_last_announced_state() {
    assert_eq!(work_state("fake", "fake-work: working\n"), Work::Working);
    assert_eq!(work_state("fake", "fake-work: blocked\n"), Work::Blocked);
    // The last announced state wins, in either direction.
    assert_eq!(
        work_state("fake", "fake-work: working\nfake-work: blocked\n"),
        Work::Blocked
    );
    assert_eq!(
        work_state("fake", "fake-work: blocked\nfake-work: working\n"),
        Work::Working
    );
    assert_eq!(
        work_state("fake", "fake runner: actor-a of acme-shop\n"),
        Work::Waiting
    );
}

/// Anything unmatched is waiting — including an unknown runner and
/// an empty screen.
#[test]
fn anything_unmatched_is_waiting() {
    assert_eq!(work_state("claude", "prompt>\n"), Work::Waiting);
    assert_eq!(work_state("codex", ""), Work::Waiting);
    assert_eq!(
        work_state("unknown-runner", "esc to interrupt\n"),
        Work::Waiting
    );
    assert_eq!(work_state("fake", ""), Work::Waiting);
}
