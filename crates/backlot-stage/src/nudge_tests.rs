//! The nudge pass: once-ever queueing against the idle clocks and
//! the escalation path — driven against a real ledger so a stage
//! restart's replay is covered too. Neutral data, never a real
//! agent: `Stage::nudge` only reads the maps and appends events.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use backlot_model::event::*;
use backlot_model::state::{apply, replay};
use backlot_model::summary::Work;
use backlot_model::testkit::*;
use backlot_model::{EventBy, NewEvent};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, DuplexStream};

use crate::daemon::Stage;
use crate::nudge::{self, Move};
use crate::reasons;
use crate::reconcile::LiveTerminal;
use crate::store::{self, Pending};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn tmp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "backlot-nudge-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// `hello`, then silence: the nudge pass makes no terminal calls.
async fn serve(stream: DuplexStream) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = Reader::new(read);
    let hello = reader.read_line().await.unwrap().unwrap();
    let request: Value = serde_json::from_slice(&hello).unwrap();
    assert_eq!(request["method"], "hello");
    let reply = serde_json::to_string(&json!({
        "jsonrpc": "2.0", "id": 0,
        "result": {
            "generation": 1, "version": "fake-core 0.0",
            "framed": true,
            "core": {"id": "core-fake-1", "name": "fake-core"},
            "methods": [], "features": [],
        }
    }))
    .unwrap();
    write.write_all(reply.as_bytes()).await.unwrap();
    write.write_all(b"\n").await.unwrap();
    while let Ok(Some(_)) = reader.read_frame().await {}
}

async fn new_stage() -> Stage {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move { serve(core_side).await });
    let (read, write) = tokio::io::split(client_side);
    Stage::for_test(
        Client::open(Box::pin(read), Box::pin(write), None)
            .await
            .unwrap(),
    )
}

/// Append one event to `dir`'s ledger and return it stamped.
fn record(dir: &Path, role: &str, kind: EventKind) -> backlot_model::Event {
    store::append(dir, move |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(EventBy::role(role), kind),
            files: Vec::new(),
        })
    })
    .unwrap()
}

fn role_started(role: &str, terminal: &str) -> EventKind {
    EventKind::RoleStarted {
        role: role.into(),
        terminal: Some(terminal.into()),
        actor: Some(actor("x")),
        session_id: None,
        reason: None,
    }
}

fn plan_request() -> EventKind {
    EventKind::MessageQueued {
        to: "writer".into(),
        file: None,
        purpose: Some("plan".into()),
        source_seq: None,
    }
}

fn publish(kind: PlanKind, seq: u64) -> EventKind {
    EventKind::PlanPublished {
        plan_kind: kind,
        file: format!("plan/{seq}-doc.md"),
        requirement_version: 1,
    }
}

fn req_v1() -> EventKind {
    EventKind::ReqVersion {
        version: 1,
        file: "req/v1.md".into(),
        summary: "first".into(),
    }
}

/// A `LiveTerminal` whose name is the studio's own for `role` — the
/// fixed-role name shape `terminal_name` builds.
fn term(studio: &str, role: &str) -> LiveTerminal {
    LiveTerminal {
        name: format!("bl-{studio}-{role}"),
        address: "t1".into(),
        argv: Vec::new(),
        cwd: String::new(),
        labels: BTreeMap::new(),
    }
}

/// Mark `(studio, role)`'s latest work sample.
fn work(stage: &Stage, studio: &str, role: &str, w: Work) {
    stage
        .work
        .borrow_mut()
        .insert((studio.into(), role.into()), w);
}

/// The `message.queued` events the ledger holds for `purpose`.
fn queued(dir: &Path, purpose: &str) -> Vec<backlot_model::Event> {
    store::load_events(dir)
        .unwrap()
        .0
        .into_iter()
        .filter(|e| {
            matches!(&e.kind, EventKind::MessageQueued { purpose: p, .. } if p.as_deref() == Some(purpose))
        })
        .collect()
}

fn attentions(dir: &Path) -> Vec<backlot_model::state::Attention> {
    store::load_state(dir).unwrap().attention
}

/// The ts `plan` sees for "now": the ledger's latest event time —
/// `store::append` stamps real time, so the tests count from it.
fn now(dir: &Path) -> u64 {
    store::load_events(dir)
        .unwrap()
        .0
        .iter()
        .map(|e| e.ts_ms)
        .max()
        .unwrap()
}

