//! The S9b role matrix: what each role key may record. `validate`
//! runs `roles::check` before any state check, so a refused role
//! hears `role '<r>' cannot do this; <who> can` — and the events a
//! role may record still land exactly as before.

use std::collections::BTreeMap;

use crate::event::*;
use crate::state::{TaskStatus, replay};
use crate::testkit::*;
use crate::validate::{Refusal, validate};

/// `running_state`: t1 runs cast to actor-a (`actor-t1-c1`), t2 is
/// created. The whitelist — own task's delivery, own task's
/// `blocked`, purpose-free messages — still lands; the rest refuses
/// naming who may.
#[test]
fn an_actor_records_only_its_own_delivery_block_and_messages() {
    let state = running_state();
    assert_eq!(
        validate(&state, &new_ev("actor-t1-c1", delivered("t1", 1, "a", 1))),
        Ok(())
    );
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "actor-t1-c1",
                task_state("t1", TaskStateChange::Blocked, "needs an answer"),
            )
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
                    file: Some("messages/9-m.md".into()),
                    purpose: None,
                    source_seq: None,
                },
            )
        ),
        Ok(())
    );

    // Everything else refuses, naming who may record it.
    for (kind, who) in [
        (
            reviewed("t1", 1, Verdict::Accepted, vec![]),
            "the user, the producer or the director",
        ),
        (
            task_finding("t1", 1, vec![finding("f1", Cause::DeliveryDefect)]),
            "the user, the producer or the director",
        ),
        (
            cast("t1", "actor-b", "b"),
            "the user, the producer or the director",
        ),
        (
            task_created("t9", "x", 1),
            "the user, the producer or the director",
        ),
        (
            task_state("t1", TaskStateChange::Paused, "hold"),
            "the user, the producer or the director",
        ),
        (
            task_state("t1", TaskStateChange::Resumed, "go"),
            "the user, the producer or the director",
        ),
        (
            task_state("t1", TaskStateChange::Abandoned, "cut"),
            "the user, the producer or the director",
        ),
        (
            task_state("t1", TaskStateChange::TakenOver, "mine"),
            "the user, the producer or the director",
        ),
        (
            EventKind::Correction {
                target: 1,
                reason: "r".into(),
                set: BTreeMap::from([("verdict".to_string(), "accepted".to_string())]),
            },
            "the user, the producer or the director",
        ),
        (
            EventKind::ReqVersion {
                version: 2,
                file: "req/v2.md".into(),
                summary: "s".into(),
            },
            "the producer or the user",
        ),
        (
            EventKind::PlanPublished {
                plan_kind: PlanKind::Plan,
                file: "plan/9.md".into(),
                requirement_version: 1,
            },
            "the writer",
        ),
        (
            EventKind::MessageQueued {
                to: "writer".into(),
                file: None,
                purpose: Some("plan".into()),
                source_seq: None,
            },
            "the producer or the stage",
        ),
        (
            EventKind::ReportSent {
                to: "user".into(),
                node: "director".into(),
                file: "reports/9.md".into(),
            },
            "the director or the producer",
        ),
        (
            EventKind::RoleApproval {
                role: "actor-t1-c1".into(),
                decision: ApprovalDecision::Allow,
            },
            "the user or the director",
        ),
        (
            EventKind::TaskPriority {
                task: "t1".into(),
                priority: Priority::High,
            },
            "the user or the director",
        ),
        (
            EventKind::StudioFinished {
                reason: "done".into(),
            },
            "the user or the producer",
        ),
        (studio_kind(), "the user"),
        (
            EventKind::RoleStarted {
                role: "actor-t1-c1".into(),
                terminal: None,
                actor: None,
                session_id: None,
                reason: None,
            },
            "the stage",
        ),
        (
            EventKind::AttentionRaised {
                role: "actor-t1-c1".into(),
                reason: PERMISSION_PROMPT.into(),
                detail: String::new(),
                needs_user: false,
            },
            "the stage",
        ),
    ] {
        assert_eq!(
            validate(&state, &new_ev("actor-t1-c1", kind.clone())),
            Err(Refusal::RoleBound {
                role: "actor-t1-c1".into(),
                who,
            }),
            "{kind:?}"
        );
    }
}

/// Only the current cast delivers, and only on its own task: a
/// recast makes the old key a stranger.
#[test]
fn an_actor_delivers_only_as_its_tasks_current_cast() {
    let state = running_state();
    // Another task's delivery is not the actor's to record.
    assert_eq!(
        validate(&state, &new_ev("actor-t1-c1", delivered("t2", 1, "a", 1))),
        Err(Refusal::RoleBound {
            role: "actor-t1-c1".into(),
            who: "the task's current actor or the user",
        })
    );
    // After a recast the old cast's key is refused, the new one may.
    let recast = state_with(
        &base_events(),
        vec![
            ("director", cast("t1", "actor-a", "a")),
            ("director", cast("t1", "actor-b", "b")),
        ],
    );
    assert_eq!(
        validate(&recast, &new_ev("actor-t1-c1", delivered("t1", 1, "a", 1))),
        Err(Refusal::RoleBound {
            role: "actor-t1-c1".into(),
            who: "the task's current actor or the user",
        })
    );
    assert_eq!(
        validate(&recast, &new_ev("actor-t1-c2", delivered("t1", 1, "a", 1))),
        Ok(())
    );
}

