//! Guard tests for `ask.answered`: stage terminal writes and lifecycle
//! events (`role.restart`, `role.recast`, `role.closed`, `role.started`,
//! `role.resumed`) must never clear an open `tell user`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use backlot_model::NewEvent;
use backlot_model::event::{
    ActorIdentity, ApprovalDecision, CheckDecl, EventBy, EventKind, RECOVERED, StudioMode,
};
use backlot_model::summary::{studio_exchange_files, summary_with_exchange};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use hc_proto::terminal::Status;
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, DuplexStream};

use crate::daemon::Stage;
use crate::reconcile::LiveTerminal;
use crate::sample::Watched;
use crate::signals::signals_path;
use crate::store::{self, Pending};
use crate::test_support::TestDir;
use crate::watch::TRUST_SETTLE_MS;

const CLAUDE_PROMPT: &str = "─────────────────────────\nBash command\nls\n╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌\nDo you want to proceed?\n❯ 1. Yes\n  2. No\n";
const CLAUDE_TRUST: &str = "Quick safety check\nDo you trust the files in this folder?\n  No, exit\n❯ Yes, I trust this folder\n";

fn tmp() -> TestDir {
    TestDir::new("backlot-ask-guards-test")
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Default)]
struct FakeCore {
    screen: String,
    keys: Vec<String>,
    sends: Vec<String>,
}

async fn serve(stream: DuplexStream, fake: Arc<Mutex<FakeCore>>) {
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = Reader::new(read);
    let _hello = reader.read_line().await.unwrap().unwrap();
    let hi = json!({
        "jsonrpc": "2.0", "id": 0,
        "result": {
            "generation": 1, "version": "fake-core 0.0", "framed": true,
            "core": {"id": "core-fake-1", "name": "fake-core"},
            "methods": ["terminal.read", "terminal.keys", "terminal.send"],
            "features": [],
        }
    });
    write.write_all(format!("{hi}\n").as_bytes()).await.unwrap();
    while let Ok(Some(frame)) = reader.read_frame().await {
        if frame.channel != 0 || frame.kind != Kind::Data {
            continue;
        }
        let req: Value = serde_json::from_slice(&frame.payload).unwrap();
        let res = match req["method"].as_str().unwrap_or("") {
            "terminal.read" => json!({
                "text": fake.lock().unwrap().screen.clone(),
                "complete": true, "how": "screen",
            }),
            "terminal.keys" => {
                let ks = req["params"]["keys"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|k| k.as_str().unwrap().into());
                fake.lock().unwrap().keys.extend(ks);
                json!({})
            }
            "terminal.send" => {
                fake.lock()
                    .unwrap()
                    .sends
                    .push(req["params"]["text"].as_str().unwrap_or("").into());
                json!({})
            }
            other => panic!("unexpected rpc method: {other}"),
        };
        let reply =
            serde_json::to_vec(&json!({"jsonrpc": "2.0", "id": req["id"], "result": res})).unwrap();
        write
            .write_all(&Frame::data(0, reply).encode())
            .await
            .unwrap();
    }
}

async fn stage() -> (Stage, Arc<Mutex<FakeCore>>) {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    let fake = Arc::new(Mutex::new(FakeCore::default()));
    tokio::spawn(serve(core_side, Arc::clone(&fake)));
    let (read, write) = tokio::io::split(client_side);
    (
        Stage::for_test(
            Client::open(Box::pin(read), Box::pin(write), None)
                .await
                .unwrap(),
        ),
        fake,
    )
}

fn actor(runner: &str, model: &str) -> ActorIdentity {
    ActorIdentity {
        runner: runner.into(),
        runner_version: None,
        model: model.into(),
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    }
}

fn append(dir: &Path, by: &str, kind: EventKind) -> backlot_model::Event {
    store::append(dir, move |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(EventBy::role(by), kind),
            files: Vec::new(),
        })
    })
    .unwrap()
}

