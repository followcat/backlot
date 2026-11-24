//! A live role's work read off its visible screen (R17): `Working`,
//! `Waiting` or `Blocked`, per runner. `Blocked` is checked first —
//! a prompt can sit on a screen that still shows the working line —
//! and anything unmatched is `Waiting`, the role answering to
//! nothing. The ledger never sees it; the watch pass samples it
//! into the summary.

use backlot_model::summary::Work;

use super::prompt::joined;
use crate::watch::trust_dialog;

/// `runner`'s current state from the terminal's visible text.
/// Checked in order: runner-specific `blocked` markers, D2's generic
/// fallback dialog rule (before `working` because pickers such as agy
/// and claude keep `esc to cancel`/`interrupt` visible on the bottom
/// bar), `working` markers, then `Waiting`.
pub(crate) fn work_state(runner: &str, screen: &str) -> Work {
    if blocked(runner, screen) || super::dialog::generic_dialog(screen) {
        return Work::Blocked;
    }
    if working(runner, screen) {
        return Work::Working;
    }
    Work::Waiting
}

/// cmd's picker: the option cursor plus both neighbouring options.
/// Joined first, so the option texts wrapping across lines cannot
/// hide the picker (S12a).
fn cmd_picker(screen: &str) -> bool {
    let flat = joined(screen);
    flat.contains("❯ 1. Yes") && flat.contains("2. Yes, don't ask") && flat.contains("3. No")
}

/// The first whole-word position of `word` in the joined text.
fn word_at(text: &str, word: &str) -> Option<usize> {
    text.match_indices(word).map(|(i, _)| i).find(|&i| {
        text[..i]
            .chars()
            .next_back()
            .is_none_or(char::is_whitespace)
            && text[i + word.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric())
    })
}

/// Devin's picker read on the joined screen: the `Allow` option with
/// its `← ◼◼◼◼ →` scope control and the `Deny` option — the shape
/// measured on 3000.12.32. Any scope counts (`Once`, `Session`, …):
/// it is the same prompt.
fn devin_blocked(screen: &str) -> bool {
    let flat = joined(screen);
    let Some(i) = word_at(&flat, "Allow") else {
        return false;
    };
    let rest = &flat[i + "Allow".len()..];
    let Some(left) = rest.find('←') else {
        return false;
    };
    let Some(right) = rest[left..].find('→') else {
        return false;
    };
    let scope = rest[left + right + '→'.len_utf8()..].trim_start();
    !scope.is_empty() && word_at(&flat, "Deny").is_some()
}

/// The runner's screen reads as an unanswered permission prompt,
/// trust dialog, or recognised question picker.
fn blocked(runner: &str, screen: &str) -> bool {
    recognized_permission(runner, screen) || recognized_question(runner, screen)
}

/// The runner's screen reads as a recognised permission prompt or
/// folder-trust dialog (kept on `permission prompt`, never `question`).
pub(crate) fn recognized_permission(runner: &str, screen: &str) -> bool {
    match runner {
        "claude" => {
            let flat = joined(screen);
            trust_dialog(screen) || (flat.contains("Do you want to") && flat.contains("❯ 1."))
        }
        "codex" => {
            let flat = joined(screen);
            flat.contains("Would you like to") && flat.contains("› 1.")
        }
        "devin" => devin_blocked(screen),
        // The OpenCode permission screen is not measured yet — see
        // docs/studio/runners.md.
        "opencode" => {
            let flat = joined(screen);
            flat.contains("Permission required") && flat.contains("Allow once")
        }
        "cursor" => cursor_trust_dialog(screen) || cursor_approval_picker(screen),
        // Command Code's permission prompt: the numbered picker under
        // its `needs to …` sentence (S12a). The folder-trust dialog is
        // blocked too — `--trust` should keep it away, so it is the
        // user's prompt, never answered.
        "cmd" => cmd_trust_dialog(screen) || cmd_picker(screen),
        "fake" => fake_state(screen) == Some(Work::Blocked),
        // cline's approval block (S14a section 4) — it should never
        // appear under `--yolo`, which forces auto-approve on; when
        // it does, the user switched it off and the prompt is
        // theirs. Nothing is pressed either way (`permit`).
        "cline" => cline_approval(screen),
        "agy" => agy_trust_dialog(screen) || agy_command_picker(screen),
        _ => false,
    }
}

/// Recognised non-permission interactive picker on `runner`'s screen
/// (`question`, R7b): e.g. agy's plan-mode `Question N/M:` picker.
pub(crate) fn recognized_question(runner: &str, screen: &str) -> bool {
    match runner {
        "agy" => agy_plan_question(screen),
        _ => false,
    }
}

