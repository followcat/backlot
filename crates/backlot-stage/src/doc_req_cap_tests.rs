//! Queue overflow, client timeout, expiry, future-skew, restart dedupe,
//! 24h pruning, refusals, and ack/result size-cap tests for B1a docs.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use backlot_model::event::{ActorIdentity, EventKind, StudioMode};
use backlot_model::hollywood::{CardAction, CardBase, HollywoodKind, NewHollywoodEvent};
use backlot_model::req_wire::{
    DEDUPE_TTL_MS, MAX_DOC_BYTES, MAX_REQ_QUEUE, MAX_RES_ACKS, MAX_RESULT_BYTES, REQ_DOC_NAME,
    REQ_FUTURE_SKEW_MS, REQ_SCHEMA, REQ_TTL_MS, RES_DOC_NAME, ReqAction, ReqEnvelope, ResDoc,
};
use backlot_model::{EventBy, NewEvent};
use serde_json::json;

use crate::cli::Env;
use crate::daemon::Stage;
use crate::doc_client;
use crate::doc_req::Consumer;
use crate::doc_req_fake_core::FakeCore;
use crate::store::{self, HPending, Pending};
use crate::test_support::TestDir;

fn env_for(dir: &TestDir) -> Env {
    let mut vars = BTreeMap::new();
    vars.insert(
        "HOME".into(),
        dir.join("home").to_string_lossy().into_owned(),
    );
    vars.insert(
        "BACKLOT_CONFIG".into(),
        dir.join("config.toml").to_string_lossy().into_owned(),
    );
    vars.insert(
        "BACKLOT_STATE_DIR".into(),
        dir.join("state").to_string_lossy().into_owned(),
    );
    Env { vars }
}

fn actor_fake() -> ActorIdentity {
    ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: "model-f".into(),
        effort: Some("medium".into()),
        permission_profile: "standard".into(),
    }
}