async fn setup_ask(runner: &str) -> (TestDir, Stage, Arc<Mutex<FakeCore>>, LiveTerminal, u64) {
    let dir = tmp();
    append(
        &dir,
        "user",
        EventKind::StudioCreated {
            title: "acme-shop".into(),
            repo: "/home/user/acme-shop".into(),
            members: Vec::new(),
            no_git: false,
            mode: StudioMode::Standard,
            roles: Box::new(backlot_model::event::RoleConfigs {
                producer: actor(runner, "m"),
                director: actor(runner, "m"),
                writer: actor(runner, "m"),
            }),
            candidates: vec![backlot_model::event::Candidate {
                name: "alice".into(),
                runner: runner.into(),
                runner_version: None,
                model: "m".into(),
                effort: Some("high".into()),
                permission_profile: "standard".into(),
            }],
            rework_cap: 2,
            stall_limit_s: 600,
            nudge_after_s: 180,
            escalate_after_s: 600,
            check_env: Vec::new(),
            checks: Vec::<CheckDecl>::new(),
            task_budget_s: None,
            limits: BTreeMap::new(),
            max_actors: backlot_model::event::default_max_actors(),
            push: Default::default(),
            design_gate: backlot_model::event::DesignGate::Off,
        },
    );
    append(
        &dir,
        "stage",
        EventKind::RoleStarted {
            role: "producer".into(),
            terminal: Some("bl-acme-shop-producer".into()),
            actor: Some(actor(runner, "m")),
            session_id: Some("s-prod".into()),
            reason: None,
        },
    );
    let msg_dir = dir.join("messages");
    fs::create_dir_all(&msg_dir).unwrap();
    let scratch = msg_dir.join(".tmp-ask.md");
    fs::write(&scratch, "what should we build?").unwrap();
    store::append(&dir, move |_, seq| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("producer"),
                EventKind::MessageQueued {
                    to: "user".into(),
                    file: Some(format!("messages/{seq}-ask.md")),
                    purpose: Some("ask".into()),
                    source_seq: None,
                },
            ),
            files: vec![store::PendingFile {
                src: scratch,
                dest: format!("messages/{seq}-ask.md"),
                append: None,
            }],
        })
    })
    .unwrap();
    let (st, fake) = stage().await;
    let tm = LiveTerminal {
        name: "bl-acme-shop-producer".into(),
        address: "t-prod".into(),
        cwd: "/home/user/acme-shop".into(),
        ..Default::default()
    };
    (dir, st, fake, tm, now_ms())
}

fn emit_signal(dir: &Path, ts_ms: u64, event: &str, sid: &str) {
    let path = signals_path(dir, "producer");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let line = format!(
        "{}\n",
        json!({"ts_ms": ts_ms, "event": event, "session_id": sid})
    );
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    std::io::Write::write_all(&mut f, line.as_bytes()).unwrap();
}

async fn sample(st: &Stage, dir: &Path, tm: &LiveTerminal, runner: &str, now_ms: u64) {
    let state = store::load_state(dir).unwrap();
    let w = Watched {
        dir,
        state: &state,
        role: "producer",
        runner,
        term: tm,
    };
    st.sample_work("acme-shop", &w, now_ms).await.unwrap();
}

