//! Notice-routing cases that span the task's own facts: the
//! changes-requested review that takes the task to `at_cap` owes
//! the producer — not the actor — and a resume that returns a
//! blocked task to `running` owes its actor.

use std::collections::BTreeMap;
use std::path::Path;

use backlot_model::event::{CANNOT_RESUME, EventKind, TaskStateChange, Verdict};
use backlot_model::state::{Message, StudioState, apply};
use backlot_model::testkit::*;

use crate::notices;

const DIR: &str = "/state/studios/acme-shop";

fn dir() -> &'static Path {
    Path::new(DIR)
}

/// `base_events` whose studio declares `unit` and `lint`.
fn checked_base() -> Vec<backlot_model::Event> {
    let mut evs = base_events();
    let EventKind::StudioCreated { checks, .. } = &mut evs[0].kind else {
        unreachable!()
    };
    *checks = vec![
        check_decl("unit", "cargo test --lib", 60),
        check_decl("lint", "cargo clippy", 60),
    ];
    evs
}

/// A `task.checked` on t1 delivery 1 for `name`.
fn checked_ev(seq: u64, name: &str, code: Option<i32>, timed_out: bool) -> backlot_model::Event {
    ev(
        seq,
        "stage",
        EventKind::TaskChecked {
            task: "t1".into(),
            delivery: 1,
            name: name.into(),
            exit_code: code,
            timed_out,
            output: format!("checks/5-{name}.log"),
            duration_ms: 10,
        },
    )
}

/// The reviewer's delivery notice waits until every declared check
/// of the delivery has its `task.checked` — then it separates what
/// the stage ran, with each log, from what the actor reported.
#[test]
fn a_delivery_notice_waits_for_the_stage_checks() {
    let mut d = delivered("t1", 1, "a", 1);
    if let EventKind::TaskDelivered { checks, .. } = &mut d {
        *checks = vec![backlot_model::event::Check {
            name: "build".into(),
            passed: true,
        }];
    }
    let mut state = state_with(
        &checked_base(),
        vec![("director", cast("t1", "actor-a", "a")), ("actor", d)],
    );
    // Nothing owed while both declared checks are unrecorded.
    assert!(
        notices::owed(&state, dir())
            .iter()
            .all(|n| n.label != "delivered")
    );
    // One of two recorded: still waiting.
    apply(&mut state, &checked_ev(6, "unit", Some(0), false));
    assert!(
        notices::owed(&state, dir())
            .iter()
            .all(|n| n.label != "delivered")
    );
    // Both recorded: the notice lands.
    apply(&mut state, &checked_ev(7, "lint", None, true));
    let n = notices::owed(&state, dir())
        .into_iter()
        .find(|n| n.label == "delivered")
        .expect("delivered notice");
    assert_eq!(n.to, "director");
    assert_eq!(n.source_seq, 5);
    for want in [
        "Stage ran:",
        "- unit: pass — ",
        "checks/5-unit.log",
        "- lint: timed out — ",
        "checks/5-lint.log",
        "The actor reported: build=pass",
    ] {
        assert!(n.body.contains(want), "notice misses {want}: {}", n.body);
    }
}

/// A task reviewed before its checks landed still gets the notice —
/// each check that never ran says it has no record.
#[test]
fn a_task_that_moved_on_gets_the_notice_anyway() {
    let state = state_with(
        &checked_base(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("actor", delivered("t1", 1, "a", 1)),
            (
                "stage",
                EventKind::TaskChecked {
                    task: "t1".into(),
                    delivery: 1,
                    name: "unit".into(),
                    exit_code: Some(1),
                    timed_out: false,
                    output: "checks/5-unit.log".into(),
                    duration_ms: 10,
                },
            ),
            (
                "director",
                reviewed("t1", 1, Verdict::ChangesRequested, vec![]),
            ),
        ],
    );
    let n = notices::owed(&state, dir())
        .into_iter()
        .find(|n| n.label == "delivered")
        .expect("delivered notice");
    assert!(n.body.contains("- unit: fail (exit 1)"), "{}", n.body);
    assert!(n.body.contains("- lint: no record"), "{}", n.body);
}

/// With no checks declared the notice says so outright, and still
/// separates the actor's own claims.
#[test]
fn a_delivery_notice_says_so_with_no_checks_declared() {
    let state = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("actor", delivered("t1", 1, "a", 1)),
        ],
    );
    let n = notices::owed(&state, dir())
        .into_iter()
        .find(|n| n.label == "delivered")
        .expect("delivered notice");
    assert!(n.body.contains("No checks were declared"), "{}", n.body);
    assert!(n.body.contains("The actor reported:"), "{}", n.body);
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
        dropped: false,
        ts_ms: 0,
        delivered_ms: None,
    });
    let owed = notices::owed(&state, dir());
    assert!(owed.iter().all(|n| n.label != "at-cap"), "{owed:?}");
}

