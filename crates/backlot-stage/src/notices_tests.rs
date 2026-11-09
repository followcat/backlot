//! Notice-routing cases that span the task's own facts: the
//! changes-requested review that takes the task to `at_cap` owes
//! the producer — not the actor — and a resume that returns a
//! blocked task to `running` owes its actor.

use std::path::Path;

use backlot_model::event::Verdict;
use backlot_model::state::{Message, StudioState};
use backlot_model::testkit::*;

use crate::notices;

const DIR: &str = "/state/studios/acme-shop";

fn dir() -> &'static Path {
    Path::new(DIR)
}

/// t1 under actor-a, delivered and sent back twice below the cap,
/// then once past it — the third `changes_requested` lands `at_cap`
/// (cap 2). The below-cap reviews' seqs are 6 and 8, the
/// cap-crosser's 10.
fn at_cap_state() -> StudioState {
    state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("actor", delivered("t1", 1, "a", 1)),
            (
                "director",
                reviewed("t1", 1, Verdict::ChangesRequested, vec![]),
            ),
            ("actor", delivered("t1", 2, "a", 1)),
            (
                "director",
                reviewed("t1", 2, Verdict::ChangesRequested, vec![]),
            ),
            ("actor", delivered("t1", 3, "a", 1)),
            (
                "director",
                reviewed("t1", 3, Verdict::ChangesRequested, vec![]),
            ),
        ],
    )
}

/// The review that takes the task to `at_cap` owes the producer
/// exactly one `at-cap` notice — conclusion, rounds and the three
/// ways on — and the actor no `rework` it could never act on; the
/// below-cap rounds' `rework` notices are untouched.
#[test]
fn the_cap_review_owes_the_producer_not_the_actor() {
    let mut state = at_cap_state();
    let owed = notices::owed(&state, dir());
    let at_cap = owed.iter().find(|n| n.label == "at-cap").expect("at-cap");
    assert_eq!(at_cap.to, "producer");
    assert_eq!(at_cap.source_seq, 10);
    let body = &at_cap.body;
    for want in [
        "conclusion on t1 d3",
        "rework cap",
        "--verdict partially-accepted",
        "backlot task abandon --task t1",
        "backlot cast --task t1",
        "decides this with the user",
    ] {
        assert!(body.contains(want), "at-cap body misses {want}: {body}");
    }
    // The actor's rework notices are the two below-cap rounds only —
    // nothing carries the cap review's seq to the actor.
    let reworks: Vec<_> = owed.iter().filter(|n| n.label == "rework").collect();
    assert_eq!(reworks.len(), 2, "{owed:?}");
    assert!(
        reworks
            .iter()
            .all(|n| n.to == "actor-t1-c1" && n.source_seq != 10)
    );

    // A message carrying the cap review's seq is the second pass —
    // nothing is owed again.
    state.messages.push(Message {
        seq: 40,
        from: "stage".into(),
        to: "producer".into(),
        file: Some("messages/10-at-cap.md".into()),
        purpose: None,
        source_seq: Some(10),
        delivered: false,
    });
    let owed = notices::owed(&state, dir());
    assert!(owed.iter().all(|n| n.label != "at-cap"), "{owed:?}");
}

/// A `changes_requested` review below the cap still owes the actor
/// its rework notice, unchanged.
#[test]
fn below_the_cap_the_rework_notice_is_unchanged() {
    let state = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("actor", delivered("t1", 1, "a", 1)),
            (
                "director",
                reviewed("t1", 1, Verdict::ChangesRequested, vec![]),
            ),
        ],
    );
    let owed = notices::owed(&state, dir());
    let rework = owed.iter().find(|n| n.label == "rework").expect("rework");
    assert_eq!(rework.to, "actor-t1-c1");
    assert_eq!(rework.source_seq, 6);
    assert!(
        rework
            .body
            .contains("Fix what it names and deliver again with `backlot deliver`."),
        "{}",
        rework.body
    );
    assert!(owed.iter().all(|n| n.label != "at-cap"));
}
