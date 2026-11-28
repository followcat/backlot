//! S9f: the signal file's tail read, the last-line pick, the
//! signal-first combine, the signal stall rule and the StopFailure
//! display map — plus the whole path through `sample_work` on a
//! duplex fake core.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use backlot_model::event::{EventKind, PERMISSION_PROMPT, STALLED};
use backlot_model::state::TaskStatus;
use backlot_model::summary::Work;
use backlot_model::{EventBy, NewEvent};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, DuplexStream};

use crate::daemon::Stage;
use crate::reconcile::LiveTerminal;
use crate::sample::Watched;
use crate::signals::{self, Signal};
use crate::stall;
use crate::store::{self, Pending};
use crate::test_support::TestDir;

fn tmp() -> TestDir {
    TestDir::new("backlot-signals-test")
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn sig(event: &str, ts_ms: u64) -> Signal {
    Signal {
        ts_ms,
        event: event.into(),
        notification_type: None,
        error: None,
        cwd: None,
        tool_name: None,
        tool_input: None,
    }
}

/// The tail read: a small file reads whole, a bigger one only its
/// last 16 KB with the partial first line dropped.
#[test]
fn tail_reads_only_the_last_window() {
    let dir = tmp();
    let file = dir.join("signals.jsonl");
    fs::write(&file, "{\"a\":1}\n{\"b\":2}\n").unwrap();
    assert!(signals::tail(&file).contains("\"b\":2"));
    // Over the window: the first line in the slice is partial and
    // must be dropped.
    let line = format!("{{\"pad\":\"{}\"}}\n", "x".repeat(4000));
    let mut body = String::new();
    while body.len() < signals::TAIL_BYTES as usize + 200 {
        body.push_str(&line);
    }
    fs::write(&file, &body).unwrap();
    let tail = signals::tail(&file);
    assert!(tail.len() <= signals::TAIL_BYTES as usize);
    for l in tail.lines() {
        let _: Value = serde_json::from_str(l).unwrap();
    }
    assert!(signals::tail(&dir.join("missing")).is_empty());
}

/// Every event maps to its work state; a notification reads its
/// type; an unmapped event reports no work.
#[test]
fn each_event_maps_to_its_state() {
    assert_eq!(sig("PermissionRequest", 0).work(), Some(Work::Blocked));
    for e in [
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "PostToolUseFailure",
    ] {
        assert_eq!(sig(e, 0).work(), Some(Work::Working), "{e}");
    }
    for e in ["Stop", "StopFailure"] {
        assert_eq!(sig(e, 0).work(), Some(Work::Waiting), "{e}");
    }
    let mut s = sig("Notification", 0);
    s.notification_type = Some("permission_prompt".into());
    assert_eq!(s.work(), Some(Work::Blocked));
    s.notification_type = Some("idle_prompt".into());
    assert_eq!(s.work(), Some(Work::Waiting));
    s.notification_type = Some("other".into());
    assert_eq!(s.work(), None);
    assert_eq!(sig("SessionStart", 0).work(), None);
}

/// `last` picks the session's own newest line and ignores the
/// rest — a newer line from another session cannot move the role.
#[test]
fn last_ignores_other_sessions_and_bad_lines() {
    let tail = concat!(
        "{\"ts_ms\":1,\"event\":\"UserPromptSubmit\",\"session_id\":\"a\"}\n",
        "{\"ts_ms\":2,\"event\":\"Stop\",\"session_id\":\"b\"}\n",
        "not-json\n",
        "{\"ts_ms\":3,\"event\":\"Stop\",\"session_id\":\"a\"}\n",
        "{\"event\":\"PreToolUse\",\"session_id\":\"b\"}\n",
    );
    let a = signals::last_work(tail, "a").unwrap();
    assert_eq!(a.event, "Stop");
    assert_eq!(a.ts_ms, 3);
    let b = signals::last_work(tail, "b").unwrap();
    assert_eq!(b.event, "PreToolUse");
    assert!(signals::last_work(tail, "c").is_none());
    assert!(signals::last_work("", "a").is_none());
}

/// The combine: Blocked wins from any source, interrupt markers end
/// working turns, trustworthy `Waiting` structured + screen beats a
/// working signal past `SIGNAL_STALE_MS`, and fresh signals stay Working.
#[test]
fn combine_puts_signals_first_and_blocked_over_all() {
    let now = 20_000;
    let fresh = Some(now - signals::SIGNAL_STALE_MS);
    let stale = Some(now - signals::SIGNAL_STALE_MS - 1);
    for (label, inputs, want) in [
        (
            "blocked signal beats waiting screen",
            signals::WorkInputs {
                runner: "claude",
                signal: Some(Work::Blocked),
                signal_ts_ms: stale,
                interrupted: true,
                structured: None,
                screen: Work::Waiting,
                now_ms: now,
            },
            Work::Blocked,
        ),
        (
            "blocked screen beats waiting signal",
            signals::WorkInputs {
                runner: "claude",
                signal: Some(Work::Waiting),
                signal_ts_ms: fresh,
                interrupted: false,
                structured: None,
                screen: Work::Blocked,
                now_ms: now,
            },
            Work::Blocked,
        ),
        (
            "blocked structured beats working screen",
            signals::WorkInputs {
                runner: "claude",
                signal: None,
                signal_ts_ms: None,
                interrupted: false,
                structured: Some(Work::Blocked),
                screen: Work::Working,
                now_ms: now,
            },
            Work::Blocked,
        ),
        (
            "stale working signal never hides blocked structured",
            signals::WorkInputs {
                runner: "claude",
                signal: Some(Work::Working),
                signal_ts_ms: stale,
                interrupted: false,
                structured: Some(Work::Blocked),
                screen: Work::Waiting,
                now_ms: now,
            },
            Work::Blocked,
        ),
        (
            "stale working signal yields to trustworthy idle core and waiting screen",
            signals::WorkInputs {
                runner: "claude",
                signal: Some(Work::Working),
                signal_ts_ms: stale,
                interrupted: false,
                structured: Some(Work::Waiting),
                screen: Work::Waiting,
                now_ms: now,
            },
            Work::Waiting,
        ),
        (
            "fresh working signal stays working during grace even when core and screen read waiting",
            signals::WorkInputs {
                runner: "claude",
                signal: Some(Work::Working),
                signal_ts_ms: fresh,
                interrupted: false,
                structured: Some(Work::Waiting),
                screen: Work::Waiting,
                now_ms: now,
            },
            Work::Working,
        ),
        (
            "interrupt marker ends even a fresh working signal",
            signals::WorkInputs {
                runner: "claude",
                signal: Some(Work::Working),
                signal_ts_ms: fresh,
                interrupted: true,
                structured: Some(Work::Working),
                screen: Work::Waiting,
                now_ms: now,
            },
            Work::Waiting,
        ),
        (
            "stale working signal stays working when upstream core reports no structured status",
            signals::WorkInputs {
                runner: "claude",
                signal: Some(Work::Working),
                signal_ts_ms: stale,
                interrupted: false,
                structured: None,
                screen: Work::Waiting,
                now_ms: now,
            },
            Work::Working,
        ),
        (
            "stale working signal stays working when screen still shows active work",
            signals::WorkInputs {
                runner: "claude",
                signal: Some(Work::Working),
                signal_ts_ms: stale,
                interrupted: false,
                structured: Some(Work::Waiting),
                screen: Work::Working,
                now_ms: now,
            },
            Work::Working,
        ),
        (
            "screen-only runners never let structured waiting override a working signal",
            signals::WorkInputs {
                runner: "fake",
                signal: Some(Work::Working),
                signal_ts_ms: stale,
                interrupted: false,
                structured: Some(Work::Waiting),
                screen: Work::Waiting,
                now_ms: now,
            },
            Work::Working,
        ),
        (
            "stop signal beats working structured and screen",
            signals::WorkInputs {
                runner: "claude",
                signal: Some(Work::Waiting),
                signal_ts_ms: fresh,
                interrupted: false,
                structured: Some(Work::Working),
                screen: Work::Working,
                now_ms: now,
            },
            Work::Waiting,
        ),
        (
            "no signal falls back to structured then screen",
            signals::WorkInputs {
                runner: "claude",
                signal: None,
                signal_ts_ms: None,
                interrupted: false,
                structured: Some(Work::Working),
                screen: Work::Waiting,
                now_ms: now,
            },
            Work::Working,
        ),
        (
            "screen fallback when both signal and structured absent",
            signals::WorkInputs {
                runner: "claude",
                signal: None,
                signal_ts_ms: None,
                interrupted: false,
                structured: None,
                screen: Work::Working,
                now_ms: now,
            },
            Work::Working,
        ),
        (
            "all idle yields waiting",
            signals::WorkInputs {
                runner: "claude",
                signal: None,
                signal_ts_ms: None,
                interrupted: false,
                structured: None,
                screen: Work::Waiting,
                now_ms: now,
            },
            Work::Waiting,
        ),
    ] {
        assert_eq!(signals::combine(&inputs), want, "{label}");
    }

    for runner in [
        "claude", "codex", "devin", "opencode", "cursor", "cmd", "agy",
    ] {
        assert!(signals::structured_idle_trustworthy(runner), "{runner}");
        let out = signals::combine(&signals::WorkInputs {
            runner,
            signal: Some(Work::Working),
            signal_ts_ms: stale,
            interrupted: false,
            structured: Some(Work::Waiting),
            screen: Work::Waiting,
            now_ms: now,
        });
        assert_eq!(out, Work::Waiting, "{runner}");
    }
    for runner in ["cline", "fake", "unknown-runner"] {
        assert!(!signals::structured_idle_trustworthy(runner), "{runner}");
    }
}

/// Neutral Claude capture with `Interrupted · What should Claude do instead?`
/// triggers the interrupt detector unless a newer turn's working marker
/// appears below it.
#[test]
fn claude_interrupt_capture_ends_working_turn_until_next_working_turn() {
    let capture = include_str!("../tests/fixtures/claude/interrupted-turn.txt");
    assert_eq!(crate::runners::work_state("claude", capture), Work::Waiting);
    assert!(crate::runners::work::interrupted("claude", capture));

    let resumed_below = format!("{capture}\n❯ Fix the tests\n· Schlepping… (4s)\n");
    assert!(!crate::runners::work::interrupted("claude", &resumed_below));
    assert_eq!(
        crate::runners::work_state("claude", &resumed_below),
        Work::Working
    );

    for other in [
        "codex", "devin", "cmd", "agy", "cursor", "opencode", "cline", "fake",
    ] {
        assert!(
            !crate::runners::work::interrupted(other, capture),
            "{other}"
        );
    }
}

/// The signal stall rule: a Working signal past the limit stalls a
/// scoped role; a new signal, a non-Working signal, a blocked role
/// and the wrong scope never count.
#[test]
fn the_signal_stall_rule() {
    let s = sig("UserPromptSubmit", 1_000);
    let mut git = |_: &[&str], _: &str| crate::workareas::GitOut {
        code: 0,
        stdout: String::new(),
        stderr: String::new(),
    };
    let input = crate::liveness::ObserveInput {
        now_ms: 1_000,
        runner: "claude",
        eligible: true,
        raw_screen: "",
        raw_same_since_ms: 1_000,
        signal: Some(&s),
        terminal_working: false,
        activity_revision: None,
        local_machine: false,
        pid: None,
        work_roots: &[],
        ledger: None,
    };
    let ev = crate::liveness::Evidence::new(
        &input,
        &|_| Vec::new(),
        &|_, _, _| crate::liveness_diff::WorkAreaSnap::default(),
        &mut git,
    );
    let now = 1_000 + 60_000;
    assert!(stall::stalled(
        "producer",
        None,
        Work::Working,
        &ev,
        now,
        60_000
    ));
    assert!(stall::stalled(
        "actor-t1-c1",
        Some(TaskStatus::Running),
        Work::Working,
        &ev,
        now,
        60_000
    ));
    // Inside the limit, or non-actor Waiting: no stall.
    assert!(!stall::stalled(
        "producer",
        None,
        Work::Working,
        &ev,
        now,
        60_001
    ));
    assert!(!stall::stalled(
        "producer",
        None,
        Work::Waiting,
        &ev,
        now,
        60_000
    ));
    // A blocked role's evidence never counts — the permission pass owns it.
    assert!(!stall::stalled(
        "producer",
        None,
        Work::Blocked,
        &ev,
        now,
        60_000
    ));
    // An actor without a running task does not stall.
    assert!(!stall::stalled(
        "actor-t1-c1",
        Some(TaskStatus::Accepted),
        Work::Working,
        &ev,
        now,
        60_000
    ));
}

/// The duplex fake core: `terminal.read` answers the shared screen.
#[derive(Default)]
struct FakeCore {
    screen: String,
    keys: Vec<String>,
}

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
        ..Default::default()
    }
}

