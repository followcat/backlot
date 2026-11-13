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

/// The fake core's memory: the screen `terminal.read` returns, the
/// keys `terminal.keys` sent it, and the texts `terminal.send`
/// delivered.
#[derive(Default)]
struct FakeCore {
    screen: String,
    keys: Vec<String>,
    sends: Vec<String>,
}

/// Serve `hello`, then `terminal.read` from the shared screen,
/// `terminal.keys` into the shared key list and `terminal.send`
/// into the shared send list. Ends on a closed wire.
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
                    "methods": ["terminal.read", "terminal.keys", "terminal.send"],
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
    studio_with_limit(dir, 600);
}

/// A studio with `stall_limit_s` seconds and one `actor-a`
/// candidate, so a seeded `task.cast` validates.
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
                        producer: actor(),
                        director: actor(),
                        writer: actor(),
                    }),
                    candidates: vec![backlot_model::event::Candidate {
                        name: "actor-a".into(),
                        runner: "fake".into(),
                        runner_version: None,
                        model: "plain".into(),
                        effort: Some("high".into()),
                        permission_profile: "standard".into(),
                    }],
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
}

/// Requirement v1 plus task `t1` cast to `actor-a`: the ledger
/// replays it `running` under `actor-t1-c1`.
fn running_task(dir: &Path) {
    for kind in [
        EventKind::ReqVersion {
            version: 1,
            file: "req/v1.md".into(),
            summary: "first".into(),
        },
        EventKind::TaskCreated {
            task: "t1".into(),
            title: "front".into(),
            task_kind: "feature".into(),
            requirement_version: 1,
            acceptance: "plan/a.md".into(),
            dependencies: Vec::new(),
        },
        EventKind::TaskCast {
            task: "t1".into(),
            candidate: "actor-a".into(),
            actor: actor(),
            reason: "picked".into(),
            priority: None,
        },
    ] {
        store::append(dir, move |_, _| {
            Ok::<_, std::convert::Infallible>(Pending {
                event: NewEvent::new(EventBy::role("director"), kind),
                files: Vec::new(),
            })
        })
        .unwrap();
    }
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

/// A blocked screen reaching for `BACKLOT_*` — the role asking to
/// change who it runs as — raises a `needs_user` attention the
/// director may not answer (S9b): the ledger refuses the
/// director's `role.approval`, and the user's sends the key.
#[tokio::test]
async fn a_prompt_reaching_for_backlot_vars_is_the_users() {
    let dir = tmp();
    studio(&dir);
    let term = term();
    let (stage, fake) = stage().await;
    fake.lock().unwrap().screen = "fake runner: actor-t1-c1 of acme-shop\n\
                                   fake-work: blocked\n\
                                   Do you want to proceed?\n\
                                   Bash command: BACKLOT_ROLE=director backlot review --task t1\n"
        .into();
    let t0 = now_ms();

    sample(&stage, &dir, &term, "actor-t1-c1", t0).await;
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 2_000).await;
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1);
    match &raised[0].kind {
        EventKind::AttentionRaised {
            role,
            reason,
            detail,
            needs_user,
        } => {
            assert_eq!(role, "actor-t1-c1");
            assert_eq!(reason, PERMISSION_PROMPT);
            assert!(needs_user, "the BACKLOT_* reach did not flag");
            assert!(detail.contains("needs the user"), "{detail}");
        }
        _ => unreachable!(),
    }

    // The director's answer refuses — the role gate is append-time,
    // so the same refusal lands through `backlot approve`.
    let refused = store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("director"),
                EventKind::RoleApproval {
                    role: "actor-t1-c1".into(),
                    decision: ApprovalDecision::Deny,
                },
            ),
            files: Vec::new(),
        })
    });
    assert!(refused.is_err(), "the director answered the user's prompt");

    // The user's `allow` still records and sends `enter`.
    approve(&dir, "actor-t1-c1", "user", ApprovalDecision::Allow);
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 3_000).await;
    assert_eq!(fake.lock().unwrap().keys.as_slice(), ["enter"]);
    assert_eq!(events(&dir, "attention.cleared").len(), 1);
}

