//! The outbox's pure pieces: per-role oldest-first order, the
//! `[backlot]` line, the screen-stability window, each send gate
//! on its own, and the notices a state owes — once per source
//! event, stable across a stage restart.

use std::path::Path;

use backlot_model::event::{ActorIdentity, Check, Severity, TaskStateChange, Verdict};
use backlot_model::state::{
    CastRecord, DeliveryRecord, Message, ReportRecord, ReqRecord, ReviewRecord, StateChangeRecord,
    StudioState, Task, TaskStatus,
};
use backlot_model::summary::Work;

use crate::deliver::{
    Gate, MIN_UP_MS, SCREEN_STABLE_MS, SEND_GAP_MS, Screen, gate_open, outbox, send_outcome,
};
use crate::notices;

const DIR: &str = "/state/studios/acme-shop";

fn dir() -> &'static Path {
    Path::new(DIR)
}

fn message(seq: u64, to: &str, delivered: bool) -> Message {
    Message {
        seq,
        from: "producer".into(),
        to: to.into(),
        file: Some(format!("messages/{seq}-note.md")),
        purpose: None,
        source_seq: None,
        delivered,
    }
}

/// Each role's oldest undelivered item wins, in seq order —
/// messages and reports share the order, a `to: user` report is
/// never sent, and what is already delivered is skipped.
#[test]
fn outbox_is_oldest_first_per_role() {
    let mut state = StudioState::default();
    state.messages.push(message(5, "writer", false));
    state.messages.push(message(3, "writer", false));
    state.messages.push(message(2, "writer", true));
    state.messages.push(message(4, "director", false));
    state.reports.push(ReportRecord {
        seq: 6,
        from: "director".into(),
        to: "producer".into(),
        node: "review".into(),
        file: "reports/6-review.md".into(),
        delivered: false,
    });
    state.reports.push(ReportRecord {
        seq: 7,
        from: "producer".into(),
        to: "user".into(),
        node: "done".into(),
        file: "reports/7-done.md".into(),
        delivered: false,
    });
    let out = outbox(&state, dir());
    assert_eq!(out.len(), 3, "{out:?}");
    let for_role = |r: &str| out.iter().find(|o| o.role == r).unwrap();
    assert_eq!(for_role("writer").seq, 3, "oldest undelivered wins");
    assert_eq!(for_role("director").seq, 4);
    assert_eq!(for_role("producer").seq, 6, "reports join the queue");
    assert!(
        for_role("writer")
            .text
            .contains(&format!("{DIR}/messages/3-note.md")),
        "{}",
        for_role("writer").text
    );
    assert!(
        for_role("writer")
            .text
            .starts_with("[backlot] producer → writer:")
    );
}

/// A file-less `plan` message says what is wanted instead of
/// naming a file.
#[test]
fn the_plan_request_says_what_is_wanted() {
    let mut state = StudioState::default();
    state.messages.push(Message {
        seq: 9,
        from: "producer".into(),
        to: "writer".into(),
        file: None,
        purpose: Some("plan".into()),
        source_seq: None,
        delivered: false,
    });
    let out = outbox(&state, dir());
    assert_eq!(out.len(), 1);
    assert!(out[0].text.contains("plan"), "{}", out[0].text);
    assert!(!out[0].text.contains("read /"), "{}", out[0].text);
}

/// A screen is stable once identical samples span two seconds;
/// a change resets the window and a clock going backwards never
/// opens it.
#[test]
fn screen_stability_needs_two_seconds_of_same() {
    let a = Screen::note(None, "idle".into(), 1_000);
    assert!(!a.stable());
    let b = Screen::note(Some(&a), "idle".into(), 1_000 + SCREEN_STABLE_MS - 1);
    assert!(!b.stable());
    let c = Screen::note(Some(&b), "idle".into(), 1_000 + SCREEN_STABLE_MS);
    assert!(c.stable(), "{c:?}");
    let d = Screen::note(Some(&c), "busy".into(), 5_000);
    assert!(!d.stable());
    let e = Screen::note(Some(&d), "busy".into(), 4_000);
    assert!(!e.stable(), "a backwards clock is never stable");
}

fn open_gate() -> Gate {
    Gate {
        live: true,
        work: Some(Work::Waiting),
        screen: Some(Screen {
            text: "idle".into(),
            same_since_ms: 0,
            at_ms: 10_000,
        }),
        role_since_ms: 0,
        in_flight: false,
        sent_ms: None,
    }
}

/// Each send gate on its own: not live, not waiting, screen still
/// changing, role too young, a send in flight, a send just done.
#[test]
fn every_gate_on_its_own() {
    let now = 60_000;
    assert!(gate_open(&open_gate(), now));

    let mut g = open_gate();
    g.live = false;
    assert!(!gate_open(&g, now), "a dead target gets nothing");

    for w in [Work::Working, Work::Blocked] {
        let mut g = open_gate();
        g.work = Some(w);
        assert!(!gate_open(&g, now), "{w:?} is not waiting");
    }
    let mut g = open_gate();
    g.work = None;
    assert!(!gate_open(&g, now), "no sample yet is not waiting");

    let mut g = open_gate();
    g.screen = Some(Screen {
        text: "new".into(),
        same_since_ms: 9_500,
        at_ms: 10_000,
    });
    assert!(!gate_open(&g, now), "a screen still changing waits");
    let mut g = open_gate();
    g.screen = None;
    assert!(!gate_open(&g, now), "no sample at all waits");

    let mut g = open_gate();
    g.role_since_ms = now - MIN_UP_MS + 1;
    assert!(!gate_open(&g, now), "a role too young waits");
    g.role_since_ms = now - MIN_UP_MS;
    assert!(gate_open(&g, now));

    let mut g = open_gate();
    g.in_flight = true;
    assert!(!gate_open(&g, now), "one at a time");

    let mut g = open_gate();
    g.sent_ms = Some(now - SEND_GAP_MS + 1);
    assert!(!gate_open(&g, now), "the send gap holds");
    g.sent_ms = Some(now - SEND_GAP_MS);
    assert!(gate_open(&g, now));
}

