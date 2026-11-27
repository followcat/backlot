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
use crate::route::{Blocker, blocker, detail, due, threshold_ms};
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

/// Past the threshold a prompt is due; before it is not.
#[test]
fn the_threshold_is_ten_minutes() {
    assert!(!due(1_000, 1_000 + threshold_ms() - 1));
    assert!(due(1_000, 1_000 + threshold_ms()));
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
                    account: None,
                    reset_at_ms: None,
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
/// A director with an open `rate limited` attention escalates at once:
/// the existing attention becomes `needs_user` with the reason and
/// wait in its detail, and the director's `approve` is refused.
#[tokio::test]
async fn a_limited_director_routes_the_prompt_to_the_user() {
    let dir = tmp();
    studio(&dir);
    let st = stage().await;
    raise(&dir, "director", crate::ratelimit::RATE_LIMITED, false);
    raise(&dir, "actor-t1-c1", PERMISSION_PROMPT, false);
    let ts = open_ts(&dir, "actor-t1-c1");

    // Rate-limited director escalates at once without waiting.
    let state = store::load_state(&dir).unwrap();
    assert!(
        st.route_prompts("acme-shop", &dir, &state, ts + 1_000)
            .await
            .unwrap()
    );
    let all = attentions(&dir, PERMISSION_PROMPT);
    assert_eq!(all.len(), 1, "{all:?}");
    let now_open = all.iter().find(|a| a.open).unwrap();
    assert!(now_open.needs_user, "{now_open:?}");
    assert!(
        now_open.detail.contains("rate-limited") && now_open.detail.contains("waiting"),
        "{now_open:?}"
    );

    // The escalated attention is the user's: the director's approval is refused.
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

/// A stall routes at once too, with the stall named in the detail; a
/// live director under the threshold leaves the prompt alone.
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
        st.route_prompts("acme-shop", &dir, &state, ts + 1_000)
            .await
            .unwrap()
    );
    let now_open = attentions(&dir, PERMISSION_PROMPT)
        .into_iter()
        .find(|a| a.open)
        .unwrap();
    assert!(now_open.needs_user);
    assert!(now_open.detail.contains("stalled"), "{now_open:?}");

    // A live director under threshold: not routed yet.
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
                    prompt_hash: None,
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
        !st.route_prompts("acme-shop", &dir2, &state, ts2 + 60_000)
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
        !st.route_prompts("acme-shop", &dir, &state, ts + threshold_ms())
            .await
            .unwrap()
    );
    let open = attentions(&dir, PERMISSION_PROMPT);
    assert_eq!(open.iter().filter(|a| a.open).count(), 2);
    assert!(open.iter().any(|a| a.role == "director" && !a.needs_user));
    assert!(open.iter().any(|a| a.role == "actor-t1-c1" && a.needs_user));
}

#[tokio::test]
async fn rate_limited_director_routes_using_true_reset_time() {
    let dir = tmp();
    studio(&dir);
    let st = stage().await;
    raise(&dir, "actor-t1-c1", PERMISSION_PROMPT, false);
    let prompt_ts = open_ts(&dir, "actor-t1-c1");

    // Pick 09:10 local time tomorrow relative to `prompt_ts` so the test
    // is independent of the current wall-clock hour.
    let now = {
        let ts = ((prompt_ts / 1000) + 86_400) as libc::time_t;
        let mut local: libc::tm = unsafe { std::mem::zeroed() };
        unsafe { libc::localtime_r(&ts, &mut local) };
        local.tm_hour = 9;
        local.tm_min = 10;
        local.tm_sec = 0;
        local.tm_isdst = -1;
        (unsafe { libc::mktime(&mut local) } as u64) * 1000
    };

    let term = crate::reconcile::LiveTerminal {
        address: "director".into(),
        status: Some(hc_proto::terminal::Status {
            state: "waiting".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let state = store::load_state(&dir).unwrap();
    let watched = crate::sample::Watched {
        dir: &dir,
        state: &state,
        role: "director",
        runner: "claude",
        term: &term,
    };
    st.rate_limit(
        "acme-shop",
        &watched,
        "You've hit your usage limit · resets 2:10pm\n",
        backlot_model::summary::Work::Waiting,
        now,
    )
    .await
    .unwrap();

    let state = store::load_state(&dir).unwrap();
    let route_now = now + 1_000;
    assert!(
        st.route_prompts("acme-shop", &dir, &state, route_now)
            .await
            .unwrap()
    );
    let now_open = attentions(&dir, PERMISSION_PROMPT)
        .into_iter()
        .find(|a| a.open)
        .unwrap();
    assert!(
        now_open.detail.contains("14:10"),
        "expected unclamped 14:10 reset in routed prompt detail, got: {}",
        now_open.detail
    );
}

#[tokio::test]
async fn rate_limited_director_escalates_actor_prompt_at_once() {
    let dir = tmp();
    studio(&dir);
    let st = stage().await;
    raise(&dir, "director", crate::ratelimit::RATE_LIMITED, false);
    raise(&dir, "actor-t1-c1", PERMISSION_PROMPT, false);
    let ts = open_ts(&dir, "actor-t1-c1");
    let state = store::load_state(&dir).unwrap();
    assert!(
        st.route_prompts("acme-shop", &dir, &state, ts + 1_000)
            .await
            .unwrap(),
        "rate-limited director must escalate actor prompt at once without waiting"
    );
    let open: Vec<_> = attentions(&dir, PERMISSION_PROMPT)
        .into_iter()
        .filter(|a| a.open)
        .collect();
    assert_eq!(open.len(), 1, "{open:?}");
    assert!(open[0].needs_user, "{open:?}");
    assert!(
        open[0].detail.contains("rate-limited") && open[0].detail.contains("waiting 1s"),
        "{}",
        open[0].detail
    );
}

#[tokio::test]
async fn live_director_holds_under_threshold_and_escalates_when_unanswered_past_threshold() {
    let dir = tmp();
    studio(&dir);
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("director"),
                EventKind::RoleStarted {
                    role: "director".into(),
                    terminal: None,
                    actor: Some(actor()),
                    session_id: None,
                    reason: None,
                    prompt_hash: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    let st = stage().await;
    raise(&dir, "actor-t1-c1", PERMISSION_PROMPT, false);
    let ts = open_ts(&dir, "actor-t1-c1");
    let threshold_ms = u64::from(backlot_model::event::default_stall_limit_s()) * 1000;

    let state = store::load_state(&dir).unwrap();
    assert!(
        !st.route_prompts("acme-shop", &dir, &state, ts + threshold_ms - 1)
            .await
            .unwrap(),
        "live director with prompt under threshold must not escalate"
    );
    assert!(!attentions(&dir, PERMISSION_PROMPT)[0].needs_user);

    let state = store::load_state(&dir).unwrap();
    assert!(
        st.route_prompts("acme-shop", &dir, &state, ts + threshold_ms)
            .await
            .unwrap(),
        "live director prompt past threshold must escalate"
    );
    let open: Vec<_> = attentions(&dir, PERMISSION_PROMPT)
        .into_iter()
        .filter(|a| a.open)
        .collect();
    assert_eq!(open.len(), 1, "{open:?}");
    assert!(open[0].needs_user, "{open:?}");
    assert!(open[0].detail.contains("10m"), "{}", open[0].detail);
}
