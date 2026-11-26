//! The trust pass end to end on a duplex fake core: the answered
//! dialog is a `stage.answered` ledger event — never an attention —
//! a dialog outside the role's work area is a `needs_user`
//! attention and no keys, and the `[stage] answer_trust` switch
//! opts the whole pass out.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use backlot_model::event::{EventKind, FOLDER_TRUST};
use backlot_model::state::StudioState;
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, DuplexStream};

use crate::config::StageCfg;
use crate::daemon::Stage;
use crate::reconcile::LiveTerminal;
use crate::store::{self, Pending};
use crate::test_support::TestDir;
use crate::watch::{TRUST_REFUSED, TRUST_SETTLE_MS};

fn tmp() -> TestDir {
    TestDir::new("backlot-trust-test")
}

/// The fake core's memory: the screen `terminal.read` returns and
/// the keys `terminal.keys` sent it.
#[derive(Default)]
struct FakeCore {
    screen: String,
    keys: Vec<String>,
}

/// Serve `hello`, then `terminal.read` from the shared screen and
/// `terminal.keys` into the shared key list. Ends on a closed wire.
async fn serve(stream: DuplexStream, fake: Arc<Mutex<FakeCore>>) {
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
                    "generation": 1, "version": "fake-core 0.0",
                    "framed": true,
                    "core": {"id": "core-fake-1", "name": "fake-core"},
                    "methods": ["terminal.read", "terminal.keys"],
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
        let result = match request["method"].as_str().unwrap_or("") {
            "terminal.read" => json!({
                "text": fake.lock().unwrap().screen.clone(),
                "complete": true,
                "how": "screen",
            }),
            "terminal.keys" => {
                let keys = request["params"]["keys"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|k| k.as_str().unwrap().to_string())
                    .collect::<Vec<_>>();
                fake.lock().unwrap().keys.extend(keys);
                json!({})
            }
            other => panic!("the stage called an unexpected method: {other}"),
        };
        let reply = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": request["id"], "result": result,
        }))
        .unwrap();
        write
            .write_all(&Frame::data(0, reply).encode())
            .await
            .unwrap();
    }
}

/// A connected `(Stage, fake memory)` pair on a duplex wire.
async fn stage() -> (Stage, Arc<Mutex<FakeCore>>) {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    let fake = Arc::new(Mutex::new(FakeCore::default()));
    tokio::spawn(serve(core_side, Arc::clone(&fake)));
    let (read, write) = tokio::io::split(client_side);
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    (Stage::for_test(client), fake)
}

fn term(cwd: &str) -> LiveTerminal {
    LiveTerminal {
        name: "bl-acme-shop-producer".into(),
        address: "t1".into(),
        argv: Vec::new(),
        cwd: cwd.into(),
        labels: BTreeMap::new(),
        ..Default::default()
    }
}

/// The dialog with the cursor on the trust option — the screen an
/// `enter` accepts.
fn on_yes() -> String {
    "Quick safety check\nDo you trust the files in this folder?\n  No, exit\n❯ Yes, I trust this folder\n".into()
}

