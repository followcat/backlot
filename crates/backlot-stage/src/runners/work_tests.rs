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

/// Devin's picker: `Allow` carries a `❭` cursor, the arrows hold the
/// scope (`Once`, `Session`, …) and `Deny` sits under it — the shape
/// read off a live 3000.12.32 prompt. Working is any `to interrupt`
/// line.
#[test]
fn devin_working_and_blocked() {
    assert_eq!(
        work_state("devin", "Thinking · 2s (esc twice to interrupt)\n"),
        Work::Working
    );
    let blocked = " ● Running command\n\
                   │ $ cd /home/user/acme-shop && cargo test\n\
                   ▲ Approval required\n\
                     This action requires your approval.\n\
                   ❭ Allow ← ◼◼◼◼ → Once     Exec(flock)\n\
                     Deny\n\
                   ↑↓ select · ←→ 1-4 scope · e edit · ↵ confirm · esc cancel\n";
    assert_eq!(work_state("devin", blocked), Work::Blocked);
    // A `Session` scope is the same prompt; a `Bypass mode` third
    // option and the cursor moved to `Deny` read blocked too.
    let session = blocked.replace("→ Once", "→ Session");
    assert_eq!(work_state("devin", &session), Work::Blocked);
    let bypass = blocked.replace("  Deny", "  Bypass mode\n  Deny");
    assert_eq!(work_state("devin", &bypass), Work::Blocked);
    let cursor_on_deny = blocked
        .replace("❭ Allow", "  Allow")
        .replace("  Deny", "❭ Deny");
    assert_eq!(work_state("devin", &cursor_on_deny), Work::Blocked);
    // Only half the pair is not a prompt.
    assert_eq!(work_state("devin", "  Deny\n"), Work::Waiting);
    assert_eq!(
        work_state("devin", "❭ Allow ← ◼◼◼◼ → Once     Exec(flock)\n"),
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

/// cursor-agent's permission prompt on 2026.11.05 — the S11a
/// capture, neutralised: the command under a full-width `─` rule,
/// then `Run this command?`, the `Not in allowlist:` line and the
/// `→ Run (once) (y)` picker. Blocked needs all three lines.
#[test]
fn cursor_blocked() {
    let screen = "  $ backlot studios 1.2s\n\
                  \x20   backlot stub studios\n\
                  ─────────────────────────────────────────\n\
                  \x20$  wc -l README.md in .\n\n\
                  \x20Run this command?\n\
                  \x20Not in allowlist: wc\n\
                  \x20 → Run (once) (y)\n\
                  \x20   Add Shell(wc) to allowlist? (tab)\n\
                  \x20   Run Everything (shift+tab)\n\
                  \x20   Skip & tell the agent what to do instead (esc or n)\n";
    assert_eq!(work_state("cursor", screen), Work::Blocked);
    // Any missing piece is not the prompt.
    for cut in ["Run this command?", "Not in allowlist:", "→ Run (once) (y)"] {
        assert_eq!(
            work_state("cursor", &screen.replacen(cut, "", 1)),
            Work::Waiting,
            "without {cut:?}"
        );
    }
}

/// cursor's workspace-trust dialog is blocked too — `--trust`
/// should keep it away, so a role still showing it is the user's
/// prompt (S11a capture, neutralised).
#[test]
fn cursor_trust_dialog_is_blocked() {
    let trust = "  │  ⚠ Workspace Trust Required\n\
                 \x20 │\n\
                 \x20 │  Cursor Agent can execute code and access files in this directory.\n\
                 \x20 │  Do you trust the contents of this directory?\n\
                 \x20 │    /home/user/acme-shop\n\
                 \x20 │    [a] Trust this workspace\n\
                 \x20 │    [q] Quit\n";
    assert_eq!(work_state("cursor", trust), Work::Blocked);
}

/// cursor's running turn on 2026.11.05: `ctrl+c to stop` in the
/// input row, or one/two braille glyphs then `Composing` or
/// `Running … tokens`.
#[test]
fn cursor_working() {
    for screen in [
        "  → Ask anything\n ctrl+c to stop\n",
        "  ⠠⠜ Composing\n",
        "  ⠘⠤ Composing\n",
        "  ⠰⠰ Composing 147 tokens\n",
        "  ⠘⠤ Running  44 tokens\n",
    ] {
        assert_eq!(work_state("cursor", screen), Work::Working, "{screen:?}");
    }
    // A transcript's `Running the…` is not the spinner, and the
    // token line needs the braille glyph.
    for screen in [
        "│ Running the shell command \"wc -l README.md\".\n",
        "Running  44 tokens\n",
    ] {
        assert_eq!(work_state("cursor", screen), Work::Waiting, "{screen:?}");
    }
}

/// cursor's idle composer (S11a capture, neutralised): the `→`
/// input hint, model row and cwd — nothing moves, so waiting.
#[test]
fn cursor_idle_is_waiting() {
    let idle = "  Cursor Agent\n\
                \x20 v2026.11.05-86047df\n\
                \x20 Tip: Use /plan to plan execution and reach the right outcome faster.\n\
                ▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄\n\
                \x20 → Plan, search, build anything\n\
                ▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀\n\
                \x20 Composer 2.5\n\
                \x20 /home/user/acme-shop · main\n";
    assert_eq!(work_state("cursor", idle), Work::Waiting);
}

/// cmd's permission prompt (the S12a capture, neutralised): the
/// `Execute Shell Command` title, the `needs to execute` sentence
/// and the numbered picker under a full-width `─` rule. Blocked
/// needs the sentence AND the `❯ 1.` cursor — and it beats a
/// working line still on screen.
#[test]
fn cmd_blocked() {
    let screen = "◇ Conjuring… esc to interrupt • 5s • ↓ 219\n\
                  ─────────────────────────────────────────\n\
                  Execute Shell Command\n\
                  Command Code needs to execute this shell command:\n\
                  \x20 touch allow-ok.txt\n\
                  ❯ 1. Yes\n\
                  \x20 2. Yes, don't ask again for `touch` commands in this project\n\
                  \x20 3. No, tell Command Code what to do differently\n";
    assert_eq!(work_state("cmd", screen), Work::Blocked);
    // Other tool prompts read the same way.
    let run = screen.replacen(
        "needs to execute this shell command:",
        "needs to run this command:",
        1,
    );
    assert_eq!(work_state("cmd", &run), Work::Blocked);
    let edit = screen.replacen("needs to execute", "wants to make this edit to", 1);
    assert_eq!(work_state("cmd", &edit), Work::Blocked);
    // The sentence without the picker cursor is not the prompt.
    assert_eq!(
        work_state("cmd", "Command Code needs to execute this shell command:\n"),
        Work::Waiting
    );
    // Nor a cursor alone.
    assert_eq!(work_state("cmd", "❯ 1. Yes\n"), Work::Waiting);
}

/// cmd's folder-trust dialog is blocked too — `--trust` should
/// keep it away, so a role still showing it is the user's prompt
/// (S12a capture, neutralised).
#[test]
fn cmd_trust_dialog_is_blocked() {
    let trust = "  Do you trust the files in this folder?\n\
                 \x20 /home/user/acme-shop\n\
                 \x20 ❯ 1. Yes, proceed\n\
                 \x20   2. No, exit\n";
    assert_eq!(work_state("cmd", trust), Work::Blocked);
}

/// cmd's working turn is the status line's `esc to interrupt`;
/// the post-turn `✻ Worked for`/`✻ Thought for` lines and the
/// idle composer are waiting.
#[test]
fn cmd_working_and_idle() {
    assert_eq!(
        work_state("cmd", "◇ Conjuring… esc to interrupt • 5s • ↓ 219\n"),
        Work::Working
    );
    for screen in [
        "✻ Worked for 3s\n",
        "✻ Thought for 1 second [ctrl+o to expand]\n",
        "❯ Ask your question...\n",
    ] {
        assert_eq!(work_state("cmd", screen), Work::Waiting, "{screen:?}");
    }
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
