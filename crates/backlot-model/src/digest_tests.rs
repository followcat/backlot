//! Digest and coverage matrix tests over synthetic neutral studios.

use std::collections::BTreeMap;

use crate::event::{
    CheckDecl, Event, EventBy, EventKind, LEDGER_VERSION, TaskStateChange, Verdict,
};
use crate::metrics::{
    DIGEST_SCHEMA_VERSION, DigestInput, StudioDigestInput, compute_digest, extract_design_refs,
};
use crate::testkit::{actor, studio_kind};

fn evt(seq: u64, ts_ms: u64, role: &str, kind: EventKind) -> Event {
    Event {
        v: LEDGER_VERSION,
        seq,
        ts_ms,
        by: EventBy::role(role),
        kind,
    }
}

fn studio_event(
    seq: u64,
    ts_ms: u64,
    title: &str,
    repo_path: &str,
    decls: Vec<CheckDecl>,
) -> Event {
    let mut kind = studio_kind();
    if let EventKind::StudioCreated {
        title: ref mut t,
        repo: ref mut r,
        checks: ref mut c,
        ..
    } = kind
    {
        *t = title.into();
        *r = repo_path.into();
        *c = decls;
    }
    evt(seq, ts_ms, "user", kind)
}

fn digest_studio_events() -> Vec<Event> {
    vec![
        studio_event(
            1,
            1_000,
            "acme-shop",
            "/home/user/acme-shop",
            vec![
                CheckDecl::new("test", "cargo test", 60),
                CheckDecl::new("lint", "cargo clippy", 60),
            ],
        ),
        evt(
            2,
            10_000,
            "director",
            EventKind::TaskCreated {
                task: "t1".into(),
                title: "Core metrics".into(),
                task_kind: "code".into(),
                requirement_version: 1,
                acceptance: "tasks/t1/acceptance.md".into(),
                dependencies: vec![],
            },
        ),
        evt(
            3,
            12_000,
            "director",
            EventKind::TaskCreated {
                task: "t2".into(),
                title: "Wait task".into(),
                task_kind: "code".into(),
                requirement_version: 1,
                acceptance: "tasks/t2/acceptance.md".into(),
                dependencies: vec![],
            },
        ),
        evt(
            4,
            15_000,
            "actor-t1-c1",
            EventKind::TaskDelivered {
                task: "t1".into(),
                delivery: 1,
                actor: actor("t1"),
                commits: "111aaa..222bbb".into(),
                report: "implemented metrics".into(),
                requirement_version: 1,
                checks: vec![],
                usage: None,
            },
        ),
        evt(
            5,
            16_000,
            "stage",
            EventKind::TaskChecked {
                task: "t1".into(),
                delivery: 1,
                name: "test".into(),
                exit_code: Some(0),
                timed_out: false,
                expect_failed: None,
                output: "checks/t1-d1.log".into(),
                duration_ms: 250,
            },
        ),
        evt(
            6,
            17_000,
            "director",
            EventKind::TaskReviewed {
                task: "t1".into(),
                delivery: 1,
                reviewer: "director".into(),
                verdict: Verdict::Accepted,
                conclusion: "accepted cleanly".into(),
                findings: vec![],
                upstream_changes: vec![],
            },
        ),
        evt(
            7,
            18_000,
            "director",
            EventKind::TaskState {
                task: "t2".into(),
                state: TaskStateChange::Blocked,
                reason: "waiting on schema".into(),
            },
        ),
        evt(
            8,
            19_000,
            "stage",
            EventKind::AttentionRaised {
                role: "director".into(),
                reason: "cannot resume".into(),
                detail: "no session".into(),
                needs_user: false,
                reset_at_ms: None,
                account: None,
            },
        ),
        evt(
            9,
            20_000,
            "stage",
            EventKind::AttentionRaised {
                role: "writer".into(),
                reason: "rate limited".into(),
                detail: "waiting until 12:00".into(),
                needs_user: false,
                reset_at_ms: Some(900_000),
                account: None,
            },
        ),
        evt(
            10,
            21_000,
            "stage",
            EventKind::Unknown {
                raw_kind: "future.digest.test".into(),
            },
        ),
    ]
}

#[test]
fn extract_design_refs_parses_versions_and_sections() {
    let md = "\
# Acceptance
Covers design v1 §R1 and Design v2 §R2.D+.
Ignore trailing punctuation (design v1 §R3.W), duplicates like design v1 §R1.
";
    assert_eq!(
        extract_design_refs(md),
        vec![
            "design v1 §R1".to_string(),
            "design v1 §R3.W".to_string(),
            "design v2 §R2.D+".to_string(),
        ]
    );
}

