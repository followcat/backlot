//! Validation-rule tests: task state changes, delivery/clear links,
//! and correction targets — each accepted and refused.

use std::collections::BTreeMap;

use crate::event::*;
use crate::state::{StudioState, TaskStatus, apply, replay};
use crate::testkit::*;
use crate::validate::{Refusal, validate};

#[test]
fn task_state_transition_rules() {
    let state = running_state();
    let pause = EventKind::TaskState {
        task: "t1".into(),
        state: TaskStateChange::Paused,
        reason: "hold".into(),
    };
    assert_eq!(validate(&state, &new_ev("director", pause)), Ok(()));
    // Resume refuses on a task that is not paused.
    let resume = EventKind::TaskState {
        task: "t1".into(),
        state: TaskStateChange::Resumed,
        reason: "go".into(),
    };
    assert_eq!(
        validate(&state, &new_ev("director", resume)),
        Err(Refusal::BadPauseResume {
            task: "t1".into(),
            state: "resumed",
            status: TaskStatus::Running
        })
    );
    // Abandon works from any state, but not twice.
    let abandon = EventKind::TaskState {
        task: "t1".into(),
        state: TaskStateChange::Abandoned,
        reason: "dropped".into(),
    };
    assert_eq!(
        validate(&state, &new_ev("director", abandon.clone())),
        Ok(())
    );
    let gone = state_with(&base_events(), vec![("director", abandon)]);
    let again = EventKind::TaskState {
        task: "t1".into(),
        state: TaskStateChange::Abandoned,
        reason: "again".into(),
    };
    assert_eq!(
        validate(&gone, &new_ev("director", again)),
        Err(Refusal::Terminal {
            task: "t1".into(),
            status: TaskStatus::Abandoned
        })
    );
}

/// `task resume` on a `blocked` task is valid and replays to
/// `running` with the same cast — the actor was never lost — so a
/// delivery validates right after. A pause of a blocked task still
/// restores `blocked`, and a resume of anything that is neither
/// paused nor blocked stays refused.
#[test]
fn a_blocked_task_resumes_to_running() {
    let blocked = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            (
                "actor",
                task_state("t1", TaskStateChange::Blocked, "needs an answer"),
            ),
        ],
    );
    let resume = task_state("t1", TaskStateChange::Resumed, "answered");
    assert_eq!(
        validate(&blocked, &new_ev("director", resume.clone())),
        Ok(())
    );
    let mut state = blocked;
    apply(&mut state, &ev(6, "director", resume));
    let t = &state.tasks["t1"];
    assert_eq!(t.status, TaskStatus::Running);
    assert_eq!(t.cast.as_ref().unwrap().candidate, "actor-a");
    // The change knows who resumed it and what it restored.
    let ch = t.state_changes.last().unwrap();
    assert_eq!(ch.by, "director");
    assert_eq!(ch.resumed_to, Some(TaskStatus::Running));
    // `deliver` validates now that the task runs again.
    let d = delivered("t1", 1, "a", 1);
    assert_eq!(validate(&state, &new_ev("actor-t1-c1", d)), Ok(()));

    // A pause of a blocked task restores `blocked`, not `running`.
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
    let t = &state.tasks["t1"];
    assert_eq!(t.status, TaskStatus::Blocked);
    assert_eq!(
        t.state_changes.last().unwrap().resumed_to,
        Some(TaskStatus::Blocked)
    );

    // A task paused from `delivered` resumes to `delivered` — the
    // review, not the actor, picks back up.
    let state = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("actor", delivered("t1", 1, "a", 1)),
            (
                "director",
                task_state("t1", TaskStateChange::Paused, "hold"),
            ),
            ("director", task_state("t1", TaskStateChange::Resumed, "go")),
        ],
    );
    let t = &state.tasks["t1"];
    assert_eq!(t.status, TaskStatus::Delivered);
    assert_eq!(
        t.state_changes.last().unwrap().resumed_to,
        Some(TaskStatus::Delivered)
    );

    // A task that is neither paused nor blocked still refuses.
    let running = state_with(
        &base_events(),
        vec![("director", cast("t1", "actor-a", "a"))],
    );
    let resume = task_state("t1", TaskStateChange::Resumed, "go");
    assert_eq!(
        validate(&running, &new_ev("director", resume)),
        Err(Refusal::BadPauseResume {
            task: "t1".into(),
            state: "resumed",
            status: TaskStatus::Running
        })
    );
}