/// Plan published and answered — `dispatch` is the producer's step.
#[tokio::test]
async fn the_idle_producer_is_nudged_once() {
    let dir = tmp();
    record(&dir, "user", studio_kind());
    record(&dir, "producer", req_v1());
    record(&dir, "producer", plan_request());
    record(&dir, "writer", publish(PlanKind::Plan, 4));
    let last = record(&dir, "writer", publish(PlanKind::Acceptance, 5));
    record(
        &dir,
        "stage",
        role_started("producer", "bl-acme-shop-producer"),
    );
    let t0 = last.ts_ms;

    let stage = new_stage().await;
    let live = vec![term("acme-shop", "producer")];
    let state = store::load_state(&dir).unwrap();
    work(&stage, "acme-shop", "producer", Work::Waiting);

    // The stretch starts on the first idle sighting — nothing yet.
    stage.nudge("acme-shop", &dir, &state, &live, t0).unwrap();
    assert!(queued(&dir, "nudge").is_empty());
    // Inside the limit: still nothing.
    stage
        .nudge("acme-shop", &dir, &state, &live, t0 + 179_000)
        .unwrap();
    assert!(queued(&dir, "nudge").is_empty());
    // Past `nudge_after_s`: one nudge to the producer, keyed on the
    // step's seq — the acceptance publish that completed the pair.
    stage
        .nudge("acme-shop", &dir, &state, &live, t0 + 180_000)
        .unwrap();
    let nudges = queued(&dir, "nudge");
    assert_eq!(nudges.len(), 1);
    match &nudges[0].kind {
        EventKind::MessageQueued {
            to,
            file,
            source_seq,
            ..
        } => {
            assert_eq!(to, "producer");
            assert_eq!(*source_seq, Some(last.seq));
            assert!(dir.join(file.as_ref().unwrap()).is_file());
        }
        other => panic!("wrong event: {other:?}"),
    }
    let body =
        fs::read_to_string(dir.join("messages").join(format!("{}-nudge.md", last.seq))).unwrap();
    assert!(body.contains("dispatch"), "{body}");
    assert!(body.contains("backlot dispatch"), "{body}");
    assert!(body.contains("backlot tell"), "{body}");

    // The queued nudge keeps it off the idle list — and the dedup
    // says never again — so a later pass adds nothing.
    let state = store::load_state(&dir).unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, t0 + 900_000)
        .unwrap();
    assert_eq!(queued(&dir, "nudge").len(), 1);
}

/// Working is not idle; queued mail is not idle. Each blocks the
/// nudge — and resets the stretch — until the owner is plainly
/// waiting again for the full limit.
#[tokio::test]
async fn working_or_busy_owners_are_never_nudged() {
    let dir = tmp();
    record(&dir, "user", studio_kind());
    record(&dir, "producer", req_v1());
    record(&dir, "producer", plan_request());
    record(&dir, "writer", publish(PlanKind::Plan, 4));
    let last = record(&dir, "writer", publish(PlanKind::Acceptance, 5));
    record(
        &dir,
        "stage",
        role_started("producer", "bl-acme-shop-producer"),
    );
    let t0 = last.ts_ms;

    let stage = new_stage().await;
    let live = vec![term("acme-shop", "producer")];
    let state = store::load_state(&dir).unwrap();

    // Working: past the limit and still no nudge, and the stretch
    // does not start — the clock needs a fresh full run after.
    work(&stage, "acme-shop", "producer", Work::Working);
    stage
        .nudge("acme-shop", &dir, &state, &live, t0 + 500_000)
        .unwrap();
    assert!(queued(&dir, "nudge").is_empty());
    work(&stage, "acme-shop", "producer", Work::Waiting);
    stage
        .nudge("acme-shop", &dir, &state, &live, t0 + 600_000)
        .unwrap();
    assert!(queued(&dir, "nudge").is_empty(), "the stretch restarted");

    // An undelivered message queued to it: not idle — even with the
    // stretch long past the limit, no nudge.
    record(
        &dir,
        "stage",
        EventKind::MessageQueued {
            to: "producer".into(),
            file: Some("messages/9-note.md".into()),
            purpose: None,
            source_seq: Some(9),
        },
    );
    let state = store::load_state(&dir).unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, t0 + 990_000)
        .unwrap();
    assert!(queued(&dir, "nudge").is_empty());
}