/// Only `sent: true` records the delivery: `sent: false` and an
/// answer with no `sent` field stay queued, the reason (when the
/// core gave one) surfacing for the stage's log.
#[test]
fn only_sent_true_delivers() {
    assert!(send_outcome(&serde_json::json!({"sent": true})).is_ok());
    assert_eq!(
        send_outcome(&serde_json::json!({"sent": false, "reason": "no person"})).unwrap_err(),
        Some("no person".to_string())
    );
    assert_eq!(send_outcome(&serde_json::json!({})).unwrap_err(), None);
    assert_eq!(send_outcome(&serde_json::json!(null)).unwrap_err(), None);
}

fn task(id: &str) -> Task {
    Task {
        id: id.into(),
        title: id.into(),
        task_kind: "feature".into(),
        requirement_version: 1,
        acceptance: "plan/1-acceptance.md".into(),
        dependencies: Vec::new(),
        status: TaskStatus::Running,
        paused_from: None,
        rework_rounds: 0,
        priority: backlot_model::event::Priority::Normal,
        cast: Some(CastRecord {
            candidate: "alice".into(),
            actor: actor(),
            reason: "best".into(),
            priority: None,
            by: "director".into(),
            seq: 2,
            ts_ms: 0,
        }),
        casts: vec![CastRecord {
            candidate: "alice".into(),
            actor: actor(),
            reason: "best".into(),
            priority: None,
            by: "director".into(),
            seq: 2,
            ts_ms: 0,
        }],
        deliveries: Vec::new(),
        reviews: Vec::new(),
        finding_reports: Vec::new(),
        state_changes: Vec::new(),
        created_seq: 1,
        created_ts_ms: 0,
        last_ts_ms: 0,
        closed_ts_ms: None,
    }
}

fn actor() -> ActorIdentity {
    ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: "plain".into(),
        effort: "high".into(),
        permission_profile: "standard".into(),
    }
}

/// Every notice kind is owed exactly once per source event, the
/// delivery notice follows the caster, and a recorded message
/// clears the debt — across a restart, replaying the same state
/// owes nothing again.
#[test]
fn notices_are_owed_once_per_source_event() {
    let mut state = StudioState::default();
    let mut t = task("t1");
    t.deliveries.push(DeliveryRecord {
        n: 1,
        actor: actor(),
        commits: "a..b".into(),
        report: "reports/9-t1-d1-report.md".into(),
        requirement_version: 1,
        checks: vec![Check {
            name: "build".into(),
            passed: true,
        }],
        seq: 9,
        ts_ms: 0,
    });
    t.reviews.push(ReviewRecord {
        delivery: 1,
        reviewer: "director".into(),
        verdict: Verdict::ChangesRequested,
        conclusion: "fix the nav".into(),
        findings: vec![backlot_model::event::Finding {
            id: "f1".into(),
            summary: "nav overlaps".into(),
            severity: Severity::High,
            cause: backlot_model::event::Cause::DeliveryDefect,
        }],
        upstream_changes: Vec::new(),
        seq: 11,
        ts_ms: 0,
    });
    t.state_changes.push(StateChangeRecord {
        state: TaskStateChange::Blocked,
        reason: "needs an answer".into(),
        seq: 13,
        ts_ms: 0,
    });
    state.tasks.insert("t1".into(), t);
    state.requirements.push(ReqRecord {
        version: 2,
        file: "req/v2.md".into(),
        summary: "dark mode".into(),
        seq: 15,
    });
    state.requirement_version = 2;

    let owed = notices::owed(&state, dir());
    assert_eq!(owed.len(), 4, "{owed:?}");
    let by_label = |l: &str| owed.iter().find(|n| n.label == l).unwrap();
    assert_eq!(by_label("delivered").to, "director");
    assert_eq!(by_label("rework").to, "actor-t1-c1");
    assert_eq!(by_label("blocked").to, "director");
    assert_eq!(by_label("req").to, "director");
    assert!(
        by_label("req").body.contains("t1"),
        "{}",
        by_label("req").body
    );
    assert!(
        by_label("rework").body.contains("nav overlaps"),
        "{}",
        by_label("rework").body
    );

    // Every source event now has a message carrying its seq — the
    // state a restart replays owes nothing again.
    for n in &owed {
        state.messages.push(Message {
            seq: 100 + n.source_seq,
            from: "stage".into(),
            to: n.to.clone(),
            file: Some(format!("messages/{}-{}.md", n.source_seq, n.label)),
            purpose: None,
            source_seq: Some(n.source_seq),
            delivered: false,
        });
    }
    assert!(notices::owed(&state, dir()).is_empty());
}

/// A light-path task — cast by the producer — routes its delivery
/// notice to the producer.
#[test]
fn a_producer_cast_routes_the_delivery_notice() {
    let mut state = StudioState::default();
    let mut t = task("t1");
    t.cast.as_mut().unwrap().by = "producer".into();
    t.casts[0].by = "producer".into();
    t.deliveries.push(DeliveryRecord {
        n: 1,
        actor: actor(),
        commits: "a..b".into(),
        report: "reports/9-t1-d1-report.md".into(),
        requirement_version: 1,
        checks: Vec::new(),
        seq: 9,
        ts_ms: 0,
    });
    state.tasks.insert("t1".into(), t);
    let owed = notices::owed(&state, dir());
    assert_eq!(owed.len(), 1);
    assert_eq!(owed[0].to, "producer");
}