fn studio(dir: &Path) {
    studio_with_limit(dir, 600);
}

/// A studio with `stall_limit_s` seconds and `producer` started
/// with session `s-1`.
fn studio_with_limit(dir: &Path, stall_limit_s: u32) {
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
                        producer: actor_cfg(),
                        director: actor_cfg(),
                        writer: actor_cfg(),
                    }),
                    candidates: Vec::new(),
                    rework_cap: 2,
                    stall_limit_s,
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
            files: Vec::new(),
        })
    })
    .unwrap();
    store::append(dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::RoleStarted {
                    role: "producer".into(),
                    terminal: Some("t1".into()),
                    session_id: Some("s-1".into()),
                    actor: Some(actor_cfg()),
                    reason: None,
                    prompt_hash: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
}

fn actor_cfg() -> backlot_model::event::ActorIdentity {
    backlot_model::event::ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: "plain".into(),
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    }
}

/// Append one JSON line to `role`'s signals file.
fn emit(dir: &Path, role: &str, line: serde_json::Value) {
    let path = signals::signals_path(dir, role);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    use std::io::Write as _;
    writeln!(f, "{line}").unwrap();
}

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

fn work_of(stage: &Stage, role: &str) -> Option<Work> {
    stage
        .work
        .borrow()
        .get(&("acme-shop".to_string(), role.to_string()))
        .copied()
}