/// S9c: an approval recorded against prompt A never lands on prompt
/// B. A is answered in the terminal itself; its stale attention is
/// still open when B blocks — so the director's approve records —
/// and the block comparison saves B from A's key: nothing is sent,
/// A's attention clears `prompt changed before the decision was
/// sent`, and B raises its own attention with its own saved screen.
#[tokio::test]
async fn a_decision_for_an_old_prompt_sends_nothing_to_the_new_one() {
    let dir = tmp();
    studio(&dir);
    let term = term();
    let (stage, fake) = stage().await;
    let t0 = now_ms();
    let screen = |tail: &str| format!("fake runner: actor-t1-c1 of acme-shop\n{tail}");
    fake.lock().unwrap().screen = screen("fake-work: blocked\nfake-prompt: rm -rf build\n");

    // A's episode: the attention saves A's screen.
    sample(&stage, &dir, &term, "actor-t1-c1", t0).await;
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 2_000).await;
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1);
    let seq_a = raised[0].seq;

    // A is answered directly, then B blocks inside the window: the
    // stale attention bridges the brief unblocked screen — B raises
    // nothing while it is open.
    fake.lock().unwrap().screen = screen("fake-approved: enter\nfake-work: waiting\n");
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 3_000).await;
    fake.lock().unwrap().screen = screen("fake-work: blocked\nfake-prompt: cargo test --lib\n");
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 4_000).await;
    assert_eq!(events(&dir, "attention.raised").len(), 1);

    // The director approves the prompt it was told about — A's. The
    // screen on show is B's: no key goes out, the stale attention
    // clears `prompt changed`, and B raises its own.
    approve(&dir, "actor-t1-c1", "director", ApprovalDecision::Allow);
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 5_000).await;
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
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 2, "the new prompt raised no attention");
    let seq_b = raised[1].seq;
    assert_ne!(seq_b, seq_a);
    let saved = fs::read_to_string(dir.join(format!("attention/{seq_b}.txt"))).unwrap();
    assert!(saved.contains("fake-prompt: cargo test --lib"), "{saved}");
    let state = store::load_state(&dir).unwrap();
    assert_eq!(open_attentions(&state, "actor-t1-c1"), 1);

    // B's own approval then sends the key — against B's block.
    approve(&dir, "actor-t1-c1", "director", ApprovalDecision::Allow);
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 6_000).await;
    assert_eq!(fake.lock().unwrap().keys.as_slice(), ["enter"]);
    assert_eq!(events(&dir, "attention.cleared").len(), 2);
}

/// The same race where B's prompt text reaches for `BACKLOT_*`: the
/// fresh attention computes `needs_user` on B's screen — A's
/// unflagged attention does not shield it — and the ledger refuses
/// the director's approve on it.
#[tokio::test]
async fn a_replaced_prompt_recomputes_needs_user() {
    let dir = tmp();
    studio(&dir);
    let term = term();
    let (stage, fake) = stage().await;
    let t0 = now_ms();
    let screen = |tail: &str| format!("fake runner: actor-t1-c1 of acme-shop\n{tail}");
    fake.lock().unwrap().screen = screen("fake-work: blocked\nfake-prompt: cargo fmt\n");
    sample(&stage, &dir, &term, "actor-t1-c1", t0).await;
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 2_000).await;
    assert_eq!(events(&dir, "attention.raised").len(), 1);

    // A answered directly; B blocks with a BACKLOT_* reach.
    fake.lock().unwrap().screen = screen("fake-approved: enter\nfake-work: waiting\n");
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 3_000).await;
    fake.lock().unwrap().screen =
        screen("fake-work: blocked\nfake-prompt: BACKLOT_ROLE=director backlot review --task t1\n");
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 4_000).await;
    approve(&dir, "actor-t1-c1", "director", ApprovalDecision::Allow);
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 5_000).await;
    assert_eq!(fake.lock().unwrap().keys.as_slice(), Vec::<String>::new());

    // B's attention is the user's: `needs_user` recomputed on B's
    // screen, and the director may not record an answer on it.
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 2);
    match &raised[1].kind {
        EventKind::AttentionRaised {
            needs_user, detail, ..
        } => {
            assert!(*needs_user, "the BACKLOT_* reach did not reflag");
            assert!(detail.contains("needs the user"), "{detail}");
        }
        _ => unreachable!(),
    }
    let refused = store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("director"),
                EventKind::RoleApproval {
                    role: "actor-t1-c1".into(),
                    decision: ApprovalDecision::Deny,
                },
            ),
            files: Vec::new(),
        })
    });
    assert!(refused.is_err(), "the director answered the user's prompt");
}

