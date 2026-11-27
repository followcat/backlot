//! Integration tests for `backlot metrics` and `backlot digest`.

use std::fs;
use std::path::Path;

use backlot_model::event::{
    ApprovalDecision, CheckDecl, Event, EventBy, EventKind, LEDGER_VERSION, PERMISSION_PROMPT,
    RefusalDisposition, TaskStateChange, Verdict,
};
use backlot_model::hollywood::{HollywoodEvent, HollywoodKind, NewHollywoodEvent};
use backlot_model::testkit::{actor, studio_kind};

use crate::cli_testkit::*;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(1_000_000)
}

fn evt(seq: u64, ts_ms: u64, role: &str, kind: EventKind) -> Event {
    Event {
        v: LEDGER_VERSION,
        seq,
        ts_ms,
        by: EventBy::role(role),
        kind,
    }
}

fn studio_ev(seq: u64, ts_ms: u64, title: &str, repo_path: &str, checks: Vec<CheckDecl>) -> Event {
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
        *c = checks;
    }
    evt(seq, ts_ms, "user", kind)
}

fn write_events(dir: &Path, events: &[Event], raw_tail: Option<&str>) {
    fs::create_dir_all(dir).unwrap();
    let mut out = String::new();
    for e in events {
        out.push_str(&serde_json::to_string(e).unwrap());
        out.push('\n');
    }
    if let Some(line) = raw_tail {
        out.push_str(line);
        out.push('\n');
    }
    fs::write(dir.join("events.jsonl"), out).unwrap();
}

fn write_hevents(dir: &Path, events: &[HollywoodEvent]) {
    fs::create_dir_all(dir).unwrap();
    let mut out = String::new();
    for e in events {
        out.push_str(&serde_json::to_string(e).unwrap());
        out.push('\n');
    }
    fs::write(dir.join("events.jsonl"), out).unwrap();
}

fn deliv(seq: u64, ts: u64, task: &str, delivery: u32, commits: &str) -> Event {
    let kind = EventKind::TaskDelivered {
        task: task.into(),
        delivery,
        actor: actor(task),
        commits: commits.into(),
        report: "done".into(),
        requirement_version: 1,
        checks: vec![],
        usage: None,
    };
    evt(seq, ts, &format!("actor-{task}-c1"), kind)
}

fn chk(seq: u64, ts: u64, task: &str, delivery: u32, exit: i32) -> Event {
    let kind = EventKind::TaskChecked {
        task: task.into(),
        delivery,
        name: "test".into(),
        exit_code: Some(exit),
        timed_out: false,
        output: format!("checks/{task}-d{delivery}.log"),
        duration_ms: 200,
    };
    evt(seq, ts, "stage", kind)
}

fn rev(seq: u64, ts: u64, task: &str, delivery: u32, verdict: Verdict, conclusion: &str) -> Event {
    let kind = EventKind::TaskReviewed {
        task: task.into(),
        delivery,
        reviewer: "director".into(),
        verdict,
        conclusion: conclusion.into(),
        findings: vec![],
        upstream_changes: vec![],
    };
    evt(seq, ts, "director", kind)
}