fn events(dir: &Path, kind: &str) -> Vec<backlot_model::event::Event> {
    store::load_events(dir)
        .unwrap()
        .0
        .into_iter()
        .filter(|e| e.kind.name() == kind)
        .collect()
}

/// A `UserPromptSubmit` reads Working on an idle screen — the
/// signal, not the screen, answers; a `Stop` after it reads
/// Waiting. A line from another session moves nothing.
#[tokio::test]
async fn signals_drive_the_work_state() {
    let dir = tmp();
    studio(&dir);
    let term = term();
    let (stage, _fake) = stage().await;
    let t0 = now_ms();
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0, "event": "UserPromptSubmit", "session_id": "s-1"}),
    );
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0 + 1, "event": "PreToolUse", "session_id": "other-session"}),
    );
    sample(&stage, &dir, &term, "producer", t0).await;
    assert_eq!(work_of(&stage, "producer"), Some(Work::Working));
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0 + 2, "event": "Stop", "session_id": "s-1"}),
    );
    sample(&stage, &dir, &term, "producer", t0 + 1_000).await;
    assert_eq!(work_of(&stage, "producer"), Some(Work::Waiting));
}

/// A `PermissionRequest` signal blocks the role even when the
/// prompt is not yet on screen — the same signal drives the
/// permission episode, so the attention still raises on the
/// window.
#[tokio::test]
async fn a_permission_request_signal_blocks_before_the_screen_does() {
    let dir = tmp();
    studio(&dir);
    let term = term();
    let (stage, _fake) = stage().await;
    let t0 = now_ms();
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0, "event": "PermissionRequest", "session_id": "s-1",
               "tool_name": "Bash", "tool_input": {"command": "cargo test"}}),
    );
    sample(&stage, &dir, &term, "producer", t0).await;
    assert_eq!(work_of(&stage, "producer"), Some(Work::Blocked));
    sample(&stage, &dir, &term, "producer", t0 + 2_000).await;
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1);
    match &raised[0].kind {
        EventKind::AttentionRaised { reason, role, .. } => {
            assert_eq!(reason.as_str(), PERMISSION_PROMPT);
            assert_eq!(role, "producer");
        }
        _ => unreachable!(),
    }
}

/// A `role.approval` for `role` the way `backlot approve` records it.
fn approve(dir: &Path, role: &str, by: &str, decision: backlot_model::event::ApprovalDecision) {
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

/// A `PermissionRequest` binds the attention it raises: the detail
/// names the tool and the trimmed command, `<seq>.signal` keeps the
/// request, and a decision made on it sends the key only while the
/// last signal is still that request.
#[tokio::test]
async fn a_request_binds_the_attention_and_the_decision_sends() {
    let dir = tmp();
    studio(&dir);
    let term = term();
    let (stage, fake) = stage().await;
    let t0 = now_ms();
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0, "event": "PermissionRequest", "session_id": "s-1",
               "tool_name": "Bash", "tool_input": {"command": "cargo test"}}),
    );
    sample(&stage, &dir, &term, "producer", t0).await;
    sample(&stage, &dir, &term, "producer", t0 + 2_000).await;
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1);
    match &raised[0].kind {
        EventKind::AttentionRaised { detail, .. } => {
            assert!(detail.contains("Bash: cargo test"), "{detail}");
        }
        _ => unreachable!(),
    }
    let seq = raised[0].seq;
    let bound: Signal = serde_json::from_str(
        &fs::read_to_string(dir.join(format!("attention/{seq}.signal"))).unwrap(),
    )
    .unwrap();
    assert_eq!(bound.event, "PermissionRequest");
    assert_eq!(bound.ts_ms, t0);
    assert_eq!(bound.tool_target(), Some("cargo test"));

    // The decision lands while the last signal is still the bound
    // request and the screen still asks: the key goes to it.
    fake.lock().unwrap().screen = "fake-work: blocked".into();
    approve(
        &dir,
        "producer",
        "user",
        backlot_model::event::ApprovalDecision::Allow,
    );
    sample(&stage, &dir, &term, "producer", t0 + 3_000).await;
    assert_eq!(fake.lock().unwrap().keys.as_slice(), ["enter"]);
    assert_eq!(events(&dir, "attention.cleared").len(), 1);
}

