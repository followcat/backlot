//! A `terminal.send` the core answers `cancelled` (or that times out)
//! is a refusal the stage must never record as delivered. The first
//! half drives the ledger seam directly; the second drives a real
//! deliver tick against a fake core that answers `cancelled`.

use std::path::Path;
use std::sync::{Arc, Mutex};

use backlot_model::event::EventKind;
use backlot_model::summary::Work;
use backlot_model::{EventBy, NewEvent};
use hc_proto::client::{CallError, Client};
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use hc_proto::jsonrpc::RpcError;
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, DuplexStream};

use crate::deliver::{SCREEN_STABLE_MS, Screen, settle};
use crate::reconcile::LiveTerminal;
use crate::store::{self, Pending};
use crate::test_support::TestDir;

fn tmp(label: &str) -> TestDir {
    TestDir::new(&format!("backlot-refusal-{label}"))
}

fn cancelled() -> CallError {
    CallError::Rpc(RpcError {
        code: -32000,
        message: "cancelled".into(),
        data: Some(json!({ "kind": "cancelled" })),
    })
}

fn delivered(dir: &Path) -> usize {
    store::load_events(dir)
        .unwrap()
        .0
        .into_iter()
        .filter(|e| e.kind.name() == "message.delivered")
        .count()
}

fn append(dir: &Path, kind: EventKind) {
    store::append(dir, move |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(EventBy::role("producer"), kind),
            files: Vec::new(),
        })
    })
    .unwrap();
}

/// The core's `cancelled` kind records nothing and says nothing was
/// typed — hc-tui's own wording.
#[test]
fn a_cancelled_answer_records_no_delivery() {
    let dir = tmp("cancelled");
    let line = settle(&dir, &[3], Err(cancelled())).unwrap();
    assert!(line.contains("Send cancelled; nothing was typed. Send it again."));
    assert_eq!(delivered(&dir), 0);
}

/// A timed-out send is ambiguous: it records nothing and never claims
/// the line was not typed.
#[test]
fn a_timed_out_send_records_no_delivery_and_may_still_be_typed() {
    let dir = tmp("timeout");
    let line = settle(&dir, &[3], Err(CallError::Timeout)).unwrap();
    assert!(line.contains("Send timed out; the core may still type it."));
    assert_eq!(delivered(&dir), 0);
}

/// An explicit `sent: false` stays queued; a plain success records
/// every seq it covered.
#[test]
fn only_a_success_records_a_delivery() {
    let dir = tmp("success");
    queued_message(&dir);
    let seqs: Vec<u64> = store::load_state(&dir)
        .unwrap()
        .messages
        .iter()
        .map(|m| m.seq)
        .collect();
    assert_eq!(seqs.len(), 1);
    let refused = settle(
        &dir,
        &seqs,
        Ok(json!({ "sent": false, "reason": "no person" })),
    )
    .unwrap();
    assert!(refused.contains("no person"));
    assert_eq!(delivered(&dir), 0);
    let ok = settle(&dir, &seqs, Ok(json!({ "status": "working" }))).unwrap();
    assert!(ok.starts_with("delivered"));
    assert_eq!(
        store::load_events(&dir)
            .unwrap()
            .0
            .into_iter()
            .filter_map(|e| match e.kind {
                EventKind::MessageDelivered { target } => Some(target),
                _ => None,
            })
            .collect::<Vec<_>>(),
        seqs
    );
}

/// A broken wire is not tolerated — it still ends the tick.
#[test]
fn a_dead_wire_still_fails_the_tick() {
    let dir = tmp("closed");
    assert!(settle(&dir, &[3], Err(CallError::Closed)).is_err());
    assert_eq!(delivered(&dir), 0);
}

const METHODS: &[&str] = &["terminal.list", "terminal.send"];

/// `hello` with the terminal methods, then `terminal.send` answers the
/// core's `cancelled` kind while everything else records a refusal.
async fn serve_cancelled(stream: DuplexStream, sends: Arc<Mutex<Vec<String>>>) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = Reader::new(read);
    let hello = reader.read_line().await.unwrap().unwrap();
    let request: Value = serde_json::from_slice(&hello).unwrap();
    assert_eq!(request["method"], "hello");
    write
        .write_all(
            serde_json::to_string(&json!({
                "jsonrpc": "2.0", "id": 0,
                "result": {
                    "generation": 1, "version": "fake-core 0.0", "framed": true,
                    "core": { "id": "core-fake-1", "name": "fake-core" },
                    "methods": METHODS, "features": [],
                }
            }))
            .unwrap()
            .as_bytes(),
        )
        .await
        .unwrap();
    write.write_all(b"\n").await.unwrap();
    while let Ok(Some(frame)) = reader.read_frame().await {
        if frame.channel != 0 || frame.kind != Kind::Data {
            continue;
        }
        let request: Value = serde_json::from_slice(&frame.payload).unwrap();
        let body = match request["method"].as_str().unwrap_or("") {
            "terminal.send" => {
                let text = request["params"]["text"].as_str().unwrap_or("").to_owned();
                sends.lock().unwrap().push(text);
                json!({
                    "jsonrpc": "2.0", "id": request["id"],
                    "error": { "code": -32000, "message": "cancelled",
                               "data": { "kind": "cancelled" } }
                })
            }
            other => json!({
                "jsonrpc": "2.0", "id": request["id"],
                "error": { "code": -32000, "message": format!("{other} refused") }
            }),
        };
        write
            .write_all(&Frame::data(0, serde_json::to_vec(&body).unwrap()).encode())
            .await
            .unwrap();
    }
}

