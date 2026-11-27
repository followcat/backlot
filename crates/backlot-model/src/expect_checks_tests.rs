//! Fail-first & regression tests for ledger backwards compatibility with
//! `expect` / `expect_failed` / `origin` / `DesignEntry.checks` / `DesignSummary.proposed_checks`.
//! Neutral fixtures only (`acme-shop`, `actor-a`, `/home/user/acme-shop`).

use crate::design::DesignPhase;
use crate::state::replay;
use crate::summary::Summary;
use crate::Event;

#[test]
fn legacy_ledger_without_check_activation_fields_replays_unchanged() {
    let raw_lines = [
        r#"{"seq":1,"ts_ms":100,"by":{"role":"user"},"kind":"studio.created","title":"Acme Shop","repo":"/home/user/acme-shop","mode":"standard","roles":{"producer":{"runner":"fake","model":"m","effort":"high","permission_profile":"standard"},"director":{"runner":"fake","model":"m","effort":"high","permission_profile":"standard"},"writer":{"runner":"fake","model":"m","effort":"high","permission_profile":"standard"}},"candidates":[{"name":"actor-a","runner":"fake","model":"m","effort":"high","permission_profile":"worktree"}],"rework_cap":2,"stall_limit_s":600,"checks":[{"name":"unit","run":"cargo test --lib","timeout_s":300}],"design_gate":"required"}"#,
        r#"{"seq":2,"ts_ms":200,"by":{"role":"producer"},"kind":"req.version","version":1,"file":"req/v1.md","summary":"first"}"#,
        r#"{"seq":3,"ts_ms":300,"by":{"role":"producer"},"kind":"design.opened","version":1,"round":1,"doc_file":"design/v1.md"}"#,
        r#"{"seq":4,"ts_ms":400,"by":{"role":"writer"},"kind":"design.entry","version":1,"round":1,"slot":"proposal","file":"design/v1/r1-4-proposal.md"}"#,
        r#"{"seq":5,"ts_ms":500,"by":{"role":"director"},"kind":"design.entry","version":1,"round":1,"slot":"buildability","file":"design/v1/r1-5-buildability.md"}"#,
        r#"{"seq":6,"ts_ms":600,"by":{"role":"producer"},"kind":"design.entry","version":1,"round":1,"slot":"summary","file":"design/v1/r1-6-summary.md"}"#,
        r#"{"seq":7,"ts_ms":700,"by":{"role":"user"},"kind":"design.signed","version":1,"round":1,"mode":"approved","doc_file":"design/v1-final.md"}"#,
        r#"{"seq":8,"ts_ms":800,"by":{"role":"director"},"kind":"task.created","task":"t1","title":"cart","task_kind":"feature","requirement_version":1,"acceptance":"plan/acc.md"}"#,
        r#"{"seq":9,"ts_ms":900,"by":{"role":"director"},"kind":"task.cast","task":"t1","candidate":"actor-a","actor":{"runner":"fake","model":"m","effort":"high","permission_profile":"worktree"},"reason":"best"}"#,
        r#"{"seq":10,"ts_ms":1000,"by":{"role":"actor-t1-c1"},"kind":"task.delivered","task":"t1","delivery":1,"actor":{"runner":"fake","model":"m","effort":"high","permission_profile":"worktree"},"commits":"a..b","report":"reports/t1-d1.md","requirement_version":1,"checks":[]}"#,
        r#"{"seq":11,"ts_ms":1100,"by":{"role":"stage"},"kind":"task.checked","task":"t1","delivery":1,"name":"unit","exit_code":0,"output":"checks/10-unit.log","duration_ms":55}"#,
    ];
    let events: Vec<Event> = raw_lines
        .iter()
        .map(|l| serde_json::from_str(l).expect("parse legacy event"))
        .collect();
    let state = replay(&events);
    let cfg = state.studio.as_ref().expect("studio");
    assert_eq!(cfg.checks.len(), 1);
    assert_eq!(cfg.checks[0].name, "unit");
    assert_eq!(cfg.checks[0].timeout_s, 300);

    let d = state.design.as_ref().expect("design");
    assert_eq!(d.phase, DesignPhase::Signed);

    let t1 = state.tasks.get("t1").expect("task t1");
    let rec = t1.checked_of(1).next().expect("checked record");
    assert!(rec.passed());
    assert_eq!(rec.exit_code, Some(0));
}

#[test]
fn legacy_summary_without_proposed_checks_deserializes() {
    let raw = r#"{
        "schema": "backlot/1",
        "generated_ms": 1000,
        "updated_ms": 1000,
        "studios": [{
            "id": "acme-shop",
            "title": "Acme Shop",
            "state": "active",
            "mode": "standard",
            "open_reviews": 0,
            "requirement_version": 1,
            "waiting": 0,
            "updated_ms": 1000,
            "roles": [],
            "design": {
                "version": 1,
                "round": 1,
                "phase": "waiting_user",
                "waiting_user": true,
                "suggest_skip": false,
                "doc_file": "design/v1.md",
                "questions": []
            }
        }]
    }"#;
    let summary: Summary = serde_json::from_str(raw).expect("legacy summary");
    assert_eq!(summary.studios.len(), 1);
    assert!(summary.studios[0].design.is_some());
}

