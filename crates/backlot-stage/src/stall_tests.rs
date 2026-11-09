//! The stall decision's truth table — an actor stalls only on a
//! `running` task, other roles only while `working`, `blocked`
//! never, nothing under the limit — and the `stalled` notice's
//! routing: the actor's reviewer, the producer for the writer and
//! the director, nobody for the producer.

use std::path::Path;

use backlot_model::event::{PERMISSION_PROMPT, STALLED};
use backlot_model::state::{Attention, StudioState, TaskStatus, replay};
use backlot_model::summary::Work;
use backlot_model::testkit::*;

use crate::deliver::Screen;
use crate::notices;
use crate::stall;

const DIR: &str = "/state/studios/acme-shop";

fn dir() -> &'static Path {
    Path::new(DIR)
}

/// A screen that has shown `text` unchanged since `since`, sampled
/// at `at`.
fn screen(text: &str, since: u64, at: u64) -> Screen {
    Screen {
        text: text.into(),
        same_since_ms: since,
        at_ms: at,
    }
}

/// The decision's truth table: who stalls at the limit and who
/// never does.
#[test]
fn stall_truth_table() {
    let limit = 5_000;
    // An actor on a running task stalls on a frozen screen,
    // working or waiting — an actor that went idle is exactly the
    // case this catches.
    for work in [Work::Working, Work::Waiting] {
        assert!(
            stall::stalled(
                "actor-t1-c1",
                Some(TaskStatus::Running),
                work,
                &screen("s", 0, 5_000),
                limit
            ),
            "actor running {work:?}"
        );
    }
    // An actor whose task is not running never stalls: delivered,
    // blocked or paused work is somebody else's to watch.
    for status in [
        TaskStatus::Delivered,
        TaskStatus::Blocked,
        TaskStatus::Paused,
    ] {
        assert!(
            !stall::stalled(
                "actor-t1-c1",
                Some(status),
                Work::Working,
                &screen("s", 0, 60_000),
                limit
            ),
            "actor {status:?}"
        );
    }
    // `blocked` never counts, whoever is showing it — a permission
    // prompt has its own attention.
    for (role, task) in [
        ("actor-t1-c1", Some(TaskStatus::Running)),
        ("director", None),
        ("producer", None),
    ] {
        assert!(
            !stall::stalled(role, task, Work::Blocked, &screen("s", 0, 60_000), limit),
            "{role} blocked"
        );
    }
    // The producer, director and writer stall only while `working` —
    // an idle one is normal.
    for role in ["producer", "director", "writer"] {
        assert!(
            stall::stalled(role, None, Work::Working, &screen("s", 0, 5_000), limit),
            "{role} working"
        );
        assert!(
            !stall::stalled(role, None, Work::Waiting, &screen("s", 0, 60_000), limit),
            "{role} waiting"
        );
    }
    // Under the limit nobody stalls, however the screen reads.
    for (role, task, work) in [
        ("actor-t1-c1", Some(TaskStatus::Running), Work::Working),
        ("actor-t1-c1", Some(TaskStatus::Running), Work::Waiting),
        ("director", None, Work::Working),
    ] {
        assert!(
            !stall::stalled(role, task, work, &screen("s", 0, 4_999), limit),
            "{role} under the limit"
        );
    }
}

/// `open_stall` finds only the role's open `stalled` attention —
/// a cleared one or another reason is not it.
#[test]
fn open_stall_is_one_reason_open() {
    let mut state = StudioState::default();
    for (seq, reason, open) in [
        (1, STALLED, false),
        (2, PERMISSION_PROMPT, true),
        (3, STALLED, true),
    ] {
        state.attention.push(Attention {
            seq,
            role: "writer".into(),
            reason: reason.into(),
            detail: String::new(),
            open,
            ts_ms: 0,
        });
    }
    assert_eq!(stall::open_stall(&state, "writer").unwrap().seq, 3);
    assert!(stall::open_stall(&state, "director").is_none());
}

