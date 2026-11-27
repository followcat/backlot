//! Fail-first lifecycle tests for Hollywood's make-before-break handoff
//! (`handoff.rs` & `handoff_run.rs`), quota/auth watch detection, delivery gating,
//! `hollywood-previous` 10-minute sweep, and `CardAction::Setup` upgrade.

use std::collections::BTreeMap;
use std::fs;
use std::sync::{Arc, Mutex};

use backlot_model::EventBy;
use backlot_model::event::{ActorIdentity, ApprovalDecision};
use backlot_model::hollywood::{
    CardAction, CardBase, CardState, HOLLYWOOD, HOLLYWOOD_TERMINAL, HollywoodKind,
    NewHollywoodEvent, RoleMotion,
};
use backlot_model::summary::{QueueReason, Work};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::daemon::Stage;
use crate::handoff::{
    HandoffPhase, HandoffRecord, PREVIOUS_RETENTION_MS, load_handoff, request_cancel,
    stage_handoff_request,
};
use crate::handoff_term::{active_hollywood_terminal, auth_error_detail, next_candidate_name};
use crate::reconcile::LiveTerminal;
use crate::store::{self, HPending};
use crate::test_support::TestDir;

#[derive(Default)]
struct FakeWorld {
    live: Vec<Value>,
    finished: Vec<Value>,
    screens: BTreeMap<String, String>,
    runs: Vec<Value>,
    kills: Vec<String>,
    rms: Vec<String>,
    labels: Vec<(String, BTreeMap<String, String>)>,
    sends: Vec<(String, String)>,
    keys: Vec<(String, String)>,
    next_id: u32,
    reject_label: bool,
}

async fn fake_core_client(world: Arc<Mutex<FakeWorld>>) -> Client {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let (read, mut write) = tokio::io::split(core_side);
        let mut reader = Reader::new(read);
        let hello = reader.read_line().await.unwrap().unwrap();
        let req: Value = serde_json::from_slice(&hello).unwrap();
        assert_eq!(req["method"], "hello");
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
            let method = req["method"].as_str().unwrap_or("");
            let id = req["id"].clone();
            let reply = {
                let mut w = world.lock().unwrap();
                match method {
                    "terminal.run" => {
                        w.runs.push(req["params"].clone());
                        w.next_id += 1;
                        let addr = format!("t-cand-{}", w.next_id);
                        let term = json!({
                            "address": addr,
                            "name": req["params"]["name"],
                            "labels": req["params"]["labels"],
                            "argv": req["params"]["argv"],
                            "cwd": req["params"]["cwd"],
                            "state": "live",
                            "created_ms": 0,
                            "screen": "",
                            "cols": 200, "rows": 120,
                        });
                        w.live.push(term.clone());
                        json!({"jsonrpc": "2.0", "id": id, "result": term})
                    }
                    "terminal.read" => {
                        let addr = req["params"]["terminal"].as_str().unwrap_or("");
                        let text = w.screens.get(addr).cloned().unwrap_or_default();
                        json!({"jsonrpc": "2.0", "id": id, "result": {"text": text, "complete": true, "how": "screen"}})
                    }
                    "terminal.kill" => {
                        let addr = req["params"]["terminal"].as_str().unwrap_or("").to_string();
                        w.kills.push(addr.clone());
                        if let Some(pos) = w.live.iter().position(|t| t["address"] == addr) {
                            let mut t = w.live.remove(pos);
                            t["state"] = json!("finished");
                            w.finished.push(t.clone());
                            json!({"jsonrpc": "2.0", "id": id, "result": t})
                        } else {
                            json!({"jsonrpc": "2.0", "id": id, "result": {}})
                        }
                    }
                    "terminal.rm" => {
                        let addr = req["params"]["terminal"].as_str().unwrap_or("").to_string();
                        w.rms.push(addr.clone());
                        w.live.retain(|t| t["address"] != addr);
                        w.finished.retain(|t| t["address"] != addr);
                        json!({"jsonrpc": "2.0", "id": id, "result": {}})
                    }
                    "terminal.label" => {
                        if w.reject_label {
                            json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": "unsupported", "data": {"kind": "unsupported"}}})
                        } else {
                            let addr = req["params"]["terminal"].as_str().unwrap_or("").to_string();
                            let patch: BTreeMap<String, String> =
                                serde_json::from_value(req["params"]["labels"].clone())
                                    .unwrap_or_default();
                            w.labels.push((addr.clone(), patch.clone()));
                            for t in &mut w.live {
                                if t["address"] == addr {
                                    let obj = t["labels"].as_object_mut().unwrap();
                                    for (k, v) in &patch {
                                        if v.is_empty() {
                                            obj.remove(k);
                                        } else {
                                            obj.insert(k.clone(), json!(v));
                                        }
                                    }
                                }
                            }
                            for t in &mut w.finished {
                                if t["address"] == addr {
                                    let obj = t["labels"].as_object_mut().unwrap();
                                    for (k, v) in &patch {
                                        if v.is_empty() {
                                            obj.remove(k);
                                        } else {
                                            obj.insert(k.clone(), json!(v));
                                        }
                                    }
                                }
                            }
                            json!({"jsonrpc": "2.0", "id": id, "result": {}})
                        }
                    }
                    "terminal.send" => {
                        let addr = req["params"]["terminal"].as_str().unwrap_or("").to_string();
                        let text = req["params"]["text"].as_str().unwrap_or("").to_string();
                        w.sends.push((addr, text));
                        json!({"jsonrpc": "2.0", "id": id, "result": {"sent": true}})
                    }
                    "terminal.keys" => {
                        let addr = req["params"]["terminal"].as_str().unwrap_or("").to_string();
                        let k = req["params"]["keys"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v.as_str())
                                    .collect::<Vec<_>>()
                                    .join(",")
                            })
                            .unwrap_or_default();
                        w.keys.push((addr, k));
                        json!({"jsonrpc": "2.0", "id": id, "result": {}})
                    }
                    _ => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
                }
            };
            let bytes = serde_json::to_vec(&reply).unwrap();
            write
                .write_all(&Frame::data(0, bytes).encode())
                .await
                .unwrap();
        }
    });
    let (r, w) = tokio::io::split(client_side);
    Client::open(Box::pin(r), Box::pin(w), None).await.unwrap()
}