/// S9c: the `needs_user` flag reads the prompt's own block, not the
/// whole screen — an earlier prompt's `BACKLOT_*` line still
/// scrolling above the new prompt must not flag it. B's own reach
/// flags B; once the user's `allow` has answered B, C raises
/// unflagged although B's line is still on screen.
#[tokio::test]
async fn a_prompts_flag_comes_from_its_own_block() {
    let dir = tmp();
    studio(&dir);
    let term = term();
    let (stage, fake) = stage().await;
    let t0 = now_ms();
    let screen = |tail: &str| format!("fake runner: actor-t1-c1 of acme-shop\n{tail}");
    fake.lock().unwrap().screen =
        screen("fake-work: blocked\nfake-prompt: BACKLOT_ROLE=director backlot review --task t1\n");

    // B's own reach makes B the user's prompt.
    sample(&stage, &dir, &term, "actor-t1-c1", t0).await;
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 2_000).await;
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1);
    match &raised[0].kind {
        EventKind::AttentionRaised { needs_user, .. } => {
            assert!(*needs_user, "B's own BACKLOT_* reach did not flag");
        }
        _ => unreachable!(),
    }

    // The user answers B; the key goes out against B's block.
    approve(&dir, "actor-t1-c1", "user", ApprovalDecision::Allow);
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 3_000).await;
    assert_eq!(fake.lock().unwrap().keys.as_slice(), ["enter"]);

    // C blocks with B's prompt line still scrolling above it — the
    // whole screen still carries `BACKLOT_*`, but C's own block is
    // clean.
    fake.lock().unwrap().screen = screen(
        "fake-work: blocked\n\
         fake-prompt: BACKLOT_ROLE=director backlot review --task t1\n\
         fake-approved: enter\n\
         fake-work: waiting\n\
         fake-work: blocked\n\
         fake-prompt: cargo test --lib\n",
    );
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 4_000).await;
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 6_000).await;
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 2, "C's prompt raised no attention");
    match &raised[1].kind {
        EventKind::AttentionRaised {
            needs_user, detail, ..
        } => {
            assert!(!needs_user, "B's leftover line flagged C");
            assert!(!detail.contains("needs the user"), "{detail}");
        }
        _ => unreachable!(),
    }
}

/// S9c: a prompt answered in the terminal itself must not hold its
/// attention open — a stale attention swallows the next prompt's
/// raise and lets a later approval fly at it. Once the role has been
/// out of its blocked episode for a full window with no decision
/// pending, the attention clears `prompt answered elsewhere` and the
/// next prompt raises its own.
#[tokio::test]
async fn a_prompt_answered_elsewhere_clears_and_the_next_raises_its_own() {
    let dir = tmp();
    studio(&dir);
    let term = term();
    let (stage, fake) = stage().await;
    let t0 = now_ms();
    let screen = |tail: &str| format!("fake runner: actor-t1-c1 of acme-shop\n{tail}");
    fake.lock().unwrap().screen = screen("fake-work: blocked\n");
    sample(&stage, &dir, &term, "actor-t1-c1", t0).await;
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 2_000).await;
    assert_eq!(events(&dir, "attention.raised").len(), 1);

    // Answered directly — the screen is no longer blocked. The stale
    // attention survives less than a full window out of the episode,
    // then clears `prompt answered elsewhere`.
    fake.lock().unwrap().screen = screen("fake-approved: enter\nfake-work: waiting\n");
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 3_000).await;
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 4_000).await;
    assert_eq!(
        events(&dir, "attention.cleared").len(),
        0,
        "cleared before the window ran out"
    );
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 5_000).await;
    let cleared = events(&dir, "attention.cleared");
    assert_eq!(cleared.len(), 1);
    match &cleared[0].kind {
        EventKind::AttentionCleared { reason, .. } => {
            assert_eq!(reason.as_deref(), Some("prompt answered elsewhere"));
        }
        _ => unreachable!(),
    }

    // The next prompt raises its own attention — not swallowed by
    // the stale one — with its own saved screen.
    fake.lock().unwrap().screen = screen("fake-work: blocked\nfake-prompt: cargo test --lib\n");
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 6_000).await;
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 8_000).await;
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 2, "the next prompt raised no attention");
    let seq = raised[1].seq;
    let saved = fs::read_to_string(dir.join(format!("attention/{seq}.txt"))).unwrap();
    assert!(saved.contains("fake-prompt: cargo test --lib"), "{saved}");
}

