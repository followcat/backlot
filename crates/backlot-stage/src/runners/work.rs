//! A live role's work read off its visible screen (R17): `Working`,
//! `Waiting` or `Blocked`, per runner. `Blocked` is checked first —
//! a prompt can sit on a screen that still shows the working line —
//! and anything unmatched is `Waiting`, the role answering to
//! nothing. The ledger never sees it; the watch pass samples it
//! into the summary.

use backlot_model::summary::Work;

use crate::watch::trust_dialog;

/// `runner`'s current state from the terminal's visible text.
pub(crate) fn work_state(runner: &str, screen: &str) -> Work {
    if blocked(runner, screen) {
        return Work::Blocked;
    }
    if working(runner, screen) {
        return Work::Working;
    }
    Work::Waiting
}

fn has_line_containing(screen: &str, needle: &str) -> bool {
    screen.lines().any(|l| l.contains(needle))
}

/// A line whose trimmed start begins with `prefix`.
fn has_line_starting(screen: &str, prefix: &str) -> bool {
    screen.lines().any(|l| l.trim_start().starts_with(prefix))
}

/// The runner's screen reads as an unanswered permission or trust
/// prompt.
fn blocked(runner: &str, screen: &str) -> bool {
    match runner {
        "claude" => {
            trust_dialog(screen)
                || (has_line_containing(screen, "Do you want to")
                    && has_line_starting(screen, "❯ 1."))
        }
        "codex" => {
            has_line_containing(screen, "Would you like to") && has_line_starting(screen, "› 1.")
        }
        "devin" => {
            screen
                .lines()
                .any(|l| l.trim_start().starts_with("Allow") && l.contains("Once"))
                && screen.lines().any(|l| l.trim() == "Deny")
        }
        // The OpenCode permission screen is not measured yet — see
        // docs/studio/runners.md.
        "opencode" => {
            has_line_containing(screen, "Permission required")
                && has_line_containing(screen, "Allow once")
        }
        "fake" => fake_state(screen) == Some(Work::Blocked),
        _ => false,
    }
}

/// The runner's screen reads as mid-turn.
fn working(runner: &str, screen: &str) -> bool {
    match runner {
        "claude" | "codex" => screen.contains("esc to interrupt"),
        "devin" => screen.contains("to interrupt"),
        // Not measured yet — see docs/studio/runners.md.
        "opencode" => screen.contains("esc interrupt"),
        "fake" => fake_state(screen) == Some(Work::Working),
        _ => false,
    }
}

/// The fake runner's last announced state: `work:<state>` input
/// echoes `fake-work: <state>`, and the latest one is current.
fn fake_state(screen: &str) -> Option<Work> {
    match screen.lines().rfind(|l| l.starts_with("fake-work:"))? {
        "fake-work: blocked" => Some(Work::Blocked),
        "fake-work: working" => Some(Work::Working),
        _ => None,
    }
}
