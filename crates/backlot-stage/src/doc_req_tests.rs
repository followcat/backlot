//! Catalog republish, immediate `doc`-event wake drain, and concurrent CAS
//! drain tests for B1a request/result/catalog documents.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use backlot_model::event::{ActorIdentity, EventKind, StudioMode};
use backlot_model::req_wire::{
    CAT_DOC_NAME, CAT_SCHEMA, CatalogDoc, REQ_DOC_NAME, RES_DOC_NAME, RES_SCHEMA, ReqAction,
    ReqDoc, ReqEnvelope, ResDoc, RunnerCatalogEntry,
};
use backlot_model::summary::Summary;
use backlot_model::{EventBy, NewEvent};
use hc_proto::events::{EventParams, SubscribeParams, event_kind};
use hc_proto::subscribe::Subscription;
use serde_json::json;

use crate::catalog_doc;
use crate::cli::Env;
use crate::daemon::{Inventory, Stage, step_daemon};
use crate::doc;
use crate::doc_client;
use crate::doc_req::{Consumer, is_req_wake_event};
use crate::doc_req_fake_core::FakeCore;
use crate::store::{self, Pending};
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
                    max_actors: backlot_model::event::default_max_actors(),
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

fn test_runners() -> Vec<RunnerCatalogEntry> {
    vec![RunnerCatalogEntry {
        runner: "fake".into(),
        status: "found".into(),
        version: Some("1.0".into()),
        path: Some("/bin/fake".into()),
        needs_path: false,
        models: vec!["model-f".into()],
        efforts: vec!["medium".into()],
    }]
}

#[tokio::test]
async fn catalog_publishes_on_start_and_republishes_when_hollywood_cfg_changes() {
    let dir = TestDir::new("blcat");
    let mut env = env_for(&dir);
    let fake = dir.join("fake-runner");
    fs::write(&fake, "#!/bin/sh\necho '1.0'\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    }
    env.vars.insert(
        "BACKLOT_FAKE_RUNNER".into(),
        fake.to_string_lossy().into_owned(),
    );

    let core = FakeCore::spawn(dir.join("hc.sock")).await;
    let mut stage = setup_stage(core.client().await, &env);
    let mut throttle = doc::Throttle::default();
    let mut catalog = catalog_doc::Publisher::with_runners("core-fake-1".into(), test_runners());
    let mut consumer = Consumer::load(&stage.base, 10_000);
    let mut live_written = None;
    let mut wake = true;
    let mut last_poll = 0u64;

    step_daemon(
        &mut stage,
        &Inventory::default(),
        &mut throttle,
        &mut catalog,
        &mut consumer,
        &mut live_written,
        &mut wake,
        &mut last_poll,
    )
    .await
    .unwrap();

    assert_eq!(core.doc_writes(CAT_DOC_NAME), 1);
    let (_, val1) = core.doc(CAT_DOC_NAME).unwrap();
    let cat1: CatalogDoc = serde_json::from_value(val1).unwrap();
    assert_eq!(cat1.schema, CAT_SCHEMA);
    assert_eq!(cat1.server_id, "core-fake-1");
    assert!(!cat1.hollywood_cfg.configured);
    assert!(cat1.default_cast.is_some());

    step_daemon(
        &mut stage,
        &Inventory::default(),
        &mut throttle,
        &mut catalog,
        &mut consumer,
        &mut live_written,
        &mut wake,
        &mut last_poll,
    )
    .await
    .unwrap();
    assert_eq!(core.doc_writes(CAT_DOC_NAME), 1);

    let now_ms = crate::live::now_ms();
    let setup_req = ReqEnvelope::new(
        "req-setup-1",
        now_ms,
        &ReqAction::HollywoodSetup {
            runner: "fake".into(),
            model: "model-f".into(),
            effort: Some("high".into()),
            key: Some("alt+y".into()),
            default_cast: None,
            force_offline: false,
        },
        None,
    )
    .unwrap();
    let c2 = core.client().await;
    doc_client::append_request(&c2, &setup_req).await.unwrap();
    wake = true;
    step_daemon(
        &mut stage,
        &Inventory::default(),
        &mut throttle,
        &mut catalog,
        &mut consumer,
        &mut live_written,
        &mut wake,
        &mut last_poll,
    )
    .await
    .unwrap();

    assert_eq!(core.doc_writes(CAT_DOC_NAME), 2);
    let (_, val2) = core.doc(CAT_DOC_NAME).unwrap();
    let cat2: CatalogDoc = serde_json::from_value(val2).unwrap();
    assert!(cat2.hollywood_cfg.configured);
    assert_eq!(cat2.hollywood_cfg.runner.as_deref(), Some("fake"));
    assert_eq!(cat2.hollywood_cfg.model.as_deref(), Some("model-f"));
    assert_eq!(cat2.hollywood_cfg.effort.as_deref(), Some("high"));
}

#[tokio::test]
async fn cas_request_drains_in_one_wake_and_publishes_ack_and_refreshed_summary() {
    let dir = TestDir::new("blwake");
    let env = env_for(&dir);
    let state_base = Path::new(env.get("BACKLOT_STATE_DIR").unwrap());
    seed_studio(state_base, "acme-shop");

    let core = FakeCore::spawn(dir.join("hc.sock")).await;
    let mut stage = setup_stage(core.client().await, &env);
    let mut sub = Subscription::connect(&core.endpoint(), None, &SubscribeParams::default())
        .await
        .unwrap();

    let mut throttle = doc::Throttle::default();
    let mut catalog = catalog_doc::Publisher::with_runners("core-fake-1".into(), test_runners());
    let mut consumer = Consumer::load(&stage.base, 10_000);
    let mut live_written = None;
    let mut wake = true;
    let mut last_poll = 0u64;

    step_daemon(
        &mut stage,
        &Inventory::default(),
        &mut throttle,
        &mut catalog,
        &mut consumer,
        &mut live_written,
        &mut wake,
        &mut last_poll,
    )
    .await
    .unwrap();
    while tokio::time::timeout(Duration::from_millis(10), sub.recv())
        .await
        .is_ok()
    {}

    let cli = core.client().await;
    let ep = core.endpoint();
    let client_task = tokio::spawn(async move {
        doc_client::send_request(
            &cli,
            Some(&ep),
            &ReqAction::Tell {
                studio: "acme-shop".into(),
                role: "producer".into(),
                text: "status check".into(),
            },
            None,
            Duration::from_secs(2),
        )
        .await
    });

    let notif = tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let ev = sub.recv().await.unwrap();
            if is_req_wake_event(&ev) {
                break ev;
            }
        }
    })
    .await
    .expect("expected immediate doc wake for plugin.backlot.req");
    assert!(is_req_wake_event(&notif));
    wake = true;

    let remote_notif = hc_proto::events::ServerNotification::Event(EventParams {
        seq: 99,
        kind: event_kind::DOC.into(),
        terminal: None,
        machine: Some("host-b".into()),
        data: json!({ "name": REQ_DOC_NAME, "version": 1 }),
    });
    assert!(!is_req_wake_event(&remote_notif));

    let drained = step_daemon(
        &mut stage,
        &Inventory::default(),
        &mut throttle,
        &mut catalog,
        &mut consumer,
        &mut live_written,
        &mut wake,
        &mut last_poll,
    )
    .await
    .unwrap();
    assert!(drained);

    let ack = client_task.await.unwrap().unwrap();
    assert!(ack.ok, "unexpected ack error: {:?}", ack.error);
    let (_, req_doc_val) = core.doc(REQ_DOC_NAME).unwrap();
    let req_doc: ReqDoc = serde_json::from_value(req_doc_val).unwrap();
    assert!(req_doc.queue.is_empty());

    let (_, res_doc_val) = core.doc(RES_DOC_NAME).unwrap();
    let res_doc: ResDoc = serde_json::from_value(res_doc_val).unwrap();
    assert_eq!(res_doc.schema, RES_SCHEMA);
    assert_eq!(res_doc.acks.len(), 1);
    assert_eq!(res_doc.acks[0].req_id, ack.req_id);

    let (_, sum_val) = core.doc("backlot").unwrap();
    let summary: Summary = serde_json::from_value(sum_val).unwrap();
    assert!(summary.studios.iter().any(|s| s.id == "acme-shop"));
}