/// A decision recorded against request A never lands on request B:
/// the next `PermissionRequest` moving the last signal on makes the
/// compare fail — nothing is sent, A's attention clears `prompt
/// changed before the decision was sent`, and B raises an attention
/// bound to itself whose own approval then sends.
#[tokio::test]
async fn a_second_request_refuses_the_earlier_decision() {
    let dir = tmp();
    studio(&dir);
    let term = term();
    let (stage, fake) = stage().await;
    let t0 = now_ms();
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0, "event": "PermissionRequest", "session_id": "s-1",
               "tool_name": "Bash", "tool_input": {"command": "cargo test"}}),
    );
    sample(&stage, &dir, &term, "producer", t0).await;
    sample(&stage, &dir, &term, "producer", t0 + 2_000).await;
    let seq_a = events(&dir, "attention.raised")[0].seq;

    // The decision lands after the prompt already moved on.
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0 + 3_000, "event": "PermissionRequest", "session_id": "s-1",
               "tool_name": "Bash", "tool_input": {"command": "cargo fmt"}}),
    );
    approve(
        &dir,
        "producer",
        "user",
        backlot_model::event::ApprovalDecision::Allow,
    );
    sample(&stage, &dir, &term, "producer", t0 + 4_000).await;
    assert_eq!(fake.lock().unwrap().keys.as_slice(), Vec::<String>::new());
    let cleared = events(&dir, "attention.cleared");
    assert_eq!(cleared.len(), 1);
    match &cleared[0].kind {
        EventKind::AttentionCleared { target, reason } => {
            assert_eq!(*target, seq_a);
            assert_eq!(
                reason.as_deref(),
                Some("prompt changed before the decision was sent")
            );
        }
        _ => unreachable!(),
    }

    // B raised its own attention, bound to B's request — its own
    // approval sends the key.
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 2);
    let seq_b = raised[1].seq;
    match &raised[1].kind {
        EventKind::AttentionRaised { detail, .. } => {
            assert!(detail.contains("Bash: cargo fmt"), "{detail}");
        }
        _ => unreachable!(),
    }
    let bound_b: Signal = serde_json::from_str(
        &fs::read_to_string(dir.join(format!("attention/{seq_b}.signal"))).unwrap(),
    )
    .unwrap();
    assert_eq!(bound_b.tool_target(), Some("cargo fmt"));
    fake.lock().unwrap().screen = "fake-work: blocked".into();
    approve(
        &dir,
        "producer",
        "user",
        backlot_model::event::ApprovalDecision::Allow,
    );
    sample(&stage, &dir, &term, "producer", t0 + 5_000).await;
    assert_eq!(fake.lock().unwrap().keys.as_slice(), ["enter"]);
}

/// A `PermissionRequest` whose command reaches for `BACKLOT_*`
/// flags the attention `needs_user`: the flag read the signal's
/// own command, though the screen never showed it, and the ledger
/// refuses the director's answer.
#[tokio::test]
async fn a_request_reaching_for_backlot_vars_is_the_users() {
    let dir = tmp();
    studio(&dir);
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::RoleStarted {
                    role: "actor-t1-c1".into(),
                    terminal: Some("t9".into()),
                    session_id: Some("s-9".into()),
                    actor: Some(actor_cfg()),
                    reason: None,
                    prompt_hash: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    let term = term();
    let (stage, _fake) = stage().await;
    let t0 = now_ms();
    emit(
        &dir,
        "actor-t1-c1",
        json!({"ts_ms": t0, "event": "PermissionRequest", "session_id": "s-9",
               "tool_name": "Bash",
               "tool_input": {"command": "env -i BACKLOT_ROLE=director backlot review --task t1"}}),
    );
    sample(&stage, &dir, &term, "actor-t1-c1", t0).await;
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 2_000).await;
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1);
    match &raised[0].kind {
        EventKind::AttentionRaised {
            detail, needs_user, ..
        } => {
            assert!(*needs_user, "the command's reach did not flag");
            assert!(detail.contains("env -i BACKLOT_ROLE=director"), "{detail}");
        }
        _ => unreachable!(),
    }
    let refused = store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("director"),
                EventKind::RoleApproval {
                    role: "actor-t1-c1".into(),
                    decision: backlot_model::event::ApprovalDecision::Deny,
                },
            ),
            files: Vec::new(),
        })
    });
    assert!(refused.is_err(), "the director answered the user's prompt");
}

/// A `Working` signal held past the stall limit raises `stalled`
/// with `no hook signal for <n>s` — a newer signal clears it even
/// while the screen is unchanged.
#[tokio::test]
async fn a_dead_signal_stalls_and_a_new_one_clears() {
    let dir = tmp();
    studio_with_limit(&dir, 60);
    let term = term();
    let (stage, _fake) = stage().await;
    let t0 = now_ms();
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0, "event": "UserPromptSubmit", "session_id": "s-1"}),
    );
    sample(&stage, &dir, &term, "producer", t0).await;
    // The signal is Working but 61s old: stalled, and the detail
    // names the signal silence, not the screen.
    sample(&stage, &dir, &term, "producer", t0 + 61_000).await;
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1);
    match &raised[0].kind {
        EventKind::AttentionRaised { reason, detail, .. } => {
            assert_eq!(reason.as_str(), STALLED);
            assert!(detail.contains("no sign of life for 61s"), "{detail}");
            assert!(detail.contains("UserPromptSubmit (hook)"), "{detail}");
        }
        _ => unreachable!(),
    }
    // A newer signal lands: the open stall clears.
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0 + 62_000, "event": "PostToolUse", "session_id": "s-1"}),
    );
    sample(&stage, &dir, &term, "producer", t0 + 62_000).await;
    let cleared = events(&dir, "attention.cleared");
    assert_eq!(cleared.len(), 1);
    match &cleared[0].kind {
        EventKind::AttentionCleared { reason, .. } => {
            assert_eq!(reason.as_deref(), Some("new hook signal"));
        }
        _ => unreachable!(),
    }
    // And a fresh Working signal can stall all over again.
    sample(&stage, &dir, &term, "producer", t0 + 123_000).await;
    assert_eq!(events(&dir, "attention.raised").len(), 2);
}