fn fake_runner_bin() -> String {
    format!("{}/../../tools/fake-runner.sh", env!("CARGO_MANIFEST_DIR"))
}

fn live_from(w: &FakeWorld) -> (Vec<LiveTerminal>, Vec<LiveTerminal>) {
    let conv = |items: &[Value]| -> Vec<LiveTerminal> {
        items
            .iter()
            .map(|v| LiveTerminal {
                address: v["address"].as_str().unwrap().into(),
                name: v["name"].as_str().unwrap().into(),
                labels: serde_json::from_value(v["labels"].clone()).unwrap_or_default(),
                argv: serde_json::from_value(v["argv"].clone()).unwrap_or_default(),
                cwd: v["cwd"].as_str().unwrap_or("").into(),
                status: None,
                pid: None,
                ..Default::default()
            })
            .collect()
    };
    (conv(&w.live), conv(&w.finished))
}

fn write_cfg(cfg_home: &std::path::Path, runner: &str, model: &str, effort: Option<&str>) {
    let dir = cfg_home.join("backlot");
    fs::create_dir_all(&dir).unwrap();
    let mut text = format!(
        "[hollywood]\nstart = \"expanded\"\nkey = \"alt+y\"\nrunner = \"{runner}\"\nmodel = \"{model}\"\n"
    );
    if let Some(e) = effort {
        text.push_str(&format!("effort = \"{e}\"\n"));
    }
    fs::write(dir.join("config.toml"), text).unwrap();
}

