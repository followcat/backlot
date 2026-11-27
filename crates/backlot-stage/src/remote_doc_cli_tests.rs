//! Fake-core and unit tests for remote CLI execution over
//! `plugin.backlot.req`/`res` and hub-local ledger bypass (B2 items 3 & 4).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use backlot_model::req_wire::{
    MAX_REQ_QUEUE, REQ_DOC_NAME, REQ_SCHEMA, RES_DOC_NAME, RES_SCHEMA, ReqAction, ResAck,
};
use serde_json::json;

use crate::args::{self};
use crate::cli::Env;
use crate::daemon::Stage;
use crate::doc_req::Consumer;
use crate::doc_req_fake_core::FakeCore;
use crate::doc_req_verbs_seed::{env_for, make_fake_runner, seed_full_world};
use crate::remote_doc_cli::{ERR_NOT_AVAILABLE, MappedCmd, execute_client, map_cmd};
use crate::store;
use crate::test_support::TestDir;

fn words(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_owned).collect()
}

fn seeded_fake_core(
    dir: &TestDir,
) -> (
    tokio::runtime::Runtime,
    Env,
    std::path::PathBuf,
    FakeCore,
    Stage,
    Consumer,
    u64,
    u64,
    u64,
) {
    let rt = crate::link::runtime().unwrap();
    let fake_bin = make_fake_runner(dir);
    let env = env_for(dir, &fake_bin);
    let base = Path::new(env.get("BACKLOT_STATE_DIR").unwrap()).to_path_buf();
    let (stalled_seq, _user_tell_seq, recast_card_seq, new_card_seq) = seed_full_world(&base, &env);
    let (core, stage) = rt.block_on(async {
        let core = FakeCore::spawn(dir.join("hc.sock")).await;
        let mut stage = Stage::for_test(core.client().await);
        stage.base = base.clone();
        stage.env_vars = env.vars.clone();
        (core, stage)
    });
    let consumer = Consumer::load(&stage.base, crate::live::now_ms());
    (
        rt,
        env,
        base,
        core,
        stage,
        consumer,
        stalled_seq,
        recast_card_seq,
        new_card_seq,
    )
}

