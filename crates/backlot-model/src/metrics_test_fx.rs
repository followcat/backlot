//! Synthetic neutral fixtures for `metrics_tests`.

use crate::event::{
    ApprovalDecision, Cause, CheckDecl, Event, EventBy, EventKind, Finding, LEDGER_VERSION,
    PERMISSION_PROMPT, RefusalDisposition, Severity, TaskStateChange, Verdict,
};
use crate::testkit::{actor, studio_kind};

pub(crate) fn evt(seq: u64, ts_ms: u64, role: &str, kind: EventKind) -> Event {
    Event {
        v: LEDGER_VERSION,
        seq,
        ts_ms,
        by: EventBy::role(role),
        kind,
    }
}

fn msg(
    seq: u64,
    ts_ms: u64,
    from: &str,
    to: &str,
    purpose: Option<&str>,
    src: Option<u64>,
) -> Event {
    let kind = EventKind::MessageQueued {
        to: to.into(),
        file: Some(format!("messages/{seq}.md")),
        purpose: purpose.map(str::to_string),
        source_seq: src,
    };
    evt(seq, ts_ms, from, kind)
}

fn task_ev(seq: u64, ts_ms: u64, id: &str, title: &str) -> Event {
    let kind = EventKind::TaskCreated {
        task: id.into(),
        title: title.into(),
        task_kind: "code".into(),
        requirement_version: 1,
        acceptance: format!("tasks/{id}/acceptance.md"),
        dependencies: vec![],
    };
    evt(seq, ts_ms, "director", kind)
}

fn cast_ev(seq: u64, ts_ms: u64, id: &str, cand: &str) -> Event {
    let kind = EventKind::TaskCast {
        task: id.into(),
        candidate: cand.into(),
        actor: actor(id),
        reason: String::new(),
        priority: None,
    };
    evt(seq, ts_ms, "director", kind)
}

fn deliv_ev(seq: u64, ts_ms: u64, id: &str, n: u32, commits: &str, rep: &str) -> Event {
    let kind = EventKind::TaskDelivered {
        task: id.into(),
        delivery: n,
        actor: actor(id),
        commits: commits.into(),
        report: rep.into(),
        requirement_version: 1,
        checks: vec![],
        usage: None,
    };
    evt(seq, ts_ms, &format!("actor-{id}-c1"), kind)
}

fn rev_ev(seq: u64, ts_ms: u64, id: &str, n: u32, verdict: Verdict, note: &str) -> Event {
    let kind = EventKind::TaskReviewed {
        task: id.into(),
        delivery: n,
        reviewer: "director".into(),
        verdict,
        conclusion: note.into(),
        findings: vec![],
        upstream_changes: vec![],
    };
    evt(seq, ts_ms, "director", kind)
}

fn chk_ev(seq: u64, ts_ms: u64, id: &str, n: u32, name: &str, exit: i32) -> Event {
    let kind = EventKind::TaskChecked {
        task: id.into(),
        delivery: n,
        name: name.into(),
        exit_code: Some(exit),
        timed_out: false,
        expect_failed: None,
        output: format!("checks/{id}-d{n}.log"),
        duration_ms: 350,
    };
    evt(seq, ts_ms, "stage", kind)
}

fn state_ev(seq: u64, ts_ms: u64, id: &str, state: TaskStateChange, reason: &str) -> Event {
    let kind = EventKind::TaskState {
        task: id.into(),
        state,
        reason: reason.into(),
    };
    evt(seq, ts_ms, "director", kind)
}

fn attn(seq: u64, ts_ms: u64, role: &str, reason: &str, detail: &str, reset: Option<u64>) -> Event {
    let kind = EventKind::AttentionRaised {
        role: role.into(),
        reason: reason.into(),
        detail: detail.into(),
        needs_user: false,
        reset_at_ms: reset,
        account: None,
    };
    evt(seq, ts_ms, "stage", kind)
}

fn clr(seq: u64, ts_ms: u64, target: u64) -> Event {
    let kind = EventKind::AttentionCleared {
        target,
        reason: None,
    };
    evt(seq, ts_ms, "stage", kind)
}

fn refused(seq: u64, ts_ms: u64, role: &str, cmd: &str, argv: &[&str], rule: &str) -> Event {
    let kind = EventKind::RoleRefused {
        role: role.into(),
        task: Some("t1".into()),
        command: cmd.into(),
        cwd: "/home/user/acme-shop".into(),
        argv: argv.iter().map(|s| (*s).to_string()).collect(),
        rule: Some(rule.into()),
        disposition: RefusalDisposition::Refused,
    };
    evt(seq, ts_ms, "stage", kind)
}

fn studio_event(title: &str, repo_path: &str, decls: Vec<CheckDecl>) -> Event {
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
    evt(1, 1_000, "user", kind)
}

