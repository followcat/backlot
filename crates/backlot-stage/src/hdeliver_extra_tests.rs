//! Additional S2 two-lane inbox tests (`passive_context` and single-notice
//! `<= 400 B` inline delivery).

use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::sync::{Arc, Mutex};

use backlot_model::EventBy;
use backlot_model::event::{ActorIdentity, Verdict};
use backlot_model::hollywood::{HOLLYWOOD_TERMINAL, HollywoodKind, NewHollywoodEvent, RoleMotion};
use backlot_model::state::{ReviewRecord, StudioState};
use backlot_model::summary::Work;
use backlot_model::testkit as tk;
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::daemon::Stage;
use crate::deliver::Screen;
use crate::hstage::hkey;
use crate::reconcile::LiveTerminal;
use crate::store;
use crate::test_support::TestDir;

async fn serve_send(sent: Arc<Mutex<Vec<String>>>) -> Client {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let (read, mut write) = tokio::io::split(core_side);
        let mut reader = Reader::new(read);
        let hello = reader.read_line().await.unwrap().unwrap();
        let req: Value = serde_json::from_slice(&hello).unwrap();
        assert_eq!(req["method"], "hello");
        write
            .write_all(
                serde_json::to_string(&json!({
                    "jsonrpc": "2.0", "id": 0,
                    "result": {
                        "generation": 1, "version": "fake-core 0.0",
                        "framed": true,
                        "core": {"id": "core-fake-1", "name": "fake-core"},
                        "methods": ["terminal.send"],
                        "features": [],
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
            let req: Value = serde_json::from_slice(&frame.payload).unwrap();
            let method = req["method"].as_str().unwrap_or_default();
            if method == "terminal.send" {
                let text = req["params"]["text"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                sent.lock().unwrap().push(text);
                let reply = serde_json::to_vec(&json!({
                    "jsonrpc": "2.0",
                    "id": req["id"],
                    "result": { "sent": true }
                }))
                .unwrap();
                write
                    .write_all(&Frame::data(0, reply).encode())
                    .await
                    .unwrap();
            }
        }
    });
    let (read, write) = tokio::io::split(client_side);
    Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap()
}

fn write_hev(hdir: &Path, seq: u64, ts_ms: u64, role: &str, kind: HollywoodKind) {
    fs::create_dir_all(hdir).unwrap();
    let ev = NewHollywoodEvent::new(EventBy::role(role), kind).stamp(seq, ts_ms);
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(hdir.join("events.jsonl"))
        .unwrap();
    writeln!(f, "{}", serde_json::to_string(&ev).unwrap()).unwrap();
}

fn open_stage_gate(stage: &Stage, since: u64, now: u64) {
    let pair = hkey();
    stage.work.borrow_mut().insert(pair.clone(), Work::Waiting);
    stage.screens.borrow_mut().insert(
        pair,
        Screen {
            text: "> ".into(),
            same_since_ms: since,
            at_ms: now,
        },
    );
}

fn live_terminals() -> Vec<LiveTerminal> {
    vec![LiveTerminal {
        name: HOLLYWOOD_TERMINAL.into(),
        address: "t-hw".into(),
        ..Default::default()
    }]
}

fn started_actor() -> ActorIdentity {
    ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: "model-h".into(),
        effort: None,
        permission_profile: "standard".into(),
    }
}

#[tokio::test]
async fn passive_context_never_wakes_and_rides_once_and_long_person_overflows_to_file() {
    let tmp = TestDir::new("backlot-hdeliver-passive-overflow");
    let hdir = store::hollywood_dir(tmp.path());
    let sent = Arc::new(Mutex::new(Vec::new()));
    let client = serve_send(Arc::clone(&sent)).await;

    write_hev(
        &hdir,
        1,
        1_000,
        "stage",
        HollywoodKind::RoleState {
            state: RoleMotion::Started,
            terminal: Some("t-hw".into()),
            actor: Some(started_actor()),
            session_id: None,
            reason: None,
        },
    );
    write_hev(&hdir, 2, 2_000, "hollywood", tk::hpropose("t1"));
    write_hev(
        &hdir,
        3,
        2_500,
        "user",
        HollywoodKind::CardApplied { card: 2 },
    );
    write_hev(
        &hdir,
        4,
        3_000,
        "stage",
        HollywoodKind::CardDone {
            card: 2,
            note: String::new(),
        },
    );

    let mut stage = Stage::for_test(client);
    stage.base = tmp.path().to_path_buf();
    let mut s = StudioState::default();
    let mut task = tk::base_state().tasks["t1"].clone();
    task.reviews.push(ReviewRecord {
        delivery: 1,
        reviewer: "director".into(),
        verdict: Verdict::Accepted,
        conclusion: "shipped".into(),
        findings: Vec::new(),
        upstream_changes: Vec::new(),
        seq: 11,
        ts_ms: 0,
    });
    s.tasks.insert("t1".into(), task);
    stage.cached.insert("acme".into(), s);

    let terminals = live_terminals();
    open_stage_gate(&stage, 1_000, 10_000);
    stage.hdeliver(&hdir, &terminals, 10_000).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert!(sent.lock().unwrap().is_empty());

    let long1 = "a".repeat(1200);
    let long2 = format!("{}tail-marker", "b".repeat(1100));
    write_hev(
        &hdir,
        5,
        11_000,
        "user",
        HollywoodKind::MessageSent {
            to: "hollywood".into(),
            studio: None,
            text: Some(long1),
            file: None,
            source_seq: None,
        },
    );
    write_hev(
        &hdir,
        6,
        12_000,
        "user",
        HollywoodKind::MessageSent {
            to: "hollywood".into(),
            studio: None,
            text: Some(long2),
            file: None,
            source_seq: None,
        },
    );
    open_stage_gate(&stage, 1_000, 15_000);
    stage.hdeliver(&hdir, &terminals, 15_000).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let batches = sent.lock().unwrap().clone();
    assert_eq!(batches.len(), 1);
    let first = &batches[0];
    assert!(first.contains("messages/user-6.md"), "{first}");
    assert!(first.contains("(2 messages; latest:"), "{first}");
    assert!(first.contains("c2 done"), "{first}");
    assert!(first.contains("acme t1 accepted"), "{first}");
    assert!(hdir.join("messages/user-6.md").is_file());

    stage.hdeliver(&hdir, &terminals, 16_000).await.unwrap();
    write_hev(
        &hdir,
        7,
        17_000,
        "user",
        HollywoodKind::MessageSent {
            to: "hollywood".into(),
            studio: None,
            text: Some("short next".into()),
            file: None,
            source_seq: None,
        },
    );
    open_stage_gate(&stage, 1_000, 22_000);
    stage.hdeliver(&hdir, &terminals, 22_000).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let batches = sent.lock().unwrap().clone();
    assert_eq!(batches.len(), 2);
    assert!(batches[1].contains("short next"), "{}", batches[1]);
    assert!(
        !batches[1].contains("c2 done") && !batches[1].contains("acme t1 accepted"),
        "passive context repeated: {}",
        batches[1]
    );
}

#[tokio::test]
async fn single_notice_under_400b_delivers_inline() {
    let tmp = TestDir::new("backlot-hdeliver-single-notice-inline");
    let hdir = store::hollywood_dir(tmp.path());
    let sent = Arc::new(Mutex::new(Vec::new()));
    let client = serve_send(Arc::clone(&sent)).await;

    write_hev(
        &hdir,
        1,
        1_000,
        "stage",
        HollywoodKind::RoleState {
            state: RoleMotion::Started,
            terminal: Some("t-hw".into()),
            actor: Some(started_actor()),
            session_id: None,
            reason: None,
        },
    );
    let mut stage = Stage::for_test(client);
    stage.base = tmp.path().to_path_buf();
    let s = StudioState {
        finished: Some(backlot_model::state::FinishRecord {
            seq: 9,
            reason: "all clean".into(),
            ts_ms: 0,
        }),
        ..Default::default()
    };
    stage.cached.insert("acme".into(), s);

    let wall = crate::live::now_ms();
    let now = wall + 10_000;
    open_stage_gate(&stage, 1_000, now);
    stage.hdeliver(&hdir, &live_terminals(), now).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let batches = sent.lock().unwrap().clone();
    assert_eq!(batches.len(), 1);
    assert!(!batches[0].contains('\n'), "{}", batches[0]);
    assert!(
        batches[0].contains("acme: studio finished"),
        "{}",
        batches[0]
    );
    let has_digest = fs::read_dir(hdir.join("messages"))
        .map(|d| {
            d.filter_map(Result::ok)
                .any(|e| e.file_name().to_string_lossy().starts_with("digest-"))
        })
        .unwrap_or(false);
    assert!(!has_digest);
}