#[test]
fn delivered_and_cleared_links_validate() {
    let mut base = base_events();
    base.push(ev(
        4,
        "producer",
        EventKind::MessageQueued {
            to: "director".into(),
            file: None,
            purpose: Some("ask".into()),
            source_seq: None,
        },
    ));
    base.push(ev(
        5,
        "stage",
        EventKind::AttentionRaised {
            role: "actor".into(),
            reason: "stalled".into(),
            detail: String::new(),
            needs_user: false,
        },
    ));
    let state = replay(&base);
    assert_eq!(
        validate(
            &state,
            &new_ev("stage", EventKind::MessageDelivered { target: 4 })
        ),
        Ok(())
    );
    assert_eq!(
        validate(
            &state,
            &new_ev("stage", EventKind::MessageDelivered { target: 5 })
        ),
        Err(Refusal::NoMessage(5))
    );
    assert_eq!(
        validate(
            &state,
            &new_ev("stage", EventKind::MessageDelivered { target: 99 })
        ),
        Err(Refusal::NoMessage(99))
    );
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "stage",
                EventKind::AttentionCleared {
                    target: 5,
                    reason: None
                }
            )
        ),
        Ok(())
    );
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "stage",
                EventKind::AttentionCleared {
                    target: 4,
                    reason: None
                }
            )
        ),
        Err(Refusal::NoAttention(4))
    );
    // Double delivery / double clear refuse.
    let mut s = state;
    apply(
        &mut s,
        &ev(6, "stage", EventKind::MessageDelivered { target: 4 }),
    );
    apply(
        &mut s,
        &ev(
            7,
            "stage",
            EventKind::AttentionCleared {
                target: 5,
                reason: None,
            },
        ),
    );
    assert_eq!(
        validate(
            &s,
            &new_ev("stage", EventKind::MessageDelivered { target: 4 })
        ),
        Err(Refusal::MessageDone(4))
    );
    assert_eq!(
        validate(
            &s,
            &new_ev(
                "stage",
                EventKind::AttentionCleared {
                    target: 5,
                    reason: None
                }
            )
        ),
        Err(Refusal::AttentionDone(5))
    );
}

/// `task.finding` records on any task that has the delivery —
/// delivered, running or already closed — and never touches status.
#[test]
fn task_finding_rules() {
    // Delivered task takes the finding.
    let state = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("actor", delivered("t1", 1, "a", 1)),
        ],
    );
    let f = task_finding("t1", 1, vec![finding("D1", Cause::DeliveryDefect)]);
    assert_eq!(validate(&state, &new_ev("producer", f.clone())), Ok(()));
    // A delivery the task does not have is refused.
    let bad = task_finding("t1", 9, vec![finding("D2", Cause::DeliveryDefect)]);
    assert_eq!(
        validate(&state, &new_ev("producer", bad)),
        Err(Refusal::NoDelivery {
            task: "t1".into(),
            delivery: 9
        })
    );
    // An unknown task is refused.
    let gone = task_finding("t9", 1, vec![finding("D3", Cause::DeliveryDefect)]);
    assert_eq!(
        validate(&state, &new_ev("producer", gone)),
        Err(Refusal::NoTask("t9".into()))
    );
    // After acceptance the finding still records — and the status
    // does not move.
    let mut closed = state;
    apply(
        &mut closed,
        &ev(9, "director", reviewed("t1", 1, Verdict::Accepted, vec![])),
    );
    assert_eq!(closed.tasks["t1"].status, TaskStatus::Accepted);
    assert_eq!(validate(&closed, &new_ev("producer", f.clone())), Ok(()));
    apply(&mut closed, &ev(10, "producer", f));
    let t = &closed.tasks["t1"];
    assert_eq!(t.status, TaskStatus::Accepted);
    assert_eq!(t.finding_reports.len(), 1);
    assert_eq!(t.finding_reports[0].delivery, 1);
    assert_eq!(t.finding_reports[0].by, "producer");
    assert_eq!(t.finding_reports[0].findings[0].id, "D1");
}