fn seed_studio(base: &Path, name: &str) {
    let dir = store::studio_dir(base, name);
    fs::create_dir_all(dir.join("repo")).unwrap();
    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("user"),
                EventKind::StudioCreated {
                    title: name.into(),
                    repo: dir.join("repo").to_string_lossy().into_owned(),
                    members: Vec::new(),
                    no_git: false,
                    mode: StudioMode::Standard,
                    roles: Box::new(backlot_model::event::RoleConfigs {
                        producer: actor_fake(),
                        director: actor_fake(),
                        writer: actor_fake(),
                    }),
                    candidates: Vec::new(),
                    rework_cap: 3,
                    stall_limit_s: 300,
                    nudge_after_s: 60,
                    escalate_after_s: 120,
                    check_env: Vec::new(),
                    checks: Vec::new(),
                    task_budget_s: None,
                    limits: BTreeMap::new(),
                    push: Default::default(),
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
}

fn setup_stage(client: hc_proto::client::Client, env: &Env) -> Stage {
    let mut stage = Stage::for_test(client);
    stage.base = Path::new(env.get("BACKLOT_STATE_DIR").unwrap()).to_path_buf();
    fs::create_dir_all(&stage.base).unwrap();
    stage.env_vars = env.vars.clone();
    stage
}

#[tokio::test]
async fn thirty_third_entry_fails_with_server_busy_and_client_times_out() {
    let dir = TestDir::new("blbusy");
    let core = FakeCore::spawn(dir.join("hc.sock")).await;
    let c = core.client().await;
    let now_ms = crate::live::now_ms();

    for i in 0..MAX_REQ_QUEUE {
        let req = ReqEnvelope::new(
            format!("r-{i}"),
            now_ms,
            &ReqAction::CardShow { id: 1 },
            None,
        )
        .unwrap();
        doc_client::append_request(&c, &req).await.unwrap();
    }
    let thirty_third =
        ReqEnvelope::new("r-33", now_ms, &ReqAction::CardShow { id: 1 }, None).unwrap();
    let err = doc_client::append_request(&c, &thirty_third)
        .await
        .unwrap_err();
    assert!(err.message().contains("server busy"), "{}", err.message());

    let mut raw_q = Vec::new();
    for i in 0..=MAX_REQ_QUEUE {
        raw_q.push(
            serde_json::to_value(
                ReqEnvelope::new(
                    format!("raw-{i}"),
                    now_ms,
                    &ReqAction::CardShow { id: 1 },
                    None,
                )
                .unwrap(),
            )
            .unwrap(),
        );
    }
    core.set_raw_doc(
        REQ_DOC_NAME,
        json!({ "schema": REQ_SCHEMA, "queue": raw_q }),
    );
    let env = env_for(&dir);
    let stage = setup_stage(core.client().await, &env);
    let mut consumer = Consumer::load(&stage.base, now_ms);
    consumer.process(&stage, now_ms).await.unwrap();

    let (_, res_val) = core.doc(RES_DOC_NAME).unwrap();
    let res_doc: ResDoc = serde_json::from_value(res_val).unwrap();
    let raw32 = res_doc.acks.iter().find(|a| a.req_id == "raw-32").unwrap();
    assert!(!raw32.ok);
    assert_eq!(raw32.error.as_deref(), Some("server busy"));

    let err = doc_client::send_request(
        &c,
        None,
        &ReqAction::CardShow { id: 1 },
        None,
        Duration::from_millis(60),
    )
    .await
    .unwrap_err();
    assert!(err.message().contains("timed out"), "{}", err.message());
}

#[tokio::test]
async fn expiry_future_skew_restart_dedupe_and_24h_prune() {
    let dir = TestDir::new("bldedupe");
    let env = env_for(&dir);
    seed_studio(
        Path::new(env.get("BACKLOT_STATE_DIR").unwrap()),
        "acme-shop",
    );

    let core = FakeCore::spawn(dir.join("hc.sock")).await;
    let stage = setup_stage(core.client().await, &env);
    let base_now = 1_000_000_000u64;

    let q = vec![
        serde_json::to_value(
            ReqEnvelope::new(
                "r-exp",
                base_now - REQ_TTL_MS - 1,
                &ReqAction::Tell {
                    studio: "acme-shop".into(),
                    role: "producer".into(),
                    text: "stale".into(),
                },
                None,
            )
            .unwrap(),
        )
        .unwrap(),
        serde_json::to_value(
            ReqEnvelope::new(
                "r-far-future",
                base_now + REQ_FUTURE_SKEW_MS + 1,
                &ReqAction::Tell {
                    studio: "acme-shop".into(),
                    role: "producer".into(),
                    text: "future".into(),
                },
                None,
            )
            .unwrap(),
        )
        .unwrap(),
        serde_json::to_value(
            ReqEnvelope::new(
                "r-once",
                base_now + REQ_FUTURE_SKEW_MS,
                &ReqAction::Tell {
                    studio: "acme-shop".into(),
                    role: "producer".into(),
                    text: "valid".into(),
                },
                None,
            )
            .unwrap(),
        )
        .unwrap(),
    ];
    core.set_raw_doc(REQ_DOC_NAME, json!({ "schema": REQ_SCHEMA, "queue": q }));

    let mut c1 = Consumer::load(&stage.base, base_now);
    c1.process(&stage, base_now).await.unwrap();

    let (_, res_val) = core.doc(RES_DOC_NAME).unwrap();
    let res_doc: ResDoc = serde_json::from_value(res_val).unwrap();
    let ack_exp = res_doc.acks.iter().find(|a| a.req_id == "r-exp").unwrap();
    assert!(!ack_exp.ok);
    assert_eq!(ack_exp.error.as_deref(), Some("request expired"));

    let ack_fut = res_doc
        .acks
        .iter()
        .find(|a| a.req_id == "r-far-future")
        .unwrap();
    assert!(!ack_fut.ok);

    assert!(
        res_doc
            .acks
            .iter()
            .find(|a| a.req_id == "r-once")
            .unwrap()
            .ok
    );

    drop(c1);
    core.set_raw_doc(
        REQ_DOC_NAME,
        json!({
            "schema": REQ_SCHEMA,
            "queue": [ReqEnvelope::new("r-once", base_now + 100, &ReqAction::Tell {
                studio: "acme-shop".into(),
                role: "producer".into(),
                text: "should-not-repeat".into(),
            }, None).unwrap()]
        }),
    );
    let mut c2 = Consumer::load(&stage.base, base_now + 5_000);
    c2.process(&stage, base_now + 5_000).await.unwrap();

    let (events, _) = store::load_events(&store::studio_dir(&stage.base, "acme-shop")).unwrap();
    let queued_count = events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::MessageQueued { .. }))
        .count();
    assert_eq!(queued_count, 1, "replayed req_id must not run twice");

    let later = base_now + DEDUPE_TTL_MS + 1;
    assert!(c2.dedupe_mut().sync_prune(later).unwrap());
    let mut reloaded = Consumer::load(&stage.base, later);
    assert!(!reloaded.dedupe_mut().contains("r-once"));
}

