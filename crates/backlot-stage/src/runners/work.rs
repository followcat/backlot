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
        "cursor" => cursor_trust_dialog(screen) || cursor_approval_picker(screen),
        // Command Code's permission prompt: the `needs to …`
        // sentence plus the numbered picker cursor (S12a). The
        // folder-trust dialog is blocked too — `--trust` should
        // keep it away, so it is the user's prompt, never answered.
        "cmd" => {
            cmd_trust_dialog(screen)
                || ((has_line_containing(screen, "needs to execute")
                    || has_line_containing(screen, "needs to run")
                    || has_line_containing(screen, "make this edit to"))
                    && has_line_starting(screen, "❯ 1."))
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
        "cursor" => screen.contains("ctrl+c to stop") || screen.lines().any(cursor_spinner_line),
        // cmd's status line mid-turn: `◇ Conjuring… esc to
        // interrupt • 5s • ↓ 219` — the verb rotates, the marker
        // is stable (S12a).
        "cmd" => screen.contains("esc to interrupt"),
        "fake" => fake_state(screen) == Some(Work::Working),
        _ => false,
    }
}

/// cursor-agent 2026.11.05's running turn: one or two braille
/// glyphs, a space, then `Composing` (a token count may follow) or
/// `Running` with a trailing token count — ` ⠠⠜ Composing`,
/// ` ⠘⠤ Running  44 tokens`. The glyph is required: a transcript
/// line starting `Running the…` is not a turn.
fn cursor_spinner_line(l: &str) -> bool {
    let mut parts = l.trim().splitn(2, ' ');
    let (Some(glyphs), Some(rest)) = (parts.next(), parts.next()) else {
        return false;
    };
    if glyphs.is_empty()
        || glyphs.chars().count() > 2
        || !glyphs
            .chars()
            .all(|c| ('\u{2800}'..='\u{28ff}').contains(&c))
    {
        return false;
    }
    let rest = rest.trim_start();
    rest.starts_with("Composing")
        || rest
            .strip_prefix("Running")
            .is_some_and(|r| r.trim_end().ends_with("tokens"))
}

/// cursor-agent's workspace-trust dialog — `⚠ Workspace Trust
/// Required` with the `[a] Trust this workspace` option. `--trust`
/// should keep it away, so a screen still showing it is the user's
/// prompt, never something the stage answers.
pub(crate) fn cursor_trust_dialog(text: &str) -> bool {
    text.contains("Workspace Trust Required") && text.contains("Trust this workspace")
}

/// A separator drawn the full width of the screen: only `─` after
/// trimming. A diff preview inside a picker draws its own `─`
/// rule — it divides the block it sits in, same test.
pub(crate) fn is_full_width_rule(l: &str) -> bool {
    let t = l.trim();
    t.chars().count() >= 3 && t.chars().all(|c| c == '─')
}

/// Any cursor approval picker — `Run this command?`, `Write to
/// this file?`, `Proceed with this edit?`, the lot: below the last
/// full-width `─` divider an option `→` cursor line, the `Run
/// Everything (shift+tab)` footer every picker shares, and
/// allowlist/approval wording or a tool question (S12f; hc's
/// `cursor.toml` maps the same shape). Below the divider is where
/// pickers live — an answered one's scrollback above it and the
/// composer's bare ` →` input row both stay out. The question's
/// own words stay out of the match so a picker kind not yet named
/// still blocks — `permit` decides whose it is.
pub(crate) fn cursor_approval_picker(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .rposition(|l| is_full_width_rule(l))
        .map_or(0, |i| i + 1);
    let region = &lines[start..];
    region.iter().any(|l| l.trim_start().starts_with("→ "))
        && region
            .iter()
            .any(|l| l.contains("Run Everything") && l.contains("(shift+tab)"))
        && region
            .iter()
            .any(|l| l.contains("allowlist") || l.contains("approval") || cursor_picker_question(l))
}

/// A tool-question line in a picker region — `Write to this
/// file?`, `Run this command?`, `Proceed with this edit?` and the
/// other shapes the manifest names — matched case-insensitively on
/// the phrase, not the verb.
fn cursor_picker_question(l: &str) -> bool {
    const WORDS: &[&str] = &[
        "this command",
        "this file",
        "this mcp",
        "proceed with this edit",
        "allow this web",
        "outside the sandbox",
    ];
    let l = l.to_lowercase();
    WORDS.iter().any(|w| l.contains(w))
}

/// Command Code's folder-trust dialog — `Do you trust the files in
/// this folder?` with the `❯ 1. Yes, proceed` picker (S12a).
/// `--trust` should keep it away — a screen still showing it means
/// the flag was forgotten or the cwd was unexpectedly non-empty; it
/// is the user's prompt, never something the stage answers.
pub(crate) fn cmd_trust_dialog(text: &str) -> bool {
    text.contains("Do you trust the files in this folder?") && text.contains("Yes, proceed")
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
