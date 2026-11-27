use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::*;
use crate::chrome::palette::Palette;

/// Every glyph in the vocabulary is exactly one column wide — the
/// spec's hard rule (§1) so no surface ever misaligns.
#[test]
fn every_glyph_is_single_width() {
    let palette = Palette::from_name("backlot").unwrap();
    for spin in 0..10 {
        for person in [
            Person::Working,
            Person::Idle,
            Person::Ready,
            Person::Slot,
            Person::UserPrompt,
            Person::DirPrompt,
            Person::NeedsYou,
            Person::Asks,
            Person::Stuck,
            Person::Failed,
            Person::Lost,
            Person::Resumable,
            Person::Closed,
            Person::Standby,
        ] {
            let glyph = person.glyph(spin);
            assert_eq!(
                glyph.chars().next().and_then(UnicodeWidthChar::width),
                Some(1),
                "{person:?} spin {spin} -> {glyph}"
            );
            let _ = person.color(&palette);
        }
    }
    for task in [
        Task::Open,
        Task::Running,
        Task::Review,
        Task::Accepted,
        Task::Partial,
        Task::Paused,
        Task::Blocked,
        Task::AtCap,
        Task::Abandoned,
    ] {
        assert_eq!(UnicodeWidthStr::width(task.glyph()), 1, "{task:?}");
        let _ = task.color(&palette);
    }
    for health in [Health::Live, Health::StageDown, Health::Offline] {
        assert_eq!(UnicodeWidthStr::width(health.glyph()), 1, "{health:?}");
        let _ = health.color(&palette);
    }
    for flag in [PriorityMark::High, PriorityMark::Low] {
        assert_eq!(UnicodeWidthStr::width(flag.glyph()), 1);
        let _ = flag.color(&palette);
    }
    // Box Drawing (`U+2500..257F`) and Block Elements (`U+2580..259F`) share
    // hc-tui's single-width grid assumption (`width == Some(1)`).
    for ch in [
        '═', '║', '╟', '╢', '╠', '╣', '╞', '╡', '╤', '╧', '╔', '╗', '╚', '╝', '╭', '╮', '╰', '╯',
        '─', '│', '└', '┘', '┌', '┐', '├', '┤', '┬', '┴', '┏', '┓', '┗', '┛', '┣', '┫', '━', '┃',
        '┄', '┆', '▀', '▄', '▌', '▎',
    ] {
        assert_eq!(
            UnicodeWidthChar::width(ch),
            Some(1),
            "box/block glyph {ch:?}"
        );
    }
    // Every ornament glyph outside Box/Block elements must be strictly narrow
    // (`width == Some(1)` and `East_Asian_Width == N` via `width_cjk == Some(1)`).
    for ch in ['✦', '‹', '›', '▪', '◉', '▸', '▾', '◌', '✓', '⏎', '↵', '↩'] {
        assert_eq!(
            UnicodeWidthChar::width(ch),
            Some(1),
            "narrow width for {ch:?}"
        );
        assert_eq!(
            UnicodeWidthChar::width_cjk(ch),
            Some(1),
            "East_Asian_Width must stay Neutral (not Ambiguous) for {ch:?}"
        );
    }
}

/// Only `working` animates — every other state is fixed across all
/// spinner frames.
#[test]
fn only_working_animates() {
    let states = [
        Person::Idle,
        Person::Ready,
        Person::Slot,
        Person::UserPrompt,
        Person::DirPrompt,
        Person::NeedsYou,
        Person::Asks,
        Person::Stuck,
        Person::Failed,
        Person::Lost,
        Person::Resumable,
        Person::Closed,
        Person::Standby,
    ];
    for person in states {
        let first = person.glyph(0);
        assert!(
            (1..10).all(|spin| person.glyph(spin) == first),
            "{person:?} moved"
        );
    }
    assert!(Person::Working.glyph(0) != Person::Working.glyph(1));
    assert_eq!(Person::Working.still(), "◐");
}

/// The spec's glyph and token for each person state.
#[test]
fn person_glyphs_and_tokens() {
    let palette = Palette::from_name("backlot").unwrap();
    let cases = [
        (Person::Idle, "○", palette.overlay1),
        (Person::Ready, "○", palette.overlay1),
        (Person::Slot, "◌", palette.peach),
        (Person::UserPrompt, "×", palette.red),
        (Person::DirPrompt, "×", palette.peach),
        (Person::NeedsYou, "■", palette.red),
        (Person::Asks, "?", palette.peach),
        (Person::Stuck, "!", palette.peach),
        (Person::Failed, "!", palette.red),
        (Person::Lost, "!", palette.red),
        (Person::Resumable, "✓", palette.teal),
        (Person::Closed, "✓", palette.overlay0),
        (Person::Standby, " ", palette.overlay0),
    ];
    for (person, glyph, color) in cases {
        assert_eq!(person.glyph(0), glyph, "{person:?}");
        assert_eq!(person.color(&palette), color, "{person:?}");
    }
    // `idle` is the calm neutral — never green.
    assert_ne!(Person::Idle.color(&palette), palette.green);
    assert_eq!(Person::Working.still(), "◐");
    assert_eq!(Person::Working.color(&palette), palette.yellow);
}

