//! Fail-first tests for PR-5 explicit `studio.halted` / `studio.resumed` validation & role gates.

use crate::event::*;
use crate::roles::check as role_check;
use crate::state::replay;
use crate::testkit::*;
use crate::validate::{Refusal, validate};

fn halt(reason: &str) -> EventKind {
    EventKind::StudioHalted {
        reason: reason.into(),
    }
}

fn resume(note: Option<&str>, false_positive: bool) -> EventKind {
    EventKind::StudioResumed {
        note: note.map(str::to_string),
        false_positive,
    }
}

fn running_events() -> Vec<Event> {
    let mut events = base_events();
    events.push(ev(4, "director", task_created("t2", "fix", 1)));
    events.push(ev(5, "director", cast("t1", "actor-a", "a")));
    events
}

#[test]
fn halted_studio_refuses_new_task_cast_but_running_work_continues() {
    let mut events = running_events();
    events.push(ev(6, "producer", halt("wrong base commit")));
    let state = replay(&events);

    // New TaskCast on uncast task t2 is hard-refused while halted.
    let err = validate(&state, &new_ev("director", cast("t2", "actor-b", "b")))
        .expect_err("TaskCast must be refused while studio is halted");
    assert!(
        matches!(err, Refusal::StudioHalted { ref by, ref reason } if by == "producer" && reason == "wrong base commit"),
        "expected StudioHalted refusal, got {err:?}"
    );

    // Dispatch messages also stop while halted; plain tells/asks continue.
    assert!(
        validate(
            &state,
            &new_ev(
                "producer",
                EventKind::MessageQueued {
                    to: "director".into(),
                    file: Some("messages/d.md".into()),
                    purpose: Some("dispatch".into()),
                    source_seq: None,
                },
            ),
        )
        .is_err(),
        "dispatch must not launch new work while halted"
    );

    // Running task t1 continues unhindered: deliver, review, progress, tell, ask all succeed.
    assert_eq!(
        validate(&state, &new_ev("actor-t1-c1", delivered("t1", 1, "a", 1))),
        Ok(())
    );
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "actor-t1-c1",
                EventKind::RoleProgress {
                    role: "actor-t1-c1".into(),
                    task: Some("t1".into()),
                    text: "finishing tests".into(),
                },
            ),
        ),
        Ok(())
    );
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "actor-t1-c1",
                EventKind::MessageQueued {
                    to: "director".into(),
                    file: Some("messages/note.md".into()),
                    purpose: None,
                    source_seq: None,
                },
            ),
        ),
        Ok(())
    );
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "producer",
                EventKind::MessageQueued {
                    to: "user".into(),
                    file: Some("messages/ask.md".into()),
                    purpose: Some("ask".into()),
                    source_seq: None,
                },
            ),
        ),
        Ok(())
    );

    // Delivery + review succeeds while halted too.
    let mut after_deliv = events.clone();
    after_deliv.push(ev(7, "actor-t1-c1", delivered("t1", 1, "a", 1)));
    let deliv_state = replay(&after_deliv);
    assert_eq!(
        validate(
            &deliv_state,
            &new_ev("director", reviewed("t1", 1, Verdict::Accepted, vec![])),
        ),
        Ok(())
    );

    // Resuming unlocks TaskCast again.
    let mut resumed = after_deliv;
    resumed.push(ev(8, "director", resume(Some("rebased"), false)));
    let resumed_state = replay(&resumed);
    assert!(resumed_state.halted.is_none());
    assert_eq!(
        validate(
            &resumed_state,
            &new_ev("director", cast("t2", "actor-b", "b"))
        ),
        Ok(())
    );
}