/// A `cannot resume` attention owes one `lost` notice routed like a
/// stall: an actor's to its task reviewer, the writer's and the
/// director's to the producer, the producer's to nobody. The actor's
/// body names the role, why, and the ways on — abandon or recast.
#[test]
fn a_cannot_resume_owes_one_lost_notice_on_the_stalled_route() {
    let state = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            (
                "stage",
                EventKind::AttentionRaised {
                    role: "actor-t1-c1".into(),
                    reason: CANNOT_RESUME.into(),
                    detail: "lost 3 times in 10 minutes".into(),
                    needs_user: false,
                },
            ),
            (
                "stage",
                EventKind::AttentionRaised {
                    role: "writer".into(),
                    reason: CANNOT_RESUME.into(),
                    detail: "no session id to resume".into(),
                    needs_user: false,
                },
            ),
            (
                "stage",
                EventKind::AttentionRaised {
                    role: "director".into(),
                    reason: CANNOT_RESUME.into(),
                    detail: "no session id to resume".into(),
                    needs_user: false,
                },
            ),
            (
                "stage",
                EventKind::AttentionRaised {
                    role: "producer".into(),
                    reason: CANNOT_RESUME.into(),
                    detail: "no session id to resume".into(),
                    needs_user: false,
                },
            ),
        ],
    );
    let owed = notices::owed(&state, dir());
    let lost: Vec<_> = owed.iter().filter(|n| n.label == "lost").collect();
    assert_eq!(lost.len(), 3, "the producer tells nobody: {owed:?}");
    let by_to: BTreeMap<&str, _> = lost.iter().map(|n| (n.to.as_str(), n)).collect();
    // The actor's to its task reviewer — the director cast it.
    let actor_notice = by_to["director"];
    assert_eq!(actor_notice.source_seq, 5);
    for want in [
        "actor-t1-c1",
        "lost 3 times in 10 minutes",
        "backlot task abandon --task t1",
        "backlot cast --task t1",
    ] {
        assert!(
            actor_notice.body.contains(want),
            "lost body misses {want}: {}",
            actor_notice.body
        );
    }
    // Writer's and director's to the producer, one each.
    let producers: Vec<u64> = lost
        .iter()
        .filter(|n| n.to == "producer")
        .map(|n| n.source_seq)
        .collect();
    assert_eq!(producers, [6, 7], "{owed:?}");
    assert!(lost.iter().all(|n| n.to != "user"), "{owed:?}");

    // Already queued once: nothing is owed again for any of them.
    let mut state = state;
    for seq in [5, 6, 7, 8] {
        state.messages.push(Message {
            seq: 40 + seq,
            from: "stage".into(),
            to: "x".into(),
            file: Some(format!("messages/{seq}-lost.md")),
            purpose: None,
            source_seq: Some(seq),
            delivered: false,
            dropped: false,
            ts_ms: 0,
            delivered_ms: None,
        });
    }
    assert!(
        notices::owed(&state, dir())
            .iter()
            .all(|n| n.label != "lost")
    );
}