/// `failed_roles`: a role whose last signal is a StopFailure names
/// its error; any newer signal clears the entry.
#[test]
fn failed_roles_reports_stop_failures() {
    let dir = tmp();
    studio(&dir);
    emit(
        &dir,
        "producer",
        json!({"ts_ms": 1, "event": "StopFailure", "session_id": "s-1", "error": "rate_limit"}),
    );
    let state = store::load_state(&dir).unwrap();
    let failed = signals::failed_roles(&dir, &state);
    assert_eq!(
        failed.get("producer").map(String::as_str),
        Some("rate_limit")
    );
    emit(
        &dir,
        "producer",
        json!({"ts_ms": 2, "event": "UserPromptSubmit", "session_id": "s-1"}),
    );
    let state = store::load_state(&dir).unwrap();
    assert!(signals::failed_roles(&dir, &state).is_empty());
    // Another session's StopFailure is not this session's.
    emit(
        &dir,
        "producer",
        json!({"ts_ms": 3, "event": "StopFailure", "session_id": "other", "error": "overloaded"}),
    );
    let state = store::load_state(&dir).unwrap();
    assert!(signals::failed_roles(&dir, &state).is_empty());
}

/// `backlot status` appends ` — last turn failed: <error>` to the
/// role's own line — a fixed role's line, and a live actor's the
/// roles section does not otherwise list.
#[test]
fn status_appends_the_last_turn_failure_to_the_roles_line() {
    let dir = tmp();
    studio(&dir);
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::RoleStarted {
                    role: "actor-t1-c1".into(),
                    terminal: Some("t9".into()),
                    session_id: Some("s-9".into()),
                    actor: Some(actor_cfg()),
                    reason: None,
                    prompt_hash: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    emit(
        &dir,
        "producer",
        json!({"ts_ms": 1, "event": "StopFailure", "session_id": "s-1", "error": "overloaded"}),
    );
    emit(
        &dir,
        "actor-t1-c1",
        json!({"ts_ms": 2, "event": "StopFailure", "session_id": "s-9", "error": "rate_limit"}),
    );
    let state = store::load_state(&dir).unwrap();
    let out = crate::render::status(
        "acme-shop",
        &state,
        &signals::failed_roles(&dir, &state),
        0,
        &Default::default(),
    );
    assert!(
        out.contains("producer  fake/plain/high/standard (live) — last turn failed: overloaded"),
        "{out}"
    );
    assert!(
        out.contains("actor-t1-c1 fake/plain/high/standard (live) — last turn failed: rate_limit"),
        "{out}"
    );
}

/// `last_work` skips a line that maps to no state: an `auth_success`
/// chime does not hand the role back to the screen.
#[test]
fn last_work_skips_unmapped_events() {
    let tail = concat!(
        "{\"ts_ms\":1,\"event\":\"PermissionRequest\",\"session_id\":\"s\",\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"cargo test\"}}\n",
        "{\"ts_ms\":2,\"event\":\"Notification\",\"session_id\":\"s\",\"notification_type\":\"auth_success\"}\n",
    );
    let s = signals::last_work(tail, "s").unwrap();
    assert_eq!(s.event, "PermissionRequest");
}

/// `pending_request` reads the tail's first non-`Notification` line:
/// the request still pending under its six-second chime, gone when
/// any other event landed after it.
#[test]
fn pending_request_skips_only_notifications() {
    let request = "{\"ts_ms\":1,\"event\":\"PermissionRequest\",\"session_id\":\"s\",\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"cargo test\"}}";
    let chime = "{\"ts_ms\":2,\"event\":\"Notification\",\"session_id\":\"s\",\"notification_type\":\"permission_prompt\"}";
    let tail = format!("{request}\n{chime}\n");
    let p = signals::pending_request(&tail, "s", &Default::default()).unwrap();
    assert_eq!(p.ts_ms, 1);
    assert_eq!(p.tool_target(), Some("cargo test"));
    // Any non-Notification event after the request ends it — and a
    // request in the answered set is no one's to send.
    let tail = format!(
        "{request}\n{chime}\n{{\"ts_ms\":3,\"event\":\"PostToolUse\",\"session_id\":\"s\"}}\n"
    );
    assert!(signals::pending_request(&tail, "s", &Default::default()).is_none());
    let answered = std::collections::BTreeSet::from([1]);
    assert!(signals::pending_request(&tail, "s", &answered).is_none());
    let tail = format!("{request}\n{chime}\n");
    assert!(signals::pending_request(&tail, "s", &answered).is_none());
    // Another session's request is not this session's.
    let tail = "{\"ts_ms\":1,\"event\":\"PermissionRequest\",\"session_id\":\"other\"}\n";
    assert!(signals::pending_request(tail, "s", &Default::default()).is_none());
    assert!(signals::pending_request("", "s", &Default::default()).is_none());
}

/// `signals::state`: an answered request reads Working — the tool
/// is running and no hook line comes until its own `PostToolUse` —
/// while a pending one and a bare chime stay Blocked.
#[test]
fn an_answered_request_reads_working_until_the_next_signal() {
    let request = "{\"ts_ms\":1,\"event\":\"PermissionRequest\",\"session_id\":\"s\",\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"cargo test\"}}";
    let chime = "{\"ts_ms\":2,\"event\":\"Notification\",\"session_id\":\"s\",\"notification_type\":\"permission_prompt\"}";
    let tail = format!("{request}\n{chime}\n");
    let none = std::collections::BTreeSet::new();
    let answered = std::collections::BTreeSet::from([1]);
    assert_eq!(signals::state(&tail, "s", &none), Some(Work::Blocked));
    assert_eq!(signals::state(&tail, "s", &answered), Some(Work::Working));
    // A bare chime — no `PermissionRequest` behind it — keeps its
    // own Blocked whatever the set holds.
    let bare = format!("{chime}\n");
    assert_eq!(signals::state(&bare, "s", &answered), Some(Work::Blocked));
    // The tool's own signal answers again.
    let tail = format!(
        "{request}\n{chime}\n{{\"ts_ms\":3,\"event\":\"PostToolUse\",\"session_id\":\"s\"}}\n"
    );
    assert_eq!(signals::state(&tail, "s", &answered), Some(Work::Working));
}

/// A `permission_prompt` notification six seconds after its
/// request is the same prompt's chime: the approve-time compare
/// reads the pending request, and the decision's key still sends.
#[tokio::test]
async fn a_notification_keeps_the_request_pending() {
    let dir = tmp();
    studio(&dir);
    let term = term();
    let (stage, fake) = stage().await;
    let t0 = now_ms();
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0, "event": "PermissionRequest", "session_id": "s-1",
               "tool_name": "Bash", "tool_input": {"command": "cargo test"}}),
    );
    sample(&stage, &dir, &term, "producer", t0).await;
    sample(&stage, &dir, &term, "producer", t0 + 2_000).await;
    assert_eq!(events(&dir, "attention.raised").len(), 1);

    // Six seconds later Claude Code chimes the same prompt; the
    // decision taken on the bound request still sends — the screen
    // asks too, which a send now needs.
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0 + 6_000, "event": "Notification", "session_id": "s-1",
               "notification_type": "permission_prompt"}),
    );
    fake.lock().unwrap().screen = "fake-work: blocked".into();
    approve(
        &dir,
        "producer",
        "user",
        backlot_model::event::ApprovalDecision::Allow,
    );
    sample(&stage, &dir, &term, "producer", t0 + 7_000).await;
    assert_eq!(fake.lock().unwrap().keys.as_slice(), ["enter"]);
}