/// `Person::of` precedence: closed/lost/held first, then the
/// attention class in priority order, an unanswered question, a
/// failure, the sample, and finally waiting or ready.
#[test]
fn person_of_reads_a_role() {
    use backlot_model::attention::AttentionClass;
    let role = |state: &str| RoleSummary {
        role: "actor-t1-c1".into(),
        name: "actor-a".into(),
        terminal: Some("t7".into()),
        runner: None,
        model: None,
        effort: None,
        state: state.into(),
        doing: None,
        progress: None,
        running: None,
        work: None,
        failed: None,
        workdir: None,
        task: Some("t1".into()),
        priority: None,
        resumable: false,
        since_ms: 0,
        attention: None,
        attention_class: None,
        asks: None,
        attention_user: false,
        attention_seq: None,
        attention_detail: None,
        stage_answer_ms: None,
        can_answer_prompt: true,
        restarted_ms: None,
        idle_since_ms: None,
        queue: None,
        account: None,
    };
    assert_eq!(Person::of(&role("closed")), Person::Closed);
    let mut resumable = role("closed");
    resumable.resumable = true;
    assert_eq!(Person::of(&resumable), Person::Resumable);
    assert_eq!(Person::of(&role("lost")), Person::Lost);
    assert_eq!(Person::of(&role("waiting")), Person::Slot);
    assert_eq!(Person::of(&role("live")), Person::Idle);

    let mut working = role("live");
    working.work = Some(Work::Working);
    assert_eq!(Person::of(&working), Person::Working);

    // A blocked actor's screen is the director's prompt; a
    // non-actor's is the user's (D9).
    let mut blocked = role("live");
    blocked.work = Some(Work::Blocked);
    assert_eq!(Person::of(&blocked), Person::DirPrompt);
    blocked.role = "producer".into();
    blocked.task = None;
    assert_eq!(Person::of(&blocked), Person::UserPrompt);

    // Attention outranks the sample and the failure flag.
    let mut prompted = role("live");
    prompted.work = Some(Work::Working);
    prompted.failed = Some("rate_limit".into());
    prompted.attention = Some("permission prompt".into());
    prompted.attention_class = Some(AttentionClass::NeedsYou);
    assert_eq!(Person::of(&prompted), Person::DirPrompt);
    prompted.attention_user = true;
    assert_eq!(Person::of(&prompted), Person::UserPrompt);
    // An older document marks the owner in the text only.
    prompted.attention_user = false;
    prompted.attention = Some("permission prompt — needs the user".into());
    assert_eq!(Person::of(&prompted), Person::UserPrompt);

    // A needs-you attention that is not a prompt, a stuck one, and an
    // info one that never decides the state.
    prompted.attention = Some("cannot resume".into());
    prompted.attention_class = Some(AttentionClass::NeedsYou);
    assert_eq!(Person::of(&prompted), Person::NeedsYou);
    prompted.attention = Some("stalled".into());
    prompted.attention_class = Some(AttentionClass::Stuck);
    assert_eq!(Person::of(&prompted), Person::Stuck);
    prompted.failed = None;
    prompted.attention = Some("session id not found".into());
    prompted.attention_class = Some(AttentionClass::Info);
    assert_eq!(Person::of(&prompted), Person::Working);

    // An unanswered question outranks the sample; the info attention
    // did not hide it.
    prompted.asks = Some("which shop?".into());
    assert_eq!(Person::of(&prompted), Person::Asks);

    let mut failed = role("live");
    failed.failed = Some("overloaded".into());
    assert_eq!(Person::of(&failed), Person::Failed);
}

