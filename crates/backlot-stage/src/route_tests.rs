//! The routing rule (field notes 1126 §5): an actor's open
//! `permission prompt` — one only the director could answer — becomes
//! the person's when the director cannot answer it. The pure decision
//! (`blocker`, `due`, `detail`) and the ledger effect on a fake core
//! both.

use std::path::Path;
use std::sync::{Arc, Mutex};

use backlot_model::event::{EventKind, PERMISSION_PROMPT};
use backlot_model::{EventBy, NewEvent};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use hc_proto::frame::{Frame, Kind};
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, DuplexStream};

use crate::daemon::Stage;
use crate::route::{Blocker, ROUTE_GRACE_MS, blocker, detail, due};
use crate::store::{self, Pending};
use crate::test_support::TestDir;

/// A rate limit beats a stall and a closed director; a live director
/// with neither open can answer.
#[test]
fn the_blocker_prefers_the_most_specific_reason() {
    assert_eq!(
        blocker(Some(5), true, true, false),
        Some(Blocker::RateLimited(Some(5)))
    );
    assert_eq!(
        blocker(None, true, false, true),
        Some(Blocker::RateLimited(None))
    );
    assert_eq!(blocker(None, false, true, true), Some(Blocker::Stalled));
    assert_eq!(blocker(None, false, false, false), Some(Blocker::Gone));
    assert_eq!(blocker(None, false, false, true), None);
}

/// Past the grace a prompt is due; before it is not.
#[test]
fn the_grace_is_one_minute() {
    assert!(!due(1_000, 1_000 + ROUTE_GRACE_MS - 1));
    assert!(due(1_000, 1_000 + ROUTE_GRACE_MS));
}

/// The detail names the reason and the wait — the reset time for a
/// rate limit.
#[test]
fn the_detail_names_the_reason_and_wait() {
    let d = detail(
        Blocker::RateLimited(Some(1_700_000_000_000)),
        14 * 60_000,
        0,
    );
    assert!(d.contains("rate-limited until"), "{d}");
    assert!(d.contains("waiting 14m"), "{d}");
    let d = detail(Blocker::RateLimited(None), 14 * 60_000, 0);
    assert!(d.contains("rate-limited"), "{d}");
    assert!(d.contains("waiting 14m"), "{d}");
    let d = detail(Blocker::Stalled, 3 * 60_000, 0);
    assert!(d.contains("stalled"), "{d}");
    assert!(d.contains("waiting 3m"), "{d}");
    let d = detail(Blocker::Gone, 60_000, 0);
    assert!(d.contains("closed"), "{d}");
    assert!(d.contains("waiting 1m"), "{d}");
}

fn tmp() -> TestDir {
    TestDir::new("backlot-route-test")
}

/// The fake core's reads only: the route pass never sends a key or a
/// message.
#[derive(Default)]
struct FakeCore {
    screen: String,
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
                    "methods": ["terminal.read"],
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
        if request["method"] != "terminal.read" {
            continue;
        }
        let result = json!({
            "text": fake.lock().unwrap().screen.clone(),
            "complete": true,
            "how": "screen",
        });
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

async fn stage() -> Stage {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    let fake = Arc::new(Mutex::new(FakeCore::default()));
    tokio::spawn(serve(core_side, fake));
    let (read, write) = tokio::io::split(client_side);
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    Stage::for_test(client)
}

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
                    candidates: vec![backlot_model::event::Candidate {
                        name: "actor-a".into(),
                        runner: "fake".into(),
                        runner_version: None,
                        model: "plain".into(),
                        effort: Some("high".into()),
                        permission_profile: "standard".into(),
                    }],
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

/// One `attention.raised` on `role`; `needs_user` marks it the
/// person's already.
fn raise(dir: &Path, role: &str, reason: &str, needs_user: bool) {
    let (role, reason) = (role.to_string(), reason.to_string());
    store::append(dir, move |_, seq| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("stage"),
                EventKind::AttentionRaised {
                    role,
                    reason,
                    detail: format!("screen saved to attention/{seq}.txt"),
                    needs_user,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
}

fn attentions(dir: &Path, reason: &str) -> Vec<backlot_model::state::Attention> {
    store::load_state(dir)
        .unwrap()
        .attention
        .into_iter()
        .filter(|a| a.reason == reason)
        .collect()
}

/// The role's open prompt timestamp — the store stamped it with the
/// wall clock, so the grace is measured from the real raise.
fn open_ts(dir: &Path, role: &str) -> u64 {
    attentions(dir, PERMISSION_PROMPT)
        .into_iter()
        .find(|a| a.open && a.role == role)
        .map(|a| a.ts_ms)
        .unwrap_or(0)
}

/// A rate-limited director re-routes the actor's prompt to the person
/// past the grace: the old attention clears `routed to the user`, a
/// fresh `needs_user` one raises with the reason and the wait, and
/// the director's `approve` is refused.
#[tokio::test]
async fn a_limited_director_routes_the_prompt_to_the_user() {
    let dir = tmp();
    studio(&dir);
    let st = stage().await;
    raise(&dir, "director", crate::ratelimit::RATE_LIMITED, false);
    raise(&dir, "actor-t1-c1", PERMISSION_PROMPT, false);
    let ts = open_ts(&dir, "actor-t1-c1");

    // Before the grace: nothing moves.
    let state = store::load_state(&dir).unwrap();
    assert!(
        !st.route_prompts("acme-shop", &dir, &state, ts + 1_000)
            .await
            .unwrap()
    );
    let open: Vec<_> = attentions(&dir, PERMISSION_PROMPT)
        .into_iter()
        .filter(|a| a.open)
        .collect();
    assert_eq!(open.len(), 1);
    assert!(!open[0].needs_user);

    // Past the grace: the old one clears, a `needs_user` one takes
    // its place naming the reason and the wait.
    let state = store::load_state(&dir).unwrap();
    assert!(
        st.route_prompts("acme-shop", &dir, &state, ts + ROUTE_GRACE_MS)
            .await
            .unwrap()
    );
    let all = attentions(&dir, PERMISSION_PROMPT);
    assert_eq!(all.len(), 2, "{all:?}");
    assert!(!all[0].open);
    assert!(
        all[0]
            .cleared_reason
            .as_deref()
            .unwrap_or_default()
            .contains("routed to the user"),
        "{:?}",
        all[0].cleared_reason
    );
    let now_open = all.iter().find(|a| a.open).unwrap();
    assert!(now_open.needs_user, "{now_open:?}");
    assert!(
        now_open.detail.contains("rate-limited") && now_open.detail.contains("waiting"),
        "{now_open:?}"
    );

    // The re-raise is the user's: the director's approval is refused.
    let refused = store::append(&dir, move |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("director"),
                EventKind::RoleApproval {
                    role: "actor-t1-c1".into(),
                    decision: backlot_model::event::ApprovalDecision::Allow,
                },
            ),
            files: Vec::new(),
        })
    });
    assert!(
        refused.is_err(),
        "the director answered a user-owned prompt"
    );
}