/// A `role.resumed` with reason `recovered` owes the role itself
/// exactly one `recovered` notice — the terminal was lost, the
/// session resumed, anything running then has stopped, a
/// permission-prompted command must be run again. A `reopened`
/// resume owes none.
#[test]
fn a_recovered_resume_owes_the_role_one_notice() {
    let resumed = |seq: u64, role: &str, reason: &str| {
        (
            "stage",
            EventKind::RoleResumed {
                role: role.into(),
                terminal: Some(format!("t{seq}")),
                actor: Some(actor("a")),
                session_id: Some("sess-1".into()),
                reason: Some(reason.into()),
            },
        )
    };
    let mut state = state_with(
        &base_events(),
        vec![
            (
                "stage",
                EventKind::RoleStarted {
                    role: "producer".into(),
                    terminal: Some("t4".into()),
                    actor: Some(actor("a")),
                    session_id: Some("sess-1".into()),
                    reason: None,
                },
            ),
            resumed(5, "producer", "recovered"),
        ],
    );
    let owed = notices::owed(&state, dir());
    let recovered: Vec<_> = owed.iter().filter(|n| n.label == "recovered").collect();
    assert_eq!(recovered.len(), 1, "{owed:?}");
    let n = recovered[0];
    assert_eq!(n.to, "producer");
    assert_eq!(n.source_seq, 5);
    for want in [
        "terminal was lost",
        "resumed its session",
        "has stopped",
        "backlot status",
        "permission prompt",
        "run again",
    ] {
        assert!(
            n.body.contains(want),
            "recovered body misses {want}: {}",
            n.body
        );
    }

    // Queued once: nothing is owed again — a restart repeats nothing.
    state.messages.push(Message {
        seq: 40,
        from: "stage".into(),
        to: "producer".into(),
        file: Some("messages/5-recovered.md".into()),
        purpose: None,
        source_seq: Some(5),
        delivered: false,
        dropped: false,
        ts_ms: 0,
        delivered_ms: None,
    });
    assert!(
        notices::owed(&state, dir())
            .iter()
            .all(|n| n.label != "recovered")
    );

    // A `reopened` resume owes none.
    let state = state_with(
        &base_events(),
        vec![
            (
                "stage",
                EventKind::RoleStarted {
                    role: "producer".into(),
                    terminal: Some("t4".into()),
                    actor: Some(actor("a")),
                    session_id: Some("sess-1".into()),
                    reason: None,
                },
            ),
            resumed(5, "producer", "reopened"),
        ],
    );
    assert!(
        notices::owed(&state, dir())
            .iter()
            .all(|n| n.label != "recovered")
    );
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

/// A resume that returns the task to `running` — straight out of
/// `blocked`, or a pause of running work — owes the current actor
/// exactly one `resumed` notice naming who resumed it and the
/// reason. A resume that only restores `delivered` or `blocked`
/// owes none, and a task with no cast owes none either.
#[test]
fn a_resume_to_running_owes_the_actor() {
    // blocked -> running: the resume the S6 loop needs.
    let state = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            (
                "actor",
                task_state("t1", TaskStateChange::Blocked, "needs an answer"),
            ),
            (
                "director",
                task_state("t1", TaskStateChange::Resumed, "answered"),
            ),
        ],
    );
    let owed = notices::owed(&state, dir());
    let resumed: Vec<_> = owed.iter().filter(|n| n.label == "resumed").collect();
    assert_eq!(resumed.len(), 1, "{owed:?}");
    let n = resumed[0];
    assert_eq!(n.to, "actor-t1-c1");
    assert_eq!(n.source_seq, 6);
    for want in ["director", "answered", "carry on"] {
        assert!(
            n.body.contains(want),
            "resumed body misses {want}: {}",
            n.body
        );
    }

    // Paused from running -> running: owed the same way.
    let state = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            (
                "director",
                task_state("t1", TaskStateChange::Paused, "hold"),
            ),
            (
                "director",
                task_state("t1", TaskStateChange::Resumed, "go on"),
            ),
        ],
    );
    let owed = notices::owed(&state, dir());
    let resumed: Vec<_> = owed.iter().filter(|n| n.label == "resumed").collect();
    assert_eq!(resumed.len(), 1, "{owed:?}");
    assert_eq!(resumed[0].to, "actor-t1-c1");

    // Paused from delivered -> delivered: the review picks back
    // up, the actor has nothing to carry on.
    let state = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("actor", delivered("t1", 1, "a", 1)),
            (
                "director",
                task_state("t1", TaskStateChange::Paused, "hold"),
            ),
            (
                "director",
                task_state("t1", TaskStateChange::Resumed, "go on"),
            ),
        ],
    );
    assert!(
        notices::owed(&state, dir())
            .iter()
            .all(|n| n.label != "resumed")
    );

    // Paused from blocked -> blocked: still waiting.
    let state = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            (
                "actor",
                task_state("t1", TaskStateChange::Blocked, "needs an answer"),
            ),
            (
                "director",
                task_state("t1", TaskStateChange::Paused, "hold"),
            ),
            (
                "director",
                task_state("t1", TaskStateChange::Resumed, "still waiting"),
            ),
        ],
    );
    assert!(
        notices::owed(&state, dir())
            .iter()
            .all(|n| n.label != "resumed")
    );

    // No cast: nobody to tell.
    let state = state_with(
        &base_events(),
        vec![
            ("actor", task_state("t1", TaskStateChange::Blocked, "stuck")),
            ("director", task_state("t1", TaskStateChange::Resumed, "go")),
        ],
    );
    assert!(
        notices::owed(&state, dir())
            .iter()
            .all(|n| n.label != "resumed")
    );
}
