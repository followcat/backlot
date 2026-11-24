//! Unit and cross-runner regression tests for D2's generic fallback
//! dialog rule: positive coverage over option-cursor + sibling and
//! option-cursor + footer shapes (including screens with `esc to
//! cancel`), plus false-positive guards for idle composers, transcript
//! prose, fenced code blocks, and regression over every runner's idle
//! and working screens.

use backlot_model::summary::Work;

use super::dialog::generic_dialog;
use super::work_state;
use crate::prompt_fixtures::{CMD_MEDIUM, CMD_NARROW, CMD_WIDE};

/// Cursor + sibling option works across all four cursor glyphs (`❯`,
/// `>`, `→`, `›`) and wins before `working` even when `esc to cancel`
/// or `esc to interrupt` remains on the bottom bar.
#[test]
fn cursor_and_sibling_option_blocks_across_runners() {
    for marker in ['❯', '>', '→', '›'] {
        let dialog = format!(
            "Which format should we use?\n\
             {marker} 1. JSON\n\
             \x20 2. YAML\n"
        );
        assert!(generic_dialog(&dialog), "marker {marker:?}");
        assert_eq!(work_state("unknown", &dialog), Work::Blocked);

        let with_esc = format!("{dialog}\nesc to cancel\n");
        assert_eq!(work_state("agy", &with_esc), Work::Blocked);
        assert_eq!(
            work_state("claude", &format!("{dialog}\nesc to interrupt\n")),
            Work::Blocked
        );
    }
    // Single-option cursor paired with an unknown dialog on `fake` also
    // overrides `fake-work: working` or `waiting`.
    let fake_picker = "fake runner: actor-a\n> Allow MCP call\n  Deny\n";
    assert_eq!(work_state("fake", fake_picker), Work::Blocked);
}

/// A single option-cursor line paired with any documented dialog footer
/// blocks as a dialog; `esc to cancel` alone never matches.
#[test]
fn cursor_paired_with_dialog_footer_blocks() {
    for footer in [
        "  Enter to select · Esc to cancel",
        "  ↑/↓ Navigate · enter Select",
        "  enter Confirm",
        "  ↑/↓ Navigate",
        "Continue anyway? (y/n)",
        "Proceed with overwrite? [Y/n]",
        "Do you want to apply this migration?",
    ] {
        let screen = if footer.starts_with("Do you want to")
            || footer.contains("(y/n)")
            || footer.contains("[Y/n]")
        {
            format!("{footer}\n> Yes, proceed\n")
        } else {
            format!("Confirm selection\n❯ Proceed\n{footer}\n")
        };
        assert!(generic_dialog(&screen), "footer {footer:?}");
        assert_eq!(work_state("agy", &screen), Work::Blocked, "{footer:?}");
    }

    assert!(!generic_dialog("esc to cancel\n"));
    assert_eq!(
        work_state("agy", "esc to cancel   accept-edits · low\n"),
        Work::Working
    );
    assert_eq!(work_state("unknown", "esc to cancel\n"), Work::Waiting);
}

