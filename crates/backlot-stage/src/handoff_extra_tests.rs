//! Additional handoff edge cases: fallback commit vs bare startup Waiting,
//! `!labels_ok` immediate old-terminal removal, and daemon restart recovery.

use std::collections::BTreeMap;
use std::fs;
use std::sync::{Arc, Mutex};

use backlot_model::EventBy;
use backlot_model::event::ActorIdentity;
use backlot_model::hollywood::{HOLLYWOOD_TERMINAL, HollywoodKind, NewHollywoodEvent, RoleMotion};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::daemon::Stage;
use crate::handoff::{
    HandoffPhase, HandoffRecord, load_handoff, save_handoff, stage_handoff_request,
};
use crate::reconcile::LiveTerminal;
use crate::store::{self, HPending};
use crate::test_support::TestDir;

fn fake_runner_bin() -> String {
    format!("{}/../../tools/fake-runner.sh", env!("CARGO_MANIFEST_DIR"))
}

#[derive(Default)]
struct MiniWorld {
    live: Vec<Value>,
    finished: Vec<Value>,
    screens: BTreeMap<String, String>,
    kills: Vec<String>,
    rms: Vec<String>,
    next_id: u32,
    reject_label: bool,
}

async fn mini_client(world: Arc<Mutex<MiniWorld>>) -> Client {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let (read, mut write) = tokio::io::split(core_side);
        let mut reader = Reader::new(read);
        let _ = reader.read_line().await.unwrap().unwrap();
        let methods = [
            "terminal.run",
            "terminal.kill",
            "terminal.rm",
            "terminal.read",
            "terminal.send",
            "terminal.keys",
            "terminal.label",
            "terminal.list",
        ];
        write
            .write_all(
                serde_json::to_string(&json!({
                    "jsonrpc": "2.0", "id": 0,
                    "result": {
                        "generation": 1, "version": "fake-core 0.0",
                        "framed": true,
                        "core": {"id": "core-fake-1", "name": "fake-core"},
                        "methods": methods, "features": [],
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
            let id = req["id"].clone();
            let reply = {
                let mut w = world.lock().unwrap();
                match req["method"].as_str().unwrap_or("") {
                    "terminal.run" => {
                        w.next_id += 1;
                        let addr = format!("t-cand-{}", w.next_id);
                        let t = json!({
                            "address": addr,
                            "name": req["params"]["name"],
                            "labels": req["params"]["labels"],
                            "argv": req["params"]["argv"],
                            "cwd": req["params"]["cwd"],
                            "state": "live", "created_ms": 0,
                            "screen": "", "cols": 200, "rows": 120,
                        });
                        w.live.push(t.clone());
                        json!({"jsonrpc": "2.0", "id": id, "result": t})
                    }
                    "terminal.read" => {
                        let a = req["params"]["terminal"].as_str().unwrap_or("");
                        json!({"jsonrpc": "2.0", "id": id, "result": {"text": w.screens.get(a).cloned().unwrap_or_default(), "complete": true, "how": "screen"}})
                    }
                    "terminal.kill" => {
                        let a = req["params"]["terminal"].as_str().unwrap_or("").to_string();
                        w.kills.push(a.clone());
                        if let Some(pos) = w.live.iter().position(|t| t["address"] == a) {
                            let mut t = w.live.remove(pos);
                            t["state"] = json!("finished");
                            w.finished.push(t.clone());
                            json!({"jsonrpc": "2.0", "id": id, "result": t})
                        } else {
                            json!({"jsonrpc": "2.0", "id": id, "result": {}})
                        }
                    }
                    "terminal.rm" => {
                        let a = req["params"]["terminal"].as_str().unwrap_or("").to_string();
                        w.rms.push(a.clone());
                        w.live.retain(|t| t["address"] != a);
                        w.finished.retain(|t| t["address"] != a);
                        json!({"jsonrpc": "2.0", "id": id, "result": {}})
                    }
                    "terminal.label" => {
                        if w.reject_label {
                            json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": "unsupported", "data": {"kind": "unsupported"}}})
                        } else {
                            json!({"jsonrpc": "2.0", "id": id, "result": {}})
                        }
                    }
                    _ => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
                }
            };
            write
                .write_all(&Frame::data(0, serde_json::to_vec(&reply).unwrap()).encode())
                .await
                .unwrap();
        }
    });
    let (r, w) = tokio::io::split(client_side);
    Client::open(Box::pin(r), Box::pin(w), None).await.unwrap()
}

fn live_pair(w: &MiniWorld) -> (Vec<LiveTerminal>, Vec<LiveTerminal>) {
    let map = |xs: &[Value]| -> Vec<LiveTerminal> {
        xs.iter()
            .map(|v| LiveTerminal {
                address: v["address"].as_str().unwrap().into(),
                name: v["name"].as_str().unwrap().into(),
                labels: serde_json::from_value(v["labels"].clone()).unwrap_or_default(),
                argv: serde_json::from_value(v["argv"].clone()).unwrap_or_default(),
                cwd: v["cwd"].as_str().unwrap_or("").into(),
                ..Default::default()
            })
            .collect()
    };
    (map(&w.live), map(&w.finished))
}

#[tokio::test]
async fn bare_boot_waiting_does_not_commit_and_unlabelled_core_removes_old_immediately() {
    let tmp = TestDir::new("bl-handoff-extra");
    let cfg_home = tmp.path().join("cfg");
    fs::create_dir_all(cfg_home.join("backlot")).unwrap();
    fs::write(
        cfg_home.join("backlot/config.toml"),
        "[hollywood]\nrunner = \"fake\"\nmodel = \"model-old\"\neffort = \"high\"\n",
    )
    .unwrap();
    let hdir = store::hollywood_dir(tmp.path());
    fs::create_dir_all(&hdir).unwrap();
    let old_actor = ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: "model-old".into(),
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    };
    store::append_hollywood(&hdir, |_, _| {
        Ok::<_, ()>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role("stage"),
                HollywoodKind::RoleState {
                    state: RoleMotion::Started,
                    terminal: Some("t-old".into()),
                    actor: Some(old_actor),
                    session_id: Some("s-old".into()),
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();

    let world = Arc::new(Mutex::new(MiniWorld {
        live: vec![json!({
            "address": "t-old", "name": HOLLYWOOD_TERMINAL,
            "labels": {"backlot.role": "hollywood"},
            "argv": ["fake"], "cwd": hdir.to_string_lossy(),
        })],
        reject_label: true,
        ..Default::default()
    }));
    let client = mini_client(world.clone()).await;
    let mut stage = Stage::for_test(client);
    stage.base = tmp.path().to_path_buf();
    stage.state_dir = tmp.path().to_string_lossy().into_owned();
    stage.env_vars.insert(
        "XDG_CONFIG_HOME".into(),
        cfg_home.to_string_lossy().into_owned(),
    );
    stage
        .env_vars
        .insert("BACKLOT_FAKE_RUNNER".into(), fake_runner_bin());
    let mut slot = true;

    stage_handoff_request(
        &hdir,
        "fake",
        "model-new",
        Some("low"),
        20_000,
        false,
        "user switch",
        None,
        1000,
    )
    .unwrap();
    let (live, fin) = live_pair(&world.lock().unwrap());
    stage.hollywood(&live, &fin, &mut slot, 1100).await.unwrap();
    let cand = load_handoff(&hdir)
        .unwrap()
        .unwrap()
        .candidate_terminal
        .unwrap();

    // Bare initial Work::Waiting with zero working time must NOT commit!
    world
        .lock()
        .unwrap()
        .screens
        .insert(cand.clone(), "idle prompt\n".into());
    let (live, fin) = live_pair(&world.lock().unwrap());
    stage.hollywood(&live, &fin, &mut slot, 2500).await.unwrap();
    assert_eq!(
        load_handoff(&hdir).unwrap().unwrap().phase,
        HandoffPhase::Verifying
    );

    // Work >= 2s then clean Waiting commits AND removes old terminal immediately when !labels_ok!
    world
        .lock()
        .unwrap()
        .screens
        .insert(cand.clone(), "fake-work: working\n".into());
    let (live, fin) = live_pair(&world.lock().unwrap());
    stage.hollywood(&live, &fin, &mut slot, 3000).await.unwrap();
    let (live, fin) = live_pair(&world.lock().unwrap());
    stage.hollywood(&live, &fin, &mut slot, 5500).await.unwrap();
    world.lock().unwrap().screens.insert(cand, "ready\n".into());
    let (live, fin) = live_pair(&world.lock().unwrap());
    stage.hollywood(&live, &fin, &mut slot, 6000).await.unwrap();
    assert_eq!(
        load_handoff(&hdir).unwrap().unwrap().phase,
        HandoffPhase::Committed
    );
    assert!(world.lock().unwrap().rms.contains(&"t-old".to_string()));
}

#[tokio::test]
async fn stage_recovers_interrupted_handoff_on_restart() {
    let tmp = TestDir::new("bl-handoff-recover");
    let hdir = store::hollywood_dir(tmp.path());
    fs::create_dir_all(&hdir).unwrap();
    save_handoff(
        &hdir,
        &HandoffRecord {
            id: "h-crash".into(),
            phase: HandoffPhase::Verifying,
            generation: 1,
            target_runner: "fake".into(),
            target_model: "model-x".into(),
            candidate_terminal: Some("t-vanished".into()),
            started_ms: 1000,
            last_tick_ms: 1000,
            timeout_ms: 90_000,
            ..Default::default()
        },
    )
    .unwrap();
    let world = Arc::new(Mutex::new(MiniWorld::default()));
    let client = mini_client(world).await;
    let mut stage = Stage::for_test(client);
    stage.base = tmp.path().to_path_buf();
    let mut slot = true;
    stage.hollywood(&[], &[], &mut slot, 2000).await.unwrap();
    let rb = load_handoff(&hdir).unwrap().unwrap();
    assert_eq!(rb.phase, HandoffPhase::RolledBack);
    assert!(rb.error.unwrap().contains("exited"));
}
