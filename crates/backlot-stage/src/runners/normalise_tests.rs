//! Per-runner screen normalisation tests: spinners, elapsed counters,
//! `Thought for Ns, N tokens`, token counters, and status clocks strip
//! identically while real content changes remain distinct.

use super::normalise_screen;

#[test]
fn agy_spinners_timers_thoughts_and_bg_clocks_normalise_equal() {
    let f1 = "\
        ⢿  Running command... 1s\n\
        ▸ Thought for 2s, 199 tokens\n\
        ● [16:36:40] timeout 660 sh -c 'while :; do :; done' running\n\
        ? for shortcuts   accept-edits · Gemini 4 Hydrogen · low · 1 task(s) · /tasks\n";
    let f2 = "\
        ⣷  Running command... 45s\n\
        ▸ Thought for 1m 20s, 2,340 tokens\n\
        ● [16:37:12] timeout 660 sh -c 'while :; do :; done' running\n\
        ? for shortcuts   accept-edits · Gemini 4 Hydrogen · low · 1 task(s) · /tasks\n";
    assert_eq!(normalise_screen("agy", f1), normalise_screen("agy", f2));

    // Real text change still differs.
    let f3 = f2.replace("timeout 660", "cargo test");
    assert_ne!(normalise_screen("agy", f2), normalise_screen("agy", &f3));
}

#[test]
fn claude_spinners_timers_tokens_and_clocks_normalise_equal() {
    let f1 = "· Schlepping… (12s · ↑ 1.2k tokens · esc to interrupt)\n";
    let f2 = "✻ Orbiting… (1m 20s · ↓ 95 tokens · esc to interrupt)\n";
    let f3 = "✽ Percolating… (4s · esc to interrupt)\n";
    assert_eq!(
        normalise_screen("claude", f1),
        normalise_screen("claude", f2)
    );
    assert_eq!(
        normalise_screen("claude", f2),
        normalise_screen("claude", f3)
    );

    let done1 = "✻ Churned for 2s · done 3:07 PM\n";
    let done2 = "✶ Churned for 14s · done 3:12 PM\n";
    assert_eq!(
        normalise_screen("claude", done1),
        normalise_screen("claude", done2)
    );
}

#[test]
fn cmd_cursor_cline_codex_devin_normalise_animated_lines() {
    assert_eq!(
        normalise_screen("cmd", "◇ Conjuring… esc to interrupt • 5s • ↓ 219\n"),
        normalise_screen("cmd", "◇ Brewing… esc to interrupt • 1m 20s • ↓ 1.4k\n"),
    );
    assert_eq!(
        normalise_screen("cmd", "✻ Worked for 3s\n"),
        normalise_screen("cmd", "✶ Worked for 1m 5s\n"),
    );
    assert_eq!(
        normalise_screen("cursor", "  ⠠⠜ Composing\n"),
        normalise_screen("cursor", "  ⠰⠰ Composing 147 tokens\n"),
    );
    assert_eq!(
        normalise_screen("cline", "⠋ Thinking... 3s (esc to cancel)\n"),
        normalise_screen("cline", "⠏ Thinking... 1m 20s (esc to cancel)\n"),
    );
    assert_eq!(
        normalise_screen("codex", "Thinking · 2s (esc to interrupt)\n"),
        normalise_screen("codex", "Thinking · 1m 5s (esc to interrupt)\n"),
    );
    assert_eq!(
        normalise_screen("devin", "Thinking · 2s (esc twice to interrupt)\n"),
        normalise_screen("devin", "Thinking · 58s (esc twice to interrupt)\n"),
    );
}

#[test]
fn fake_strips_braille_claude_spinners_counters_and_status_clocks() {
    let f1 = "fake-work: working\n⠋ Thinking... 1s (↑ 10 tokens)\n14:02:01 · running\n";
    let f2 = "fake-work: working\n⠙ Thinking... 1m 20s (↓ 240 tokens)\n14:03:45 · running\n";
    let f3 = "fake-work: working\n✻ Thinking... 9s\n14:04:00 · running\n";
    assert_eq!(normalise_screen("fake", f1), normalise_screen("fake", f2));
    assert_eq!(normalise_screen("fake", f2), normalise_screen("fake", f3));
}

#[test]
fn real_code_and_command_numbers_are_not_stripped() {
    let a = "src/main.rs: sleep 660\nlet x = foo_12s;\n";
    let b = "src/main.rs: sleep 661\nlet x = foo_12s;\n";
    assert_ne!(normalise_screen("agy", a), normalise_screen("agy", b));
    assert!(normalise_screen("agy", a).contains("foo_12s"));
    assert!(normalise_screen("agy", a).contains("src/main.rs"));
}