/// A restart loses the in-memory stretches, so the idle clock starts
/// over — but a nudge already in the ledger can never repeat, and
/// its delivered clock keeps escalation honest.
#[tokio::test]
async fn a_restart_restarts_the_stretch_but_never_the_nudge() {
    let dir = tmp();
    record(&dir, "user", studio_kind());
    record(&dir, "producer", req_v1());
    record(&dir, "producer", plan_request());
    record(&dir, "writer", publish(PlanKind::Plan, 4));
    let last = record(&dir, "writer", publish(PlanKind::Acceptance, 5));
    record(
        &dir,
        "stage",
        role_started("producer", "bl-acme-shop-producer"),
    );
    let t0 = last.ts_ms;

    let stage = new_stage().await;
    let live = vec![term("acme-shop", "producer")];
    let state = store::load_state(&dir).unwrap();
    work(&stage, "acme-shop", "producer", Work::Waiting);
    // First sighting starts the stretch; the limit later, the nudge.
    stage.nudge("acme-shop", &dir, &state, &live, t0).unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, t0 + 190_000)
        .unwrap();
    assert_eq!(queued(&dir, "nudge").len(), 1);
    // Deliver it so the ledger's nudge is past.
    let nudge_seq = queued(&dir, "nudge")[0].seq;
    record(
        &dir,
        "stage",
        EventKind::MessageDelivered { target: nudge_seq },
    );

    // A new stage on the same ledger: the delivered nudge is
    // replayed, so even a long-idle stretch queues nothing — and a
    // fresh stage's stretch starts at its first sighting.
    let stage = new_stage().await;
    work(&stage, "acme-shop", "producer", Work::Waiting);
    let state = store::load_state(&dir).unwrap();
    let delivered_at = state
        .messages
        .iter()
        .find(|m| m.seq == nudge_seq)
        .unwrap()
        .delivered_ms
        .unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, delivered_at + 500_000)
        .unwrap();
    assert_eq!(queued(&dir, "nudge").len(), 1);
    // It escalates instead: still owed, still idle, past the limit.
    let state = store::load_state(&dir).unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, delivered_at + 600_000)
        .unwrap();
    let open: Vec<_> = attentions(&dir)
        .into_iter()
        .filter(|a| a.open && a.reason == reasons::OWED)
        .collect();
    assert_eq!(open.len(), 1, "{open:?}");
    assert_eq!(open[0].role, "producer");
    assert!(open[0].detail.contains("dispatch"), "{}", open[0].detail);
}

/// The director case (S9a): every task accepted, no node report —
/// the director is nudged, then the `owed` attention raises after
/// `escalate_after_s`, the producer gets its `owed` notice, and the
/// attention clears once the report lands.
#[tokio::test]
async fn the_still_idle_director_escalates_then_clears() {
    let dir = tmp();
    record(&dir, "user", studio_kind());
    record(&dir, "producer", req_v1());
    record(&dir, "director", task_created("t1", "feature", 1));
    record(&dir, "director", cast("t1", "actor-a", "a"));
    record(&dir, "actor-t1-c1", delivered("t1", 1, "a", 1));
    let closed = record(
        &dir,
        "director",
        reviewed("t1", 1, Verdict::Accepted, vec![]),
    );
    record(
        &dir,
        "stage",
        role_started("director", "bl-acme-shop-director"),
    );
    let t0 = closed.ts_ms;

    let stage = new_stage().await;
    let live = vec![term("acme-shop", "director")];
    let state = store::load_state(&dir).unwrap();
    work(&stage, "acme-shop", "director", Work::Waiting);

    // First sighting starts the stretch; the limit later, the nudge.
    stage.nudge("acme-shop", &dir, &state, &live, t0).unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, t0 + 190_000)
        .unwrap();
    let nudges = queued(&dir, "nudge");
    assert_eq!(nudges.len(), 1);
    match &nudges[0].kind {
        EventKind::MessageQueued { to, source_seq, .. } => {
            assert_eq!(to, "director");
            assert_eq!(*source_seq, Some(closed.seq), "keyed on the closing event");
        }
        other => panic!("wrong event: {other:?}"),
    }
    record(
        &dir,
        "stage",
        EventKind::MessageDelivered {
            target: nudges[0].seq,
        },
    );
    let delivered_at = now(&dir);

    // Still inside the escalate window: no attention.
    let state = store::load_state(&dir).unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, delivered_at + 599_000)
        .unwrap();
    assert!(attentions(&dir).iter().all(|a| a.reason != reasons::OWED));

    // Past it, still owed and still idle: the `owed` attention
    // raises on the director — and the producer's `owed` notice is
    // owed by that raise.
    let state = store::load_state(&dir).unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, delivered_at + 600_000)
        .unwrap();
    let open: Vec<_> = attentions(&dir)
        .into_iter()
        .filter(|a| a.open && a.reason == reasons::OWED)
        .collect();
    assert_eq!(open.len(), 1, "{open:?}");
    assert_eq!(open[0].role, "director");
    let state = store::load_state(&dir).unwrap();
    let owed = crate::notices::owed(&state, &dir);
    assert!(
        owed.iter().any(|n| n.label == "owed" && n.to == "producer"),
        "{owed:?}"
    );
    // A second pass while the attention stands raises nothing new.
    stage
        .nudge("acme-shop", &dir, &state, &live, delivered_at + 900_000)
        .unwrap();
    assert_eq!(
        attentions(&dir)
            .iter()
            .filter(|a| a.reason == reasons::OWED)
            .count(),
        1
    );

    // The director's report lands: the step resolves, the attention
    // clears with `done`.
    record(
        &dir,
        "director",
        EventKind::ReportSent {
            to: "producer".into(),
            node: "integration".into(),
            file: "reports/d.md".into(),
        },
    );
    let state = store::load_state(&dir).unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, delivered_at + 901_000)
        .unwrap();
    assert!(attentions(&dir).iter().all(|a| !a.open));
    let events = store::load_events(&dir).unwrap();
    assert!(events.0.iter().any(|e| matches!(
        &e.kind,
        EventKind::AttentionCleared { reason, .. } if reason.as_deref() == Some("done")
    )));
}