/// cline's `Cline needs permission` / `Approve tool call?` block.
/// Under `--yolo` it must not appear: auto-approve is forced on, so a
/// screen still showing it means the user switched the mode off — the
/// prompt is theirs and the stage never answers it.
pub(crate) fn cline_approval(text: &str) -> bool {
    let flat = joined(text);
    flat.contains("Cline needs permission") && flat.contains("Approve tool call?")
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
        // cline's turn: a braille spinner line — `⠏ Thinking...
        // (esc to cancel)` — above the composer, which stays on
        // screen while it works (S14a section 7). The idle composer
        // alone is not a turn; `(esc to cancel)` is the stable half
        // of the line.
        "cline" => screen.contains("(esc to cancel)") || screen.lines().any(cline_spinner_line),
        "agy" => agy_working(screen),
        "fake" => fake_state(screen) == Some(Work::Working),
        _ => false,
    }
}

/// cline's running turn: a braille glyph from `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`, a
/// space, then the turn's word — `⠏ Thinking... (esc to cancel)`. The
/// glyph is required: a transcript line starting `Thinking:` is a
/// finished thought, not a turn.
fn cline_spinner_line(l: &str) -> bool {
    const GLYPHS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let mut parts = l.trim().splitn(2, ' ');
    let (Some(glyph), Some(rest)) = (parts.next(), parts.next()) else {
        return false;
    };
    glyph.chars().count() == 1
        && GLYPHS.contains(&glyph.chars().next().unwrap_or_default())
        && !rest.trim().is_empty()
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

/// agy's folder-trust dialog on first launch in an untrusted workspace.
pub(crate) fn agy_trust_dialog(screen: &str) -> bool {
    let flat = joined(screen);
    flat.contains("Accessing workspace:")
        && flat.contains("Do you trust the contents of this project?")
        && flat.contains("Yes, I trust this folder")
}

/// agy's command permission picker (`request-review` or `strict`).
pub(crate) fn agy_command_picker(screen: &str) -> bool {
    let flat = joined(screen);
    flat.contains("Requesting permission for:")
        && flat.contains("Run this command?")
        && flat.contains("1. Yes, run command")
        && flat.contains("↑/↓ Navigate")
}

/// agy's plan-mode `ask_question` picker (`Question N/M: ...`).
pub(crate) fn agy_plan_question(screen: &str) -> bool {
    let flat = joined(screen);
    screen.lines().any(agy_question_line)
        && flat.contains("↑/↓ Navigate")
        && flat.contains("enter Select")
}

/// `Question <N>/<M>:` line heading an agy plan-mode question picker.
pub(crate) fn agy_question_line(l: &str) -> bool {
    let Some(rest) = l.trim_start().strip_prefix("Question ") else {
        return false;
    };
    let Some((head, _)) = rest.split_once(':') else {
        return false;
    };
    let Some((n, m)) = head.split_once('/') else {
        return false;
    };
    !n.is_empty()
        && !m.is_empty()
        && n.bytes().all(|b| b.is_ascii_digit())
        && m.bytes().all(|b| b.is_ascii_digit())
}

/// agy 1.5.16 working turn (R7a): braille spinner + two spaces + verb,
/// running background-task row or task bar, or `esc to cancel` footer.
fn agy_working(screen: &str) -> bool {
    screen.lines().any(agy_spinner_line)
        || screen.lines().any(agy_bg_task_line)
        || screen.lines().any(agy_tasks_bar)
        || screen
            .lines()
            .any(|l| l.trim_start().starts_with("esc to cancel"))
}

/// Braille spinner glyph (`\u{2801}..=\u{28ff}`) followed by two
/// spaces and a verb (`Generating...`, `Reading file...`,
/// `Running command... Ns`, `Editing files...`).
fn agy_spinner_line(l: &str) -> bool {
    let t = l.trim_start();
    let mut chars = t.chars();
    let Some(glyph) = chars.next() else {
        return false;
    };
    if !('\u{2801}'..='\u{28ff}').contains(&glyph) {
        return false;
    }
    let Some(verb) = chars.as_str().strip_prefix("  ") else {
        return false;
    };
    verb.trim_start()
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
}

/// Background-task row `● [HH:MM:SS] <cmd> running` shown when a long
/// command auto-backgrounds after ~10 s.
fn agy_bg_task_line(l: &str) -> bool {
    let Some(rest) = l.trim_start().strip_prefix("● [") else {
        return false;
    };
    let Some((stamp, tail)) = rest.split_once(']') else {
        return false;
    };
    let b = stamp.as_bytes();
    b.len() == 8
        && b[2] == b':'
        && b[5] == b':'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 2 || i == 5 || c.is_ascii_digit())
        && tail.trim_end().ends_with("running")
}

/// Bottom shortcut bar showing `task(s) · /tasks` during background work.
fn agy_tasks_bar(l: &str) -> bool {
    let t = l.trim_start();
    t.starts_with("? for shortcuts") && t.contains("task(s) · /tasks")
}