/// A screen frozen past `stall_limit_s` raises one `stalled`
/// attention per episode — the screen saved beside it — a changed
/// screen clears it, and a later freeze raises its own: raised
/// once per episode, cleared on change, re-raised after.
#[tokio::test]
async fn a_frozen_screen_raises_stalls_once_and_clears_on_change() {
    let dir = tmp();
    studio_with_limit(&dir, 4); // 4 s of one screen is a stall
    running_task(&dir);
    let term = term();
    let (stage, fake) = stage().await;
    fake.lock().unwrap().screen =
        "fake runner: actor-t1-c1 of acme-shop\nfake-work: working\n".into();
    let t0 = now_ms();

    // The same_since track starts at the first sample, so the
    // raise lands exactly at the limit.
    for k in 0..4 {
        sample(&stage, &dir, &term, "actor-t1-c1", t0 + k * 1_000).await;
        assert!(
            events(&dir, "attention.raised").is_empty(),
            "raised before the limit at +{k}s"
        );
    }
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 4_000).await;
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1, "the freeze raised nothing");
    let seq = match &raised[0].kind {
        EventKind::AttentionRaised {
            role,
            reason,
            detail,
            ..
        } => {
            assert_eq!(role, "actor-t1-c1");
            assert_eq!(reason, backlot_model::event::STALLED);
            assert!(detail.contains("screen unchanged for 4s"), "{detail}");
            assert!(
                detail.contains(&format!("attention/{}.txt", raised[0].seq)),
                "{detail}"
            );
            raised[0].seq
        }
        _ => unreachable!(),
    };
    let saved = dir.join(format!("attention/{seq}.txt"));
    assert!(
        fs::read_to_string(&saved)
            .unwrap()
            .contains("fake-work: working"),
        "{saved:?} lacks the screen"
    );

    // Two more frozen samples raise nothing — one open attention
    // per role and reason.
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 5_000).await;
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 6_000).await;
    assert_eq!(events(&dir, "attention.raised").len(), 1);

    // A changed screen clears the episode.
    fake.lock().unwrap().screen =
        "fake runner: actor-t1-c1 of acme-shop\nfake-work: waiting\n".into();
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 7_000).await;
    let cleared = events(&dir, "attention.cleared");
    assert_eq!(cleared.len(), 1);
    match &cleared[0].kind {
        EventKind::AttentionCleared { target, reason } => {
            assert_eq!(*target, seq);
            assert_eq!(reason.as_deref(), Some("screen changed"));
        }
        _ => unreachable!(),
    }

    // The new screen freezes too: one limit later it raises its
    // own attention — a lost in-memory track (restart) behaves the
    // same way.
    for k in 8..11 {
        sample(&stage, &dir, &term, "actor-t1-c1", t0 + k * 1_000).await;
        assert_eq!(
            events(&dir, "attention.raised").len(),
            1,
            "re-raised before the new screen's limit at +{k}s"
        );
    }
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 11_000).await;
    assert_eq!(events(&dir, "attention.raised").len(), 2);
}