/// False-positive guards: composer boxes, short-turn scrollback above
/// an active composer box, fenced code blocks inside/spanning the tail,
/// markdown blockquotes, transcript bullets, and options buried >12
/// non-blank lines above the tail must all stay false.
#[test]
fn false_positive_guards_reject_composers_prose_and_code_blocks() {
    // 1. Composer boxes with placeholder/draft text, even if earlier
    // assistant prose asked a `Do you want to ...` question.
    let agy_box_with_prose = "\
        ● Everything passes.\n\
        \x20 Do you want to proceed with a PR?\n\
        ────────────────────────────────────────\n\
        > Accept-edits mode: Describe your edit\n\
        ────────────────────────────────────────\n\
        ? for shortcuts\n";
    assert!(!generic_dialog(agy_box_with_prose));
    assert_eq!(work_state("agy", agy_box_with_prose), Work::Waiting);

    // 2. Short turn where the prior user prompt and 2 indented reply
    // lines remain inside the 12-line tail above the bottom composer.
    let short_turn_idle = "\
        > Summarize README.md\n\
        \x20 First bullet point\n\
        \x20 Second bullet point\n\
        ────────────────────────────────────────\n\
        > Accept-edits mode: Describe your edit\n\
        ────────────────────────────────────────\n\
        ? for shortcuts   accept-edits · low\n";
    assert!(!generic_dialog(short_turn_idle));
    assert_eq!(work_state("agy", short_turn_idle), Work::Waiting);

    // 3. Fenced code block inside or starting above the 12-line window.
    let in_code_fence = "\
        Here is the menu I rendered:\n\
        ```text\n\
        > 1. Option A\n\
        \x20 2. Option B\n\
        ↑/↓ Navigate · Enter to select\n\
        ```\n";
    assert!(!generic_dialog(in_code_fence));
    assert_eq!(work_state("unknown", in_code_fence), Work::Waiting);

    let unclosed_fence_above_tail = "\
        ```markdown\n\
        line 1\nline 2\nline 3\nline 4\nline 5\n\
        line 6\nline 7\nline 8\nline 9\nline 10\n\
        > 1. Yes\n\
        \x20 2. No\n\
        Enter to select\n";
    assert!(!generic_dialog(unclosed_fence_above_tail));

    // 4. Markdown blockquote adjacent `> ` lines + question phrase.
    let blockquote = "\
        Do you want to check the note?\n\
        > Quote line one\n\
        > Quote line two\n";
    assert!(!generic_dialog(blockquote));

    // 5. Bullet list under a user prompt (no bottom composer): `•` is
    // rejected as a sibling option.
    let prompt_with_bullets = "> List changes\n  • Added foo.rs\n  • Added bar.rs\n";
    assert!(!generic_dialog(prompt_with_bullets));

    // 6. Real picker buried above 12 non-blank lines of newer output is
    // outside the tail window.
    let mut buried = String::from("> 1. Old yes\n  2. Old no\n");
    for k in 1..=12 {
        buried.push_str(&format!("line {k}\n"));
    }
    assert!(!generic_dialog(&buried));

    // 7. `❯` cursor alone without text, sibling, or footer.
    assert!(!generic_dialog("───\n❯\n───\n"));
    assert!(!generic_dialog("❯ 1. Yes\n"));
}