#[test]
fn active_terminal_resolver_and_candidate_naming_avoid_previous_and_handoff() {
    let mut role = backlot_model::hollywood::HollywoodState::default().role;
    let terms = vec![
        LiveTerminal {
            address: "t-old".into(),
            name: "bl-hollywood".into(),
            labels: BTreeMap::from([("backlot.role".into(), "hollywood-previous".into())]),
            ..Default::default()
        },
        LiveTerminal {
            address: "t-cand".into(),
            name: "bl-hollywood-next".into(),
            labels: BTreeMap::from([("backlot.role".into(), "hollywood-handoff".into())]),
            ..Default::default()
        },
        LiveTerminal {
            address: "t-active".into(),
            name: "bl-hollywood-g1".into(),
            labels: BTreeMap::from([("backlot.role".into(), "hollywood".into())]),
            ..Default::default()
        },
    ];
    // Unique backlot.role=hollywood wins over name fallback
    assert_eq!(
        active_hollywood_terminal(&role, &terms).map(|t| t.address.as_str()),
        Some("t-active")
    );
    // Explicit role.terminal wins
    role.terminal = Some("t-active".into());
    assert_eq!(
        active_hollywood_terminal(&role, &terms).map(|t| t.address.as_str()),
        Some("t-active")
    );

    let used: std::collections::BTreeSet<String> = terms.iter().map(|t| t.name.clone()).collect();
    assert_eq!(next_candidate_name(&used, 2), "bl-hollywood-g2");

    let empty = std::collections::BTreeSet::new();
    assert_eq!(next_candidate_name(&empty, 1), "bl-hollywood");

    let mut only_main = std::collections::BTreeSet::new();
    only_main.insert("bl-hollywood".into());
    assert_eq!(next_candidate_name(&only_main, 1), "bl-hollywood-next");

    assert_eq!(
        auth_error_detail(
            &LiveTerminal::default(),
            "Error: not signed in — run login",
            Work::Waiting
        )
        .as_deref(),
        Some("Error: not signed in — run login")
    );
    assert!(auth_error_detail(&LiveTerminal::default(), "all good", Work::Working).is_none());
}