/// A studio ledger with one started role, so every event this test
/// appends validates.
fn seeded(dir: &Path) {
    use backlot_model::event::{RoleConfigs, StudioMode};
    let actor = |runner: &str| backlot_model::event::ActorIdentity {
        runner: runner.into(),
        runner_version: None,
        model: "m".into(),
        effort: None,
        permission_profile: "standard".into(),
    };
    store::append(dir, move |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: backlot_model::NewEvent::new(
                backlot_model::EventBy::role("user"),
                EventKind::StudioCreated {
                    title: "acme-shop".into(),
                    repo: "/home/user/acme-shop".into(),
                    members: Vec::new(),
                    no_git: false,
                    mode: StudioMode::Standard,
                    roles: Box::new(RoleConfigs {
                        producer: actor("claude"),
                        director: actor("claude"),
                        writer: actor("claude"),
                    }),
                    candidates: Vec::new(),
                    rework_cap: 2,
                    stall_limit_s: 600,
                    nudge_after_s: 180,
                    escalate_after_s: 600,
                    check_env: Vec::new(),
                    checks: Vec::new(),
                    task_budget_s: None,
                    limits: BTreeMap::new(),
                    max_actors: backlot_model::event::default_max_actors(),
                    push: Default::default(),
                    design_gate: backlot_model::event::DesignGate::Off,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    store::append(dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: backlot_model::NewEvent::new(
                backlot_model::EventBy::role("stage"),
                EventKind::RoleStarted {
                    role: "producer".into(),
                    terminal: Some("t1".into()),
                    actor: None,
                    session_id: None,
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
}

/// One `answer_trust` sighting on the freshest ledger state.
async fn answer(
    stage: &Stage,
    dir: &Path,
    term: &LiveTerminal,
    cwd: &str,
    now_ms: u64,
) -> Result<(), crate::cli::Fail> {
    let state: StudioState = store::load_state(dir).unwrap();
    stage
        .answer_trust(dir, "acme-shop", "producer", term, cwd, &state, now_ms)
        .await
}

fn events(dir: &Path, kind: &str) -> Vec<backlot_model::event::Event> {
    store::load_events(dir)
        .unwrap()
        .0
        .into_iter()
        .filter(|e| e.kind.name() == kind)
        .collect()
}

/// The answered dialog: the paced `enter` records `stage.answered`
/// with the dialog, key and work area — and never an attention. A
/// second settled sighting of the same screen sends no second
/// answer event's worth of keys without a gap, and the ledger ends
/// with exactly one event.
#[tokio::test]
async fn an_answered_dialog_is_a_stage_answered_event() {
    let dir = tmp();
    seeded(&dir);
    let (stage, fake) = stage().await;
    fake.lock().unwrap().screen = on_yes();
    let term = term("/home/user/acme-shop");

    // First sighting records the clock, sends nothing.
    answer(&stage, &dir, &term, "/home/user/acme-shop", 1_000)
        .await
        .unwrap();
    assert!(fake.lock().unwrap().keys.is_empty(), "an unsettled key");
    // At the settle the `enter` goes out and the answer records.
    answer(
        &stage,
        &dir,
        &term,
        "/home/user/acme-shop",
        1_000 + TRUST_SETTLE_MS,
    )
    .await
    .unwrap();
    assert_eq!(fake.lock().unwrap().keys, vec!["enter".to_string()]);

    let answered = events(&dir, "stage.answered");
    assert_eq!(answered.len(), 1, "{answered:?}");
    assert!(matches!(
        &answered[0].kind,
        EventKind::StageAnswered { role, dialog, key, cwd }
            if role == "producer"
                && dialog == FOLDER_TRUST
                && key == "enter"
                && cwd == "/home/user/acme-shop"
    ));
    // Not an attention: nothing raised, nothing to clear.
    assert!(
        events(&dir, "attention.raised").is_empty(),
        "{:?}",
        events(&dir, "attention.raised")
    );
}

/// The refusal: a dialog outside the role's own work area raises
/// one `needs_user` attention and sends no key — the stage does not
/// answer outside the repo — and never records a `stage.answered`.
#[tokio::test]
async fn a_dialog_outside_the_work_area_is_refused() {
    let dir = tmp();
    seeded(&dir);
    let (stage, fake) = stage().await;
    fake.lock().unwrap().screen = on_yes();
    let term = term("/home/user/somewhere-else");

    for now in [1_000, 1_000 + TRUST_SETTLE_MS, 1_000 + 2 * TRUST_SETTLE_MS] {
        answer(&stage, &dir, &term, "/home/user/acme-shop", now)
            .await
            .unwrap();
    }
    assert!(
        fake.lock().unwrap().keys.is_empty(),
        "a key went to a dialog the stage refuses"
    );
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1, "{raised:?}");
    assert!(matches!(
        &raised[0].kind,
        EventKind::AttentionRaised {
            role,
            reason,
            needs_user: true,
            ..
        } if role == "producer" && reason == TRUST_REFUSED
    ));
    assert!(events(&dir, "stage.answered").is_empty());
}

/// The switch: `[stage] answer_trust = false` opts the whole pass
/// out — no read, no key, no attention, no event.
#[tokio::test]
async fn the_switch_opts_the_whole_pass_out() {
    let dir = tmp();
    seeded(&dir);
    let (stage, fake) = stage().await;
    stage.stage_cfg.set(StageCfg {
        answer_trust: false,
    });
    fake.lock().unwrap().screen = on_yes();
    let term = term("/home/user/acme-shop");

    answer(
        &stage,
        &dir,
        &term,
        "/home/user/acme-shop",
        1_000 + TRUST_SETTLE_MS,
    )
    .await
    .unwrap();
    assert!(fake.lock().unwrap().keys.is_empty());
    assert!(events(&dir, "stage.answered").is_empty());
    assert!(events(&dir, "attention.raised").is_empty());
}
