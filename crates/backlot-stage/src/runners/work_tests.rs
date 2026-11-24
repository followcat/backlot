//! `work_state` table tests: each runner's blocked and working
//! markers from its real screen, blocked beating working, and
//! anything unmatched reading `Waiting`.

use backlot_model::summary::Work;

use super::work_state;
use crate::prompt_fixtures::CMD_NARROW;

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
/// `→ Run (once) (y)` picker. The shared picker footer — a `→`
/// option-cursor line plus `Run Everything (shift+tab)` — is the
/// blocked marker (S12f): the question's own words stay out of it,
/// so every approval kind blocks, named or not.
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
    // Without the option cursor the picker is waiting; without the
    // `Run Everything` footer `cursor_approval_picker` itself no
    // longer claims it (though D2's generic rule still blocks on the
    // multi-option picker).
    assert_eq!(
        work_state("cursor", &screen.replacen("→ Run (once) (y)", "", 1)),
        Work::Waiting
    );
    for cut in ["Run Everything", "(shift+tab)"] {
        assert!(
            !super::work::cursor_approval_picker(&screen.replacen(cut, "", 1)),
            "without {cut:?}"
        );
    }
    // The question's words are not the marker — a picker kind not
    // yet named still blocks.
    for cut in ["Run this command?", "Not in allowlist: wc"] {
        assert_eq!(
            work_state("cursor", &screen.replacen(cut, "", 1)),
            Work::Blocked,
            "without {cut:?}"
        );
    }
}

