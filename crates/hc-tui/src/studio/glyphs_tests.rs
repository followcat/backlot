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
            Person::Slot,
            Person::UserPrompt,
            Person::DirPrompt,
            Person::Stalled,
            Person::Failed,
            Person::Lost,
            Person::Resumable,
            Person::Closed,
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
}

/// Only `working` animates — every other state is fixed across all
/// spinner frames.
#[test]
fn only_working_animates() {
    let states = [
        Person::Idle,
        Person::Slot,
        Person::UserPrompt,
        Person::DirPrompt,
        Person::Stalled,
        Person::Failed,
        Person::Lost,
        Person::Resumable,
        Person::Closed,
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
        (Person::Slot, "◌", palette.peach),
        (Person::UserPrompt, "×", palette.red),
        (Person::DirPrompt, "×", palette.peach),
        (Person::Stalled, "!", palette.peach),
        (Person::Failed, "!", palette.red),
        (Person::Lost, "!", palette.red),
        (Person::Resumable, "✓", palette.teal),
        (Person::Closed, "✓", palette.overlay0),
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

/// `Person::of` precedence: closed/lost/held first, then attention,
/// failure, then the sampled screen.
#[test]
fn person_of_reads_a_role() {
    let role = |state: &str| RoleSummary {
        role: "actor-t1-c1".into(),
        name: "actor-a".into(),
        terminal: Some("t7".into()),
        runner: None,
        model: None,
        effort: None,
        state: state.into(),
        doing: None,
        work: None,
        failed: None,
        workdir: None,
        task: Some("t1".into()),
        priority: None,
        resumable: false,
        since_ms: 0,
        attention: None,
        attention_user: false,
        attention_seq: None,
        attention_detail: None,
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
    assert_eq!(Person::of(&prompted), Person::DirPrompt);
    prompted.attention_user = true;
    assert_eq!(Person::of(&prompted), Person::UserPrompt);
    // An older document marks the owner in the text only.
    prompted.attention_user = false;
    prompted.attention = Some("permission prompt — needs the user".into());
    assert_eq!(Person::of(&prompted), Person::UserPrompt);
    prompted.attention = Some("stalled".into());
    assert_eq!(Person::of(&prompted), Person::Stalled);

    let mut failed = role("live");
    failed.failed = Some("overloaded".into());
    assert_eq!(Person::of(&failed), Person::Failed);
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