fn actor() -> backlot_model::event::ActorIdentity {
    backlot_model::event::ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: "plain".into(),
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    }
}

/// A studio with one role and a queued message to it.
fn queued_message(dir: &Path) {
    append(
        dir,
        EventKind::StudioCreated {
            title: "acme-shop".into(),
            repo: dir.to_string_lossy().into_owned(),
            mode: backlot_model::event::StudioMode::Standard,
            roles: Box::new(backlot_model::event::RoleConfigs {
                producer: actor(),
                director: actor(),
                writer: actor(),
            }),
            candidates: vec![backlot_model::event::Candidate {
                name: "actor-a".into(),
                runner: "fake".into(),
                runner_version: None,
                model: "plain".into(),
                effort: Some("high".into()),
                permission_profile: "standard".into(),
            }],
            rework_cap: 2,
            stall_limit_s: 600,
            nudge_after_s: 180,
            escalate_after_s: 600,
            check_env: Vec::new(),
            checks: Vec::new(),
            task_budget_s: None,
            limits: Default::default(),
        },
    );
    append(
        dir,
        EventKind::MessageQueued {
            to: "writer".into(),
            file: None,
            purpose: None,
            source_seq: None,
        },
    );
    // A started role is live with a terminal; the stage reads `live`
    // and sends only to a role that is up.
    append(
        dir,
        EventKind::RoleStarted {
            role: "writer".into(),
            terminal: Some("bl-acme-shop-writer".into()),
            actor: Some(actor()),
            session_id: None,
            reason: None,
        },
    );
}

/// The whole tick against a fake core: the send goes out, the core
/// answers `cancelled`, and no `message.delivered` is ever recorded —
/// the message is still owed on the next tick.
#[tokio::test]
async fn a_cancelled_send_from_the_core_leaves_the_message_queued() {
    let dir = tmp("tick");
    queued_message(&dir);
    let sends = Arc::new(Mutex::new(Vec::new()));
    let fake = Arc::clone(&sends);
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    tokio::spawn(serve_cancelled(core_side, fake));
    let (read, write) = tokio::io::split(client_side);
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    let stage = crate::daemon::Stage::for_test(client);
    let mut state = store::load_state(&dir).unwrap();
    assert!(!state.messages.is_empty(), "the message is in the ledger");
    // The role is live and has been up for longer than `MIN_UP_MS`.
    let writer = state.roles.get_mut("writer").unwrap();
    writer.live = true;
    writer.since_ms = 0;
    let live = [LiveTerminal {
        name: crate::reconcile::terminal_name("acme-shop", "writer").unwrap(),
        address: "t1".into(),
        ..Default::default()
    }];
    // The gate: live and up, waiting, and the same screen for 2 s.
    stage
        .work
        .borrow_mut()
        .insert(("acme-shop".into(), "writer".into()), Work::Waiting);
    stage.screens.borrow_mut().insert(
        ("acme-shop".into(), "writer".into()),
        Screen {
            text: "…".into(),
            same_since_ms: 0,
            at_ms: SCREEN_STABLE_MS,
        },
    );
    let now = 10_000;
    stage
        .deliver("acme-shop", &dir, &state, &live, now)
        .await
        .unwrap();
    // The send is spawned off the loop; wait for the fake core to see it.
    for _ in 0..100 {
        if !sends.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(sends.lock().unwrap().len(), 1, "the send went out");
    // Let the answer land, then collect it on the next tick.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    stage
        .deliver("acme-shop", &dir, &state, &live, now + 6_000)
        .await
        .unwrap();
    assert_eq!(delivered(&dir), 0, "a cancelled send is never delivered");
    let state = store::load_state(&dir).unwrap();
    assert!(
        state.messages.iter().all(|m| !m.delivered),
        "the message is still queued"
    );
}
