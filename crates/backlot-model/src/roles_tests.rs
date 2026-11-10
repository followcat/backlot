//! The S9b role matrix: what each role key may record. `validate`
//! runs `roles::check` before any state check, so a refused role
//! hears `role '<r>' cannot do this; <who> can` — and the events a
//! role may record still land exactly as before.

use std::collections::BTreeMap;

use crate::event::*;
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
/// is the user's alone — producer and director do not deliver, a
/// bare `actor` is not a role key, and the stage never delivers.
#[test]
fn delivery_belongs_to_the_current_actor_or_the_user() {
    let state = running_state();
    let d = delivered("t1", 1, "a", 1);
    assert_eq!(validate(&state, &new_ev("user", d.clone())), Ok(()));
    for role in ["producer", "director", "stage", "writer", "actor", "nobody"] {
        assert_eq!(
            validate(&state, &new_ev(role, d.clone())),
            Err(Refusal::RoleBound {
                role: role.into(),
                who: "the task's current actor or the user",
            }),
            "{role}"
        );
    }
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
                },
            )
        ),
        Ok(())
    );
}