/// The real case: a live producer whose work reads waiting and whose
/// `doing` reads idle, carrying an open `session id not found`
/// attention, resolves cleanly to `idle` — never flagged and never `ready`.
#[test]
fn an_info_attention_leaves_a_live_producer_idle() {
    use backlot_model::attention::AttentionClass;
    let producer: RoleSummary = serde_json::from_value(serde_json::json!({
        "role": "producer",
        "name": "producer",
        "state": "live",
        "work": "waiting",
        "doing": "idle",
        "attention": "session id not found",
        "attention_class": "info",
        "attention_detail": "session id not found",
        "attention_seq": 7
    }))
    .unwrap();
    assert_eq!(
        Person::attention_class(&producer),
        Some(AttentionClass::Info)
    );
    assert_eq!(Person::of(&producer), Person::Ready);
    assert_eq!(Person::of(&producer).word(), "idle");
    let inline = Person::inline_state(&producer, 60_000);
    assert_eq!(inline.word, "idle");
    assert_eq!(inline.detail, None);
}

/// The unified inline state vocabulary: priority, word, elapsed age,
/// and meaningful detail tail.
#[test]
fn inline_state_uses_the_priority_words() {
    use backlot_model::attention::AttentionClass;
    let now_ms = 90 * 60_000;
    let mut role: RoleSummary = serde_json::from_value(serde_json::json!({
        "role": "producer",
        "name": "producer",
        "state": "live",
        "doing": "900 tokens",
        "work": "working",
        "since_ms": now_ms - 4 * 60_000
    }))
    .unwrap();
    let st = Person::inline_state(&role, now_ms);
    assert_eq!(st.word, "working");
    assert_eq!(st.age.as_deref(), Some("4m"));
    assert_eq!(st.detail.as_deref(), Some("900 tokens"));

    role.work = Some(Work::Waiting);
    role.idle_since_ms = Some(now_ms - 35 * 60_000);
    let st = Person::inline_state(&role, now_ms);
    assert_eq!(st.word, "idle");
    assert_eq!(st.age.as_deref(), Some("35m"));
    assert_eq!(st.detail.as_deref(), Some("900 tokens"));

    role.work = None;
    role.doing = None;
    role.running = None;
    role.idle_since_ms = None;
    role.since_ms = now_ms - 3 * 60_000;
    role.attention = Some("cannot resume".into());
    role.attention_class = Some(AttentionClass::NeedsYou);
    role.attention_detail = Some("no session id to resume".into());
    let st = Person::inline_state(&role, now_ms);
    assert_eq!(st.word, "needs you");
    assert_eq!(st.age.as_deref(), Some("3m"));
    assert_eq!(st.detail.as_deref(), Some("no session id to resume"));

    role.asks = Some("which shop?".into());
    assert_eq!(Person::inline_state(&role, now_ms).word, "needs you");
    role.attention = None;
    role.attention_class = None;
    let st = Person::inline_state(&role, now_ms);
    assert_eq!(st.word, "asks");
    assert_eq!(st.detail.as_deref(), Some("which shop?"));

    role.asks = None;
    role.attention = Some("rate limited".into());
    role.attention_class = Some(AttentionClass::Stuck);
    role.attention_detail = Some("rate limited; auto-retry after the reset".into());
    let st = Person::inline_state(&role, now_ms);
    assert_eq!(st.word, "stuck");
    assert_eq!(
        st.detail.as_deref(),
        Some("rate limited; auto-retry after the reset")
    );
}

/// Every ledger task status maps to its spec glyph and token.
#[test]
fn task_glyphs_cover_every_status() {
    let palette = Palette::from_name("backlot").unwrap();
    let cases = [
        (TaskStatus::Created, "·", palette.overlay0),
        (TaskStatus::Running, "◐", palette.yellow),
        (TaskStatus::Delivered, "◈", palette.blue),
        (TaskStatus::Accepted, "✓", palette.green),
        (TaskStatus::PartiallyAccepted, "✓", palette.teal),
        (TaskStatus::Paused, "=", palette.overlay1),
        (TaskStatus::Blocked, "×", palette.red),
        (TaskStatus::AtCap, "×", palette.peach),
        (TaskStatus::Abandoned, "-", palette.overlay0),
        (TaskStatus::TakenOver, "-", palette.overlay0),
    ];
    for (status, glyph, color) in cases {
        let task = Task::of(status);
        assert_eq!(task.glyph(), glyph, "{status:?}");
        assert_eq!(task.color(&palette), color, "{status:?}");
    }
}

/// Priority flags: `↑` high in peach, `↓` low in overlay0, nothing
/// for normal.
#[test]
fn priority_flags() {
    let palette = Palette::from_name("backlot").unwrap();
    let high = PriorityMark::of(Priority::High).unwrap();
    assert_eq!(high.glyph(), "↑");
    assert_eq!(high.color(&palette), palette.peach);
    let low = PriorityMark::of(Priority::Low).unwrap();
    assert_eq!(low.glyph(), "↓");
    assert_eq!(low.color(&palette), palette.overlay0);
    assert!(PriorityMark::of(Priority::Normal).is_none());
}