/// A bare `error` status — no limit text in `status.msg` or the
/// screen — is `agent error`, never `rate limited` (S12d review):
/// the stage raises it once, sends no `continue`, and clears it
/// `status recovered` when the status leaves `error`.
#[tokio::test]
async fn a_plain_error_raises_agent_error_and_sends_nothing() {
    let dir = tmp();
    studio(&dir);
    running_task(&dir);
    let mut term = term();
    term.status = Some(hc_proto::terminal::Status {
        state: "error".into(),
        msg: "transport died".into(),
        ..Default::default()
    });
    let (stage, fake) = stage().await;
    fake.lock().unwrap().screen =
        "fake runner: actor-t1-c1 of acme-shop\nfake-work: working\n".into();
    let t0 = now_ms();

    sample(&stage, &dir, &term, "actor-t1-c1", t0).await;
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1);
    match &raised[0].kind {
        EventKind::AttentionRaised {
            role,
            reason,
            detail,
            needs_user,
        } => {
            assert_eq!(role, "actor-t1-c1");
            assert_eq!(reason, crate::ratelimit::AGENT_ERROR);
            assert!(!needs_user, "an actor's plain error is the director's");
            assert!(detail.contains("transport died"), "{detail}");
        }
        _ => unreachable!(),
    }
    assert!(
        fake.lock().unwrap().sends.is_empty(),
        "a plain error sent a continue"
    );

    // Once per episode — the next errored sample re-raises nothing.
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 1_000).await;
    assert_eq!(events(&dir, "attention.raised").len(), 1);

    // The status leaving `error` clears the attention.
    term.status = Some(hc_proto::terminal::Status {
        state: "working".into(),
        ..Default::default()
    });
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 2_000).await;
    let cleared = events(&dir, "attention.cleared");
    assert_eq!(cleared.len(), 1);
    match &cleared[0].kind {
        EventKind::AttentionCleared { reason, .. } => {
            assert_eq!(reason.as_deref(), Some("status recovered"));
        }
        _ => unreachable!(),
    }
}

/// The other side of the taxonomy: an `error` with observed limit
/// text is `rate limited` — stage-owned — and the reset's
/// auto-`continue` really goes out over `terminal.send`.
#[tokio::test]
async fn an_error_with_limit_text_gets_its_continue() {
    let dir = tmp();
    studio(&dir);
    running_task(&dir);
    let mut term = term();
    term.status = Some(hc_proto::terminal::Status {
        state: "error".into(),
        msg: "denied".into(),
        ..Default::default()
    });
    let (stage, fake) = stage().await;
    fake.lock().unwrap().screen = "fake runner: actor-t1-c1 of acme-shop\n\
                                   fake-work: waiting\n\
                                   usage limit, resets in 30s\n"
        .into();
    let t0 = now_ms();

    // The limit raises the stage-owned attention; the retry pace
    // waits the parsed reset plus the margin.
    sample(&stage, &dir, &term, "actor-t1-c1", t0).await;
    let raised = events(&dir, "attention.raised");
    assert_eq!(raised.len(), 1);
    match &raised[0].kind {
        EventKind::AttentionRaised {
            reason, needs_user, ..
        } => {
            assert_eq!(reason, crate::ratelimit::RATE_LIMITED);
            assert!(!needs_user, "the stage owns the rate-limit episode");
        }
        _ => unreachable!(),
    }
    assert!(fake.lock().unwrap().sends.is_empty());

    // Past the 30s reset + 5s margin the `continue` goes out.
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 30_000).await;
    assert!(fake.lock().unwrap().sends.is_empty());
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 36_000).await;
    assert_eq!(fake.lock().unwrap().sends.as_slice(), ["continue"]);

    // The limit lifting — the status recovers — resolves `resumed`.
    term.status = Some(hc_proto::terminal::Status {
        state: "working".into(),
        ..Default::default()
    });
    sample(&stage, &dir, &term, "actor-t1-c1", t0 + 37_000).await;
    let cleared = events(&dir, "attention.cleared");
    assert_eq!(cleared.len(), 1);
    match &cleared[0].kind {
        EventKind::AttentionCleared { reason, .. } => {
            assert_eq!(reason.as_deref(), Some("resumed"));
        }
        _ => unreachable!(),
    }
}