fn assert_still_asking(dir: &Path) {
    let state = store::load_state(dir).unwrap();
    assert!(state.ask_answers.is_empty(), "{:?}", state.ask_answers);
    let mut snippets = BTreeMap::new();
    for (seq, rel) in studio_exchange_files(&state) {
        if let Some(s) = crate::exchange_read::read_snippet(dir, &rel) {
            snippets.insert(("acme-shop".into(), seq), s);
        }
    }
    let studio = summary_with_exchange(
        &[("acme-shop".into(), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &snippets,
        1,
    )
    .studios
    .remove(0);
    let asks = studio
        .roles
        .iter()
        .find(|r| r.role == "producer")
        .and_then(|r| r.asks.as_deref());
    assert_eq!(asks, Some("what should we build?"));
}

#[tokio::test]
async fn stage_terminal_sends_after_ask_prevent_clearing() {
    // 1. Delivered notice/message.
    let (dir, st, fake, tm, t0) = setup_ask("claude").await;
    emit_signal(&dir, t0 + 10, "Stop", "s-prod");
    append(
        &dir,
        "stage",
        EventKind::MessageQueued {
            to: "producer".into(),
            file: Some("messages/notice.md".into()),
            purpose: Some("nudge".into()),
            source_seq: Some(1),
        },
    );
    fake.lock().unwrap().screen = "> \n".into();
    sample(&st, &dir, &tm, "claude", t0 + 50).await;
    sample(&st, &dir, &tm, "claude", t0 + 2_500).await;
    st.deliver(
        "acme-shop",
        &dir,
        &store::load_state(&dir).unwrap(),
        std::slice::from_ref(&tm),
        t0 + 6_000,
    )
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    st.deliver(
        "acme-shop",
        &dir,
        &store::load_state(&dir).unwrap(),
        std::slice::from_ref(&tm),
        t0 + 6_100,
    )
    .await
    .unwrap();
    assert!(!fake.lock().unwrap().sends.is_empty());
    emit_signal(&dir, t0 + 6_200, "UserPromptSubmit", "s-prod");
    sample(&st, &dir, &tm, "claude", t0 + 6_300).await;
    assert_still_asking(&dir);

    // 2. Permission prompt answer (`sent enter`).
    let (dir, st, fake, tm, t0) = setup_ask("claude").await;
    fake.lock().unwrap().screen = CLAUDE_PROMPT.into();
    sample(&st, &dir, &tm, "claude", t0).await;
    sample(&st, &dir, &tm, "claude", t0 + 2_000).await;
    append(
        &dir,
        "user",
        EventKind::RoleApproval {
            role: "producer".into(),
            decision: ApprovalDecision::Allow,
        },
    );
    sample(&st, &dir, &tm, "claude", t0 + 3_000).await;
    assert_eq!(fake.lock().unwrap().keys, vec!["enter"]);
    emit_signal(&dir, t0 + 4_000, "UserPromptSubmit", "s-prod");
    sample(&st, &dir, &tm, "claude", t0 + 4_100).await;
    assert_still_asking(&dir);

    // 3. Folder-trust answer (`answer_trust` keys + `stage.answered`).
    let (dir, st, fake, tm, t0) = setup_ask("claude").await;
    fake.lock().unwrap().screen = CLAUDE_TRUST.into();
    st.answer_trust(
        &dir,
        "acme-shop",
        "producer",
        &tm,
        "/home/user/acme-shop",
        &store::load_state(&dir).unwrap(),
        t0,
    )
    .await
    .unwrap();
    st.answer_trust(
        &dir,
        "acme-shop",
        "producer",
        &tm,
        "/home/user/acme-shop",
        &store::load_state(&dir).unwrap(),
        t0 + TRUST_SETTLE_MS,
    )
    .await
    .unwrap();
    assert_eq!(fake.lock().unwrap().keys, vec!["enter"]);
    emit_signal(
        &dir,
        t0 + TRUST_SETTLE_MS + 100,
        "UserPromptSubmit",
        "s-prod",
    );
    sample(&st, &dir, &tm, "claude", t0 + TRUST_SETTLE_MS + 200).await;
    assert_still_asking(&dir);

    // 4. Rate-limit `continue` & 5. Connection-error `continue` (ledgerless terminal sends).
    for screen in [
        "fake runner: producer of acme-shop\nfake-work: waiting\nusage limit, resets in 30s\n",
        "fake runner: producer of acme-shop\nfake-work: waiting\nConnection error — send a message to continue.\n",
    ] {
        let (dir, st, fake, mut tm, t0) = setup_ask("fake").await;
        tm.status = Some(Status {
            state: "error".into(),
            msg: "denied".into(),
            ..Default::default()
        });
        fake.lock().unwrap().screen = screen.into();
        sample(&st, &dir, &tm, "fake", t0).await;
        sample(&st, &dir, &tm, "fake", t0 + 40_000).await;
        assert_eq!(fake.lock().unwrap().sends, vec!["continue"]);
        tm.status = Some(Status {
            state: "working".into(),
            ..Default::default()
        });
        fake.lock().unwrap().screen = "fake-work: working\n".into();
        sample(&st, &dir, &tm, "fake", t0 + 41_500).await;
        assert_still_asking(&dir);
    }
}

#[tokio::test]
async fn lifecycle_events_after_ask_do_not_clear_it() {
    for (by, kind) in [
        (
            "user",
            EventKind::RoleRestart {
                role: "producer".into(),
                fresh: false,
                reason: None,
            },
        ),
        (
            "user",
            EventKind::RoleRecast {
                role: "producer".into(),
                actor: actor("claude", "claude-opus-4"),
                reason: Some("swap".into()),
                card: None,
            },
        ),
        (
            "stage",
            EventKind::RoleClosed {
                role: "producer".into(),
                reason: None,
            },
        ),
        (
            "stage",
            EventKind::RoleStarted {
                role: "producer".into(),
                terminal: Some("bl-acme-shop-producer".into()),
                actor: Some(actor("claude", "m")),
                session_id: Some("s-prod-new".into()),
                reason: None,
            },
        ),
        (
            "stage",
            EventKind::RoleResumed {
                role: "producer".into(),
                terminal: Some("bl-acme-shop-producer".into()),
                session_id: Some("s-prod-2".into()),
                actor: None,
                reason: Some(RECOVERED.into()),
            },
        ),
    ] {
        let (dir, st, fake, tm, t0) = setup_ask("claude").await;
        append(&dir, by, kind);
        fake.lock().unwrap().screen = "> \n".into();
        sample(&st, &dir, &tm, "claude", t0 + 50).await;
        let sid = store::load_state(&dir).unwrap().roles["producer"]
            .session_id
            .clone()
            .unwrap();
        emit_signal(&dir, t0 + 1_200, "UserPromptSubmit", &sid);
        sample(&st, &dir, &tm, "claude", t0 + 1_300).await;
        assert_still_asking(&dir);
    }
}