pub(crate) fn acme_events() -> Vec<Event> {
    let decls = vec![
        CheckDecl::new("test", "cargo test", 60),
        CheckDecl::new("lint", "cargo clippy", 60),
    ];
    let findings = vec![
        Finding {
            id: "F1".into(),
            summary: "null check".into(),
            severity: Severity::High,
            cause: Cause::DeliveryDefect,
        },
        Finding {
            id: "F2".into(),
            summary: "rename".into(),
            severity: Severity::Low,
            cause: Cause::DeliveryDefect,
        },
    ];
    vec![
        studio_event("acme-shop", "/home/user/acme-shop", decls),
        evt(
            2,
            2_000,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req-v1.md".into(),
                summary: "ask".into(),
            },
        ),
        task_ev(3, 3_000, "t1", "Build core"),
        msg(4, 5_000, "director", "writer", Some("dispatch"), None),
        task_ev(5, 6_000, "t2", "Build UI"),
        cast_ev(6, 10_000, "t1", "alice"),
        cast_ev(7, 11_000, "t2", "bob"),
        state_ev(8, 11_100, "t1", TaskStateChange::Paused, "wait"),
        state_ev(9, 11_200, "t2", TaskStateChange::Paused, "wait"),
        state_ev(10, 11_300, "t1", TaskStateChange::Paused, "again"),
        state_ev(11, 11_400, "t1", TaskStateChange::Resumed, "go"),
        state_ev(12, 11_500, "t2", TaskStateChange::Resumed, "go"),
        state_ev(13, 11_600, "t2", TaskStateChange::Paused, "single"),
        state_ev(14, 11_700, "t2", TaskStateChange::Resumed, "go"),
        msg(15, 12_000, "director", "actor-t1-c1", None, None),
        msg(16, 13_000, "director", "actor-t2-c1", None, None),
        msg(
            17,
            14_000,
            "director",
            "actor-t1-c1",
            Some("dispatch"),
            None,
        ),
        msg(18, 15_000, "director", "actor-t1-c1", None, Some(4)),
        msg(19, 16_000, "director", "writer", None, None),
        deliv_ev(20, 20_000, "t1", 1, "a1..b2", "first try"),
        chk_ev(21, 20_500, "t1", 1, "test", 1),
        rev_ev(
            22,
            21_000,
            "t1",
            1,
            Verdict::ChangesRequested,
            "fix edge case",
        ),
        evt(
            23,
            21_200,
            "director",
            EventKind::TaskFinding {
                task: "t1".into(),
                delivery: 1,
                findings,
            },
        ),
        deliv_ev(24, 30_000, "t1", 2, "b2..c3", "second try"),
        chk_ev(25, 30_500, "t1", 2, "test", 0),
        rev_ev(26, 33_000, "t1", 2, Verdict::Accepted, "looks good"),
        deliv_ev(27, 40_000, "t2", 1, "d4..e5", "ui done"),
        rev_ev(28, 50_000, "t2", 1, Verdict::Accepted, "clean"),
        attn(
            29,
            60_000,
            "director",
            "cannot resume",
            "session missing",
            None,
        ),
        clr(30, 62_000, 29),
        attn(31, 65_000, "actor-t1-c1", "stalled", "no output", None),
        clr(32, 69_000, 31),
        attn(
            33,
            70_000,
            "actor-t2-c1",
            "outward action",
            "curl posted",
            None,
        ),
        attn(
            34,
            71_000,
            "actor-t1-c1",
            PERMISSION_PROMPT,
            "allow read",
            None,
        ),
        clr(35, 72_500, 34),
        attn(
            36,
            80_000,
            "actor-t1-c1",
            "rate limited",
            "waiting until 10:00",
            Some(1_000_000),
        ),
        clr(37, 81_000, 36),
        attn(
            38,
            82_000,
            "actor-t1-c1",
            "rate limited",
            "waiting until 10:00",
            Some(1_010_000),
        ),
        clr(39, 83_000, 38),
        attn(40, 84_000, "actor-t1-c1", "rate limited", "retry", None),
        clr(41, 85_000, 40),
        attn(
            42,
            90_000,
            "actor-t1-c1",
            "rate limited",
            "waiting until 12:00",
            Some(5_000_000),
        ),
        clr(43, 91_000, 42),
        evt(
            44,
            92_000,
            "director",
            EventKind::RoleApproval {
                role: "actor-t1-c1".into(),
                decision: ApprovalDecision::Allow,
            },
        ),
        evt(
            45,
            93_000,
            "director",
            EventKind::RoleApproval {
                role: "actor-t1-c1".into(),
                decision: ApprovalDecision::Allow,
            },
        ),
        evt(
            46,
            94_000,
            "director",
            EventKind::RoleApproval {
                role: "actor-t2-c1".into(),
                decision: ApprovalDecision::Deny,
            },
        ),
        refused(
            47,
            95_000,
            "actor-t1-c1",
            "git rev-parse --show-toplevel",
            &["git", "rev-parse", "--show-toplevel"],
            "probe-rule",
        ),
        refused(
            48,
            96_000,
            "actor-t1-c1",
            "git --version",
            &["git", "--version"],
            "probe-rule",
        ),
        refused(
            49,
            97_000,
            "actor-t1-c1",
            "rm -rf /home/user/acme-shop",
            &["rm", "-rf", "/home/user/acme-shop"],
            "danger-rm",
        ),
        refused(
            50,
            98_000,
            "actor-t2-c1",
            "git push origin main",
            &["git", "push", "origin", "main"],
            "no-push",
        ),
        evt(
            51,
            105_000,
            "stage",
            EventKind::Unknown {
                raw_kind: "future.metric.test".into(),
            },
        ),
    ]
}