/// `role.session` records only on a role the log knows.
#[test]
fn role_session_needs_a_recorded_role() {
    let mut base = base_events();
    base.push(ev(
        4,
        "stage",
        EventKind::RoleStarted {
            role: "producer".into(),
            terminal: Some("t3".into()),
            actor: None,
            session_id: None,
            reason: None,
        },
    ));
    let state = replay(&base);
    let known = EventKind::RoleSession {
        role: "producer".into(),
        session_id: "s-1".into(),
    };
    assert_eq!(validate(&state, &new_ev("stage", known)), Ok(()));
    let unknown = EventKind::RoleSession {
        role: "actor-t9-c1".into(),
        session_id: "s-2".into(),
    };
    assert_eq!(
        validate(&state, &new_ev("stage", unknown)),
        Err(Refusal::NoRole("actor-t9-c1".into()))
    );
    // No studio at all is refused before the role check.
    let no_studio = EventKind::RoleSession {
        role: "producer".into(),
        session_id: "s-1".into(),
    };
    assert_eq!(
        validate(&StudioState::default(), &new_ev("stage", no_studio)),
        Err(Refusal::NoStudio)
    );
}

/// `stage.answered` is the stage's own word, on a role the log knows —
/// an actor key may not record it.
#[test]
fn stage_answered_is_the_stages_and_needs_a_role() {
    let mut base = base_events();
    base.push(ev(4, "stage", role_started("producer")));
    let state = replay(&base);
    let kind = |role: &str| EventKind::StageAnswered {
        role: role.into(),
        dialog: FOLDER_TRUST.into(),
        key: "enter".into(),
        cwd: "/home/user/acme-shop".into(),
    };
    assert_eq!(validate(&state, &new_ev("stage", kind("producer"))), Ok(()));
    assert_eq!(
        validate(&state, &new_ev("stage", kind("actor-t9-c1"))),
        Err(Refusal::NoRole("actor-t9-c1".into()))
    );
    assert_eq!(
        validate(&state, &new_ev("actor-t1-c1", kind("producer"))),
        Err(Refusal::RoleBound {
            role: "actor-t1-c1".into(),
            who: "the stage",
        })
    );
}

#[test]
fn ask_answered_is_the_stages_and_requires_an_open_ask() {
    let mut base = base_events();
    base.push(ev(4, "stage", role_started("producer")));
    base.push(ev(
        5,
        "producer",
        EventKind::MessageQueued {
            to: "user".into(),
            file: Some("messages/5-q.md".into()),
            purpose: Some("ask".into()),
            source_seq: None,
        },
    ));
    base.push(ev(
        6,
        "director",
        EventKind::MessageQueued {
            to: "producer".into(),
            file: Some("messages/6-hi.md".into()),
            purpose: None,
            source_seq: None,
        },
    ));
    let state = replay(&base);
    let kind = |role: &str, target: u64| EventKind::AskAnswered {
        role: role.into(),
        target,
    };

    assert_eq!(
        validate(&state, &new_ev("stage", kind("producer", 5))),
        Ok(())
    );
    // Only the stage may write `ask.answered`.
    assert_eq!(
        validate(&state, &new_ev("producer", kind("producer", 5))),
        Err(Refusal::RoleBound {
            role: "producer".into(),
            who: "the stage",
        })
    );
    // Non-ask message (director -> producer) or wrong role is refused.
    assert_eq!(
        validate(&state, &new_ev("stage", kind("producer", 6))),
        Err(Refusal::NoAsk {
            role: "producer".into(),
            target: 6,
        })
    );
    assert_eq!(
        validate(&state, &new_ev("stage", kind("director", 5))),
        Err(Refusal::NoAsk {
            role: "director".into(),
            target: 5,
        })
    );

    // Once answered, a repeat is refused.
    base.push(ev(7, "stage", kind("producer", 5)));
    let answered = replay(&base);
    assert_eq!(
        validate(&answered, &new_ev("stage", kind("producer", 5))),
        Err(Refusal::AskAnswered(5))
    );
}