/// The `owed` attention ties to the step it escalated, not the
/// role: once `tasks` is answered the attention clears even though
/// the director already owes `cast t1` — which then earns its own
/// nudge, its own escalation, a detail that names it.
#[tokio::test]
async fn the_owed_attention_follows_its_step_not_the_role() {
    let dir = tmp();
    record(&dir, "user", studio_kind());
    record(&dir, "producer", req_v1());
    record(&dir, "producer", plan_request());
    record(&dir, "writer", publish(PlanKind::Plan, 4));
    record(&dir, "writer", publish(PlanKind::Acceptance, 5));
    record(
        &dir,
        "producer",
        EventKind::MessageQueued {
            to: "director".into(),
            file: Some("messages/6-dispatch.md".into()),
            purpose: Some("dispatch".into()),
            source_seq: None,
        },
    );
    record(&dir, "stage", EventKind::MessageDelivered { target: 6 });
    record(
        &dir,
        "stage",
        role_started("director", "bl-acme-shop-director"),
    );

    let stage = new_stage().await;
    let live = vec![term("acme-shop", "director")];
    let state = store::load_state(&dir).unwrap();
    work(&stage, "acme-shop", "director", Work::Waiting);
    let t0 = now(&dir);

    // `tasks` owed: past the idle limit the nudge queues, and the
    // escalate window runs from its delivery.
    stage.nudge("acme-shop", &dir, &state, &live, t0).unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, t0 + 190_000)
        .unwrap();
    let nudge_seq = queued(&dir, "nudge")[0].seq;
    record(
        &dir,
        "stage",
        EventKind::MessageDelivered { target: nudge_seq },
    );
    let delivered_at = now(&dir);
    let state = store::load_state(&dir).unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, delivered_at + 600_000)
        .unwrap();
    let open: Vec<_> = attentions(&dir)
        .into_iter()
        .filter(|a| a.open && a.reason == reasons::OWED)
        .collect();
    assert_eq!(open.len(), 1, "{open:?}");
    assert!(open[0].detail.contains("tasks"), "{}", open[0].detail);

    // `task new` answers `tasks` — but leaves `cast t1` owed and
    // unnudged: the answered step's attention clears anyway.
    let created = record(&dir, "director", task_created("t1", "feature", 1));
    let state = store::load_state(&dir).unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, created.ts_ms)
        .unwrap();
    let owed: Vec<_> = attentions(&dir)
        .into_iter()
        .filter(|a| a.reason == reasons::OWED)
        .collect();
    assert_eq!(owed.len(), 1, "{owed:?}");
    assert!(!owed[0].open, "the answered step's attention clears");

    // `cast t1` runs its own course: its own nudge after the limit,
    // keyed on the task creation, then its own escalation.
    let state = store::load_state(&dir).unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, created.ts_ms + 190_000)
        .unwrap();
    let nudges = queued(&dir, "nudge");
    assert_eq!(nudges.len(), 2, "{nudges:?}");
    match &nudges[1].kind {
        EventKind::MessageQueued { to, source_seq, .. } => {
            assert_eq!(to, "director");
            assert_eq!(*source_seq, Some(created.seq));
        }
        other => panic!("wrong event: {other:?}"),
    }
    record(
        &dir,
        "stage",
        EventKind::MessageDelivered {
            target: nudges[1].seq,
        },
    );
    let delivered_at = now(&dir);
    let state = store::load_state(&dir).unwrap();
    stage
        .nudge("acme-shop", &dir, &state, &live, delivered_at + 600_000)
        .unwrap();
    let open: Vec<_> = attentions(&dir)
        .into_iter()
        .filter(|a| a.open && a.reason == reasons::OWED)
        .collect();
    assert_eq!(open.len(), 1, "{open:?}");
    assert_eq!(open[0].role, "director");
    assert!(open[0].detail.contains("cast t1"), "{}", open[0].detail);
}

