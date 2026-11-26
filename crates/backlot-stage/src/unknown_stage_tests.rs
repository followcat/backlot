//! Stage read-only pause and summary projection tests for unknown ledger event kinds.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::sync::{Arc, Mutex};

use backlot_model::event::{EventKind, RoleConfigs, StudioMode, default_max_actors};
use backlot_model::summary::Work;
use backlot_model::{ActorIdentity, EventBy, NewEvent};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, DuplexStream};

use crate::daemon::{Inventory, Stage};
use crate::deliver::{SCREEN_STABLE_MS, Screen};
use crate::reconcile::LiveTerminal;
use crate::store::{self, Pending};
use crate::test_support::TestDir;

fn actor() -> ActorIdentity {
    ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: "plain".into(),
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    }
}

fn seed_studio(dir: &std::path::Path, title: &str) {
    let kind = EventKind::StudioCreated {
        title: title.into(),
        repo: format!("/home/user/{title}"),
        members: Vec::new(),
        no_git: false,
        mode: StudioMode::Standard,
        roles: Box::new(RoleConfigs {
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
        max_actors: default_max_actors(),
        push: Default::default(),
    };
    store::append::<(), _>(dir, |_, _| {
        Ok(Pending {
            event: NewEvent::new(EventBy::role("user"), kind),
            files: Vec::new(),
        })
    })
    .unwrap();
}

fn append_msg(dir: &std::path::Path, to: &str) {
    store::append::<(), _>(dir, |_, _| {
        Ok(Pending {
            event: NewEvent::new(
                EventBy::role("user"),
                EventKind::MessageQueued {
                    to: to.into(),
                    file: None,
                    purpose: None,
                    source_seq: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
}

#[derive(Default)]
struct RecordedRpc {
    calls: Vec<(String, Value)>,
    docs: BTreeMap<String, (u64, Value)>,
}

async fn serve_recording(stream: DuplexStream, rec: Arc<Mutex<RecordedRpc>>) {
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
                    "core": { "id": "core-fake-1", "name": "host-a" },
                    "methods": [
                        "terminal.list", "terminal.run", "terminal.send",
                        "terminal.kill", "terminal.rm", "terminal.set_labels",
                        "terminal.screen", "doc.get", "doc.set"
                    ],
                    "features": []
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
        let method = request["method"].as_str().unwrap_or("").to_owned();
        let params = request["params"].clone();
        let body = {
            let mut st = rec.lock().unwrap();
            st.calls.push((method.clone(), params.clone()));
            match method.as_str() {
                "terminal.run" => json!({
                    "jsonrpc": "2.0", "id": request["id"],
                    "result": {
                        "address": "t-new",
                        "name": params["name"],
                        "labels": params["labels"],
                        "argv": params["argv"],
                        "cwd": "/tmp",
                        "state": "live",
                        "pid": 4242,
                        "created_ms": 1_700_000_000_000u64,
                        "screen": "ok",
                        "cols": 80,
                        "rows": 24
                    }
                }),
                "doc.get" => {
                    let name = params["name"].as_str().unwrap_or("").to_owned();
                    let (ver, val) = st.docs.get(&name).cloned().unwrap_or((0, Value::Null));
                    json!({
                        "jsonrpc": "2.0", "id": request["id"],
                        "result": { "version": ver, "value": val }
                    })
                }
                "doc.set" => {
                    let name = params["name"].as_str().unwrap_or("").to_owned();
                    let next = st.docs.get(&name).map(|(v, _)| *v + 1).unwrap_or(1);
                    st.docs.insert(name, (next, params["value"].clone()));
                    json!({
                        "jsonrpc": "2.0", "id": request["id"],
                        "result": { "version": next }
                    })
                }
                _ => json!({
                    "jsonrpc": "2.0", "id": request["id"],
                    "result": {}
                }),
            }
        };
        write
            .write_all(&Frame::data(0, serde_json::to_vec(&body).unwrap()).encode())
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn stage_pauses_read_only_for_unknown_studio_marks_summary_and_runs_others() {
    let base = TestDir::new("backlot-stage-unknown");
    let acme = store::studio_dir(&base, "acme");
    let beta = store::studio_dir(&base, "beta");

    seed_studio(&acme, "acme");
    store::append::<(), _>(&acme, |_, _| {
        Ok(Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::RoleStarted {
                    role: "writer".into(),
                    terminal: Some("bl-acme-writer".into()),
                    actor: Some(actor()),
                    session_id: None,
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    append_msg(&acme, "writer");
    append_msg(&acme, "producer");
    let unknown_line =
        r#"{"v":1,"seq":5,"ts_ms":5000,"by":{"role":"producer"},"kind":"design.opened"}"#;
    OpenOptions::new()
        .append(true)
        .open(acme.join("events.jsonl"))
        .unwrap()
        .write_all(format!("{unknown_line}\n").as_bytes())
        .unwrap();
    let acme_bytes_before = fs::read(acme.join("events.jsonl")).unwrap();

    seed_studio(&beta, "beta");
    append_msg(&beta, "producer");

    let rec = Arc::new(Mutex::new(RecordedRpc::default()));
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    tokio::spawn(serve_recording(core_side, Arc::clone(&rec)));
    let (read, write) = tokio::io::split(client_side);
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    let mut stage = Stage::for_test(client);
    stage.base = base.to_path_buf();

    stage
        .work
        .borrow_mut()
        .insert(("acme".into(), "writer".into()), Work::Waiting);
    stage.screens.borrow_mut().insert(
        ("acme".into(), "writer".into()),
        Screen {
            text: "ready".into(),
            same_since_ms: 0,
            at_ms: SCREEN_STABLE_MS,
        },
    );

    let inv = Inventory {
        terms: vec![LiveTerminal {
            name: crate::reconcile::terminal_name("acme", "writer").unwrap(),
            address: "t1".into(),
            ..Default::default()
        }],
        finished: Vec::new(),
        doing: BTreeMap::new(),
    };

    stage.tick(&inv).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    assert!(!stage.failed.contains_key("acme"), "{:?}", stage.failed);
    assert_eq!(
        fs::read(acme.join("events.jsonl")).unwrap(),
        acme_bytes_before,
        "acme ledger must stay untouched"
    );

    let calls = rec.lock().unwrap().calls.clone();
    let touched_acme = calls.iter().any(|(method, params)| {
        if method == "doc.get" || method == "doc.set" {
            return false;
        }
        let s = params.to_string();
        s.contains("acme") || s.contains("\"t1\"")
    });
    assert!(
        !touched_acme,
        "stage must not launch, relabel, watch, or send to unknown studio: {calls:?}"
    );
    let launched_beta = calls
        .iter()
        .any(|(method, params)| method == "terminal.run" && params.to_string().contains("beta"));
    assert!(launched_beta, "healthy sibling studio must run: {calls:?}");

    let summary_json = serde_json::to_value(stage.summary_view(&inv.doing)).unwrap();
    let studios = summary_json["studios"].as_array().unwrap();
    let acme_sum = studios.iter().find(|s| s["id"] == "acme").unwrap();
    assert_eq!(acme_sum["needs_upgrade"], true, "{acme_sum}");
    assert_eq!(
        acme_sum["unknown_kinds"],
        json!(["design.opened"]),
        "{acme_sum}"
    );
    let beta_sum = studios.iter().find(|s| s["id"] == "beta").unwrap();
    assert!(beta_sum.get("needs_upgrade").is_none(), "{beta_sum}");
    assert!(beta_sum.get("unknown_kinds").is_none(), "{beta_sum}");
}
