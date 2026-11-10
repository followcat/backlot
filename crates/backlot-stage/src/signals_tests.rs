//! S9f: the signal file's tail read, the last-line pick, the
//! signal-first combine, the signal stall rule and the StopFailure
//! display map — plus the whole path through `sample_work` on a
//! duplex fake core.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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

static NEXT: AtomicU64 = AtomicU64::new(0);

fn tmp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "backlot-signals-test-{}-{}",
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

fn sig(event: &str, ts_ms: u64) -> Signal {
    Signal {
        ts_ms,
        event: event.into(),
        notification_type: None,
        error: None,
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
    let a = signals::last(tail, "a").unwrap();
    assert_eq!(a.event, "Stop");
    assert_eq!(a.ts_ms, 3);
    let b = signals::last(tail, "b").unwrap();
    assert_eq!(b.event, "PreToolUse");
    assert!(signals::last(tail, "c").is_none());
    assert!(signals::last("", "a").is_none());
}

/// The combine: Blocked wins from either side, otherwise the
/// signal's state — the screen answers only when there is none.
#[test]
fn combine_puts_signals_first_and_blocked_over_all() {
    for (signal, screen, want) in [
        (Some(Work::Blocked), Work::Waiting, Work::Blocked),
        (Some(Work::Waiting), Work::Blocked, Work::Blocked),
        (Some(Work::Working), Work::Waiting, Work::Working),
        (Some(Work::Waiting), Work::Working, Work::Waiting),
        (None, Work::Working, Work::Working),
    ] {
        assert_eq!(
            signals::combine(signal, screen),
            want,
            "{signal:?}/{screen:?}"
        );
    }
}

/// The signal stall rule: a Working signal past the limit stalls a
/// scoped role; a new signal, a non-Working signal, a blocked role
/// and the wrong scope never count.
#[test]
fn the_signal_stall_rule() {
    let s = sig("UserPromptSubmit", 1_000);
    let now = 1_000 + 60_000;
    assert!(stall::signal_stalled(
        "producer",
        None,
        Work::Working,
        &s,
        60_000,
        now
    ));
    assert!(stall::signal_stalled(
        "actor-t1-c1",
        Some(TaskStatus::Running),
        Work::Working,
        &s,
        60_000,
        now
    ));
    // Inside the limit, or not Working: no stall.
    assert!(!stall::signal_stalled(
        "producer",
        None,
        Work::Working,
        &s,
        60_001,
        now
    ));
    assert!(!stall::signal_stalled(
        "producer",
        None,
        Work::Working,
        &sig("Stop", 1_000),
        60_000,
        now
    ));
    // A blocked role's signal never counts — the permission pass
    // owns it.
    assert!(!stall::signal_stalled(
        "producer",
        None,
        Work::Blocked,
        &s,
        60_000,
        now
    ));
    // An actor without a running task does not stall.
    assert!(!stall::signal_stalled(
        "actor-t1-c1",
        Some(TaskStatus::Accepted),
        Work::Working,
        &s,
        60_000,
        now
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
        effort: "high".into(),
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
    // request: the key goes to the screen.
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
            assert!(detail.contains("no hook signal for 61s"), "{detail}");
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

/// `backlot status` shows `last turn failed: <error>` on the role.
#[test]
fn status_shows_the_last_turn_failure() {
    let dir = tmp();
    studio(&dir);
    emit(
        &dir,
        "producer",
        json!({"ts_ms": 1, "event": "StopFailure", "session_id": "s-1", "error": "overloaded"}),
    );
    let state = store::load_state(&dir).unwrap();
    let out = crate::render::status("acme-shop", &state, &signals::failed_roles(&dir, &state), 0);
    assert!(
        out.contains("producer  last turn failed: overloaded"),
        "{out}"
    );
}
