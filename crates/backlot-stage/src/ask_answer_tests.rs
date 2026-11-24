//! Stage tests for `ask.answered`: prompt-hook detection (`claude`,
//! `agy` `PreInvocation`, `fake` with hooks), watch fallback (`codex`,
//! `cmd`, signal-less `fake`, `HooksUnavailable`), and existing clear paths.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use backlot_model::event::{ActorIdentity, CheckDecl, EventBy, EventKind, StudioMode};
use backlot_model::state::{StudioState, replay};
use backlot_model::summary::{studio_exchange_files, summary_with_exchange};
use backlot_model::{NewEvent, caps};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use hc_proto::terminal::Status;
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, DuplexStream};

use crate::ask_answer::prompt_hook_active;
use crate::daemon::Stage;
use crate::reasons;
use crate::reconcile::LiveTerminal;
use crate::sample::Watched;
use crate::signals::signals_path;
use crate::store::{self, Pending};
use crate::test_support::TestDir;

fn tmp() -> TestDir {
    TestDir::new("backlot-ask-answer-test")
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
            "methods": ["terminal.read"],
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
            other => panic!("unexpected rpc method: {other}"),
        };
        let reply =
            serde_json::to_vec(&json!({ "jsonrpc": "2.0", "id": req["id"], "result": res }))
                .unwrap();
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
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    (Stage::for_test(client), fake)
}