#[tokio::test]
async fn handoff_commits_on_strong_proof_drains_queued_tells_and_retains_previous_for_10m() {
    let tmp = TestDir::new("bl-handoff-commit");
    let cfg_home = tmp.path().join("cfg");
    write_cfg(&cfg_home, "fake", "model-old", Some("high"));
    let hdir = store::hollywood_dir(tmp.path());
    fs::create_dir_all(&hdir).unwrap();
    fs::write(hdir.join("memory.md"), "Keep focus on acme-shop.\n").unwrap();

    // Seed live Hollywood on t-old + a queued user tell + quota screen
    let actor_old = ActorIdentity {
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
                    actor: Some(actor_old),
                    session_id: Some("sess-old".into()),
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    store::append_hollywood(&hdir, |_, _| {
        Ok::<_, ()>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role("user"),
                HollywoodKind::MessageSent {
                    to: HOLLYWOOD.into(),
                    studio: None,
                    text: Some("summarize acme-shop after switch".into()),
                    file: None,
                    source_seq: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();

    let world = Arc::new(Mutex::new(FakeWorld {
        live: vec![json!({
            "address": "t-old",
            "name": HOLLYWOOD_TERMINAL,
            "labels": {"backlot.role": "hollywood", "backlot.space": "hollywood"},
            "argv": ["fake"],
            "cwd": hdir.to_string_lossy(),
            "state": "live",
        })],
        screens: BTreeMap::from([(
            "t-old".into(),
            "You've hit your weekly limit · resets Nov 30\n> ".into(),
        )]),
        ..Default::default()
    }));
    let client = fake_core_client(world.clone()).await;
    let mut env = BTreeMap::new();
    env.insert(
        "XDG_CONFIG_HOME".into(),
        cfg_home.to_string_lossy().into_owned(),
    );
    env.insert("BACKLOT_FAKE_RUNNER".into(), fake_runner_bin());
    let mut stage = Stage::for_test(client);
    stage.base = tmp.path().to_path_buf();
    stage.state_dir = tmp.path().to_string_lossy().into_owned();
    stage.env_vars = env;
    let mut slot = true;

    let t0 = crate::live::now_ms();
    // Tick 1 (pre-switch): quota banner blocks delivery and summary exposes OutOfQuota
    let (live, fin) = live_from(&world.lock().unwrap());
    stage.hollywood(&live, &fin, &mut slot, t0).await.unwrap();
    assert!(
        world.lock().unwrap().sends.is_empty(),
        "queued tell must wait while out of quota"
    );
    let sum = stage.summary_view_at(&BTreeMap::new(), t0);
    let hw_sum = sum.hollywood.unwrap();
    assert_eq!(
        hw_sum.queue.as_ref().map(|q| q.reason),
        Some(QueueReason::OutOfQuota)
    );
    assert_eq!(hw_sum.attention.as_deref(), Some("out of quota"));

    // Request switch to fake / model-new
    let staged = stage_handoff_request(
        &hdir,
        "fake",
        "model-new",
        Some("low"),
        90_000,
        false,
        "user switch",
        None,
        t0 + 1_000,
    )
    .unwrap();
    assert_eq!(staged.phase, HandoffPhase::Preparing);

    // Tick 2: launches candidate beside old terminal; config.toml untouched; old stays alive
    let (live, fin) = live_from(&world.lock().unwrap());
    stage
        .hollywood(&live, &fin, &mut slot, t0 + 1_100)
        .await
        .unwrap();
    let cfg_mid = fs::read_to_string(cfg_home.join("backlot/config.toml")).unwrap();
    assert!(
        cfg_mid.contains("model-old"),
        "config.toml untouched during probation: {cfg_mid}"
    );
    let cand = load_handoff(&hdir).unwrap().unwrap();
    assert_eq!(cand.phase, HandoffPhase::Verifying);
    let cand_addr = cand.candidate_terminal.clone().expect("candidate addr");
    assert!(hdir.join(format!("handoffs/{}.md", cand.id)).is_file());
    assert_eq!(world.lock().unwrap().live.len(), 2);

    // Stray report from OLD terminal must NOT falsely commit!
    store::append_hollywood(&hdir, |_, _| {
        Ok::<_, ()>(HPending {
            event: NewHollywoodEvent::new(
                EventBy {
                    role: HOLLYWOOD.into(),
                    session: Some("t-old".into()),
                    via: None,
                },
                HollywoodKind::Reported {
                    headline: "old stale note".into(),
                    body: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    let (live, fin) = live_from(&world.lock().unwrap());
    stage
        .hollywood(&live, &fin, &mut slot, t0 + 2_000)
        .await
        .unwrap();
    assert_eq!(
        load_handoff(&hdir).unwrap().unwrap().phase,
        HandoffPhase::Verifying
    );

    // Candidate writes hollywood.reported with candidate session/handoff identity -> commits!
    store::append_hollywood(&hdir, |_, _| {
        Ok::<_, ()>(HPending {
            event: NewHollywoodEvent::new(
                EventBy {
                    role: HOLLYWOOD.into(),
                    session: Some(cand_addr.clone()),
                    via: None,
                },
                HollywoodKind::Reported {
                    headline: "Handoff ready — switched to fake/model-new".into(),
                    body: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    let (live, fin) = live_from(&world.lock().unwrap());
    stage
        .hollywood(&live, &fin, &mut slot, t0 + 3_000)
        .await
        .unwrap();
    let committed = load_handoff(&hdir).unwrap().unwrap();
    assert_eq!(
        committed.phase,
        HandoffPhase::Committed,
        "handoff failed: {:?}",
        committed.error
    );
    let cfg_after = fs::read_to_string(cfg_home.join("backlot/config.toml")).unwrap();
    assert!(cfg_after.contains("model-new"), "{cfg_after}");
    assert!(cfg_after.contains("start = \"expanded\""), "{cfg_after}");
    assert!(cfg_after.contains("key = \"alt+y\""), "{cfg_after}");
    assert!(world.lock().unwrap().kills.contains(&"t-old".to_string()));
    assert!(
        world.lock().unwrap().rms.is_empty(),
        "old retained read-only as previous for 10m"
    );

    // Next tick before 10m keeps previous; candidate settles quiet and receives queued user tell
    world
        .lock()
        .unwrap()
        .screens
        .insert(cand_addr.clone(), "> ".into());
    for ts in [t0 + 4_000, t0 + 9_500, t0 + 12_500] {
        let (live, fin) = live_from(&world.lock().unwrap());
        stage.hollywood(&live, &fin, &mut slot, ts).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(
        world.lock().unwrap().rms.is_empty(),
        "still before 10m grace"
    );
    assert!(
        world
            .lock()
            .unwrap()
            .sends
            .iter()
            .any(|(a, txt)| a == &cand_addr && txt.contains("summarize acme-shop after switch")),
        "queued tell drained to new terminal: {:?}",
        world.lock().unwrap().sends
    );

    // Past 10m grace -> previous is swept via terminal.rm
    let (live, fin) = live_from(&world.lock().unwrap());
    stage
        .hollywood(
            &live,
            &fin,
            &mut slot,
            t0 + 3_000 + PREVIOUS_RETENTION_MS + 1,
        )
        .await
        .unwrap();
    assert!(world.lock().unwrap().rms.contains(&"t-old".to_string()));
}

#[tokio::test]
async fn handoff_rolls_back_on_quota_exit_timeout_cancel_and_pauses_clock_on_approval() {
    let tmp = TestDir::new("bl-handoff-rb");
    let cfg_home = tmp.path().join("cfg");
    write_cfg(&cfg_home, "fake", "model-old", Some("high"));
    let hdir = store::hollywood_dir(tmp.path());
    fs::create_dir_all(&hdir).unwrap();
    let actor_old = ActorIdentity {
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
                    actor: Some(actor_old),
                    session_id: Some("sess-old".into()),
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();

    let world = Arc::new(Mutex::new(FakeWorld {
        live: vec![json!({
            "address": "t-old",
            "name": HOLLYWOOD_TERMINAL,
            "labels": {"backlot.role": "hollywood"},
            "argv": ["fake"],
            "cwd": hdir.to_string_lossy(),
            "state": "live",
        })],
        ..Default::default()
    }));
    let client = fake_core_client(world.clone()).await;
    let mut env = BTreeMap::new();
    env.insert(
        "XDG_CONFIG_HOME".into(),
        cfg_home.to_string_lossy().into_owned(),
    );
    env.insert("BACKLOT_FAKE_RUNNER".into(), fake_runner_bin());
    let mut stage = Stage::for_test(client);
    stage.base = tmp.path().to_path_buf();
    stage.state_dir = tmp.path().to_string_lossy().into_owned();
    stage.env_vars = env;
    let mut slot = true;

    // Case 1: quota error on candidate -> immediate rollback, old & config untouched
    stage_handoff_request(
        &hdir,
        "fake",
        "model-bad",
        Some("low"),
        10_000,
        false,
        "user switch",
        None,
        1000,
    )
    .unwrap();
    let (live, fin) = live_from(&world.lock().unwrap());
    stage.hollywood(&live, &fin, &mut slot, 1100).await.unwrap();
    let cand1 = load_handoff(&hdir)
        .unwrap()
        .unwrap()
        .candidate_terminal
        .unwrap();
    world.lock().unwrap().screens.insert(
        cand1.clone(),
        "rate limit reached (resets 18:00)\n> ".into(),
    );
    let (live, fin) = live_from(&world.lock().unwrap());
    stage.hollywood(&live, &fin, &mut slot, 1200).await.unwrap();
    let rb = load_handoff(&hdir).unwrap().unwrap();
    assert_eq!(rb.phase, HandoffPhase::RolledBack);
    assert!(world.lock().unwrap().rms.contains(&cand1));
    assert!(
        fs::read_to_string(cfg_home.join("backlot/config.toml"))
            .unwrap()
            .contains("model-old")
    );

    // Case 2: cancel before timeout
    stage_handoff_request(
        &hdir,
        "fake",
        "model-c",
        Some("low"),
        10_000,
        false,
        "user switch",
        None,
        2000,
    )
    .unwrap();
    let (live, fin) = live_from(&world.lock().unwrap());
    stage.hollywood(&live, &fin, &mut slot, 2100).await.unwrap();
    request_cancel(&hdir).unwrap();
    let (live, fin) = live_from(&world.lock().unwrap());
    stage.hollywood(&live, &fin, &mut slot, 2200).await.unwrap();
    assert_eq!(
        load_handoff(&hdir).unwrap().unwrap().phase,
        HandoffPhase::RolledBack
    );

    // Case 3: NeedsApproval pauses clock, routes approval key to candidate, resumes to clean Working->Waiting commit
    stage_handoff_request(
        &hdir,
        "fake",
        "model-ok",
        Some("low"),
        5_000,
        false,
        "user switch",
        None,
        3000,
    )
    .unwrap();
    let (live, fin) = live_from(&world.lock().unwrap());
    stage.hollywood(&live, &fin, &mut slot, 3100).await.unwrap();
    let cand3 = load_handoff(&hdir)
        .unwrap()
        .unwrap()
        .candidate_terminal
        .unwrap();
    // Simulated permission prompt with ❯ cursor so work_state resolves Blocked
    world.lock().unwrap().screens.insert(
        cand3.clone(),
        "Allow bash command?\n❯ 1. Yes\n  2. No\n".into(),
    );
    let (live, fin) = live_from(&world.lock().unwrap());
    stage.hollywood(&live, &fin, &mut slot, 3200).await.unwrap();
    // Advance clock well past 5s timeout while still in NeedsApproval: must NOT time out
    let (live, fin) = live_from(&world.lock().unwrap());
    stage
        .hollywood(&live, &fin, &mut slot, 20_000)
        .await
        .unwrap();
    let st = load_handoff(&hdir).unwrap().unwrap();
    assert_eq!(st.phase, HandoffPhase::NeedsApproval);
    assert!(
        st.active_elapsed_ms < 5_000,
        "clock paused during NeedsApproval: {:?}",
        st
    );

    // User approves -> stage forwards key ("enter") to CANDIDATE terminal (not old t-old)
    store::append_hollywood(&hdir, |_, _| {
        Ok::<_, ()>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role("user"),
                HollywoodKind::RoleApproval {
                    decision: ApprovalDecision::Allow,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    let (live, fin) = live_from(&world.lock().unwrap());
    stage
        .hollywood(&live, &fin, &mut slot, 20_100)
        .await
        .unwrap();
    assert!(
        world
            .lock()
            .unwrap()
            .keys
            .iter()
            .any(|(a, k)| a == &cand3 && k == "enter"),
        "approval key must go to candidate: {:?}",
        world.lock().unwrap().keys
    );
    // Candidate clears prompt, works 2.5s cleanly, settles to waiting -> fallback commit!
    world
        .lock()
        .unwrap()
        .screens
        .insert(cand3.clone(), "fake-work: working\n".into());
    let (live, fin) = live_from(&world.lock().unwrap());
    stage
        .hollywood(&live, &fin, &mut slot, 20_500)
        .await
        .unwrap();
    let (live, fin) = live_from(&world.lock().unwrap());
    stage
        .hollywood(&live, &fin, &mut slot, 23_000)
        .await
        .unwrap();
    world
        .lock()
        .unwrap()
        .screens
        .insert(cand3.clone(), "ready\n".into());
    let (live, fin) = live_from(&world.lock().unwrap());
    stage
        .hollywood(&live, &fin, &mut slot, 23_500)
        .await
        .unwrap();
    assert_eq!(
        load_handoff(&hdir).unwrap().unwrap().phase,
        HandoffPhase::Committed
    );
    let _ = (
        CardAction::Pause {
            studio: "s".into(),
            task: "t".into(),
        },
        CardBase::StudioAbsent,
        CardState::Open,
    );
}

#[tokio::test]
async fn card_action_setup_runs_make_before_break_when_live_and_records_done_or_failed() {
    let tmp = TestDir::new("bl-card-setup-mb");
    let cfg_home = tmp.path().join("cfg");
    write_cfg(&cfg_home, "fake", "model-old", Some("high"));
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
                    actor: Some(old_actor.clone()),
                    session_id: Some("sess-old".into()),
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    let proposed = store::append_hollywood(&hdir, |_, _| {
        Ok::<_, ()>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role(HOLLYWOOD),
                HollywoodKind::CardProposed {
                    action: CardAction::Setup {
                        runner: "fake".into(),
                        model: "model-card".into(),
                        effort: Some("low".into()),
                        permission_profile: None,
                    },
                    why: "upgrade".into(),
                    base: CardBase::HollywoodIdentity {
                        actor: Some(old_actor),
                    },
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    store::append_hollywood(&hdir, |_, _| {
        Ok::<_, ()>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role("user"),
                HollywoodKind::CardApplied { card: proposed.seq },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();

    let world = Arc::new(Mutex::new(FakeWorld {
        live: vec![json!({
            "address": "t-old",
            "name": HOLLYWOOD_TERMINAL,
            "labels": {"backlot.role": "hollywood"},
            "argv": ["fake"],
            "cwd": hdir.to_string_lossy(),
            "state": "live",
        })],
        ..Default::default()
    }));
    let client = fake_core_client(world.clone()).await;
    let mut env = BTreeMap::new();
    env.insert(
        "XDG_CONFIG_HOME".into(),
        cfg_home.to_string_lossy().into_owned(),
    );
    env.insert("BACKLOT_FAKE_RUNNER".into(), fake_runner_bin());
    let mut stage = Stage::for_test(client);
    stage.base = tmp.path().to_path_buf();
    stage.state_dir = tmp.path().to_string_lossy().into_owned();
    stage.env_vars = env;
    let mut slot = true;

    let (live, fin) = live_from(&world.lock().unwrap());
    stage
        .hollywood(&live, &fin, &mut slot, 10_000)
        .await
        .unwrap();
    // Card should still be Applied while handoff verifies!
    let hs = store::load_hstate(&hdir).unwrap();
    assert_eq!(
        hs.cards[&proposed.seq].state,
        CardState::Applied,
        "card state failed: {:?}",
        hs.cards[&proposed.seq].detail
    );
    let cand = load_handoff(&hdir)
        .unwrap()
        .unwrap()
        .candidate_terminal
        .unwrap();

    // Candidate confirms readiness -> card becomes Done and config updates
    store::append_hollywood(&hdir, |_, _| {
        Ok::<_, ()>(HPending {
            event: NewHollywoodEvent::new(
                EventBy {
                    role: HOLLYWOOD.into(),
                    session: Some(cand),
                    via: None,
                },
                HollywoodKind::Reported {
                    headline: "ready".into(),
                    body: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    let (live, fin) = live_from(&world.lock().unwrap());
    stage
        .hollywood(&live, &fin, &mut slot, 11_000)
        .await
        .unwrap();
    let hs2 = store::load_hstate(&hdir).unwrap();
    assert_eq!(hs2.cards[&proposed.seq].state, CardState::Done);
    let _ = HandoffRecord::default;
}

#[test]
fn hollywood_auto_continue_waits_for_unclamped_reset_before_out_of_quota() {
    let mut rate_pace = None;
    let mut conn_pace = None;
    let screen = "Session limit reached · resets 2:10pm\nContinuing automatically at 2:10pm\n";
    let term = crate::reconcile::LiveTerminal::default();

    let ts = ((86_400_000u64 * 20_000) / 1000) as libc::time_t;
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&ts, &mut local) };
    local.tm_hour = 9;
    local.tm_min = 10;
    local.tm_sec = 0;
    local.tm_isdst = -1;
    let t0 = (unsafe { libc::mktime(&mut local) } as u64) * 1000;

    let out0 = crate::handoff_term::sample_hollywood_error(
        &term,
        screen,
        Work::Waiting,
        &mut rate_pace,
        &mut conn_pace,
        t0,
    );
    assert!(matches!(out0.blocked, Some(crate::handoff::HBlockedKind::RateLimited { .. })), "{out0:?}");
    assert_eq!(out0.retry_send, None);
    assert_eq!(out0.quota_hit.unwrap().reset_at_ms, Some(t0 + 5 * 3_600_000));

    // Past the 1 h retry clamp (10:15): still waiting (RateLimited), not OutOfQuota yet.
    let out1 = crate::handoff_term::sample_hollywood_error(
        &term,
        screen,
        Work::Waiting,
        &mut rate_pace,
        &mut conn_pace,
        t0 + 65 * 60_000,
    );
    assert!(matches!(out1.blocked, Some(crate::handoff::HBlockedKind::RateLimited { .. })), "{out1:?}");

    // At 14:11 (past true 5h reset): escalates to OutOfQuota.
    let out2 = crate::handoff_term::sample_hollywood_error(
        &term,
        screen,
        Work::Waiting,
        &mut rate_pace,
        &mut conn_pace,
        t0 + 5 * 3_600_000 + 60_000,
    );
    assert!(matches!(out2.blocked, Some(crate::handoff::HBlockedKind::OutOfQuota { .. })), "{out2:?}");
}