fn setup_two_studios(scratch: &Scratch, now: u64) {
    let acme_dir = crate::store::studio_dir(&scratch.dir, "acme-shop");
    let beta_dir = crate::store::studio_dir(&scratch.dir, "beta-api");

    let recent = now.saturating_sub(3_600_000);
    let old = now.saturating_sub(10 * 86_400_000);

    fs::create_dir_all(acme_dir.join("tasks/t1")).unwrap();
    fs::write(
        acme_dir.join("tasks/t1/acceptance.md"),
        "# t1\nImplements design v1 §R1 and design v1 §R2.\n",
    )
    .unwrap();

    let decls = vec![CheckDecl {
        name: "test".into(),
        run: "cargo test".into(),
        timeout_s: 60,
    }];
    let acme_events = vec![
        studio_ev(1, old, "acme-shop", "/home/user/acme-shop", decls),
        evt(
            2,
            recent,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "ask".into(),
            },
        ),
        evt(
            3,
            recent + 1_000,
            "director",
            EventKind::TaskCreated {
                task: "t1".into(),
                title: "Build engine".into(),
                task_kind: "code".into(),
                requirement_version: 1,
                acceptance: "tasks/t1/acceptance.md".into(),
                dependencies: vec![],
            },
        ),
        evt(
            4,
            recent + 2_000,
            "director",
            EventKind::MessageQueued {
                to: "writer".into(),
                file: Some("messages/4.md".into()),
                purpose: Some("dispatch".into()),
                source_seq: None,
            },
        ),
        evt(
            5,
            recent + 3_000,
            "director",
            EventKind::TaskCast {
                task: "t1".into(),
                candidate: "alice".into(),
                actor: actor("t1"),
                reason: String::new(),
                priority: None,
            },
        ),
        evt(
            6,
            recent + 4_000,
            "director",
            EventKind::MessageQueued {
                to: "actor-t1-c1".into(),
                file: Some("messages/6.md".into()),
                purpose: None,
                source_seq: None,
            },
        ),
        deliv(7, recent + 10_000, "t1", 1, "abc123..def456"),
        chk(8, recent + 11_000, "t1", 1, 1),
        rev(
            9,
            recent + 12_000,
            "t1",
            1,
            Verdict::ChangesRequested,
            "add edge test",
        ),
        deliv(10, recent + 20_000, "t1", 2, "def456..7890ab"),
        chk(11, recent + 21_000, "t1", 2, 0),
        rev(12, recent + 24_000, "t1", 2, Verdict::Accepted, "ok"),
        evt(
            13,
            recent + 25_000,
            "stage",
            EventKind::AttentionRaised {
                role: "actor-t1-c1".into(),
                reason: PERMISSION_PROMPT.into(),
                detail: "allow read".into(),
                needs_user: false,
                reset_at_ms: None,
                account: None,
            },
        ),
        evt(
            14,
            recent + 26_000,
            "stage",
            EventKind::AttentionRaised {
                role: "director".into(),
                reason: "rate limited".into(),
                detail: "waiting until 12:00".into(),
                needs_user: false,
                reset_at_ms: Some(recent + 900_000),
                account: None,
            },
        ),
        evt(
            15,
            recent + 27_000,
            "director",
            EventKind::RoleApproval {
                role: "actor-t1-c1".into(),
                decision: ApprovalDecision::Allow,
            },
        ),
        evt(
            16,
            recent + 28_000,
            "stage",
            EventKind::RoleRefused {
                role: "actor-t1-c1".into(),
                task: Some("t1".into()),
                command: "git --version".into(),
                cwd: "/home/user/acme-shop".into(),
                argv: vec!["git".into(), "--version".into()],
                rule: Some("probe".into()),
                disposition: RefusalDisposition::Refused,
            },
        ),
    ];
    let unknown_line = format!(
        r#"{{"v":1,"seq":17,"ts_ms":{},"by":{{"role":"stage"}},"kind":"future.event.kind","payload":{{"x":1}}}}"#,
        recent + 29_000
    );
    write_events(&acme_dir, &acme_events, Some(&unknown_line));

    let beta_events = vec![
        studio_ev(1, recent, "beta-api", "/home/user/beta-api", vec![]),
        evt(
            2,
            recent + 1_000,
            "director",
            EventKind::TaskCreated {
                task: "b1".into(),
                title: "Blocked task".into(),
                task_kind: "docs".into(),
                requirement_version: 1,
                acceptance: "tasks/b1/missing.md".into(),
                dependencies: vec![],
            },
        ),
        evt(
            3,
            recent + 2_000,
            "director",
            EventKind::TaskState {
                task: "b1".into(),
                state: TaskStateChange::Blocked,
                reason: "awaiting spec".into(),
            },
        ),
    ];
    write_events(&beta_dir, &beta_events, None);

    let hevs = vec![
        NewHollywoodEvent {
            by: EventBy::role("stage"),
            kind: HollywoodKind::MessageSent {
                to: "hollywood".into(),
                studio: Some("acme-shop".into()),
                text: None,
                file: Some("h1.md".into()),
                source_seq: Some(12),
            },
        }
        .stamp(1, recent + 30_000),
        NewHollywoodEvent {
            by: EventBy::role("stage"),
            kind: HollywoodKind::MessageDropped {
                target: 1,
                reason: "stale".into(),
            },
        }
        .stamp(2, recent + 31_000),
    ];
    write_hevents(&crate::store::hollywood_dir(&scratch.dir), &hevs);
}

