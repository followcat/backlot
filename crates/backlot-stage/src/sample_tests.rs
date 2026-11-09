//! The D9 episode pass end to end on a duplex fake core: a second
//! permission prompt right after an answered one — no unblocked
//! sample between — starts its own episode and raises its own
//! attention once it has held the window, never sooner.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use backlot_model::event::{ApprovalDecision, EventKind, PERMISSION_PROMPT};
use backlot_model::state::StudioState;
use backlot_model::{EventBy, NewEvent};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, DuplexStream};

use crate::daemon::Stage;
use crate::reconcile::LiveTerminal;
use crate::sample::Watched;
use crate::store::{self, Pending};

static NEXT: AtomicU64 = AtomicU64::new(0);

fn tmp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "backlot-sample-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The fake core's memory: the screen `terminal.read` returns and
/// the keys `terminal.keys` sent it.
#[derive(Default)]
struct FakeCore {
    screen: String,
    keys: Vec<String>,
}

/// Serve `hello`, then `terminal.read` from the shared screen and
/// `terminal.keys` into the shared list. Ends on a closed wire.
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

fn term() -> LiveTerminal {
    LiveTerminal {
        name: "bl-acme-shop-t1".into(),
        address: "t1".into(),
        argv: Vec::new(),
        cwd: "/home/user/acme-shop".into(),
        labels: BTreeMap::new(),
    }
}

/// One `sample_work` on the freshest ledger state — the tick's own
/// reload between samples.
async fn sample(stage: &Stage, dir: &Path, term: &LiveTerminal, role: &str, now_ms: u64) {
    let state = store::load_state(dir).unwrap();
    let watched = Watched {
        dir,
        state: &state,
        role,
        runner: "fake",
        term,
    };
    stage
        .sample_work("acme-shop", &watched, now_ms)
        .await
        .unwrap();
}

fn events(dir: &Path, kind: &str) -> Vec<backlot_model::event::Event> {
    store::load_events(dir)
        .unwrap()
        .0
        .into_iter()
        .filter(|e| e.kind.name() == kind)
        .collect()
}

/// A `role.approval` for `role` the way `backlot approve` records it.
fn approve(dir: &Path, role: &str, by: &str, decision: ApprovalDecision) {
    let (role, by) = (role.to_string(), by.to_string());
    store::append(dir, move |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role(by),
                EventKind::RoleApproval { role, decision },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
}

fn studio(dir: &Path) {
    store::append(dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("user"),
                EventKind::StudioCreated {
                    title: "acme-shop".into(),
                    repo: "/home/user/acme-shop".into(),
                    mode: backlot_model::event::StudioMode::Standard,
                    roles: Box::new(backlot_model::event::RoleConfigs {
                        producer: actor(),
                        director: actor(),
                        writer: actor(),
                    }),
                    candidates: Vec::new(),
                    rework_cap: 2,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
}

fn actor() -> backlot_model::event::ActorIdentity {
    backlot_model::event::ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: "plain".into(),
        effort: "high".into(),
        permission_profile: "standard".into(),
    }
}

fn open_attentions(state: &StudioState, role: &str) -> usize {
    state
        .open_attention()
        .filter(|a| a.role == role && a.reason == PERMISSION_PROMPT)
        .count()
}

/// A role shows a blocked screen the whole time — a new prompt the
/// sample after its predecessor was answered, or the same prompt
/// still waiting: the stage cannot tell the two apart, and must
/// not. The sent key clears the attention *and* the episode's
/// start, so the still-blocked screen that follows begins a fresh
/// count and raises its own attention at the window — never
/// sooner, and the spent decision sends no second key.
#[tokio::test]
async fn a_prompt_after_an_answered_one_raises_its_own_attention() {
    let dir = tmp();
    studio(&dir);
    let term = term();
    let (stage, fake) = stage().await;
    fake.lock().unwrap().screen =
        "fake runner: actor-t1-c1 of acme-shop\nfake-work: blocked\n".into();
    let t0 = now_ms();

    // The first episode: two blocked samples a window apart raise
    // the first attention.
    sample(&stage, &dir, &term, "actor-t1-c1", t0).await;
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 1_000).await;
    assert!(events(&dir, "attention.raised").is_empty());
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 2_000).await;
    assert_eq!(events(&dir, "attention.raised").len(), 1);

    // The recorded allow answers it: `enter` goes out and the
    // attention clears — the episode is over whatever the screen
    // shows next.
    approve(&dir, "actor-t1-c1", "director", ApprovalDecision::Allow);
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 3_000).await;
    assert_eq!(fake.lock().unwrap().keys.as_slice(), ["enter"]);
    assert_eq!(events(&dir, "attention.cleared").len(), 1);

    // The screen is still blocked — a second prompt, or the first
    // one not yet gone. It is its own episode: no new attention
    // until it has held the full window.
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 4_000).await;
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 5_000).await;
    assert_eq!(
        events(&dir, "attention.raised").len(),
        1,
        "the still-blocked screen raised before its own window"
    );
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 6_000).await;
    assert_eq!(
        events(&dir, "attention.raised").len(),
        2,
        "the still-blocked screen never raised its own attention"
    );

    // The spent `allow` answers nothing new: no second key went
    // out, and the fresh attention is the open one.
    assert_eq!(fake.lock().unwrap().keys.as_slice(), ["enter"]);
    let state = store::load_state(&dir).unwrap();
    assert_eq!(open_attentions(&state, "actor-t1-c1"), 1);
}