/// The S12e capture's file picker, neutralised: `Write to this
/// file?` and the other approval questions share the same footer,
/// so they block too — `permit` decides whose each is. The
/// composer's bare ` →` is not an option cursor.
#[test]
fn cursor_file_picker_is_blocked() {
    let write = "some output\n\
                 ─────────────────────────────────────────\n\
                 \x20  .../work/scratchpad/plan.md -1\n\
                 \x20  ─────────────────────────\n\
                 \x20  + a new line\n\
                 \x20Write to this file?\n\
                 \x20 in /home/user/work/scratchpad/plan.md\n\
                 \x20 → Proceed (y)\n\
                 \x20   Reject & propose changes (esc or n or p)\n\
                 \x20   Add Write(/home/user/work/scratchpad/plan.md) to allowlist? (tab)\n\
                 \x20   Run Everything (shift+tab)\n\
                 \x20 ←/→ to switch approval\n\
                 \x20 →\n";
    assert_eq!(work_state("cursor", write), Work::Blocked);
    for question in [
        "Write to this file?",
        "Read this file?",
        "Delete this file?",
        "Proceed with this edit?",
    ] {
        assert_eq!(
            work_state(
                "cursor",
                &write.replacen("Write to this file?", question, 1)
            ),
            Work::Blocked,
            "{question}"
        );
    }
    // The bare composer arrow alone is not a picker.
    assert_eq!(work_state("cursor", " →\n"), Work::Waiting);
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

/// The S12a picker on a 21-column terminal: the sentence and the
/// option texts wrap across lines, and the picker still reads
/// blocked.
#[test]
fn cmd_picker_blocks_at_a_narrow_width() {
    assert_eq!(work_state("cmd", CMD_NARROW), Work::Blocked);
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

/// cline's idle screen (S14a's capture, neutralised): the composer
/// `❯ Ask anything...` between two full-width `─` rules, the footer
/// under it, a finished thought above — no spinner, so waiting.
const CLINE_IDLE: &str = "\
 \x20❯ Think briefly about how to list this directory, then run the shell command: ls -la\n\
 \x20\n\
 \x20▶ Thinking: The directory is empty. Report.\n\
 \x20\n\
 ────────────────────────────────────\n\
 ❯ Ask anything...\n\
 ────────────────────────────────────\n\
 \x20ClinePass: DeepSeek V4.1 Flash 1017 (medium) ██████ (7,267)    ○ Plan ● Act (Tab)\n\
 \x20acme-shop\n\
 \x20▶▶ Auto-approve all enabled (Shift+Tab)\n";

/// cline's working screen: the same composer with the braille spinner
/// line above it — `⠏ Thinking... (esc to cancel)`.
const CLINE_WORKING: &str = "\
 \x20❯ Think briefly about how to list this directory, then run the shell command: ls -la\n\
 \x20\n\
 ⠏ Thinking... (esc to cancel)\n\
 \x20\n\
 ────────────────────────────────────\n\
 ❯ Ask anything...\n\
 ────────────────────────────────────\n\
 \x20ClinePass: DeepSeek V4.1 Flash 1017 (medium) ██████ (0)    ○ Plan ● Act (Tab)\n";

/// cline's approval block (S14a's capture, neutralised): it shows only
/// with auto-approve off — under `--yolo` it must never appear.
const CLINE_APPROVAL: &str = "\
 \x20❯ Run the shell command: ls\n\
 \x20\n\
 \x20▶ Thinking: Just run ls.\n\
 ────────────────────────────────────\n\
 \x20Cline needs permission\n\
 \x20\n\
 \x20Approve tool call?\n\
 \x20\n\
 \x20run_commands\n\
 \x20\n\
 \x20  $ ls\n\
 \x20\n\
 \x20 [y] Approve   [n] Deny\n";

/// The idle capture is waiting: the composer is on screen with no
/// spinner line. The fresh-launch composer (`❯ What can I do for
/// you?`) reads the same.
#[test]
fn cline_idle_is_waiting() {
    assert_eq!(work_state("cline", CLINE_IDLE), Work::Waiting);
    let fresh = CLINE_IDLE.replace("Ask anything...", "What can I do for you?");
    assert_eq!(work_state("cline", &fresh), Work::Waiting);
    // A finished thought above the composer is not a turn.
    assert_eq!(
        work_state(
            "cline",
            " ▶ Thinking: The directory is empty. Report.\n ❯ Ask anything...\n"
        ),
        Work::Waiting
    );
}

/// The working capture is a turn even though the composer stays on
/// screen; the braille line carries it.
#[test]
fn cline_working() {
    assert_eq!(work_state("cline", CLINE_WORKING), Work::Working);
    assert_eq!(
        work_state("cline", " ⠋ Thinking... (esc to cancel)\n"),
        Work::Working
    );
    // The marker alone, with no glyph on the line, still counts.
    assert_eq!(
        work_state("cline", " Thinking... (esc to cancel)\n"),
        Work::Working
    );
    // A spinner glyph with nothing after it is not the turn.
    assert_eq!(work_state("cline", " ⠏\n"), Work::Waiting);
}

/// The approval capture is blocked — and blocked beats the spinner a
/// stale turn left above it.
#[test]
fn cline_blocked() {
    assert_eq!(work_state("cline", CLINE_APPROVAL), Work::Blocked);
    let stale = format!(" ⠏ Thinking... (esc to cancel)\n{CLINE_APPROVAL}");
    assert_eq!(work_state("cline", &stale), Work::Blocked);
    // The banner without the question is not the prompt.
    assert_eq!(
        work_state("cline", " Cline needs permission\n"),
        Work::Waiting
    );
    assert_eq!(work_state("cline", " Approve tool call?\n"), Work::Waiting);
}

/// Every R7a agy 1.5.16 fixture under `tests/fixtures/agy/` maps to its
/// measured state: pickers and trust dialog block (even with `esc to
/// cancel` at the bottom), active spinner and backgrounded tasks work,
/// and idle/post-turn/deny screens wait.
#[test]
fn agy_fixtures_map_to_measured_states() {
    let cases = [
        (
            "folder-trust",
            include_str!("../../tests/fixtures/agy/folder-trust.txt"),
            Work::Blocked,
        ),
        (
            "perm-command",
            include_str!("../../tests/fixtures/agy/perm-command.txt"),
            Work::Blocked,
        ),
        (
            "perm-strict",
            include_str!("../../tests/fixtures/agy/perm-strict.txt"),
            Work::Blocked,
        ),
        (
            "plan-question",
            include_str!("../../tests/fixtures/agy/plan-question.txt"),
            Work::Blocked,
        ),
        (
            "working-turn",
            include_str!("../../tests/fixtures/agy/working-turn.txt"),
            Work::Working,
        ),
        (
            "working-streaming",
            include_str!("../../tests/fixtures/agy/working-streaming.txt"),
            Work::Working,
        ),
        (
            "long-cpu",
            include_str!("../../tests/fixtures/agy/long-cpu.txt"),
            Work::Working,
        ),
        (
            "long-flock",
            include_str!("../../tests/fixtures/agy/long-flock.txt"),
            Work::Working,
        ),
        (
            "long-sleep",
            include_str!("../../tests/fixtures/agy/long-sleep.txt"),
            Work::Working,
        ),
        (
            "idle-launch",
            include_str!("../../tests/fixtures/agy/idle-launch.txt"),
            Work::Waiting,
        ),
        (
            "idle-after-turn",
            include_str!("../../tests/fixtures/agy/idle-after-turn.txt"),
            Work::Waiting,
        ),
        (
            "resume",
            include_str!("../../tests/fixtures/agy/resume.txt"),
            Work::Waiting,
        ),
        (
            "perm-deny-push",
            include_str!("../../tests/fixtures/agy/perm-deny-push.txt"),
            Work::Waiting,
        ),
    ];
    for (name, screen, want) in cases {
        assert_eq!(work_state("agy", screen), want, "agy fixture {name}");
    }
    // `▸ Thought for 3s, 42 tokens` and `? for shortcuts` alone are
    // post-turn waiting, never working.
    assert_eq!(
        work_state(
            "agy",
            " ▸ Thought for 3s, 42 tokens\n > Accept-edits mode\n ? for shortcuts\n"
        ),
        Work::Waiting
    );
}