#[test]
fn extract_design_refs_handles_cjk_and_multibyte_text() {
    let md = "验收：已完成 design v1 §R1 的接口，见 design v2 §R2.W";
    assert_eq!(
        extract_design_refs(md),
        vec!["design v1 §R1".to_string(), "design v2 §R2.W".to_string()]
    );
    // Also exercise multibyte text right after every word/symbol boundary.
    assert_eq!(
        extract_design_refs("design版本；设计 v1 §R1；design v1；design v1 §R2的接口"),
        vec!["design v1 §R2".to_string()]
    );
}

#[test]
fn digest_builds_coverage_matrix_and_activity_sections() {
    let mut acceptance_texts = BTreeMap::new();
    acceptance_texts.insert(
        "tasks/t1/acceptance.md".into(),
        "Verify design v1 §R1 and design v1 §R2 metrics.".into(),
    );

    let report = compute_digest(&DigestInput {
        studios: vec![StudioDigestInput {
            studio: "acme-shop".into(),
            events: digest_studio_events(),
            acceptance_texts,
        }],
        hollywood_events: vec![],
        since_ms: 10_000,
        now_ms: 30_000,
    });

    assert_eq!(report.schema_version, DIGEST_SCHEMA_VERSION);
    assert_eq!(report.unknown_events_skipped, 1);
    assert_eq!(report.studios.len(), 1);
    let s = &report.studios[0];
    assert_eq!(s.checks, vec!["test", "lint"]);
    assert_eq!(s.finished.len(), 1);
    assert_eq!(s.finished[0].task, "t1");
    assert_eq!(s.delivered.len(), 1);
    assert_eq!(s.blocked.len(), 1);
    assert_eq!(s.blocked[0].task, "t2");
    assert_eq!(s.waiting_on_user.attentions.len(), 1);
    assert_eq!(s.waiting_on_user.attentions[0].role, "director");
    assert_eq!(s.rate_limits.open.len(), 1);
    assert_eq!(s.rate_limits.events_in_window, 1);

    // Coverage matrix has one row per cited design section (plus t2 unreferenced row).
    let r1 = s
        .coverage
        .iter()
        .find(|r| r.design_section.as_deref() == Some("design v1 §R1") && r.task == "t1")
        .expect("R1 coverage row");
    assert_eq!(r1.commits.as_deref(), Some("111aaa..222bbb"));
    assert_eq!(r1.verdict, "accepted");
    assert_eq!(r1.checks_summary, "test:pass, lint:-");

    let r2 = s
        .coverage
        .iter()
        .find(|r| r.design_section.as_deref() == Some("design v1 §R2") && r.task == "t1")
        .expect("R2 coverage row");
    assert_eq!(r2.verdict, "accepted");
}

#[test]
fn studio_without_checks_reports_checks_none_cleanly() {
    let events = vec![
        studio_event(1, 1_000, "beta-api", "/home/user/beta-api", vec![]),
        evt(
            2,
            10_000,
            "director",
            EventKind::TaskCreated {
                task: "t1".into(),
                title: "Docs update".into(),
                task_kind: "docs".into(),
                requirement_version: 1,
                acceptance: "tasks/t1/acceptance.md".into(),
                dependencies: vec![],
            },
        ),
    ];

    let report = compute_digest(&DigestInput {
        studios: vec![StudioDigestInput {
            studio: "beta-api".into(),
            events,
            acceptance_texts: BTreeMap::new(),
        }],
        hollywood_events: vec![],
        since_ms: 0,
        now_ms: 20_000,
    });

    let s = &report.studios[0];
    assert!(s.checks.is_empty());
    assert_eq!(s.checks_status, "checks: none");
    assert_eq!(s.coverage.len(), 1);
    assert_eq!(s.coverage[0].checks_summary, "checks: none");
}

#[test]
fn digest_json_shape_serializes_cleanly() {
    let report = compute_digest(&DigestInput {
        studios: vec![StudioDigestInput {
            studio: "acme-shop".into(),
            events: digest_studio_events(),
            acceptance_texts: BTreeMap::new(),
        }],
        hollywood_events: vec![],
        since_ms: 0,
        now_ms: 30_000,
    });
    let val = serde_json::to_value(&report).expect("serialize digest");
    assert_eq!(val["schema_version"], 1);
    assert!(val["studios"].is_array());
    assert!(!val["studios"][0]["coverage"].as_array().unwrap().is_empty());
    assert!(val["studios"][0]["waiting_on_user"].is_object());
    assert!(val["studios"][0]["rate_limits"].is_object());
}