#[test]
fn metrics_table_fits_100_cols_and_json_matches_ledger_read_only() {
    let (scratch, env) = scratch();
    let now = now_ms();
    setup_two_studios(&scratch, now);

    let acme_log =
        fs::read(crate::store::studio_dir(&scratch.dir, "acme-shop").join("events.jsonl")).unwrap();

    let table = ok(&env, "metrics --all");
    assert!(table.contains("acme-shop"), "{table}");
    assert!(table.contains("beta-api"), "{table}");
    assert!(table.contains("ALL"), "{table}");
    for line in table.lines() {
        assert!(
            line.chars().count() <= 100,
            "line > 100 chars ({}): {line}",
            line.chars().count()
        );
    }

    let json_raw = ok(&env, "metrics --studio acme-shop --since 48h --json");
    let v: serde_json::Value = serde_json::from_str(&json_raw).unwrap();
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["unknown_events_skipped"], 1);
    assert_eq!(v["studios"].as_array().unwrap().len(), 1);
    let s = &v["studios"][0];
    assert_eq!(s["studio"], "acme-shop");
    assert_eq!(s["quality"]["early_tasks"], 1);
    assert_eq!(s["quality"]["changes_requested_reviews"], 1);
    assert_eq!(s["refused"]["total"], 1);
    assert_eq!(s["refused"]["without_probes"], 0);
    assert_eq!(s["hollywood"]["notices_sent"], 1);
    assert_eq!(s["hollywood"]["notices_dropped"], 1);

    let acme_after =
        fs::read(crate::store::studio_dir(&scratch.dir, "acme-shop").join("events.jsonl")).unwrap();
    assert_eq!(acme_log, acme_after);
}

#[test]
fn digest_md_and_json_render_matrix_and_checks_none() {
    let (scratch, env) = scratch();
    let now = now_ms();
    setup_two_studios(&scratch, now);

    let md = ok(&env, "digest");
    assert!(md.contains("acme-shop"), "{md}");
    assert!(md.contains("design v1 §R1"), "{md}");
    assert!(md.contains("design v1 §R2"), "{md}");
    assert!(md.contains("def456..7890ab"), "{md}");
    assert!(md.contains("test:pass"), "{md}");
    assert!(md.contains("beta-api"), "{md}");
    assert!(md.contains("checks: none"), "{md}");
    assert!(md.contains("Blocked"), "{md}");

    let json_raw = ok(&env, "digest --studio acme-shop --since 24h --format json");
    let v: serde_json::Value = serde_json::from_str(&json_raw).unwrap();
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["studios"][0]["studio"], "acme-shop");
    assert_eq!(v["studios"][0]["sent_back"].as_array().unwrap().len(), 1);
    assert_eq!(v["studios"][0]["coverage"].as_array().unwrap().len(), 2);
}

#[test]
fn metrics_and_digest_validate_flags() {
    let (_scratch, env) = scratch();
    assert!(usage(&env, "metrics --all --studio acme-shop").contains("--all"));
    assert!(usage(&env, "metrics --since invalid").contains("duration"));
    assert!(usage(&env, "digest --format xml").contains("md|json"));
    assert!(usage(&env, "digest --all --studio acme-shop").contains("--all"));
}
