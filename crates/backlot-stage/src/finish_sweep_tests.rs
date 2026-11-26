//! The finish sweep through a tick: a finished studio's leftover
//! `finished` records get their `terminal.rm` from the stage — the
//! client's own reaper never touches a `backlot.*` terminal — and a
//! removal the core refuses is one clean pass, not a studio error.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use backlot_model::event::EventKind;
use backlot_model::{EventBy, NewEvent};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, DuplexStream};

use crate::daemon::{Inventory, Stage};
use crate::reconcile::LiveTerminal;
use crate::store::{self, Pending};
use crate::test_support::TestDir;

fn tmp() -> TestDir {
    TestDir::new("backlot-finish-sweep-test")
}

/// What the fake core answers to `terminal.rm`.
#[derive(Clone, Copy, Default, PartialEq)]
enum Rm {
    /// The record goes.
    #[default]
    Removed,
    /// A core still counting the program live refuses it.
    Live,
    /// The record is already gone.
    Gone,
}

/// The fake core's memory: every request it answered, what `rm` says,
/// and whether the core offers `terminal.rm` at all.
#[derive(Default)]
struct FakeCore {
    calls: Vec<(String, Value)>,
    rm: Rm,
    offer_rm: bool,
}

async fn serve(stream: DuplexStream, fake: Arc<Mutex<FakeCore>>) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = Reader::new(read);
    let hello = reader.read_line().await.unwrap().unwrap();
    let request: Value = serde_json::from_slice(&hello).unwrap();
    assert_eq!(request["method"], "hello");
    let mut methods = vec![
        "terminal.list",
        "terminal.kill",
        "terminal.label",
        "terminal.read",
        "terminal.send",
    ];
    if fake.lock().unwrap().offer_rm {
        methods.push("terminal.rm");
    }
    write
        .write_all(
            serde_json::to_string(&json!({
                "jsonrpc": "2.0", "id": 0,
                "result": {
                    "generation": 1, "version": "fake-core 0.0",
                    "framed": true,
                    "core": {"id": "core-fake-1", "name": "fake-core"},
                    "methods": methods,
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
        let request: Value = serde_json::from_slice(&frame.payload).unwrap();
        let method = request["method"].as_str().unwrap_or("").to_string();
        let params = request["params"].clone();
        let rm = {
            let mut fake = fake.lock().unwrap();
            fake.calls.push((method.clone(), params));
            fake.rm
        };
        let refusal = match rm {
            Rm::Removed => None,
            Rm::Live => Some(json!({"kind": "live"})),
            Rm::Gone => Some(json!({"kind": "not_found"})),
        };
        let reply = if let (true, Some(data)) = (method == "terminal.rm", refusal) {
            json!({
                "jsonrpc": "2.0", "id": request["id"],
                "error": {"code": -32000, "message": "refused", "data": data},
            })
        } else {
            let result = match method.as_str() {
                "terminal.list" => json!({"terminals": []}),
                _ => json!({}),
            };
            json!({"jsonrpc": "2.0", "id": request["id"], "result": result})
        };
        write
            .write_all(&Frame::data(0, serde_json::to_vec(&reply).unwrap()).encode())
            .await
            .unwrap();
    }
}

async fn stage_on(rm: Rm, offer_rm: bool) -> (Stage, Arc<Mutex<FakeCore>>) {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    let fake = Arc::new(Mutex::new(FakeCore {
        rm,
        offer_rm,
        ..Default::default()
    }));
    tokio::spawn(serve(core_side, Arc::clone(&fake)));
    let (read, write) = tokio::io::split(client_side);
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    (Stage::for_test(client), fake)
}

/// A core offering `terminal.rm`, answering `rm` with whatever the
/// caller chose.
async fn stage(rm: Rm) -> (Stage, Arc<Mutex<FakeCore>>) {
    stage_on(rm, true).await
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

/// `studios/<name>` with a studio, one abandoned task and
/// `studio.finished` — the state a finish leaves.
fn finished_studio(base: &std::path::Path, name: &str) -> PathBuf {
    let dir = store::studio_dir(base, name);
    fs::create_dir_all(&dir).unwrap();
    let repo = dir.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let kinds = vec![
        (
            "user",
            EventKind::StudioCreated {
                title: name.into(),
                repo: repo.to_string_lossy().into_owned(),
                members: Vec::new(),
                no_git: false,
                mode: backlot_model::event::StudioMode::Standard,
                roles: Box::new(backlot_model::event::RoleConfigs {
                    producer: actor(),
                    director: actor(),
                    writer: actor(),
                }),
                candidates: Vec::new(),
                rework_cap: 2,
                stall_limit_s: 600,
                nudge_after_s: 180,
                escalate_after_s: 600,
                check_env: Vec::new(),
                checks: Vec::new(),
                task_budget_s: None,
                limits: Default::default(),
                max_actors: backlot_model::event::default_max_actors(),
                push: Default::default(),
                design_gate: backlot_model::event::DesignGate::Off,
            },
        ),
        (
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "first".into(),
            },
        ),
        (
            "director",
            EventKind::TaskCreated {
                task: "t1".into(),
                title: "front".into(),
                task_kind: "feature".into(),
                requirement_version: 1,
                acceptance: "plan/a.md".into(),
                dependencies: Vec::new(),
            },
        ),
        (
            "director",
            EventKind::TaskState {
                task: "t1".into(),
                state: backlot_model::event::TaskStateChange::Abandoned,
                reason: "cut".into(),
            },
        ),
        (
            "producer",
            EventKind::StudioFinished {
                reason: "accepted".into(),
            },
        ),
    ];
    for (role, kind) in kinds {
        store::append(&dir, move |_, _| {
            Ok::<_, std::convert::Infallible>(Pending {
                event: NewEvent::new(EventBy::role(role), kind),
                files: Vec::new(),
            })
        })
        .unwrap();
    }
    dir
}

/// A `finished` record with the labels it launched under.
fn dead(name: &str, address: &str, studio: &str, role: &str) -> LiveTerminal {
    LiveTerminal {
        name: name.into(),
        address: address.into(),
        labels: BTreeMap::from([
            ("backlot.studio".to_string(), studio.to_string()),
            ("backlot.role".to_string(), role.to_string()),
        ]),
        ..Default::default()
    }
}

fn rm_calls(fake: &Arc<Mutex<FakeCore>>) -> Vec<String> {
    fake.lock()
        .unwrap()
        .calls
        .iter()
        .filter(|(m, _)| m == "terminal.rm")
        .map(|(_, p)| p["terminal"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// A tick on a finished studio removes its own leftover record and
/// nothing else — another studio's record and a shell stay.
#[tokio::test]
async fn a_tick_removes_a_finished_studios_records() {
    let base = tmp();
    finished_studio(&base, "acme");
    let (mut stage, fake) = stage(Rm::Removed).await;
    stage.base = base.to_path_buf();
    let live = Inventory {
        terms: Vec::new(),
        finished: vec![
            dead("bl-acme-t1", "t7", "acme", "actor-t1-c1"),
            dead("bl-other-t1", "t8", "other", "actor-t1-c1"),
            dead("bl-acme-sh1", "t9", "acme", "shell"),
        ],
        doing: BTreeMap::new(),
    };
    // The shell carries the `backlot.shell` label a shell launch sets.
    let mut live = live;
    live.finished[2]
        .labels
        .insert("backlot.shell".into(), "1".into());

    stage.tick(&live).await.unwrap();
    assert_eq!(rm_calls(&fake), ["t7"]);
}

/// A removal the core refuses — the record it still counts live — is
/// a clean pass: no studio error, and the next tick tries again.
#[tokio::test]
async fn a_refused_removal_does_not_error_the_pass() {
    let base = tmp();
    finished_studio(&base, "acme");
    let (mut stage, fake) = stage(Rm::Live).await;
    stage.base = base.to_path_buf();
    let live = Inventory {
        terms: Vec::new(),
        finished: vec![dead("bl-acme-t1", "t7", "acme", "actor-t1-c1")],
        doing: BTreeMap::new(),
    };
    stage.tick(&live).await.unwrap();
    stage.tick(&live).await.unwrap();
    assert_eq!(rm_calls(&fake), ["t7", "t7"], "retried, never an error");
    assert!(
        stage.summary_view(&BTreeMap::new()).studios[0]
            .error
            .is_none(),
        "a refused removal is not the studio's error"
    );
}

/// An address the core already dropped answers `not_found` — the
/// sweep asked for a record another client removed first, and that is
/// no error; the next tick simply has nothing left to remove.
#[tokio::test]
async fn a_record_already_gone_is_clean() {
    let base = tmp();
    finished_studio(&base, "acme");
    let (mut stage, fake) = stage(Rm::Gone).await;
    stage.base = base.to_path_buf();
    let live = Inventory {
        terms: Vec::new(),
        finished: vec![dead("bl-acme-t1", "t7", "acme", "actor-t1-c1")],
        doing: BTreeMap::new(),
    };
    stage.tick(&live).await.unwrap();
    assert_eq!(rm_calls(&fake), ["t7"]);
    assert!(
        stage.summary_view(&BTreeMap::new()).studios[0]
            .error
            .is_none()
    );
}

/// A core that does not offer `terminal.rm` keeps the record and the
/// pass stays clean — the panes are as they were, and no studio error
/// appears for a method the core never had.
#[tokio::test]
async fn a_core_without_terminal_rm_leaves_the_record() {
    let base = tmp();
    finished_studio(&base, "acme");
    let (mut stage, fake) = stage(Rm::Removed).await;
    stage.base = base.to_path_buf();
    let live = Inventory {
        terms: Vec::new(),
        finished: vec![dead("bl-acme-t1", "t7", "acme", "actor-t1-c1")],
        doing: BTreeMap::new(),
    };
    let (mut quiet, quiet_fake) = stage_on(Rm::Removed, false).await;
    quiet.base = base.to_path_buf();
    quiet.tick(&live).await.unwrap();
    assert!(rm_calls(&quiet_fake).is_empty(), "nothing is dialed");
    assert!(
        quiet.summary_view(&BTreeMap::new()).studios[0]
            .error
            .is_none(),
        "an unoffered method is not the studio's error"
    );

    // And the same fake with the method offered does dial it — the
    // test above is not passing for a broken stage.
    stage.tick(&live).await.unwrap();
    assert_eq!(rm_calls(&fake), ["t7"]);
}

/// Finished `bl-hollywood` tombstones (both named and unnamed with
/// `backlot.role == "hollywood"`) are removed via `terminal.rm` during
/// `tick` even when no studio has finished, while live terminals and
/// `backlot.shell` stay untouched.
#[tokio::test]
async fn a_tick_sweeps_finished_hollywood_tombstones() {
    let base = tmp();
    let (mut st, fake) = stage(Rm::Removed).await;
    st.base = base.to_path_buf();
    let live = Inventory {
        terms: vec![LiveTerminal {
            name: "bl-hollywood".into(),
            address: "t20".into(),
            labels: BTreeMap::from([("backlot.role".into(), "hollywood".into())]),
            ..Default::default()
        }],
        finished: vec![
            LiveTerminal {
                name: "bl-hollywood".into(),
                address: "t10".into(),
                labels: BTreeMap::from([("backlot.role".into(), "hollywood".into())]),
                ..Default::default()
            },
            LiveTerminal {
                name: String::new(),
                address: "t11".into(),
                labels: BTreeMap::from([("backlot.role".into(), "hollywood".into())]),
                ..Default::default()
            },
            LiveTerminal {
                name: "bl-hollywood".into(),
                address: "t12".into(),
                labels: BTreeMap::from([
                    ("backlot.role".into(), "hollywood".into()),
                    ("backlot.shell".into(), "1".into()),
                ]),
                ..Default::default()
            },
        ],
        doing: BTreeMap::new(),
    };
    st.tick(&live).await.unwrap();
    assert_eq!(rm_calls(&fake), ["t10", "t11"]);

    // On RPC refusal (`live`), `sweep_hollywood_ticks` stays armed to retry.
    let (mut retry_stage, retry_fake) = stage(Rm::Live).await;
    retry_stage.base = base.to_path_buf();
    retry_stage.tick(&live).await.unwrap();
    retry_stage.tick(&live).await.unwrap();
    assert_eq!(rm_calls(&retry_fake), ["t10", "t11", "t10", "t11"]);
}