#[tokio::test]
async fn concurrent_append_during_drain_is_preserved_not_lost() {
    let dir = TestDir::new("blcas");
    let env = env_for(&dir);
    seed_studio(
        Path::new(env.get("BACKLOT_STATE_DIR").unwrap()),
        "acme-shop",
    );

    let core = FakeCore::spawn(dir.join("hc.sock")).await;
    let stage = setup_stage(core.client().await, &env);
    let mut consumer = Consumer::load(&stage.base, crate::live::now_ms());

    let now_ms = crate::live::now_ms();
    let req1 = ReqEnvelope::new(
        "req-1",
        now_ms,
        &ReqAction::Tell {
            studio: "acme-shop".into(),
            role: "producer".into(),
            text: "first".into(),
        },
        None,
    )
    .unwrap();
    let req2 = ReqEnvelope::new(
        "req-2",
        now_ms,
        &ReqAction::Tell {
            studio: "acme-shop".into(),
            role: "producer".into(),
            text: "second".into(),
        },
        None,
    )
    .unwrap();

    let c = core.client().await;
    doc_client::append_request(&c, &req1).await.unwrap();
    core.inject_concurrent_req_on_next_set(serde_json::to_value(&req2).unwrap());

    assert!(consumer.process(&stage, now_ms).await.unwrap());

    let (_, req_val) = core.doc(REQ_DOC_NAME).unwrap();
    let req_doc: ReqDoc = serde_json::from_value(req_val).unwrap();
    assert_eq!(req_doc.queue.len(), 1);
    let remaining: ReqEnvelope = serde_json::from_value(req_doc.queue[0].clone()).unwrap();
    assert_eq!(remaining.req_id, "req-2");

    assert!(consumer.process(&stage, now_ms).await.unwrap());
    let (_, req_val2) = core.doc(REQ_DOC_NAME).unwrap();
    let req_doc2: ReqDoc = serde_json::from_value(req_val2).unwrap();
    assert!(req_doc2.queue.is_empty());

    let (events, _) = store::load_events(&store::studio_dir(&stage.base, "acme-shop")).unwrap();
    let msgs: Vec<_> = events
        .into_iter()
        .filter(|e| matches!(e.kind, EventKind::MessageQueued { .. }))
        .collect();
    assert_eq!(msgs.len(), 2);
}
