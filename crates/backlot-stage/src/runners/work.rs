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

/// A Devin option line without its selection cursor: the `❭` marks
/// the highlighted option (`❭ Allow` by default) and can sit on
/// `Deny` or `Bypass mode` instead.
fn devin_option(l: &str) -> &str {
    let t = l.trim();
    t.strip_prefix('❭').map_or(t, str::trim_start)
}

/// Devin's picker line as read on 3000.12.32: `Allow` — after an
/// optional `❭` cursor — then `← ◼◼◼◼ →` and a scope word. Any
/// scope counts (`Once`, `Session`, …): it is the same prompt.
fn devin_picker_line(l: &str) -> bool {
    let Some(rest) = devin_option(l).strip_prefix("Allow") else {
        return false;
    };
    let Some(i) = rest.find('←') else {
        return false;
    };
    let Some(j) = rest[i..].find('→') else {
        return false;
    };
    !rest[i + j + '→'.len_utf8()..].trim().is_empty()
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
            screen.lines().any(devin_picker_line)
                && screen.lines().any(|l| devin_option(l) == "Deny")
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
        "claude" => screen.contains("esc to interrupt") || claude_spinner(screen),
        "codex" => screen.contains("esc to interrupt"),
        "devin" => screen.contains("to interrupt"),
        // Not measured yet — see docs/studio/runners.md.
        "opencode" => screen.contains("esc interrupt"),
        "fake" => fake_state(screen) == Some(Work::Working),
        _ => false,
    }
}

/// Claude Code's running turn on 2.1.319: a spinner glyph from
/// `·✢✳✶✻✽*`, a space, then a word ending in `…` —
/// `· Schlepping… (17s · ↓ 95 tokens)`. The finished line keeps the
/// glyph but loses the `…` (`✻ Churned for 2s · done 3:07 PM`), and
/// a transcript bullet was never a turn — both stay waiting. Older
/// versions' `esc to interrupt` line still matches separately.
fn claude_spinner(screen: &str) -> bool {
    screen.lines().any(claude_spinner_line)
}

/// One line of the running-turn spinner: a single glyph from
/// `·✢✳✶✻✽*`, a space, then a word ending in `…`. The permission
/// block comparison drops these lines — the timer and token count
/// move between samples of the same prompt.
pub(crate) fn claude_spinner_line(l: &str) -> bool {
    const GLYPHS: &[char] = &['·', '✢', '✳', '✶', '✻', '✽', '*'];
    let mut parts = l.trim().splitn(2, ' ');
    let (Some(glyph), Some(rest)) = (parts.next(), parts.next()) else {
        return false;
    };
    glyph.chars().count() == 1
        && GLYPHS.contains(&glyph.chars().next().unwrap_or_default())
        && rest
            .split_whitespace()
            .next()
            .is_some_and(|w| w.ends_with('…'))
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
