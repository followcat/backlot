//! Two-lane delivery for Hollywood's inbox (`hdeliver.rs`):
//! person-lane inline merging with deferred notice footer, 5 s defer on
//! stage notices, `--answers` `message.seen` skip, passive `card.done`
//! / `accepted` context, and > 2 KiB / single-notice formatting.

use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::sync::{Arc, Mutex};

use backlot_model::EventBy;
use backlot_model::event::{ActorIdentity, PERMISSION_PROMPT};
use backlot_model::hollywood::{HOLLYWOOD_TERMINAL, HollywoodKind, NewHollywoodEvent, RoleMotion};
use backlot_model::state::{Attention, StudioState};
use backlot_model::summary::Work;
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::daemon::Stage;
use crate::deliver::Screen;
use crate::hstage::hkey;
use crate::reconcile::LiveTerminal;
use crate::store::{self, HPending};
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

fn attention(seq: u64, role: &str, needs_user: bool) -> Attention {
    Attention {
        seq,
        role: role.into(),
        reason: PERMISSION_PROMPT.into(),
        detail: "Bash: cargo test".into(),
        open: true,
        ts_ms: 0,
        needs_user,
        cleared_reason: None,
        account: None,
        reset_at_ms: None,
    }
}

fn digest_files(hdir: &Path) -> Vec<String> {
    let dir = hdir.join("messages");
    if !dir.exists() {
        return Vec::new();
    }
    fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| {
            let name = e.ok()?.file_name().to_string_lossy().into_owned();
            name.starts_with("digest-").then_some(name)
        })
        .collect()
}

#[tokio::test]
async fn person_lane_delivers_inline_merged_before_notices_with_footer() {
    let tmp = TestDir::new("backlot-hdeliver-two-lane");
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
    write_hev(
        &hdir,
        2,
        2_000,
        "user",
        HollywoodKind::MessageSent {
            to: "hollywood".into(),
            studio: None,
            text: Some("first user ask".into()),
            file: None,
            source_seq: None,
        },
    );
    write_hev(
        &hdir,
        3,
        3_000,
        "user",
        HollywoodKind::MessageSent {
            to: "hollywood".into(),
            studio: None,
            text: Some("second user ask".into()),
            file: None,
            source_seq: None,
        },
    );

    let mut stage = Stage::for_test(client);
    stage.base = tmp.path().to_path_buf();
    let mut s = StudioState::default();
    s.attention.push(attention(1, "producer", true));
    s.attention.push(attention(2, "actor-t1-c1", false));
    stage.cached.insert("acme".into(), s);

    let wall = crate::live::now_ms();
    let t_pass1 = wall + 1_000;
    open_stage_gate(&stage, 1_000, t_pass1);
    let terminals = live_terminals();
    stage.hdeliver(&hdir, &terminals, t_pass1).await.unwrap();

    // Wait briefly so tokio's spawned send finishes on the mock core.
    tokio::task::yield_now().await;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let first_batch = sent.lock().unwrap().clone();
    assert_eq!(first_batch.len(), 1, "expected 1 merged person send");
    let msg = &first_batch[0];
    assert!(
        !msg.contains('\n'),
        "inline send must be single-line: {msg}"
    );
    assert!(msg.contains("first user ask"), "{msg}");
    assert!(msg.contains("second user ask"), "{msg}");
    assert!(
        msg.find("first user ask").unwrap() < msg.find("second user ask").unwrap(),
        "oldest-to-newest order required: {msg}"
    );
    assert!(
        msg.contains("(+2 stage notices waiting: prompt, attention)"),
        "expected waiting footer: {msg}"
    );
    assert!(
        digest_files(&hdir).is_empty(),
        "no digest should be written on person lane turn"
    );

    // Collect delivery and verify 5 s defer holds notices even if send gap is ready.
    let t_collect = wall + 4_500;
    open_stage_gate(&stage, 1_000, t_collect);
    stage.hdeliver(&hdir, &terminals, t_collect).await.unwrap();
    // Clear sent_ms manually to isolate NOTICE_DEFER_MS (5_000 ms) from SEND_GAP_MS.
    *stage.sent_ms.borrow_mut() = Default::default();
    stage.hdeliver(&hdir, &terminals, t_collect).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert_eq!(
        sent.lock().unwrap().len(),
        1,
        "notices younger than 5 s must stay deferred"
    );

    // After 5 s defer elapses, the notice digest lands on Lane 2.
    let t_notice = wall + 6_500;
    open_stage_gate(&stage, 1_000, t_notice);
    stage.hdeliver(&hdir, &terminals, t_notice).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let batches = sent.lock().unwrap().clone();
    assert_eq!(batches.len(), 2, "expected notice digest after 5 s defer");
    assert!(batches[1].contains("digest-"), "{}", batches[1]);
    assert!(batches[1].contains("(2 items)"), "{}", batches[1]);
    assert_eq!(digest_files(&hdir).len(), 1);
}

#[tokio::test]
async fn answers_flag_marks_message_seen_and_skips_delivery() {
    let tmp = TestDir::new("backlot-hdeliver-seen");
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
    write_hev(
        &hdir,
        2,
        2_000,
        "user",
        HollywoodKind::MessageSent {
            to: "hollywood".into(),
            studio: None,
            text: Some("answered out of band".into()),
            file: None,
            source_seq: None,
        },
    );
    write_hev(
        &hdir,
        3,
        3_000,
        "hollywood",
        HollywoodKind::Reported {
            headline: "done early".into(),
            body: None,
        },
    );
    write_hev(
        &hdir,
        4,
        3_001,
        "hollywood",
        HollywoodKind::MessageSeen {
            target: 2,
            by_seq: 3,
        },
    );

    let mut stage = Stage::for_test(client);
    stage.base = tmp.path().to_path_buf();
    let now = 20_000;
    open_stage_gate(&stage, 1_000, now);
    stage.hdeliver(&hdir, &live_terminals(), now).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert!(
        sent.lock().unwrap().is_empty(),
        "seen user tell must not be pasted to terminal: {:?}",
        *sent.lock().unwrap()
    );

    // Queue a fresh un-seen tell; only the new tell delivers.
    store::append_hollywood(&hdir, |_, _| {
        Ok::<HPending, std::convert::Infallible>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role("user"),
                HollywoodKind::MessageSent {
                    to: "hollywood".into(),
                    studio: None,
                    text: Some("follow up".into()),
                    file: None,
                    source_seq: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    stage
        .hdeliver(&hdir, &live_terminals(), 25_000)
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let batches = sent.lock().unwrap().clone();
    assert_eq!(batches.len(), 1);
    assert!(batches[0].contains("follow up"));
    assert!(!batches[0].contains("answered out of band"));
}