fn raised(seq: u64, role: &str) -> Attention {
    Attention {
        seq,
        role: role.into(),
        reason: STALLED.into(),
        detail: format!("screen unchanged for 5s; screen saved to attention/{seq}.txt"),
        open: true,
        ts_ms: 0,
    }
}

/// A `stalled` attention owes exactly one notice: the actor's to
/// its reviewer — the producer on a producer cast — the writer's
/// and the director's to the producer, and the producer's to
/// nobody.
#[test]
fn stalled_notice_routes_per_role() {
    let base = base_events();
    // Director-cast task: the actor's stall is the director's.
    let mut state = state_with(&base, vec![("director", cast("t1", "actor-a", "a"))]);
    state.attention.push(raised(20, "actor-t1-c1"));
    let owed = notices::owed(&state, dir());
    assert_eq!(owed.len(), 1, "{owed:?}");
    assert_eq!(owed[0].label, "stalled");
    assert_eq!(owed[0].to, "director");
    assert_eq!(owed[0].source_seq, 20);
    // A message carrying the seq is the second pass: nothing owed.
    state.messages.push(backlot_model::state::Message {
        seq: 21,
        from: "stage".into(),
        to: "director".into(),
        file: Some("messages/20-stalled.md".into()),
        purpose: None,
        source_seq: Some(20),
        delivered: false,
    });
    assert!(notices::owed(&state, dir()).is_empty());

    // Producer-cast task: the actor's stall is the producer's.
    let mut state = state_with(&base, vec![("producer", cast("t1", "actor-a", "a"))]);
    state.attention.push(raised(20, "actor-t1-c1"));
    let owed = notices::owed(&state, dir());
    assert_eq!(owed.len(), 1, "{owed:?}");
    assert_eq!(owed[0].to, "producer");

    // The writer and the director report to the producer; the
    // producer's stall is already on the panel — nobody is told.
    for (role, want) in [("writer", Some("producer")), ("director", Some("producer"))] {
        let mut state = replay(&base);
        state.attention.push(raised(20, role));
        let owed = notices::owed(&state, dir());
        assert_eq!(owed.len(), 1, "{role}: {owed:?}");
        assert_eq!(owed[0].to, want.unwrap());
    }
    let mut state = replay(&base);
    state.attention.push(raised(20, "producer"));
    assert!(notices::owed(&state, dir()).is_empty());
}

/// The notice names the role, how long the screen held still, the
/// saved file, the `tell` nudge — and for an actor the abandon or
/// recast ways on, which a non-actor's does not get.
#[test]
fn stalled_notice_body_names_the_ways_on() {
    let base = base_events();
    let mut state = state_with(&base, vec![("director", cast("t1", "actor-a", "a"))]);
    state.attention.push(raised(20, "actor-t1-c1"));
    let body = notices::owed(&state, dir())
        .pop()
        .expect("a stalled notice")
        .body;
    for want in [
        "actor-t1-c1",
        "screen unchanged for 5s",
        "attention/20.txt",
        "backlot tell actor-t1-c1 --file <file>",
        "backlot task abandon --task t1",
        "backlot cast --task t1",
    ] {
        assert!(body.contains(want), "actor body misses {want}: {body}");
    }

    let mut state = replay(&base);
    state.attention.push(raised(20, "writer"));
    let body = notices::owed(&state, dir())
        .pop()
        .expect("a stalled notice")
        .body;
    for want in [
        "writer",
        "screen unchanged for 5s",
        "attention/20.txt",
        "backlot tell writer --file <file>",
    ] {
        assert!(body.contains(want), "writer body misses {want}: {body}");
    }
    for banned in ["backlot task abandon", "backlot cast"] {
        assert!(
            !body.contains(banned),
            "writer body must not name {banned}: {body}"
        );
    }
}
