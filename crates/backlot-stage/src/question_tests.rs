//! Unit tests for `question` routing (`notices::reviewer_for` for
//! actors, `"producer"` for `director` and `writer`, none/`needs_user`
//! for `producer`), notice body instructions, and staleness cleanup.

use std::path::Path;

use backlot_model::event::QUESTION;
use backlot_model::state::{Attention, Message, replay};
use backlot_model::testkit::*;

use backlot_model::summary::Work;

use super::in_episode;
use crate::{notices, stale};

const DIR: &str = "/state/studios/acme-shop";

fn dir() -> &'static Path {
    Path::new(DIR)
}

fn raised(seq: u64, role: &str, needs_user: bool) -> Attention {
    Attention {
        seq,
        role: role.into(),
        reason: QUESTION.into(),
        detail: format!("Which option?; screen saved to attention/{seq}.txt"),
        open: true,
        ts_ms: 0,
        needs_user,
        cleared_reason: None,
        account: None,
        reset_at_ms: None,
    }
}

#[test]
fn question_notice_routes_per_role() {
    let base = base_events();
    // Director-cast actor -> director.
    let mut state = state_with(&base, vec![("director", cast("t1", "actor-a", "a"))]);
    state.attention.push(raised(20, "actor-t1-c1", false));
    let owed = notices::owed(&state, dir());
    assert_eq!(owed.len(), 1, "{owed:?}");
    assert_eq!(owed[0].label, "question");
    assert_eq!(owed[0].to, "director");
    assert_eq!(owed[0].source_seq, 20);

    // Producer-cast actor -> producer.
    let mut state = state_with(&base, vec![("producer", cast("t1", "actor-a", "a"))]);
    state.attention.push(raised(20, "actor-t1-c1", false));
    let owed = notices::owed(&state, dir());
    assert_eq!(owed.len(), 1, "{owed:?}");
    assert_eq!(owed[0].to, "producer");

    // Director and writer -> producer; producer -> nobody (`needs_user`).
    for role in ["director", "writer"] {
        let mut state = replay(&base);
        state.attention.push(raised(20, role, false));
        let owed = notices::owed(&state, dir());
        assert_eq!(owed.len(), 1, "{role}: {owed:?}");
        assert_eq!(owed[0].label, "question");
        assert_eq!(owed[0].to, "producer");
    }
    let mut state = replay(&base);
    state.attention.push(raised(20, "producer", true));
    assert!(notices::owed(&state, dir()).is_empty());
}

#[test]
fn question_notice_body_and_staleness() {
    let base = base_events();
    let mut state = state_with(&base, vec![("director", cast("t1", "actor-a", "a"))]);
    state.attention.push(raised(20, "actor-t1-c1", false));
    let n = notices::owed(&state, dir()).pop().expect("actor question");
    for want in [
        "actor-t1-c1",
        "attention/20.txt",
        "backlot tell actor-t1-c1 --file <file>",
        "take over",
        "backlot task abandon --task t1",
        "backlot cast --task t1",
    ] {
        assert!(
            n.body.contains(want),
            "actor body misses {want}: {}",
            n.body
        );
    }

    let mut fixed = replay(&base);
    fixed.attention.push(raised(21, "writer", false));
    let fw = notices::owed(&fixed, dir()).pop().expect("writer question");
    assert_eq!(fw.to, "producer");
    assert!(fw.body.contains("backlot tell writer --file <file>"));
    assert!(fw.body.contains("attention/21.txt"));
    assert!(!fw.body.contains("backlot task abandon"));

    // Once the open attention clears (`dialog closed`), a pending
    // `question` notice is dropped as stale (`attention cleared`).
    fixed.attention[0].open = false;
    fixed.attention[0].cleared_reason = Some("dialog closed".into());
    let pending = Message {
        seq: 30,
        from: "stage".into(),
        to: "producer".into(),
        file: Some("messages/21-question.md".into()),
        purpose: None,
        source_seq: Some(21),
        delivered: false,
        dropped: false,
        ts_ms: 0,
        delivered_ms: None,
    };
    assert_eq!(
        stale::stale_reason(&fixed, &pending),
        Some("attention cleared")
    );
}

/// The weekly quota picker stays out of generic question episodes.
#[test]
fn weekly_limit_paid_choice_dialog_is_not_a_question() {
    let screen = include_str!("../tests/fixtures/claude/weekly-limit.txt");
    assert!(!in_episode("claude", Work::Blocked, None, screen, None));
}