#[test]
fn halt_and_resume_role_permissions_and_state_checks() {
    let state = running_state();

    // Any studio role (producer, director, writer, actor-<task>-c<n>) or user can halt.
    for role in ["user", "producer", "director", "writer", "actor-t1-c1"] {
        assert_eq!(
            role_check(&state, &new_ev(role, halt("bad assumption"))),
            Ok(()),
            "role {role} must be allowed to halt"
        );
        assert_eq!(
            validate(&state, &new_ev(role, halt("bad assumption"))),
            Ok(()),
            "validate should allow {role} to halt"
        );
    }

    // Bare "actor" and "hollywood" cannot halt.
    assert_eq!(
        role_check(&state, &new_ev("actor", halt("nope"))),
        Err(Refusal::BareActor)
    );
    assert!(matches!(
        role_check(&state, &new_ev("hollywood", halt("nope"))),
        Err(Refusal::RoleBound { .. })
    ));

    // Empty halt reason refuses.
    assert_eq!(
        validate(&state, &new_ev("producer", halt("   "))),
        Err(Refusal::EmptyHaltReason)
    );

    // Resuming when not halted refuses; resuming while halted checks role.
    assert_eq!(
        validate(&state, &new_ev("director", resume(None, false))),
        Err(Refusal::NotHalted)
    );

    let mut halted_events = running_events();
    halted_events.push(ev(6, "writer", halt("spec conflict")));
    let halted_state = replay(&halted_events);

    // Double halt refuses.
    assert_eq!(
        validate(&halted_state, &new_ev("director", halt("again"))),
        Err(Refusal::AlreadyHalted)
    );

    // Actor, writer, hollywood cannot resume.
    for blocked in ["actor-t1-c1", "writer", "hollywood"] {
        assert!(
            matches!(
                validate(&halted_state, &new_ev(blocked, resume(None, false))),
                Err(Refusal::RoleBound { .. })
            ),
            "role {blocked} must be refused on resume"
        );
    }
    assert_eq!(
        role_check(&halted_state, &new_ev("actor", resume(None, false))),
        Err(Refusal::BareActor)
    );

    // Only director, producer, user can resume.
    for allowed in ["director", "producer", "user"] {
        assert_eq!(
            validate(
                &halted_state,
                &new_ev(allowed, resume(Some("checked"), true)),
            ),
            Ok(()),
            "role {allowed} must be allowed to resume"
        );
    }
}

#[test]
fn halted_steps_and_metrics_track_halts_resumes_fp_and_durations() {
    use crate::metrics::{MetricsInput, StudioEventsInput, compute_metrics};
    use crate::steps::next_steps;

    let mut events = running_events();
    // Add uncast t2 check: before halt, next_steps asks director to cast t2.
    assert!(
        next_steps(&replay(&events))
            .iter()
            .any(|s| s.label == "cast t2")
    );

    // First halt at 10_000, resumed false-positive at 25_000 -> 15_000ms.
    events.push(Event {
        v: LEDGER_VERSION,
        seq: 6,
        ts_ms: 10_000,
        by: by("writer"),
        kind: halt("wrong assumption"),
    });
    let mid_state = replay(&events);
    let mid_steps = next_steps(&mid_state);
    assert!(
        !mid_steps.iter().any(|s| s.label == "cast t2"),
        "cast t2 step must wait while halted: {mid_steps:?}"
    );
    assert!(
        mid_steps.iter().any(|s| s.label == "deliver t1"),
        "running task step continues while halted: {mid_steps:?}"
    );

    events.push(Event {
        v: LEDGER_VERSION,
        seq: 7,
        ts_ms: 25_000,
        by: by("director"),
        kind: resume(Some("checked"), true),
    });
    // Second halt at 40_000, still open at now_ms = 60_000 -> +20_000ms (total 35_000ms).
    events.push(Event {
        v: LEDGER_VERSION,
        seq: 8,
        ts_ms: 40_000,
        by: by("actor-t1-c1"),
        kind: halt("upstream API mismatch"),
    });

    let report = compute_metrics(&MetricsInput {
        studios: vec![StudioEventsInput {
            studio: "acme-shop".into(),
            events: events.clone(),
        }],
        hollywood_events: Vec::new(),
        hollywood_include_unscoped: false,
        since_ms: None,
        now_ms: 60_000,
    });
    let h = &report.studios[0].halts;
    assert_eq!(h.halts, 2);
    assert_eq!(h.resumes, 1);
    assert_eq!(h.false_positives, 1);
    assert!((h.false_positive_share.unwrap() - 1.0).abs() < 1e-9);
    assert_eq!(h.halted_ms, 35_000);
    assert_eq!(report.all.halts, *h);

    // `--since 20_000` clips first halt's interval to [20_000, 25_000] + [40_000, 60_000] = 25_000ms.
    let windowed = compute_metrics(&MetricsInput {
        studios: vec![StudioEventsInput {
            studio: "acme-shop".into(),
            events,
        }],
        hollywood_events: Vec::new(),
        hollywood_include_unscoped: false,
        since_ms: Some(20_000),
        now_ms: 60_000,
    });
    assert_eq!(windowed.studios[0].halts.halts, 1);
    assert_eq!(windowed.studios[0].halts.resumes, 1);
    assert_eq!(windowed.studios[0].halts.halted_ms, 25_000);
}