/// An answered request never comes back: the send cleared its
/// attention, `answered_requests` rebuilds the set from the
/// ledger plus `<seq>.signal`, and the same request pending no
/// more means no second raise and no second key — across a stage
/// restart too, the answered set riding the ledger rather than
/// memory. Until the tool's own signal lands the role reads
/// Working.
#[tokio::test]
async fn an_answered_request_never_raises_again() {
    let dir = tmp();
    studio(&dir);
    let term = term();
    let (st, fake) = stage().await;
    let t0 = now_ms();
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0, "event": "PermissionRequest", "session_id": "s-1",
               "tool_name": "Bash", "tool_input": {"command": "cargo test"}}),
    );
    sample(&st, &dir, &term, "producer", t0).await;
    sample(&st, &dir, &term, "producer", t0 + 2_000).await;
    assert_eq!(events(&dir, "attention.raised").len(), 1);
    fake.lock().unwrap().screen = "fake-work: blocked".into();
    approve(
        &dir,
        "producer",
        "user",
        backlot_model::event::ApprovalDecision::Allow,
    );
    sample(&st, &dir, &term, "producer", t0 + 3_000).await;
    assert_eq!(fake.lock().unwrap().keys.as_slice(), ["enter"]);

    // The prompt is answered; the tool now runs. The screen moved
    // past the prompt — no hook line will say so until the tool's
    // own — and the request never raises a second attention.
    fake.lock().unwrap().screen = "fake-work: waiting".into();
    sample(&st, &dir, &term, "producer", t0 + 4_000).await;
    assert_eq!(events(&dir, "attention.raised").len(), 1);
    assert_eq!(work_of(&st, "producer"), Some(Work::Working));

    // A stage restart rebuilds the answered set from the ledger —
    // the request still does not come back.
    let (stage2, _fake2) = stage().await;
    sample(&stage2, &dir, &term, "producer", t0 + 6_000).await;
    assert_eq!(events(&dir, "attention.raised").len(), 1);
    assert_eq!(work_of(&stage2, "producer"), Some(Work::Working));

    // The tool's own signal then answers in place.
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0 + 7_000, "event": "PostToolUse", "session_id": "s-1"}),
    );
    sample(&stage2, &dir, &term, "producer", t0 + 8_000).await;
    assert_eq!(work_of(&stage2, "producer"), Some(Work::Working));
}

/// The prompt the decision was made for can go away at the role's
/// own terminal — Claude Code emits no hook for that. The bound
/// request stays the pending one, so the screen is the proof: not
/// Blocked any more means `prompt gone`, nothing sent and nothing
/// raised.
#[tokio::test]
async fn a_bound_attention_refuses_when_the_screen_moved_on() {
    let dir = tmp();
    studio(&dir);
    let term = term();
    let (stage, fake) = stage().await;
    let t0 = now_ms();
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0, "event": "PermissionRequest", "session_id": "s-1",
               "tool_name": "Bash", "tool_input": {"command": "cargo test"}}),
    );
    sample(&stage, &dir, &term, "producer", t0).await;
    sample(&stage, &dir, &term, "producer", t0 + 2_000).await;
    assert_eq!(events(&dir, "attention.raised").len(), 1);

    // Answered at the terminal: the request is still the tail's
    // pending line, but the screen reads waiting — refuse.
    fake.lock().unwrap().screen = "fake-work: waiting".into();
    approve(
        &dir,
        "producer",
        "user",
        backlot_model::event::ApprovalDecision::Allow,
    );
    sample(&stage, &dir, &term, "producer", t0 + 3_000).await;
    assert_eq!(fake.lock().unwrap().keys.as_slice(), Vec::<String>::new());
    match &events(&dir, "attention.cleared")[0].kind {
        EventKind::AttentionCleared { reason, .. } => {
            assert_eq!(
                reason.as_deref(),
                Some("prompt gone before the decision was sent")
            );
        }
        _ => unreachable!(),
    }
    assert_eq!(events(&dir, "attention.raised").len(), 1);
}

/// A tool event after the request means the prompt already ended:
/// the decision lands on nothing — the attention clears `prompt
/// gone`, and no key sends.
#[tokio::test]
async fn a_tool_event_ends_the_request() {
    let dir = tmp();
    studio(&dir);
    let term = term();
    let (stage, fake) = stage().await;
    let t0 = now_ms();
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0, "event": "PermissionRequest", "session_id": "s-1",
               "tool_name": "Bash", "tool_input": {"command": "cargo test"}}),
    );
    sample(&stage, &dir, &term, "producer", t0).await;
    sample(&stage, &dir, &term, "producer", t0 + 2_000).await;
    assert_eq!(events(&dir, "attention.raised").len(), 1);

    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0 + 3_000, "event": "PostToolUse", "session_id": "s-1"}),
    );
    approve(
        &dir,
        "producer",
        "user",
        backlot_model::event::ApprovalDecision::Allow,
    );
    sample(&stage, &dir, &term, "producer", t0 + 4_000).await;
    assert_eq!(fake.lock().unwrap().keys.as_slice(), Vec::<String>::new());
    match &events(&dir, "attention.cleared")[0].kind {
        EventKind::AttentionCleared { reason, .. } => {
            assert_eq!(
                reason.as_deref(),
                Some("prompt gone before the decision was sent")
            );
        }
        _ => unreachable!(),
    }
}

