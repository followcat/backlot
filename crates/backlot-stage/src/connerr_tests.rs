//! The connection-error taxonomy's tests: the sample rules per
//! reporting-core state, and the full raise → three `continue`s →
//! escalate → `resumed` cycle on a fake core with an injected clock.

use std::path::Path;
use std::sync::{Arc, Mutex};

use backlot_model::event::EventKind;
use backlot_model::{EventBy, NewEvent};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use hc_proto::terminal::Status;
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, DuplexStream};

use super::*;
use crate::daemon::Stage;
use crate::reconcile::LiveTerminal;
use crate::store::{self, Pending};
use crate::test_support::TestDir;

const T0: u64 = 86_400_000 * 20_000 + 12 * 3_600_000;

fn tmp() -> TestDir {
    TestDir::new("backlot-connerr-test")
}

fn term_with(state: &str, msg: &str) -> LiveTerminal {
    LiveTerminal {
        status: Some(Status {
            state: state.into(),
            msg: msg.into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn a_non_working_screen_reports_the_connection_error() {
    let banner = "Connection error — send a message to continue.";
    // Devin reports `done` while it waits; a silent core has no
    // status at all. Both read the fresh tail.
    assert!(matches(&term_with("done", ""), banner, Work::Waiting));
    assert!(matches(&term_with("idle", ""), banner, Work::Waiting));
    assert!(matches(&LiveTerminal::default(), banner, Work::Waiting));
    // A working terminal may be printing the words: never a match.
    assert!(!matches(&term_with("working", ""), banner, Work::Working));
    assert!(!matches(&term_with("working", ""), banner, Work::Waiting));
    // Neither is a non-working screen whose derived work is working.
    assert!(!matches(&term_with("done", ""), banner, Work::Working));
    // The error status's message is read directly.
    let errored = term_with("error", "Connection error");
    assert!(matches(&errored, "", Work::Waiting));
    assert!(matches(&errored, "", Work::Working));
    // A stale banner in scrollback is not the fresh tail's.
    let stale = format!("{banner}\n{}", "the agent's fresh output line\n".repeat(14));
    assert!(!matches(&term_with("done", ""), &stale, Work::Waiting));
    // Clean text everywhere is nothing.
    assert!(!matches(&term_with("done", ""), "all good", Work::Waiting));
}

#[test]
fn the_retry_budget_escalates() {
    let pace = ConnPace {
        retries: 0,
        due_ms: 100,
    };
    assert!(matches!(step(pace, true, 99), Step::Wait));
    assert!(matches!(step(pace, true, 100), Step::Send));
    assert!(matches!(step(pace, false, 9_999), Step::Escalate));
}

/// The fake core's memory: the texts `terminal.send` delivered.
#[derive(Default)]
struct FakeCore {
    sends: Vec<String>,
}

/// Serve `hello`, then answer `terminal.send` from the shared list.
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
        let request: Value = serde_json::from_slice(&frame.payload).unwrap();
        let result = match request["method"].as_str().unwrap_or("") {
            "terminal.send" => {
                let text = request["params"]["text"].as_str().unwrap_or("").to_string();
                fake.lock().unwrap().sends.push(text);
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

/// A ledger with a bare `studio.created` — enough for one role's
/// attention records.
fn studio(dir: &Path) {
    store::append(dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("user"),
                EventKind::StudioCreated {
                    title: "acme-shop".into(),
                    repo: "/home/user/acme-shop".into(),
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
                    push: Default::default(),
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
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    }
}

/// One sample of both taxonomy passes — the sample pass's own order —
/// on the freshest ledger state.
async fn sample(stage: &Stage, dir: &Path, term: &LiveTerminal, work: Work, now_ms: u64) {
    let state = store::load_state(dir).unwrap();
    let w = Watched {
        dir,
        state: &state,
        role: "actor-t1-c1",
        runner: "fake",
        term,
    };
    let text = if term.status.as_ref().is_some_and(|s| s.state == "error") {
        ""
    } else {
        "Connection error — send a message to continue."
    };
    stage
        .rate_limit("acme-shop", &w, text, work, now_ms)
        .await
        .unwrap();
    stage
        .connection_error("acme-shop", &w, text, work, now_ms)
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

/// The cycle: the first sighting raises the stage-owned attention
/// and schedules the first `continue` 15 s out; three deliveries
/// spend the budget; the fourth sample escalates to a person; the
/// banner lifting clears it `resumed`.
#[tokio::test]
async fn three_retries_escalate_and_the_banner_lifting_resumes() {
    let dir = tmp();
    studio(&dir);
    let term = term_with("done", "");
    let (stage, fake) = stage().await;

    sample(&stage, &dir, &term, Work::Waiting, T0).await;
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1);
    match &raised[0].kind {
        EventKind::AttentionRaised {
            reason, needs_user, ..
        } => {
            assert_eq!(reason, CONNECTION_ERROR);
            assert!(!needs_user, "the stage owns the first episode");
        }
        _ => unreachable!(),
    }
    assert!(
        fake.lock().unwrap().sends.is_empty(),
        "sent before the wait"
    );

    // Retry 1 due at 15 s, then one every 15 s after each send.
    sample(&stage, &dir, &term, Work::Waiting, T0 + 14_000).await;
    assert!(fake.lock().unwrap().sends.is_empty());
    sample(&stage, &dir, &term, Work::Waiting, T0 + 15_000).await;
    assert_eq!(fake.lock().unwrap().sends.len(), 1);
    sample(&stage, &dir, &term, Work::Waiting, T0 + 30_000).await;
    assert_eq!(fake.lock().unwrap().sends.len(), 2);
    sample(&stage, &dir, &term, Work::Waiting, T0 + 45_000).await;
    assert_eq!(fake.lock().unwrap().sends.len(), 3);
    assert_eq!(fake.lock().unwrap().sends.as_slice(), ["continue"; 3]);

    // The budget is spent: the stage-owned attention clears and a
    // `needs_user` one takes its place; no fourth `continue`.
    sample(&stage, &dir, &term, Work::Waiting, T0 + 60_000).await;
    assert_eq!(fake.lock().unwrap().sends.len(), 3);
    let cleared = events(&dir, "attention.cleared");
    assert_eq!(cleared.len(), 1);
    match &cleared[0].kind {
        EventKind::AttentionCleared { reason, .. } => {
            assert_eq!(reason.as_deref(), Some("auto-retries exhausted after 3"));
        }
        _ => unreachable!(),
    }
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 2);
    match &raised[1].kind {
        EventKind::AttentionRaised {
            reason,
            needs_user,
            detail,
            ..
        } => {
            assert_eq!(reason, CONNECTION_ERROR);
            assert!(*needs_user, "the exhausted episode stayed the stage's");
            assert!(detail.contains("needs the user"), "{detail}");
        }
        _ => unreachable!(),
    }
    // Still erroring: the open attention is a person's, so the
    // stage sends nothing more.
    sample(&stage, &dir, &term, Work::Waiting, T0 + 75_000).await;
    assert_eq!(fake.lock().unwrap().sends.len(), 3);
    assert_eq!(events(&dir, "attention.raised").len(), 2);

    // The banner lifts — the open attention clears `resumed`.
    let clean = term_with("done", "");
    let state = store::load_state(&dir).unwrap();
    let w = Watched {
        dir: dir.path(),
        state: &state,
        role: "actor-t1-c1",
        runner: "fake",
        term: &clean,
    };
    stage
        .connection_error("acme-shop", &w, "all good", Work::Waiting, T0 + 76_000)
        .await
        .unwrap();
    let cleared = events(&dir, "attention.cleared");
    assert_eq!(cleared.len(), 2);
    match &cleared[1].kind {
        EventKind::AttentionCleared { reason, .. } => {
            assert_eq!(reason.as_deref(), Some("resumed"));
        }
        _ => unreachable!(),
    }
    assert!(stage.conn_pace.borrow().is_empty());
}

/// An `error` status whose message is the connection error is that
/// episode's — never `agent error`.
#[tokio::test]
async fn a_connection_error_status_is_not_an_agent_error() {
    let dir = tmp();
    studio(&dir);
    let term = term_with("error", "Connection error");
    let (stage, _fake) = stage().await;
    sample(&stage, &dir, &term, Work::Waiting, T0).await;
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1);
    match &raised[0].kind {
        EventKind::AttentionRaised { reason, .. } => {
            assert_eq!(reason, CONNECTION_ERROR);
        }
        _ => unreachable!(),
    }
    assert!(events(&dir, "attention.raised").iter().all(
        |e| !matches!(&e.kind, EventKind::AttentionRaised { reason, .. } if reason == "agent error")
    ));
}

/// Both words on one sample: the rate limit takes precedence — its
/// reset is the honest clock, and no connection-retry runs beside
/// it.
#[tokio::test]
async fn a_rate_limit_wins_over_a_connection_error() {
    let dir = tmp();
    studio(&dir);
    let term = term_with("error", "Connection error");
    let (stage, fake) = stage().await;
    let state = store::load_state(&dir).unwrap();
    let w = Watched {
        dir: dir.path(),
        state: &state,
        role: "actor-t1-c1",
        runner: "fake",
        term: &term,
    };
    let both = "Connection error\n\
                Reached free model rate limit, your limit will reset in 54 minutes";
    stage
        .rate_limit("acme-shop", &w, both, Work::Waiting, T0)
        .await
        .unwrap();
    stage
        .connection_error("acme-shop", &w, both, Work::Waiting, T0)
        .await
        .unwrap();
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1);
    match &raised[0].kind {
        EventKind::AttentionRaised { reason, .. } => {
            assert_eq!(reason, crate::ratelimit::RATE_LIMITED);
        }
        _ => unreachable!(),
    }
    assert!(
        stage.conn_pace.borrow().is_empty(),
        "the connection retry ran beside a rate limit"
    );
    let state = store::load_state(&dir).unwrap();
    let w = Watched {
        dir: dir.path(),
        state: &state,
        role: "actor-t1-c1",
        runner: "fake",
        term: &term,
    };
    stage
        .rate_limit("acme-shop", &w, both, Work::Waiting, T0 + 15_000)
        .await
        .unwrap();
    stage
        .connection_error("acme-shop", &w, both, Work::Waiting, T0 + 15_000)
        .await
        .unwrap();
    assert!(fake.lock().unwrap().sends.is_empty(), "the conn retry ran");
}

/// One rate-limit pass on the freshest ledger state.
async fn rate_pass(
    stage: &Stage,
    dir: &Path,
    term: &LiveTerminal,
    text: &str,
    work: Work,
    now_ms: u64,
) {
    let state = store::load_state(dir).unwrap();
    let w = Watched {
        dir,
        state: &state,
        role: "actor-t1-c1",
        runner: "fake",
        term,
    };
    stage
        .rate_limit("acme-shop", &w, text, work, now_ms)
        .await
        .unwrap();
}

/// The rate-limit twin of the cycle: three `continue`s at the parsed
/// reset, then a `needs_user` attention; the status recovering
/// clears it `resumed`.
#[tokio::test]
async fn three_rate_limit_retries_escalate_and_recover() {
    let dir = tmp();
    studio(&dir);
    let term = term_with("error", "denied");
    let (stage, fake) = stage().await;
    let screen = "usage limit, resets in 30s";

    rate_pass(&stage, &dir, &term, screen, Work::Waiting, T0).await;
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1);
    match &raised[0].kind {
        EventKind::AttentionRaised {
            reason, needs_user, ..
        } => {
            assert_eq!(reason, crate::ratelimit::RATE_LIMITED);
            assert!(!needs_user);
        }
        _ => unreachable!(),
    }
    // Reset 30 s + margin 5 s.
    rate_pass(&stage, &dir, &term, screen, Work::Waiting, T0 + 34_000).await;
    assert!(fake.lock().unwrap().sends.is_empty());
    rate_pass(&stage, &dir, &term, screen, Work::Waiting, T0 + 35_000).await;
    rate_pass(&stage, &dir, &term, screen, Work::Waiting, T0 + 70_000).await;
    rate_pass(&stage, &dir, &term, screen, Work::Waiting, T0 + 105_000).await;
    assert_eq!(fake.lock().unwrap().sends.as_slice(), ["continue"; 3]);

    rate_pass(&stage, &dir, &term, screen, Work::Waiting, T0 + 140_000).await;
    assert_eq!(fake.lock().unwrap().sends.len(), 3);
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 2);
    match &raised[1].kind {
        EventKind::AttentionRaised { needs_user, .. } => assert!(*needs_user),
        _ => unreachable!(),
    }

    // The status recovers: the escalated attention clears `resumed`.
    let clean = term_with("working", "");
    rate_pass(
        &stage,
        &dir,
        &clean,
        "all good",
        Work::Working,
        T0 + 141_000,
    )
    .await;
    let cleared = events(&dir, "attention.cleared");
    assert_eq!(cleared.len(), 2);
    match &cleared[1].kind {
        EventKind::AttentionCleared { reason, .. } => {
            assert_eq!(reason.as_deref(), Some("resumed"));
        }
        _ => unreachable!(),
    }
}

/// A weekly limit is a person's at once — no `continue` ever goes
/// out — and the pace sentinel keeps a manually cleared attention
/// from re-raising while the banner stays on screen.
#[tokio::test]
async fn a_weekly_limit_escalates_at_once_and_never_loops() {
    let dir = tmp();
    studio(&dir);
    let term = term_with("done", "");
    let (stage, fake) = stage().await;
    let screen = "You've hit your weekly limit · resets Nov 21, 4pm";

    rate_pass(&stage, &dir, &term, screen, Work::Waiting, T0).await;
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1);
    match &raised[0].kind {
        EventKind::AttentionRaised {
            reason,
            needs_user,
            detail,
            ..
        } => {
            assert_eq!(reason, crate::ratelimit::RATE_LIMITED);
            assert!(*needs_user, "a weekly quota stayed the stage's");
            assert!(detail.contains("needs the user"), "{detail}");
        }
        _ => unreachable!(),
    }
    // No wait loop: the next sample sends nothing, even far later.
    rate_pass(&stage, &dir, &term, screen, Work::Waiting, T0 + 3_600_000).await;
    assert!(fake.lock().unwrap().sends.is_empty());

    // The user clears the attention; the banner is still there, and
    // the sentinel keeps it from re-raising on every sample.
    let seq = raised[0].seq;
    store::append(&dir, move |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("user"),
                EventKind::AttentionCleared {
                    target: seq,
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    rate_pass(&stage, &dir, &term, screen, Work::Waiting, T0 + 3_601_000).await;
    rate_pass(&stage, &dir, &term, screen, Work::Waiting, T0 + 3_602_000).await;
    assert_eq!(events(&dir, "attention.raised").len(), 1);
}
