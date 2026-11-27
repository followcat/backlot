//! PR-4 tests (read-only audit tasks): `readonly` permission profile
//! in cast/recast, cast-time lowering to `readonly`, empty-commit
//! delivery rules, and refusal of integrating an audit task.

use crate::cast;
use crate::event::*;
use crate::recast;
use crate::state::replay;
use crate::testkit::*;
use crate::validate::{Refusal, validate};

#[test]
fn cast_and_recast_accept_readonly_profile() {
    let state = replay(&base_events());
    let mut next = cast::CastSettings::of(&state).unwrap();
    next.candidates[0].permission_profile = "readonly".into();
    assert_eq!(cast::check(&state, &next), Ok(()));
    assert_eq!(cast::check_runnable(&state, &next), Ok(()));

    let recast_id = ActorIdentity {
        runner: "claude".into(),
        runner_version: None,
        model: "claude-opus-4".into(),
        effort: Some("high".into()),
        permission_profile: "readonly".into(),
    };
    assert_eq!(recast::check(&state, "director", &recast_id), Ok(()));

    let msg = Refusal::ProfileNotSupported("bogus".into()).to_string();
    assert!(msg.contains("readonly"), "{msg}");
}

#[test]
fn cast_readonly_lowers_candidate_profile_and_audit_requires_readonly() {
    let mut base = base_events();
    base.push(ev(4, "director", task_created("t_audit", "audit", 1)));
    let state = replay(&base);

    // Lowering a writable candidate ("actor-a") to readonly on `t1` succeeds.
    let mut lowered = actor("a");
    lowered.permission_profile = "readonly".into();
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "director",
                EventKind::TaskCast {
                    task: "t1".into(),
                    candidate: "actor-a".into(),
                    actor: lowered.clone(),
                    reason: "read-only check".into(),
                    priority: None,
                },
            ),
        ),
        Ok(())
    );

    // Casting a non-readonly actor on `task_kind == "audit"` refuses.
    assert!(
        validate(
            &state,
            &new_ev("director", cast("t_audit", "actor-a", "a")),
        )
        .is_err(),
        "audit task must refuse non-readonly cast"
    );

    // Casting with lowered readonly identity on `t_audit` succeeds.
    assert_eq!(
        validate(
            &state,
            &new_ev(
                "director",
                EventKind::TaskCast {
                    task: "t_audit".into(),
                    candidate: "actor-a".into(),
                    actor: lowered,
                    reason: "audit".into(),
                    priority: None,
                },
            ),
        ),
        Ok(())
    );
}

#[test]
fn readonly_delivery_refuses_commits_and_allows_empty_commits() {
    let mut lowered = actor("a");
    lowered.permission_profile = "readonly".into();
    let state = state_with(
        &base_events(),
        vec![(
            "director",
            EventKind::TaskCast {
                task: "t1".into(),
                candidate: "actor-a".into(),
                actor: lowered.clone(),
                reason: "audit".into(),
                priority: None,
            },
        )],
    );

    let with_commits = EventKind::TaskDelivered {
        task: "t1".into(),
        delivery: 1,
        actor: lowered.clone(),
        commits: "111aaa..222bbb".into(),
        report: "reports/audit.md".into(),
        checks: Vec::new(),
        requirement_version: 1,
        usage: None,
    };
    assert!(
        validate(&state, &new_ev("actor-t1-c1", with_commits)).is_err(),
        "readonly delivery must refuse commits"
    );

    let report_only = EventKind::TaskDelivered {
        task: "t1".into(),
        delivery: 1,
        actor: lowered,
        commits: String::new(),
        report: "reports/audit.md".into(),
        checks: Vec::new(),
        requirement_version: 1,
        usage: None,
    };
    assert_eq!(validate(&state, &new_ev("actor-t1-c1", report_only)), Ok(()));
}

#[test]
fn validate_refuses_integrating_audit_task() {
    let mut studio = studio_kind();
    if let EventKind::StudioCreated { checks, .. } = &mut studio {
        *checks = vec![check_decl("unit", "cargo test", 60)];
    }
    let mut lowered = actor("a");
    lowered.permission_profile = "readonly".into();
    let state = replay(&[
        ev(1, "user", studio),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "v1".into(),
            },
        ),
        ev(3, "director", task_created("t_audit", "audit", 1)),
        ev(
            4,
            "director",
            EventKind::TaskCast {
                task: "t_audit".into(),
                candidate: "actor-a".into(),
                actor: lowered.clone(),
                reason: "audit".into(),
                priority: None,
            },
        ),
        ev(
            5,
            "actor-t_audit-c1",
            EventKind::TaskDelivered {
                task: "t_audit".into(),
                delivery: 1,
                actor: lowered,
                commits: String::new(),
                report: "reports/audit.md".into(),
                checks: Vec::new(),
                requirement_version: 1,
                usage: None,
            },
        ),
    ]);

    // A task cannot declare a dependency that would integrate an audit task.
    let child = EventKind::TaskCreated {
        task: "t_next".into(),
        title: "follow-up".into(),
        task_kind: "feature".into(),
        requirement_version: 1,
        acceptance: "plan/t_next.md".into(),
        dependencies: vec!["t_audit".into()],
    };
    assert!(
        validate(&state, &new_ev("director", child)).is_err(),
        "must refuse dependency integrating audit task"
    );

    // A review of an audit task cannot carry upstream integration edits.
    let upstream = EventKind::TaskReviewed {
        task: "t_audit".into(),
        delivery: 1,
        reviewer: "director".into(),
        verdict: Verdict::Accepted,
        conclusion: "ok".into(),
        findings: Vec::new(),
        upstream_changes: vec![UpstreamChange {
            summary: "tweak".into(),
            reason: "fix".into(),
            defect: None,
        }],
    };
    assert!(
        validate(&state, &new_ev("director", upstream)).is_err(),
        "must refuse upstream_changes on audit task"
    );

    // Accepting an audit task without commits does not wait on stage code checks.
    let accept = EventKind::TaskReviewed {
        task: "t_audit".into(),
        delivery: 1,
        reviewer: "director".into(),
        verdict: Verdict::Accepted,
        conclusion: "clean audit".into(),
        findings: Vec::new(),
        upstream_changes: Vec::new(),
    };
    assert_eq!(validate(&state, &new_ev("director", accept)), Ok(()));
}