/// A notification that maps to no state keeps the role on its last
/// real signal: `auth_success` after a request stays Blocked.
#[tokio::test]
async fn an_unmapped_notification_keeps_the_role_blocked() {
    let dir = tmp();
    studio(&dir);
    let term = term();
    let (stage, _fake) = stage().await;
    let t0 = now_ms();
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0, "event": "PermissionRequest", "session_id": "s-1",
               "tool_name": "Bash", "tool_input": {"command": "cargo test"}}),
    );
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0 + 1, "event": "Notification", "session_id": "s-1",
               "notification_type": "auth_success"}),
    );
    sample(&stage, &dir, &term, "producer", t0).await;
    assert_eq!(work_of(&stage, "producer"), Some(Work::Blocked));
}

/// A notification does not restart the signal-silence clock: a
/// Working signal 61 s old still stalls though an unmapped chime
/// came in between.
#[tokio::test]
async fn a_notification_does_not_restart_a_stalled_turn() {
    let dir = tmp();
    studio_with_limit(&dir, 60);
    let term = term();
    let (stage, _fake) = stage().await;
    let t0 = now_ms();
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0, "event": "UserPromptSubmit", "session_id": "s-1"}),
    );
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0 + 30_000, "event": "Notification", "session_id": "s-1",
               "notification_type": "auth_success"}),
    );
    sample(&stage, &dir, &term, "producer", t0).await;
    sample(&stage, &dir, &term, "producer", t0 + 61_000).await;
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1);
    match &raised[0].kind {
        EventKind::AttentionRaised { reason, detail, .. } => {
            assert_eq!(reason.as_str(), STALLED);
            assert!(detail.contains("no sign of life for 61s"), "{detail}");
            assert!(detail.contains("UserPromptSubmit (hook)"), "{detail}");
        }
        _ => unreachable!(),
    }
}

fn claude_actor_cfg() -> backlot_model::event::ActorIdentity {
    backlot_model::event::ActorIdentity {
        runner: "claude".into(),
        runner_version: None,
        model: "sonnet".into(),
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    }
}

fn setup_claude_studio(base: &Path) -> std::path::PathBuf {
    let dir = store::studio_dir(base, "acme-shop");
    store::append(&dir, |_, _| {
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
                        producer: claude_actor_cfg(),
                        director: claude_actor_cfg(),
                        writer: claude_actor_cfg(),
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
            files: Vec::new(),
        })
    })
    .unwrap();
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::RoleStarted {
                    role: "producer".into(),
                    terminal: Some("bl-acme-shop-producer".into()),
                    session_id: Some("s-claude-1".into()),
                    actor: Some(claude_actor_cfg()),
                    reason: None,
                    prompt_hash: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    dir
}