/// A stall routes too, with the stall named in the detail; a live
/// director with nothing open leaves the prompt alone.
#[tokio::test]
async fn a_stalled_director_routes_and_a_healthy_one_does_not() {
    let st = stage().await;

    // Stalled: an open `stalled` attention on the director.
    let dir = tmp();
    studio(&dir);
    raise(&dir, "director", backlot_model::event::STALLED, false);
    raise(&dir, "actor-t1-c1", PERMISSION_PROMPT, false);
    let ts = open_ts(&dir, "actor-t1-c1");
    let state = store::load_state(&dir).unwrap();
    assert!(
        st.route_prompts("acme-shop", &dir, &state, ts + ROUTE_GRACE_MS)
            .await
            .unwrap()
    );
    let now_open = attentions(&dir, PERMISSION_PROMPT)
        .into_iter()
        .find(|a| a.open)
        .unwrap();
    assert!(now_open.needs_user);
    assert!(now_open.detail.contains("stalled"), "{now_open:?}");

    // A live director with no attention open: never routed.
    let dir2 = tmp();
    studio(&dir2);
    store::append(&dir2, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("director"),
                EventKind::RoleStarted {
                    role: "director".into(),
                    terminal: None,
                    actor: Some(actor()),
                    session_id: None,
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    raise(&dir2, "actor-t1-c1", PERMISSION_PROMPT, false);
    let ts2 = open_ts(&dir2, "actor-t1-c1");
    let state = store::load_state(&dir2).unwrap();
    assert!(
        !st.route_prompts("acme-shop", &dir2, &state, ts2 + ROUTE_GRACE_MS)
            .await
            .unwrap()
    );
    let after = attentions(&dir2, PERMISSION_PROMPT);
    assert_eq!(after.iter().filter(|a| a.open).count(), 1);
    assert!(!after[0].needs_user, "{after:?}");
}

/// A director's own prompt and an already-flagged actor prompt are
/// never routed — only an actor's unflagged one is.
#[tokio::test]
async fn only_an_actors_unflagged_prompt_is_routed() {
    let dir = tmp();
    studio(&dir);
    let st = stage().await;
    raise(&dir, "director", PERMISSION_PROMPT, false);
    raise(&dir, "actor-t1-c1", PERMISSION_PROMPT, true);
    let ts = open_ts(&dir, "actor-t1-c1");
    let state = store::load_state(&dir).unwrap();
    assert!(
        !st.route_prompts("acme-shop", &dir, &state, ts + ROUTE_GRACE_MS)
            .await
            .unwrap()
    );
    let open = attentions(&dir, PERMISSION_PROMPT);
    assert_eq!(open.iter().filter(|a| a.open).count(), 2);
    assert!(open.iter().any(|a| a.role == "director" && !a.needs_user));
    assert!(open.iter().any(|a| a.role == "actor-t1-c1" && a.needs_user));
}