/// A dropped message is not pending for the idle rule: `queued_for`
/// reads only what the outbox would send, so a role whose queued
/// notice was dropped reads idle again.
#[test]
fn a_dropped_message_does_not_count_as_queued() {
    let mut state = base_state();
    apply(
        &mut state,
        &ev(
            4,
            "stage",
            EventKind::MessageQueued {
                to: "director".into(),
                file: Some("messages/3-attention.md".into()),
                purpose: None,
                source_seq: Some(3),
            },
        ),
    );
    assert!(
        nudge::queued_for(&state, "director"),
        "a queued item counts"
    );
    apply(
        &mut state,
        &ev(
            5,
            "stage",
            EventKind::MessageDropped {
                target: 4,
                reason: "attention cleared".into(),
            },
        ),
    );
    assert!(
        !nudge::queued_for(&state, "director"),
        "a dropped one does not"
    );
}

/// A nudge and an ordinary notice share a `source_seq` without
/// suppressing each other: the delivered-task step's key is the
/// delivery's seq — the same seq the delivery notice claims.
#[test]
fn a_nudge_and_a_notice_share_a_source_seq() {
    let mut events = base_events();
    events.extend([
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(5, "actor-t1-c1", delivered("t1", 1, "a", 1)),
    ]);

    // The delivery notice queued first does not stop the nudge.
    let mut state = replay(&events);
    apply(
        &mut state,
        &ev(
            6,
            "stage",
            EventKind::MessageQueued {
                to: "director".into(),
                file: Some("messages/5-delivered.md".into()),
                purpose: None,
                source_seq: Some(5),
            },
        ),
    );
    let wanted = BTreeSet::new();
    let moves = nudge::plan(&state, &wanted, |_| Some(0), 10_000_000);
    assert!(
        matches!(moves.as_slice(), [Move::Nudge { step }] if step.key == 5),
        "{moves:?}"
    );

    // And the nudge queued first does not stop the delivery notice.
    let mut state = replay(&events);
    apply(
        &mut state,
        &ev(
            6,
            "stage",
            EventKind::MessageQueued {
                to: "director".into(),
                file: Some("messages/5-nudge.md".into()),
                purpose: Some("nudge".into()),
                source_seq: Some(5),
            },
        ),
    );
    let owed = crate::notices::owed(&state, Path::new("/state/acme-shop"));
    assert!(
        owed.iter()
            .any(|n| n.source_seq == 5 && n.label == "delivered"),
        "{owed:?}"
    );
}

/// A step the user owes is shown by `next_steps` and pushed to no
/// one — never nudged, never escalated, never attention-worthy.
#[test]
fn the_users_step_is_never_pushed() {
    let mut events = base_events();
    events.extend([
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(5, "actor-t1-c1", delivered("t1", 1, "a", 1)),
        ev(6, "director", reviewed("t1", 1, Verdict::Accepted, vec![])),
        ev(
            7,
            "director",
            EventKind::ReportSent {
                to: "producer".into(),
                node: "integration".into(),
                file: "reports/r.md".into(),
            },
        ),
        ev(8, "stage", EventKind::MessageDelivered { target: 7 }),
        ev(
            9,
            "producer",
            EventKind::ReportSent {
                to: "user".into(),
                node: "integration".into(),
                file: "reports/r.md".into(),
            },
        ),
    ]);
    let state = replay(&events);
    let wanted = BTreeSet::new();
    let moves = nudge::plan(&state, &wanted, |_| Some(0), 10_000_000);
    assert!(moves.is_empty(), "{moves:?}");
}