#[test]
fn expect_failed_keeps_exit_code_zero_and_refuses_accepted_review() {
    use crate::event::{CheckDecl, EventKind, Verdict};
    use crate::testkit::*;
    use crate::validate::{validate, Refusal};

    let mut kind = studio_kind();
    let EventKind::StudioCreated { checks, .. } = &mut kind else {
        unreachable!()
    };
    *checks = vec![CheckDecl {
        expect: Some("files_scanned=[1-9]".into()),
        min_matches: Some(1),
        ..check_decl("scan", "sh scan.sh", 60)
    }];
    let state = replay(&[
        ev(1, "user", kind),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "first".into(),
            },
        ),
        ev(3, "director", task_created("t1", "feature", 1)),
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(5, "actor-t1-c1", delivered("t1", 1, "a", 1)),
        ev(
            6,
            "stage",
            EventKind::TaskChecked {
                task: "t1".into(),
                delivery: 1,
                name: "scan".into(),
                exit_code: Some(0),
                timed_out: false,
                expect_failed: Some("expect 'files_scanned=[1-9]' matched 0 (min 1)".into()),
                output: "checks/5-scan.log".into(),
                duration_ms: 12,
            },
        ),
    ]);
    let t1 = state.tasks.get("t1").unwrap();
    let rec = t1.checked_of(1).next().unwrap();
    assert_eq!(rec.exit_code, Some(0));
    assert!(!rec.passed());

    let review_ev = new_ev(
        "director",
        EventKind::TaskReviewed {
            task: "t1".into(),
            delivery: 1,
            reviewer: "director".into(),
            verdict: Verdict::Accepted,
            conclusion: "clean".into(),
            findings: Vec::new(),
            upstream_changes: Vec::new(),
        },
    );
    assert_eq!(
        validate(&state, &review_ev),
        Err(Refusal::ChecksFailing {
            task: "t1".into(),
            checks: vec!["scan".into()],
        })
    );
}

#[test]
fn merge_design_checks_replaces_only_design_origin_and_refuses_collisions() {
    use crate::cast::merge_design_checks;
    use crate::event::{CheckDecl, CheckOrigin};
    use crate::testkit::check_decl;

    let cast_c = check_decl("cast_unit", "cargo test", 60);
    let user_c = CheckDecl {
        origin: CheckOrigin::User,
        ..check_decl("user_smoke", "sh smoke.sh", 30)
    };
    let old_design_c = CheckDecl {
        origin: CheckOrigin::Design,
        ..check_decl("old_prop", "echo 1", 20)
    };
    let existing = vec![cast_c.clone(), user_c.clone(), old_design_c];

    let new_prop = CheckDecl {
        expect: Some("ok".into()),
        min_matches: Some(2),
        ..check_decl("new_prop", "echo ok ok", 45)
    };
    let merged = merge_design_checks(&existing, &[new_prop]).unwrap();
    assert_eq!(merged.len(), 3);
    assert_eq!(merged[0], cast_c);
    assert_eq!(merged[1], user_c);
    assert_eq!(merged[2].name, "new_prop");
    assert_eq!(merged[2].origin, CheckOrigin::Design);

    let collide = check_decl("user_smoke", "echo hijacked", 10);
    assert!(merge_design_checks(&existing, &[collide]).is_err());
}

#[test]
fn design_summary_carries_untruncated_proposed_checks_until_dispatched() {
    use crate::design::{DesignGate, DesignSlot};
    use crate::event::{CheckDecl, EventKind};
    use crate::summary_build;
    use crate::testkit::*;

    let mut kind = studio_kind();
    let EventKind::StudioCreated { design_gate, .. } = &mut kind else {
        unreachable!()
    };
    *design_gate = DesignGate::Required;

    let long_cmd = "sh -c 'find src -type f | wc -l && echo files_scanned=5 # ".to_string()
        + &"x".repeat(250)
        + "'";
    let proposed = CheckDecl {
        expect: Some("files_scanned=[1-9]".into()),
        min_matches: Some(1),
        ..check_decl("scan", &long_cmd, 90)
    };
    let events = vec![
        ev(1, "user", kind),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "first".into(),
            },
        ),
        ev(
            3,
            "producer",
            EventKind::DesignOpened {
                version: 1,
                round: 1,
                suggest_skip: false,
                reason: None,
                doc_file: "design/v1.md".into(),
            },
        ),
        ev(
            4,
            "writer",
            EventKind::DesignEntry {
                version: 1,
                round: 1,
                slot: DesignSlot::Proposal,
                file: "design/v1/r1-4-proposal.md".into(),
                source: None,
                refs: Vec::new(),
                questions: Vec::new(),
                checks: Vec::new(),
            },
        ),
        ev(
            5,
            "director",
            EventKind::DesignEntry {
                version: 1,
                round: 1,
                slot: DesignSlot::Buildability,
                file: "design/v1/r1-5-buildability.md".into(),
                source: None,
                refs: Vec::new(),
                questions: Vec::new(),
                checks: vec![proposed.clone()],
            },
        ),
        ev(
            6,
            "producer",
            EventKind::DesignEntry {
                version: 1,
                round: 1,
                slot: DesignSlot::Summary,
                file: "design/v1/r1-6-summary.md".into(),
                source: None,
                refs: Vec::new(),
                questions: Vec::new(),
                checks: Vec::new(),
            },
        ),
    ];
    let state = replay(&events);
    let d = summary_build::design_summary(&state, &std::collections::BTreeMap::new())
        .expect("design summary");
    assert_eq!(d.proposed_checks, vec![proposed]);
}