/// Regression over every runner's existing idle and working screens:
/// none of them trigger `generic_dialog`, and `work_state` keeps every
/// fixture's expected state (`Waiting` or `Working`).
#[test]
fn generic_rule_never_breaks_any_runners_idle_or_working_fixtures() {
    let non_blocked = [
        // claude
        (
            "claude",
            "✻ Orbiting… (12s · ↑ 1.2k tokens · esc to interrupt)\n",
            Work::Working,
        ),
        (
            "claude",
            "· Schlepping… (17s · ↓ 95 tokens)\n",
            Work::Working,
        ),
        ("claude", "✻ Churned for 2s · done 3:07 PM\n", Work::Waiting),
        ("claude", "· a bullet in the transcript\n", Work::Waiting),
        ("claude", "prompt>\n", Work::Waiting),
        ("claude", "❯ 1. Yes\n", Work::Waiting),
        // codex
        ("codex", "Thinking · 2s (esc to interrupt)\n", Work::Working),
        (
            "codex",
            "Would you like to run this?\n  1. yes\n",
            Work::Waiting,
        ),
        ("codex", "", Work::Waiting),
        // devin
        (
            "devin",
            "Thinking · 2s (esc twice to interrupt)\n",
            Work::Working,
        ),
        (
            "devin",
            "❭ Allow ← ◼◼◼◼ → Once     Exec(flock)\n",
            Work::Waiting,
        ),
        // opencode
        ("opencode", "esc interrupt\n", Work::Working),
        ("opencode", "Permission required\n", Work::Waiting),
        // cursor
        (
            "cursor",
            "  → Ask anything\n ctrl+c to stop\n",
            Work::Working,
        ),
        ("cursor", "  ⠘⠤ Running  44 tokens\n", Work::Working),
        (
            "cursor",
            "  Cursor Agent\n\
             \x20 v2026.11.05-86047df\n\
             \x20 Tip: Use /plan to plan execution.\n\
             ▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄\n\
             \x20 → Plan, search, build anything\n\
             ▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀\n\
             \x20 Composer 2.5\n\
             \x20 /home/user/acme-shop · main\n",
            Work::Waiting,
        ),
        ("cursor", " →\n", Work::Waiting),
        // cmd
        (
            "cmd",
            "◇ Conjuring… esc to interrupt • 5s • ↓ 219\n",
            Work::Working,
        ),
        ("cmd", "✻ Worked for 3s\n", Work::Waiting),
        ("cmd", "❯ Ask your question...\n", Work::Waiting),
        // cline
        (
            "cline",
            "  ❯ Think briefly\n\
             \x20 ▶ Thinking: The directory is empty.\n\
             ────────────────────────────────────\n\
             ❯ Ask anything...\n\
             ────────────────────────────────────\n\
             \x20 ClinePass: DeepSeek V4.1 (medium) ○ Plan ● Act\n\
             \x20 acme-shop\n\
             \x20 ▶▶ Auto-approve all enabled (Shift+Tab)\n",
            Work::Waiting,
        ),
        (
            "cline",
            "  ❯ Think briefly\n\
             ⠏ Thinking... (esc to cancel)\n\
             ────────────────────────────────────\n\
             ❯ Ask anything...\n\
             ────────────────────────────────────\n",
            Work::Working,
        ),
        // agy R7a non-blocked fixtures
        (
            "agy",
            include_str!("../../tests/fixtures/agy/working-turn.txt"),
            Work::Working,
        ),
        (
            "agy",
            include_str!("../../tests/fixtures/agy/working-streaming.txt"),
            Work::Working,
        ),
        (
            "agy",
            include_str!("../../tests/fixtures/agy/long-cpu.txt"),
            Work::Working,
        ),
        (
            "agy",
            include_str!("../../tests/fixtures/agy/long-flock.txt"),
            Work::Working,
        ),
        (
            "agy",
            include_str!("../../tests/fixtures/agy/long-sleep.txt"),
            Work::Working,
        ),
        (
            "agy",
            include_str!("../../tests/fixtures/agy/idle-launch.txt"),
            Work::Waiting,
        ),
        (
            "agy",
            include_str!("../../tests/fixtures/agy/idle-after-turn.txt"),
            Work::Waiting,
        ),
        (
            "agy",
            include_str!("../../tests/fixtures/agy/resume.txt"),
            Work::Waiting,
        ),
        (
            "agy",
            include_str!("../../tests/fixtures/agy/perm-deny-push.txt"),
            Work::Waiting,
        ),
        // fake
        (
            "fake",
            "fake runner: actor-a of acme-shop\nfake-work: working\n",
            Work::Working,
        ),
        (
            "fake",
            "fake runner: actor-a of acme-shop\nfake-work: waiting\n",
            Work::Waiting,
        ),
    ];
    for (runner, screen, want) in non_blocked {
        assert!(
            !generic_dialog(screen),
            "{runner} idle/working matched generic_dialog:\n{screen}"
        );
        assert_eq!(
            work_state(runner, screen),
            want,
            "{runner} non-blocked fixture"
        );
    }

    // Known narrow/medium/wide cmd blocked fixtures still stay Blocked.
    for screen in [CMD_NARROW, CMD_MEDIUM, CMD_WIDE] {
        assert_eq!(work_state("cmd", screen), Work::Blocked);
    }
}
