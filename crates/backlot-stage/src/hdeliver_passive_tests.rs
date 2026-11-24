//! S2 fix-round-1 tests for upgrade passive-context skipping (dropped old
//! notices, finished studios) and the 5-item newest-first `(context: …)` cap.

use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::sync::{Arc, Mutex};

use backlot_model::EventBy;
use backlot_model::event::{ActorIdentity, EventKind, Verdict};
use backlot_model::hollywood::{HOLLYWOOD_TERMINAL, HollywoodKind, NewHollywoodEvent, RoleMotion};
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
            if req["method"].as_str() == Some("terminal.send") {
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
async fn dropped_accepted_and_finished_studio_omit_passive_tail() {
    let tmp = TestDir::new("backlot-hdeliver-upgrade-skip");
    let hdir = store::hollywood_dir(tmp.path());
    let sent = Arc::new(Mutex::new(Vec::new()));
    let client = serve_send(Arc::clone(&sent)).await;

    // Replay a finished studio that has an accepted review (`seq 6`),
    // plus an active studio whose old-style `accepted` notice was dropped.
    let finished_state = tk::state_with(
        &tk::base_events(),
        vec![
            ("director", tk::cast("t1", "actor-a", "a")),
            ("actor-a", tk::delivered("t1", 1, "a", 1)),
            ("director", tk::reviewed("t1", 1, Verdict::Accepted, vec![])),
            (
                "user",
                EventKind::StudioFinished {
                    reason: "done".into(),
                },
            ),
        ],
    );
    let live_state = tk::state_with(
        &tk::base_events(),
        vec![
            ("director", tk::cast("t1", "actor-a", "a")),
            ("actor-a", tk::delivered("t1", 1, "a", 1)),
            ("director", tk::reviewed("t1", 1, Verdict::Accepted, vec![])),
        ],
    );
    assert!(finished_state.finished.is_some());

    // Replay Hollywood ledger containing dropped old-style `accepted` and `card` notices.
    let events = vec![
        tk::hev(
            1,
            "stage",
            HollywoodKind::RoleState {
                state: RoleMotion::Started,
                terminal: Some("t-hw".into()),
                actor: Some(started_actor()),
                session_id: None,
                reason: None,
            },
        ),
        tk::hev(
            2,
            "stage",
            HollywoodKind::MessageSent {
                to: "hollywood".into(),
                studio: Some("live-shop".into()),
                text: None,
                file: Some("messages/2-accepted.md".into()),
                source_seq: Some(6),
            },
        ),
        tk::hev(
            3,
            "stage",
            HollywoodKind::MessageDropped {
                target: 2,
                reason: "studio finished".into(),
            },
        ),
        tk::hev(4, "hollywood", tk::hpropose("t1")),
        tk::hev(5, "user", HollywoodKind::CardApplied { card: 4 }),
        tk::hev(
            6,
            "stage",
            HollywoodKind::CardDone {
                card: 4,
                note: String::new(),
            },
        ),
        tk::hev(
            7,
            "stage",
            HollywoodKind::MessageSent {
                to: "hollywood".into(),
                studio: None,
                text: None,
                file: Some("messages/7-card.md".into()),
                source_seq: Some(6),
            },
        ),
        tk::hev(
            8,
            "stage",
            HollywoodKind::MessageDropped {
                target: 7,
                reason: "stale".into(),
            },
        ),
        tk::hev(
            9,
            "stage",
            HollywoodKind::MessageSent {
                to: "hollywood".into(),
                studio: Some("done-shop".into()),
                text: None,
                file: Some("messages/9-finished.md".into()),
                source_seq: Some(7),
            },
        ),
        tk::hev(10, "stage", HollywoodKind::MessageDelivered { target: 9 }),
        tk::hev(
            11,
            "user",
            HollywoodKind::MessageSent {
                to: "hollywood".into(),
                studio: None,
                text: Some("hello after upgrade".into()),
                file: None,
                source_seq: None,
            },
        ),
    ];
    fs::create_dir_all(&hdir).unwrap();
    let mut raw = String::new();
    for ev in &events {
        raw.push_str(&serde_json::to_string(ev).unwrap());
        raw.push('\n');
    }
    fs::write(hdir.join("events.jsonl"), raw).unwrap();

    let mut stage = Stage::for_test(client);
    stage.base = tmp.path().to_path_buf();
    stage.cached.insert("done-shop".into(), finished_state);
    stage.cached.insert("live-shop".into(), live_state);
    stage.hrefresh(&hdir);

    let now = 20_000;
    open_stage_gate(&stage, 1_000, now);
    stage.hdeliver(&hdir, &live_terminals(), now).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let batches = sent.lock().unwrap().clone();
    assert_eq!(batches.len(), 1);
    assert_eq!(
        batches[0],
        "[backlot] user → hollywood: hello after upgrade"
    );
}

#[tokio::test]
async fn passive_context_caps_at_five_newest_first_and_marks_overflow_accounted() {
    let tmp = TestDir::new("backlot-hdeliver-passive-cap");
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

    // Propose -> Apply -> Done for cards c2..c8 (seqs 2..=22).
    let mut seq = 2;
    for _ in 0..7 {
        let card = seq;
        write_hev(&hdir, seq, seq * 100, "hollywood", tk::hpropose("t1"));
        seq += 1;
        write_hev(
            &hdir,
            seq,
            seq * 100,
            "user",
            HollywoodKind::CardApplied { card },
        );
        seq += 1;
        write_hev(
            &hdir,
            seq,
            seq * 100,
            "stage",
            HollywoodKind::CardDone {
                card,
                note: String::new(),
            },
        );
        seq += 1;
    }
    write_hev(
        &hdir,
        seq,
        seq * 100,
        "user",
        HollywoodKind::MessageSent {
            to: "hollywood".into(),
            studio: None,
            text: Some("first tell".into()),
            file: None,
            source_seq: None,
        },
    );
    seq += 1;

    let mut stage = Stage::for_test(client);
    stage.base = tmp.path().to_path_buf();
    let terminals = live_terminals();
    open_stage_gate(&stage, 1_000, 10_000);
    stage.hdeliver(&hdir, &terminals, 10_000).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let batches = sent.lock().unwrap().clone();
    assert_eq!(batches.len(), 1);
    assert_eq!(
        batches[0],
        "[backlot] user → hollywood: first tell · (context: c20 done, c17 done, c14 done, c11 done, c8 done, +2 more)"
    );

    // Finish collection -> marks ALL 7 items accounted (including +2 more).
    stage.hdeliver(&hdir, &terminals, 11_000).await.unwrap();
    write_hev(
        &hdir,
        seq,
        12_000,
        "user",
        HollywoodKind::MessageSent {
            to: "hollywood".into(),
            studio: None,
            text: Some("second tell".into()),
            file: None,
            source_seq: None,
        },
    );
    open_stage_gate(&stage, 1_000, 17_000);
    stage.hdeliver(&hdir, &terminals, 17_000).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let batches = sent.lock().unwrap().clone();
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[1], "[backlot] user → hollywood: second tell");
}