/// A plain `tell` to a fixed role is fine, and so is one to an actor
/// whose task is still open; one to an actor whose task is finished or
/// abandoned is refused — the actor is not woken for a closed task.
#[test]
fn a_tell_to_an_actor_of_a_closed_task_is_refused() {
    let tell = |to: &str| EventKind::MessageQueued {
        to: to.into(),
        file: Some("messages/x.md".into()),
        purpose: None,
        source_seq: None,
    };

    // Running task: a tell to its actor is fine.
    let running = running_state();
    assert_eq!(
        validate(&running, &new_ev("producer", tell("actor-t1-c1"))),
        Ok(())
    );

    // Abandoned task: the tell is refused, naming the task and state.
    let mut base = base_events();
    base.push(ev(4, "director", cast("t1", "actor-a", "a")));
    base.push(ev(
        5,
        "director",
        task_state("t1", TaskStateChange::Abandoned, "dropped"),
    ));
    let state = replay(&base);
    assert_eq!(
        validate(&state, &new_ev("producer", tell("actor-t1-c1"))),
        Err(Refusal::TellToClosedActor {
            role: "actor-t1-c1".into(),
            task: "t1".into(),
            status: TaskStatus::Abandoned,
        })
    );

    // A nudge (purpose-bearing) is not a plain tell and never wakes a
    // closed actor — but it is not the user's tell path either, so it
    // still validates even for a closed task.
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "stage",
                EventKind::MessageQueued {
                    to: "actor-t1-c1".into(),
                    file: Some("messages/x-nudge.md".into()),
                    purpose: Some("nudge".into()),
                    source_seq: Some(9),
                }
            )
        ),
        Ok(())
    );
}

#[test]
fn correction_needs_a_known_target_and_field() {
    let state = running_state();
    let mut set = BTreeMap::new();
    set.insert("title".to_string(), "renamed".to_string());
    // t1's task.created is seq 3 in base_events.
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "producer",
                EventKind::Correction {
                    target: 3,
                    reason: "typo".into(),
                    set: set.clone()
                }
            )
        ),
        Ok(())
    );
    // Unknown seq.
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "producer",
                EventKind::Correction {
                    target: 77,
                    reason: "typo".into(),
                    set: set.clone()
                }
            )
        ),
        Err(Refusal::NoCorrectionTarget(77))
    );
    // Field the target does not have.
    let mut bad = BTreeMap::new();
    bad.insert("verdict".to_string(), "accepted".to_string());
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "producer",
                EventKind::Correction {
                    target: 3,
                    reason: "wrong".into(),
                    set: bad
                }
            )
        ),
        Err(Refusal::BadCorrectionKey {
            target: 3,
            key: "verdict".into()
        })
    );
    // Unparseable value.
    let mut badv = BTreeMap::new();
    badv.insert("requirement_version".to_string(), "many".to_string());
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "producer",
                EventKind::Correction {
                    target: 3,
                    reason: "wrong".into(),
                    set: badv
                }
            )
        ),
        Err(Refusal::BadCorrectionValue {
            target: 3,
            key: "requirement_version".into(),
            value: "many".into()
        })
    );
}