#[test]
fn tell_and_person_verbs_round_trip_through_fake_core_and_format_ok_lines() {
    let dir = TestDir::new("blrem-ok");
    let (rt, env, base, core, stage, mut consumer, stalled_seq, recast_card_seq, _) =
        seeded_fake_core(&dir);
    let setup_card_seq = recast_card_seq + 1;

    let mut run_cli = |owned: Vec<String>| -> String {
        let (common, cmd) = args::parse(&owned).unwrap();
        let action = match map_cmd(&common, &cmd, &env, None).unwrap() {
            MappedCmd::Action(a) => a,
            MappedCmd::NeedsStudio(mk) => mk("acme-shop".into()),
            MappedCmd::Unsupported => panic!("unexpected unsupported for {owned:?}"),
        };
        let (client, ep) = rt.block_on(async { (core.client().await, core.endpoint()) });
        rt.block_on(async {
            let fut = tokio::spawn(async move {
                execute_client(&client, Some(&ep), &action, Duration::from_secs(2)).await
            });
            loop {
                if fut.is_finished() {
                    break fut.await.unwrap().unwrap();
                }
                let _ = consumer
                    .process(&stage, crate::live::now_ms())
                    .await
                    .unwrap();
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
    };

    let out_tell = run_cli(vec![
        "tell".into(),
        "--studio".into(),
        "acme-shop".into(),
        "producer".into(),
        "--text".into(),
        "ship it".into(),
    ]);
    assert!(out_tell.contains("message.queued"), "{out_tell}");
    assert!(out_tell.contains("producer"), "{out_tell}");

    let (events, _) = store::load_events(&store::studio_dir(&base, "acme-shop")).unwrap();
    assert_eq!(events.last().unwrap().by.via.as_deref(), Some("doc"));

    let out_prio = run_cli(words("priority --studio acme-shop --task t1 --level high"));
    assert!(out_prio.contains("t1"));
    assert!(out_prio.contains("high"));

    for (line, needle) in [
        (
            "task pause --studio acme-shop --task t1 --reason hold",
            "t1",
        ),
        ("task resume --studio acme-shop --task t1 --reason go", "t1"),
        (
            "task abandon --studio acme-shop --task t1 --reason drop",
            "t1",
        ),
        ("tell hollywood status", "hollywood"),
        ("close --studio acme-shop --person writer", "writer"),
        ("reopen --studio acme-shop --person director", "director"),
        (
            "restart --studio acme-shop --person producer --reason refresh",
            "producer",
        ),
        ("restart --person hollywood", "role.restart"),
        (
            "hollywood setup --runner fake --model model-alt --effort medium",
            "[hollywood]",
        ),
        (
            "recast --studio acme-shop --person producer --runner fake --model model-alt --effort medium --reason up",
            "producer",
        ),
        (&format!("card show c{recast_card_seq}"), "producer"),
        (
            &format!("card cancel c{setup_card_seq} --reason hold"),
            "card.cancelled",
        ),
        (
            &format!("attention clear --studio acme-shop {stalled_seq}"),
            "attention",
        ),
    ] {
        let out = run_cli(words(line));
        assert!(out.contains(needle), "{line} -> {out}");
    }

    assert!(
        !run_cli(words(&format!("card apply c{recast_card_seq}")))
            .trim()
            .is_empty()
    );
}

#[test]
fn server_busy_request_expired_and_missing_ack_give_single_clear_lines() {
    let dir = TestDir::new("blrem-errs");
    let (rt, _env, _base, core, _stage, _consumer, _s, _r, _n) = seeded_fake_core(&dir);
    let action = ReqAction::Tell {
        studio: "acme-shop".into(),
        role: "producer".into(),
        text: "ping".into(),
    };

    // 1. Missing ack: bounded timeout -> one clear line.
    let err_missing = rt.block_on(async {
        let client = core.client().await;
        let ep = core.endpoint();
        execute_client(&client, Some(&ep), &action, Duration::from_millis(60))
            .await
            .unwrap_err()
    });
    assert!(!err_missing.message().contains('\n'));
    assert!(err_missing.message().contains("timed out"));

    // 2. Server busy (32 queued entries already present).
    let q: Vec<serde_json::Value> = (0..MAX_REQ_QUEUE)
        .map(|i| json!({ "req_id": format!("r{i}"), "ts_ms": 1 }))
        .collect();
    core.set_raw_doc(REQ_DOC_NAME, json!({ "schema": REQ_SCHEMA, "queue": q }));
    let err_busy = rt.block_on(async {
        let client = core.client().await;
        let ep = core.endpoint();
        execute_client(&client, Some(&ep), &action, Duration::from_secs(1))
            .await
            .unwrap_err()
    });
    assert_eq!(err_busy.message(), "server busy");

    // 3. `request expired` and `server busy` acks format cleanly.
    let err_exp =
        crate::remote_doc_cli::format_ack(&action, ResAck::err("req-1", 100, "request expired"))
            .unwrap_err();
    assert_eq!(err_exp.message(), "request expired");

    core.set_raw_doc(REQ_DOC_NAME, json!({ "schema": REQ_SCHEMA, "queue": [] }));
    core.set_raw_doc(RES_DOC_NAME, json!({ "schema": RES_SCHEMA, "acks": [] }));
    let err_ack_busy =
        crate::remote_doc_cli::format_ack(&action, ResAck::err("req-2", 100, "server busy"))
            .unwrap_err();
    assert_eq!(err_ack_busy.message(), "server busy");
}

#[test]
fn non_b1a_verbs_refuse_with_not_available_when_server_named_remotely() {
    let dir = TestDir::new("blrem-refuse");
    let env = Env {
        vars: BTreeMap::from([(
            "XDG_CONFIG_HOME".into(),
            dir.as_ref().to_string_lossy().into_owned(),
        )]),
    };

    for line in [
        "approve --person producer --decision allow",
        "task takeover --task t1 --reason mine",
        "req revise --file /home/user/req.md --summary s",
        "plan publish --kind plan --file /home/user/plan.md",
        "deliver --task t1 --commits a..b --report /home/user/report.md",
    ] {
        let mut owned = words("--server host-a/backlot");
        owned.extend(words(line));
        let err =
            crate::cli_run::run_stdin_with_host(&owned, &env, None, Some("host-b"), Some("alice"))
                .unwrap_err();
        assert_eq!(err.message(), ERR_NOT_AVAILABLE, "for {line}");
    }
}

#[test]
fn on_hub_cli_writes_local_ledger_directly_while_server_stopped() {
    let dir = TestDir::new("blhub-direct");
    let fake_bin = make_fake_runner(&dir);
    let env = env_for(&dir, &fake_bin);
    let base = Path::new(env.get("BACKLOT_STATE_DIR").unwrap()).to_path_buf();
    let _ = seed_full_world(&base, &env);

    // No hc daemon running; resolved `--server host-a/backlot` matches local
    // hostname `host-a`, so CLI writes directly to the local ledger.
    let out = crate::cli_run::run_stdin_with_host(
        &words("--server host-a/backlot tell --as user --studio acme-shop producer --text hello"),
        &env,
        None,
        Some("host-a"),
        Some("alice"),
    )
    .expect("hub CLI writes ledger directly while server stopped");
    assert!(out.contains("message.queued"), "{out}");

    let (events, _) = store::load_events(&store::studio_dir(&base, "acme-shop")).unwrap();
    assert!(events.last().unwrap().by.via.is_none());

    let prio = crate::cli_run::run_stdin_with_host(
        &words(
            "--server host-a/backlot priority --as user --studio acme-shop --task t1 --level high",
        ),
        &env,
        None,
        Some("host-a"),
        Some("alice"),
    )
    .unwrap();
    assert!(prio.contains("high"), "{prio}");
}

#[test]
fn config_client_does_not_hijack_agent_pinned_state_dir() {
    let dir = TestDir::new("blagent-pin");
    let fake_bin = make_fake_runner(&dir);
    let mut env = env_for(&dir, &fake_bin);
    let cfg = dir.join("backlot").join("config.toml");
    fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    fs::write(&cfg, "[client]\nserver = \"host-a\"\n").unwrap();
    env.vars.insert(
        "XDG_CONFIG_HOME".into(),
        dir.as_ref().to_string_lossy().into_owned(),
    );
    let base = Path::new(env.get("BACKLOT_STATE_DIR").unwrap()).to_path_buf();
    let _ = seed_full_world(&base, &env);

    let out = crate::cli_run::run_stdin_with_host(
        &words("tell --as user --studio acme-shop producer --text localmsg"),
        &env,
        None,
        Some("host-b"),
        Some("alice"),
    )
    .unwrap();
    assert!(out.contains("message.queued"), "{out}");
}

#[test]
fn remote_card_apply_and_cancel_travel_over_the_doc_queue() {
    let dir = TestDir::new("blrem-card-queue");
    let (rt, env, base, core, stage, mut consumer, _s, recast_card_seq, _n) =
        seeded_fake_core(&dir);
    let setup_card_seq = recast_card_seq + 1;

    let mut run_cli = |line: &str| -> String {
        let (common, cmd) = args::parse(&words(line)).unwrap();
        let action = match map_cmd(&common, &cmd, &env, None).unwrap() {
            MappedCmd::Action(a) => a,
            MappedCmd::NeedsStudio(mk) => mk("acme-shop".into()),
            MappedCmd::Unsupported => panic!("unexpected unsupported for {line}"),
        };
        rt.block_on(async {
            let client = core.client().await;
            let ep = core.endpoint();
            let fut = tokio::spawn(async move {
                execute_client(&client, Some(&ep), &action, Duration::from_secs(2)).await
            });
            loop {
                if fut.is_finished() {
                    break fut.await.unwrap().unwrap();
                }
                let _ = consumer
                    .process(&stage, crate::live::now_ms())
                    .await
                    .unwrap();
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
    };

    let apply_out = run_cli(&format!("card apply c{recast_card_seq}"));
    assert!(apply_out.contains("card.applied"), "{apply_out}");
    let cancel_out = run_cli(&format!("card cancel c{setup_card_seq} --reason hold"));
    assert!(cancel_out.contains("card.cancelled"), "{cancel_out}");

    assert!(core.doc_writes(REQ_DOC_NAME) >= 2);
    let (hevents, _) = store::load_hevents(&store::hollywood_dir(&base)).unwrap();
    assert!(hevents.iter().any(|e| matches!(
        e.kind,
        backlot_model::hollywood::HollywoodKind::CardApplied { card } if card == recast_card_seq
    )));
    assert!(hevents.iter().any(|e| matches!(
        e.kind,
        backlot_model::hollywood::HollywoodKind::CardCancelled { card, .. } if card == setup_card_seq
    )));
}

#[test]
fn remote_hollywood_switch_cancel_and_status_travel_over_doc_queue() {
    let dir = TestDir::new("blrem-hswitch-queue");
    let (rt, env, base, core, stage, mut consumer, _s, _r, _n) = seeded_fake_core(&dir);

    let mut run_cli = |line: &str| -> Result<String, crate::cli::Fail> {
        let (common, cmd) = args::parse(&words(line)).unwrap();
        let json = crate::remote_doc_cli::wants_json(&cmd);
        let action = match map_cmd(&common, &cmd, &env, None).unwrap() {
            MappedCmd::Action(a) => a,
            MappedCmd::NeedsStudio(mk) => mk("acme-shop".into()),
            MappedCmd::Unsupported => panic!("unexpected unsupported for {line}"),
        };
        rt.block_on(async {
            let client = core.client().await;
            let ep = core.endpoint();
            let fut = tokio::spawn(async move {
                crate::remote_doc_cli::execute_client_with(
                    &client,
                    Some(&ep),
                    &action,
                    Duration::from_secs(2),
                    json,
                )
                .await
            });
            loop {
                if fut.is_finished() {
                    break fut.await.unwrap();
                }
                let _ = consumer
                    .process(&stage, crate::live::now_ms())
                    .await
                    .unwrap();
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
    };

    // Initial `--status` when no handoff exists returns empty handoff.
    let status0: backlot_model::req_results::HollywoodSwitchStatusResult =
        serde_json::from_str(&run_cli("hollywood switch --status --json").unwrap()).unwrap();
    assert!(!status0.active);
    assert!(status0.handoff.is_none());

    // `switch --no-wait --json` stages handoff.json over doc queue.
    let queued: backlot_model::req_results::HollywoodSwitchResult = serde_json::from_str(
        &run_cli("hollywood switch --runner fake --model model-b --effort high --no-wait --json")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(queued.action, "queued");
    assert!(queued.handoff.is_some());

    // `switch --status --json` reports active handoff.
    let status1: backlot_model::req_results::HollywoodSwitchStatusResult =
        serde_json::from_str(&run_cli("hollywood switch --status --json").unwrap()).unwrap();
    assert!(status1.active);

    // `switch --cancel --json` sets cancel_requested.
    let cancelled: backlot_model::req_results::HollywoodSwitchResult =
        serde_json::from_str(&run_cli("hollywood switch --cancel --json").unwrap()).unwrap();
    assert_eq!(cancelled.action, "cancel-requested");
    let rec = crate::handoff::load_handoff(&store::hollywood_dir(&base))
        .unwrap()
        .unwrap();
    assert!(rec.cancel_requested);

    // Non-user `--as producer` is refused on switch map_cmd.
    let (common, cmd) = args::parse(&words(
        "hollywood switch --runner fake --model model-b --effort high --no-wait --as producer",
    ))
    .unwrap();
    assert!(map_cmd(&common, &cmd, &env, None).is_err());

    // Live stage + live Hollywood refuses `hollywood setup` over RPC unless `--force-offline`.
    crate::live::write(
        &base,
        &crate::live::Live {
            updated_ms: crate::live::now_ms(),
            roles: Vec::new(),
            accounts: Vec::new(),
        },
    )
    .unwrap();
    let err = run_cli("hollywood setup --runner fake --model model-b --effort high").unwrap_err();
    assert!(err.message().contains("hollywood switch"), "{err:?}");
    let ok = run_cli("hollywood setup --runner fake --model model-b --effort high --force-offline")
        .unwrap();
    assert!(ok.contains("[hollywood]"), "{ok}");
}