/// The writer's whitelist is `plan.published` and `tell`/`ask` —
/// `review` and everything else refuse.
#[test]
fn the_writer_records_only_plans_and_messages() {
    let state = running_state();
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "writer",
                EventKind::PlanPublished {
                    plan_kind: PlanKind::Plan,
                    file: "plan/9.md".into(),
                    requirement_version: 1,
                },
            )
        ),
        Ok(())
    );
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "writer",
                EventKind::MessageQueued {
                    to: "director".into(),
                    file: None,
                    purpose: None,
                    source_seq: None,
                },
            )
        ),
        Ok(())
    );
    for (kind, who) in [
        (
            reviewed("t1", 1, Verdict::Accepted, vec![]),
            "the user, the producer or the director",
        ),
        (
            EventKind::MessageQueued {
                to: "actor-t1-c1".into(),
                file: None,
                purpose: Some("dispatch".into()),
                source_seq: None,
            },
            "the producer or the stage",
        ),
        (
            task_created("t9", "x", 1),
            "the user, the producer or the director",
        ),
        (
            task_state("t1", TaskStateChange::Blocked, "x"),
            "the task's own actor or a person",
        ),
        (
            EventKind::RoleApproval {
                role: "writer".into(),
                decision: ApprovalDecision::Deny,
            },
            "the user or the director",
        ),
        (studio_kind(), "the user"),
        (
            EventKind::ReportSent {
                to: "user".into(),
                node: "director".into(),
                file: "r.md".into(),
            },
            "the director or the producer",
        ),
    ] {
        assert_eq!(
            validate(&state, &new_ev("writer", kind.clone())),
            Err(Refusal::RoleBound {
                role: "writer".into(),
                who,
            }),
            "{kind:?}"
        );
    }
}

/// `task.delivered` from anyone who is not the task's current actor
/// is the user's alone — producer and director do not deliver and
/// the stage never does; a bare `actor` is refused as no caller at
/// all (S9g).
#[test]
fn delivery_belongs_to_the_current_actor_or_the_user() {
    let state = running_state();
    let d = delivered("t1", 1, "a", 1);
    assert_eq!(validate(&state, &new_ev("user", d.clone())), Ok(()));
    for role in ["producer", "director", "stage", "writer", "nobody"] {
        assert_eq!(
            validate(&state, &new_ev(role, d.clone())),
            Err(Refusal::RoleBound {
                role: role.into(),
                who: "the task's current actor or the user",
            }),
            "{role}"
        );
    }
    assert_eq!(
        validate(&state, &new_ev("actor", d)),
        Err(Refusal::BareActor)
    );
}

/// The bare `actor` kind name is no caller (S9g): every kind — the
/// ones an actor key may record and the forbidden ones alike —
/// refuses, naming the key an actor records under. Replay stays
/// permissive: an old v0.1.0 log's `actor` events still fold.
#[test]
fn a_bare_actor_records_nothing() {
    let state = running_state();
    // The three kinds an actor key may record refuse under the bare
    // name, and so do the rest — there is no permissive fallthrough.
    let kinds = [
        delivered("t1", 1, "a", 1),
        task_state("t1", TaskStateChange::Blocked, "needs an answer"),
        EventKind::MessageQueued {
            to: "director".into(),
            file: Some("messages/9-m.md".into()),
            purpose: None,
            source_seq: None,
        },
        reviewed("t1", 1, Verdict::Accepted, vec![]),
        cast("t1", "actor-b", "b"),
        EventKind::TaskChecked {
            task: "t1".into(),
            delivery: 1,
            name: "units".into(),
            exit_code: Some(0),
            timed_out: false,
            output: "checks/9.txt".into(),
            duration_ms: 1,
        },
        EventKind::RoleApproval {
            role: "actor-t1-c1".into(),
            decision: ApprovalDecision::Allow,
        },
    ];
    for kind in &kinds {
        assert_eq!(
            validate(&state, &new_ev("actor", kind.clone())),
            Err(Refusal::BareActor),
            "{kind:?}"
        );
    }
    assert_eq!(
        Refusal::BareActor.to_string(),
        "role 'actor' cannot do this; an actor records under its key actor-<task>-c<n>"
    );

    // Replay is permissive: a v0.1.0 log recorded under the bare
    // name still loads and folds — the refusal is on append only.
    let mut log = base_events();
    log.push(ev(4, "director", cast("t1", "actor-a", "a")));
    log.push(ev(5, "actor", delivered("t1", 1, "a", 1)));
    log.push(ev(6, "actor", reviewed("t1", 1, Verdict::Accepted, vec![])));
    let state = replay(&log);
    assert_eq!(state.tasks["t1"].status, TaskStatus::Accepted);
}

/// User, producer, director and stage keep what they had — the gate
/// only narrows actor and writer and claims `task.delivered`.
#[test]
fn the_other_roles_keep_what_they_had() {
    let state = running_state();
    assert_eq!(
        validate(&state, &new_ev("director", cast("t2", "actor-a", "a"))),
        Ok(())
    );
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "producer",
                task_state("t2", TaskStateChange::Abandoned, "cut"),
            )
        ),
        Ok(())
    );
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "stage",
                EventKind::AttentionRaised {
                    role: "actor-t1-c1".into(),
                    reason: PERMISSION_PROMPT.into(),
                    detail: String::new(),
                    needs_user: false,
                },
            )
        ),
        Ok(())
    );
}