#[tokio::test]
async fn unknown_action_approve_outward_and_prompt_attention_refusals() {
    let dir = TestDir::new("blrefuse");
    let env = env_for(&dir);
    let base = Path::new(env.get("BACKLOT_STATE_DIR").unwrap());
    seed_studio(base, "acme-shop");

    let sdir = store::studio_dir(base, "acme-shop");
    for reason in [
        backlot_model::event::PERMISSION_PROMPT,
        backlot_model::event::QUESTION,
        backlot_model::event::FOLDER_TRUST,
    ] {
        store::append(&sdir, |_, _| {
            Ok::<_, std::convert::Infallible>(Pending {
                event: NewEvent::new(
                    EventBy::role("stage"),
                    EventKind::AttentionRaised {
                        role: "director".into(),
                        reason: reason.into(),
                        detail: "confirm".into(),
                        needs_user: true,
                    },
                ),
                files: Vec::new(),
            })
        })
        .unwrap();
    }

    let core = FakeCore::spawn(dir.join("hc.sock")).await;
    let stage = setup_stage(core.client().await, &env);
    let now_ms = crate::live::now_ms();

    let q = vec![
        json!({"req_id": "bad-unknown", "ts_ms": now_ms, "action": {"kind": "no-such-verb"}}),
        json!({"req_id": "bad-approve", "ts_ms": now_ms, "action": {"kind": "approve", "studio": "acme-shop", "person": "director", "decision": "allow"}}),
        json!({"req_id": "bad-shell", "ts_ms": now_ms, "action": {"kind": "shell", "cmd": "rm -rf /"}}),
        json!({"req_id": "bad-prompt-clear", "ts_ms": now_ms, "action": {"kind": "attention-clear", "studio": "acme-shop", "target": 2}}),
        json!({"req_id": "bad-question-clear", "ts_ms": now_ms, "action": {"kind": "attention-clear", "studio": "acme-shop", "target": 3}}),
        json!({"req_id": "bad-trust-clear", "ts_ms": now_ms, "action": {"kind": "attention-clear", "studio": "acme-shop", "target": 4}}),
        serde_json::to_value(
            ReqEnvelope::new(
                "good-sibling",
                now_ms,
                &ReqAction::Tell {
                    studio: "acme-shop".into(),
                    role: "producer".into(),
                    text: "survives bad siblings".into(),
                },
                None,
            )
            .unwrap(),
        )
        .unwrap(),
    ];
    core.set_raw_doc(REQ_DOC_NAME, json!({ "schema": REQ_SCHEMA, "queue": q }));
    let mut consumer = Consumer::load(&stage.base, now_ms);
    consumer.process(&stage, now_ms).await.unwrap();

    let (_, res_val) = core.doc(RES_DOC_NAME).unwrap();
    let res_doc: ResDoc = serde_json::from_value(res_val).unwrap();
    for bad in [
        "bad-unknown",
        "bad-approve",
        "bad-shell",
        "bad-prompt-clear",
        "bad-question-clear",
        "bad-trust-clear",
    ] {
        let ack = res_doc.acks.iter().find(|a| a.req_id == bad).unwrap();
        assert!(!ack.ok, "{bad} must be refused");
    }
    assert!(
        res_doc
            .acks
            .iter()
            .find(|a| a.req_id == "good-sibling")
            .unwrap()
            .ok
    );
}