/// Fail-first / regression sample-pass test: a Claude Code role started 31h
/// ago emits `UserPromptSubmit` (`Working`, no `Stop`), then the person hits
/// Esc — the screen shows `Interrupted · What should Claude do instead?`
/// and the core goes `idle`. The summary's work must flip to `Waiting` and
/// the `working 31h` duration must stop counting. A later turn recovers.
#[tokio::test]
async fn sample_pass_interrupted_claude_turn_becomes_waiting_and_stops_working_duration() {
    let base = tmp();
    let dir = setup_claude_studio(base.path());
    let state = store::load_state(&dir).unwrap();
    let started_ms = state.roles["producer"].since_ms;
    let h31_ms = started_ms + 31 * 3_600_000;

    let mut term = LiveTerminal {
        name: "bl-acme-shop-producer".into(),
        address: "t-prod".into(),
        cwd: "/home/user/acme-shop".into(),
        status: Some(hc_proto::terminal::Status {
            state: "working".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let (mut stage, fake) = stage().await;
    stage.base = base.path().to_path_buf();
    stage.cached.insert("acme-shop".into(), state.clone());

    emit(
        &dir,
        "producer",
        json!({"ts_ms": h31_ms, "event": "UserPromptSubmit", "session_id": "s-claude-1"}),
    );
    fake.lock().unwrap().screen = "✻ Orbiting… (12s · ↑ 1.2k tokens · esc to interrupt)\n".into();
    let watched = Watched {
        dir: &dir,
        state: &state,
        role: "producer",
        runner: "claude",
        term: &term,
    };
    stage
        .sample_work("acme-shop", &watched, h31_ms)
        .await
        .unwrap();
    stage
        .nudge(
            "acme-shop",
            &dir,
            &state,
            std::slice::from_ref(&term),
            h31_ms,
        )
        .unwrap();
    let summary_before = stage.summary_view_at(&BTreeMap::new(), h31_ms);
    let prod_before = summary_before.studios[0]
        .roles
        .iter()
        .find(|r| r.role == "producer")
        .unwrap();
    assert_eq!(prod_before.work, Some(Work::Working));
    let live_before = crate::live::from_summary(&summary_before, h31_ms);
    let status_before = crate::render::status(
        "acme-shop",
        &state,
        &BTreeMap::new(),
        h31_ms,
        &crate::live::LiveView {
            work: crate::live::work_map(&live_before),
            running_cmds: crate::live::running_map(&live_before),
            idle: crate::live::idle_map(&live_before),
            running: true,
            accounts: Vec::new(),
        },
    );
    assert!(status_before.contains("working 31h"), "{status_before}");

    // Person presses Esc: screen shows Claude's interrupt marker, core reports `idle`, no Stop hook fires.
    fake.lock().unwrap().screen =
        include_str!("../tests/fixtures/claude/interrupted-turn.txt").into();
    term.status = Some(hc_proto::terminal::Status {
        state: "idle".into(),
        ..Default::default()
    });
    let after_ms = h31_ms + 1_000;
    let watched = Watched {
        dir: &dir,
        state: &state,
        role: "producer",
        runner: "claude",
        term: &term,
    };
    stage
        .sample_work("acme-shop", &watched, after_ms)
        .await
        .unwrap();
    stage
        .nudge(
            "acme-shop",
            &dir,
            &state,
            std::slice::from_ref(&term),
            after_ms,
        )
        .unwrap();
    let summary_after = stage.summary_view_at(&BTreeMap::new(), after_ms);
    let prod_after = summary_after.studios[0]
        .roles
        .iter()
        .find(|r| r.role == "producer")
        .unwrap();
    assert_eq!(prod_after.work, Some(Work::Waiting));
    assert!(prod_after.running.is_none());
    let live_after = crate::live::from_summary(&summary_after, after_ms);
    let status_after = crate::render::status(
        "acme-shop",
        &state,
        &BTreeMap::new(),
        after_ms,
        &crate::live::LiveView {
            work: crate::live::work_map(&live_after),
            running_cmds: crate::live::running_map(&live_after),
            idle: crate::live::idle_map(&live_after),
            running: true,
            accounts: Vec::new(),
        },
    );
    assert!(!status_after.contains("working"), "{status_after}");
    assert!(status_after.contains("waiting"), "{status_after}");

    // Another hour passes: working duration stays gone (`idle`, never `working 32h`).
    let later_ms = after_ms + 3_600_000;
    stage
        .sample_work("acme-shop", &watched, later_ms)
        .await
        .unwrap();
    stage
        .nudge(
            "acme-shop",
            &dir,
            &state,
            std::slice::from_ref(&term),
            later_ms,
        )
        .unwrap();
    let summary_later = stage.summary_view_at(&BTreeMap::new(), later_ms);
    let prod_later = summary_later.studios[0]
        .roles
        .iter()
        .find(|r| r.role == "producer")
        .unwrap();
    assert_eq!(prod_later.work, Some(Work::Waiting));
    assert_eq!(prod_later.idle_since_ms, Some(after_ms));
    let live_later = crate::live::from_summary(&summary_later, later_ms);
    let status_later = crate::render::status(
        "acme-shop",
        &state,
        &BTreeMap::new(),
        later_ms,
        &crate::live::LiveView {
            work: crate::live::work_map(&live_later),
            running_cmds: crate::live::running_map(&live_later),
            idle: crate::live::idle_map(&live_later),
            running: true,
            accounts: Vec::new(),
        },
    );
    assert!(!status_later.contains("working"), "{status_later}");
    assert!(status_later.contains("idle 1h"), "{status_later}");

    // Starting a new turn (`UserPromptSubmit`) makes the role `Working` again
    // even while the earlier turn's interrupt marker stays in scrollback.
    let turn2_ms = later_ms + 5_000;
    emit(
        &dir,
        "producer",
        json!({"ts_ms": turn2_ms, "event": "UserPromptSubmit", "session_id": "s-claude-1"}),
    );
    fake.lock().unwrap().screen = format!(
        "{}\n❯ Continue\n",
        include_str!("../tests/fixtures/claude/interrupted-turn.txt")
    );
    stage
        .sample_work("acme-shop", &watched, turn2_ms + 1_000)
        .await
        .unwrap();
    stage
        .nudge(
            "acme-shop",
            &dir,
            &state,
            std::slice::from_ref(&term),
            turn2_ms + 1_000,
        )
        .unwrap();
    let summary_turn2 = stage.summary_view_at(&BTreeMap::new(), turn2_ms + 1_000);
    let prod_turn2 = summary_turn2.studios[0]
        .roles
        .iter()
        .find(|r| r.role == "producer")
        .unwrap();
    assert_eq!(prod_turn2.work, Some(Work::Working));
    assert_eq!(prod_turn2.idle_since_ms, None);

    // Interrupting Turn 2 as well (a second marker on screen) immediately ends Turn 2.
    fake.lock().unwrap().screen = format!(
        "{}\n{}",
        include_str!("../tests/fixtures/claude/interrupted-turn.txt"),
        include_str!("../tests/fixtures/claude/interrupted-turn.txt"),
    );
    stage
        .sample_work("acme-shop", &watched, turn2_ms + 2_000)
        .await
        .unwrap();
    assert_eq!(work_of(&stage, "producer"), Some(Work::Waiting));
}

/// Fail-first / regression sample-pass test: when a trustworthy core reports
/// `idle` and the screen reads `Waiting` with no interrupt marker, a fresh
/// `UserPromptSubmit` stays `Working` during the grace window, then becomes
/// `Waiting` once stale (tested for both fork-inference `claude` and native
/// 7501 `agy`; `fake` `ScreenOnly` stays signal-driven).
#[tokio::test]
async fn sample_pass_stale_working_signal_yields_to_idle_core_and_waiting_screen() {
    let base = tmp();
    let dir = setup_claude_studio(base.path());
    let state = store::load_state(&dir).unwrap();
    let t0 = state.roles["producer"].since_ms + 60_000;
    let term = LiveTerminal {
        name: "bl-acme-shop-producer".into(),
        address: "t-prod".into(),
        cwd: "/home/user/acme-shop".into(),
        status: Some(hc_proto::terminal::Status {
            state: "idle".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let (stage, fake) = stage().await;
    fake.lock().unwrap().screen = "❯ \n".into();
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0, "event": "UserPromptSubmit", "session_id": "s-claude-1"}),
    );
    let watched = Watched {
        dir: &dir,
        state: &state,
        role: "producer",
        runner: "claude",
        term: &term,
    };
    stage
        .sample_work("acme-shop", &watched, t0 + 1_000)
        .await
        .unwrap();
    assert_eq!(work_of(&stage, "producer"), Some(Work::Working));

    stage
        .sample_work("acme-shop", &watched, t0 + 10_000)
        .await
        .unwrap();
    assert_eq!(work_of(&stage, "producer"), Some(Work::Waiting));

    // Native OSC 7501 runner (`agy`) also yields past grace on `idle` + waiting composer.
    fake.lock().unwrap().screen = include_str!("../tests/fixtures/agy/idle-after-turn.txt").into();
    let agy_watched = Watched {
        dir: &dir,
        state: &state,
        role: "producer",
        runner: "agy",
        term: &term,
    };
    emit(
        &dir,
        "producer",
        json!({"ts_ms": t0 + 20_000, "event": "UserPromptSubmit", "session_id": "s-claude-1"}),
    );
    stage
        .sample_work("acme-shop", &agy_watched, t0 + 21_000)
        .await
        .unwrap();
    assert_eq!(work_of(&stage, "producer"), Some(Work::Working));
    stage
        .sample_work("acme-shop", &agy_watched, t0 + 30_000)
        .await
        .unwrap();
    assert_eq!(work_of(&stage, "producer"), Some(Work::Waiting));
}