fn actor(runner: &str) -> ActorIdentity {
    ActorIdentity {
        runner: runner.into(),
        runner_version: None,
        model: "m".into(),
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

fn queue_ask(dir: &Path, from: &str, text: &str) -> u64 {
    let msg_dir = dir.join("messages");
    fs::create_dir_all(&msg_dir).unwrap();
    let scratch = msg_dir.join(".tmp-ask.md");
    fs::write(&scratch, text).unwrap();
    store::append(dir, move |_, seq| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role(from),
                EventKind::MessageQueued {
                    to: "user".into(),
                    file: Some(format!("messages/{seq}-ask.md")),
                    purpose: None,
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
    .unwrap()
    .seq
}

async fn setup_ask(runner: &str) -> (TestDir, Stage, Arc<Mutex<FakeCore>>, LiveTerminal, u64, u64) {
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
                producer: actor(runner),
                director: actor(runner),
                writer: actor(runner),
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
        },
    );
    append(
        &dir,
        "stage",
        EventKind::RoleStarted {
            role: "producer".into(),
            terminal: Some("bl-acme-shop-producer".into()),
            actor: Some(actor(runner)),
            session_id: Some("s-prod".into()),
            reason: None,
        },
    );
    let ask_seq = queue_ask(&dir, "producer", "what should we build?");
    let (st, fake) = stage().await;
    let tm = LiveTerminal {
        name: "bl-acme-shop-producer".into(),
        address: "t-prod".into(),
        cwd: "/home/user/acme-shop".into(),
        ..Default::default()
    };
    (dir, st, fake, tm, ask_seq, now_ms())
}

fn emit_signal(dir: &Path, role: &str, ts_ms: u64, event: &str, sid: &str) {
    let path = signals_path(dir, role);
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

async fn sample_role(
    st: &Stage,
    dir: &Path,
    tm: &LiveTerminal,
    role: &str,
    runner: &str,
    now_ms: u64,
) {
    let state = store::load_state(dir).unwrap();
    let w = Watched {
        dir,
        state: &state,
        role,
        runner,
        term: tm,
    };
    st.sample_work("acme-shop", &w, now_ms).await.unwrap();
}

fn summary_role(dir: &Path, role: &str) -> (Option<String>, Vec<String>) {
    let state = store::load_state(dir).unwrap();
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
        .find(|r| r.role == role)
        .and_then(|r| r.asks.clone());
    (asks, studio.exchange.into_iter().map(|e| e.text).collect())
}

fn answered_targets(dir: &Path, role: &str) -> Vec<u64> {
    store::load_state(dir)
        .unwrap()
        .ask_answers
        .into_iter()
        .filter(|a| a.role == role)
        .map(|a| a.target)
        .collect()
}

#[test]
fn runner_prompt_hook_facts_come_from_caps_and_session_availability() {
    let state = StudioState::default();
    for r in ["claude", "devin", "agy"] {
        assert!(caps::has_prompt_hook(r), "{r}");
    }
    for no_hook in ["codex", "opencode", "cursor", "cmd", "cline", "unknown"] {
        assert!(!caps::has_prompt_hook(no_hook), "{no_hook}");
        assert!(!prompt_hook_active(
            no_hook,
            &state,
            "producer",
            Some("s1"),
            true
        ));
    }
    assert!(prompt_hook_active(
        "claude",
        &state,
        "producer",
        Some("s1"),
        false
    ));
    assert!(!prompt_hook_active(
        "claude", &state, "producer", None, false
    ));
    assert!(!prompt_hook_active(
        "fake",
        &state,
        "producer",
        Some("s1"),
        false
    ));
    assert!(prompt_hook_active(
        "fake",
        &state,
        "producer",
        Some("s1"),
        true
    ));
    let unavailable = replay(&[backlot_model::testkit::ev(
        1,
        "stage",
        EventKind::AttentionRaised {
            role: "producer".into(),
            reason: reasons::HOOKS_UNAVAILABLE.into(),
            detail: "tracked".into(),
            needs_user: true,
        },
    )]);
    assert!(!prompt_hook_active(
        "agy",
        &unavailable,
        "producer",
        Some("s1"),
        true
    ));
}

#[tokio::test]
async fn live_hook_runner_clears_after_prompt_submit_but_not_on_progress_or_waiting_jitter() {
    let (dir, st, fake, tm, ask_seq, t0) = setup_ask("claude").await;
    emit_signal(&dir, "producer", t0 - 500, "UserPromptSubmit", "s-prod");
    emit_signal(&dir, "producer", t0 + 10, "Stop", "s-prod");
    fake.lock().unwrap().screen = "> \n".into();
    sample_role(&st, &dir, &tm, "producer", "claude", t0 + 50).await;
    assert!(answered_targets(&dir, "producer").is_empty());

    // `role.progress` 1s later and screen jitter while waiting keep ask open.
    append(
        &dir,
        "producer",
        EventKind::RoleProgress {
            role: "producer".into(),
            task: None,
            text: "waiting on answer".into(),
        },
    );
    fake.lock().unwrap().screen = "> typing...\n".into();
    sample_role(&st, &dir, &tm, "producer", "claude", t0 + 1_100).await;
    assert!(answered_targets(&dir, "producer").is_empty());
    assert_eq!(
        summary_role(&dir, "producer").0.as_deref(),
        Some("what should we build?")
    );

    // User submits in terminal -> `UserPromptSubmit` (+ fast `PreToolUse`).
    emit_signal(&dir, "producer", t0 + 2_000, "UserPromptSubmit", "s-prod");
    emit_signal(&dir, "producer", t0 + 2_020, "PreToolUse", "s-prod");
    sample_role(&st, &dir, &tm, "producer", "claude", t0 + 2_200).await;
    assert_eq!(answered_targets(&dir, "producer"), vec![ask_seq]);
    let (asks, exchange) = summary_role(&dir, "producer");
    assert!(asks.is_none(), "{asks:?}");
    assert_eq!(exchange, vec!["what should we build?"]);
}

#[tokio::test]
async fn agy_pre_invocation_and_hooks_unavailable_fallback() {
    let (dir, st, _, tm, ask1, t0) = setup_ask("agy").await;
    emit_signal(&dir, "producer", t0 - 500, "UserPromptSubmit", "s-prod");
    emit_signal(&dir, "producer", t0 + 10, "Stop", "s-prod");
    sample_role(&st, &dir, &tm, "producer", "agy", t0 + 50).await;
    emit_signal(&dir, "producer", t0 + 1_500, "PreInvocation", "s-prod");
    sample_role(&st, &dir, &tm, "producer", "agy", t0 + 1_600).await;
    assert_eq!(answered_targets(&dir, "producer"), vec![ask1]);

    // With `HooksUnavailable` on a session lacking hooks, watch fallback applies.
    append(
        &dir,
        "stage",
        EventKind::RoleStarted {
            role: "director".into(),
            terminal: Some("bl-acme-shop-director".into()),
            actor: Some(actor("agy")),
            session_id: Some("s-dir".into()),
            reason: None,
        },
    );
    append(
        &dir,
        "stage",
        EventKind::AttentionRaised {
            role: "director".into(),
            reason: reasons::HOOKS_UNAVAILABLE.into(),
            detail: "tracked".into(),
            needs_user: true,
        },
    );
    let ask2 = queue_ask(&dir, "director", "which port?");
    let mut dir_tm = LiveTerminal {
        name: "bl-acme-shop-director".into(),
        address: "t-dir".into(),
        status: Some(Status {
            state: "waiting".into(),
            ..Default::default()
        }),
        ..tm
    };
    sample_role(&st, &dir, &dir_tm, "director", "agy", t0 + 2_700).await;
    dir_tm.status = Some(Status {
        state: "working".into(),
        ..Default::default()
    });
    sample_role(&st, &dir, &dir_tm, "director", "agy", t0 + 3_800).await;
    assert_eq!(answered_targets(&dir, "director"), vec![ask2]);
}

#[tokio::test]
async fn fallback_and_signal_bearing_fake_respect_hook_presence() {
    for runner in ["codex", "cmd", "fake"] {
        let (dir, st, fake, mut tm, ask_seq, t0) = setup_ask(runner).await;
        let (working, waiting) = match runner {
            "fake" => ("fake-work: working\n", "fake-work: waiting\n"),
            "cmd" => (
                "◇ Conjuring… esc to interrupt • 5s • ↓ 219\n",
                "❯ Ask your question...\n",
            ),
            _ => ("• Working (1s • esc to interrupt)\n", "? \n"),
        };
        fake.lock().unwrap().screen = working.into();
        sample_role(&st, &dir, &tm, "producer", runner, t0).await;
        sample_role(&st, &dir, &tm, "producer", runner, t0 + 1_100).await;
        assert!(answered_targets(&dir, "producer").is_empty(), "{runner}");

        fake.lock().unwrap().screen = waiting.into();
        if runner == "codex" {
            tm.status = Some(Status {
                state: "done".into(),
                ..Default::default()
            });
        }
        sample_role(&st, &dir, &tm, "producer", runner, t0 + 2_200).await;
        assert!(answered_targets(&dir, "producer").is_empty(), "{runner}");

        fake.lock().unwrap().screen = working.into();
        if runner == "codex" {
            tm.status = Some(Status {
                state: "working".into(),
                ..Default::default()
            });
        }
        sample_role(&st, &dir, &tm, "producer", runner, t0 + 3_300).await;
        assert_eq!(
            answered_targets(&dir, "producer"),
            vec![ask_seq],
            "{runner}"
        );
        assert!(summary_role(&dir, "producer").0.is_none(), "{runner}");
    }

    // Signal-bearing `fake` session does not clear on `work:working` until `UserPromptSubmit`.
    let (dir, st, fake, tm, ask_seq, t0) = setup_ask("fake").await;
    emit_signal(&dir, "producer", t0 - 100, "Stop", "s-prod");
    fake.lock().unwrap().screen = "fake-work: waiting\n".into();
    sample_role(&st, &dir, &tm, "producer", "fake", t0).await;
    fake.lock().unwrap().screen = "fake-work: working\n".into();
    sample_role(&st, &dir, &tm, "producer", "fake", t0 + 1_100).await;
    assert!(answered_targets(&dir, "producer").is_empty());
    emit_signal(&dir, "producer", t0 + 1_500, "UserPromptSubmit", "s-prod");
    sample_role(&st, &dir, &tm, "producer", "fake", t0 + 2_200).await;
    assert_eq!(answered_targets(&dir, "producer"), vec![ask_seq]);
}

#[tokio::test]
async fn existing_tell_role_action_and_finish_clear_paths_still_work() {
    // 1. User `tell` reply back to producer.
    let (dir, _, _, _, _, _) = setup_ask("fake").await;
    append(
        &dir,
        "user",
        EventKind::MessageQueued {
            to: "producer".into(),
            file: Some("messages/reply.md".into()),
            purpose: None,
            source_seq: None,
        },
    );
    assert!(summary_role(&dir, "producer").0.is_none());

    // 2. Role records a later ledger action of its own.
    let (dir, _, _, _, _, _) = setup_ask("fake").await;
    append(
        &dir,
        "producer",
        EventKind::ReqVersion {
            version: 1,
            file: "req/v1.md".into(),
            summary: "shop".into(),
        },
    );
    assert!(summary_role(&dir, "producer").0.is_none());

    // 3. `studio.finished` answers open questions.
    let (dir, _, _, _, _, _) = setup_ask("fake").await;
    append(
        &dir,
        "producer",
        EventKind::ReqVersion {
            version: 1,
            file: "req/v1.md".into(),
            summary: "shop".into(),
        },
    );
    let _ask2 = queue_ask(&dir, "producer", "ship it?");
    assert_eq!(
        summary_role(&dir, "producer").0.as_deref(),
        Some("ship it?")
    );
    append(
        &dir,
        "producer",
        EventKind::StudioFinished {
            reason: "done".into(),
        },
    );
    assert!(summary_role(&dir, "producer").0.is_none());
}