#[tokio::test]
async fn ack_and_result_size_caps_hold_under_large_payloads() {
    let dir = TestDir::new("blcaps");
    let env = env_for(&dir);
    let base = Path::new(env.get("BACKLOT_STATE_DIR").unwrap());
    let hdir = store::hollywood_dir(base);
    fs::create_dir_all(hdir.join("drafts/huge")).unwrap();
    fs::write(hdir.join("drafts/huge/cast.toml"), "[producer]\nrunner='fake'\nmodel='m'\npermission_profile='standard'\n[director]\nrunner='fake'\nmodel='m'\npermission_profile='standard'\n[writer]\nrunner='fake'\nmodel='m'\npermission_profile='standard'\n").unwrap();
    fs::write(
        hdir.join("drafts/huge/brief.md"),
        "x".repeat(MAX_RESULT_BYTES + 1024),
    )
    .unwrap();
    let card = store::append_hollywood(&hdir, |_, _| {
        Ok::<HPending, crate::cli::Fail>(HPending {
            event: NewHollywoodEvent::new(
                EventBy::role("hollywood"),
                HollywoodKind::CardProposed {
                    action: CardAction::StudioNew {
                        studio: "huge".into(),
                        title: "Huge".into(),
                        repo: "/home/user/huge".into(),
                        cast: "drafts/huge/cast.toml".into(),
                        brief: "drafts/huge/brief.md".into(),
                        no_git: true,
                    },
                    why: "size test".into(),
                    base: CardBase::StudioAbsent,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();

    let core = FakeCore::spawn(dir.join("hc.sock")).await;
    let stage = setup_stage(core.client().await, &env);
    let now_ms = crate::live::now_ms();

    let big_req = ReqEnvelope::new(
        "big-card-show",
        now_ms,
        &ReqAction::CardShow { id: card.seq },
        None,
    )
    .unwrap();
    core.set_raw_doc(
        REQ_DOC_NAME,
        json!({ "schema": REQ_SCHEMA, "queue": [big_req] }),
    );
    let mut consumer = Consumer::load(&stage.base, now_ms);
    consumer.process(&stage, now_ms).await.unwrap();
    let (_, res_val) = core.doc(RES_DOC_NAME).unwrap();
    let res_doc: ResDoc = serde_json::from_value(res_val).unwrap();
    let big_ack = res_doc
        .acks
        .iter()
        .find(|a| a.req_id == "big-card-show")
        .unwrap();
    assert!(!big_ack.ok);
    assert!(big_ack.error.as_deref().unwrap().contains("65536"));

    for batch in 0..3 {
        let mut q = Vec::new();
        for i in 0..24 {
            let n = batch * 24 + i;
            q.push(
                ReqEnvelope::new(
                    format!("cap-{n}"),
                    now_ms,
                    &ReqAction::CardCancel {
                        id: 9999 + n as u64,
                        reason: "nope".into(),
                    },
                    None,
                )
                .unwrap(),
            );
        }
        core.set_raw_doc(REQ_DOC_NAME, json!({ "schema": REQ_SCHEMA, "queue": q }));
        consumer.process(&stage, now_ms).await.unwrap();
    }
    let (_, res_val2) = core.doc(RES_DOC_NAME).unwrap();
    let serialized = serde_json::to_vec(&res_val2).unwrap();
    assert!(serialized.len() < MAX_DOC_BYTES);
    let res_doc2: ResDoc = serde_json::from_value(res_val2).unwrap();
    assert_eq!(res_doc2.acks.len(), MAX_RES_ACKS);
    assert!(res_doc2.acks.iter().all(|a| a.req_id != "cap-0"));
    assert!(res_doc2.acks.iter().any(|a| a.req_id == "cap-71"));
}
