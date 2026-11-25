//! Parity & ledger tests covering every B1a person verb: each verb via
//! `plugin.backlot.req` produces the exact same ledger events as its CLI
//! counterpart, differing only by `by.via == Some("doc")`.

use std::fs;
use std::path::Path;
use std::time::Duration;

use backlot_model::event::{EventKind, Priority, TaskStateChange};
use backlot_model::hollywood::HollywoodKind;
use backlot_model::req_results::{
    AttentionClearResult, CardShowResult, CloseResult, HollywoodSetupResult, RecastResult,
    ReopenResult, RestartResult, TaskPriorityResult, TaskStateResult, TellHollywoodResult,
    TellResult,
};
use backlot_model::req_wire::ReqAction;

use crate::daemon::Stage;
use crate::doc_client;
use crate::doc_req::Consumer;
use crate::doc_req_fake_core::FakeCore;
use crate::doc_req_verbs_seed::{cli_ok, env_for, make_fake_runner, seed_full_world};
use crate::store;
use crate::test_support::TestDir;

#[test]
fn all_fourteen_verbs_match_cli_ledger_events_with_via_doc() {
    let rt = crate::link::runtime().unwrap();
    let cli_dir = TestDir::new("blcli");
    let doc_dir = TestDir::new("bldoc");
    let fake_bin = make_fake_runner(&cli_dir);

    let cli_env = env_for(&cli_dir, &fake_bin);
    let doc_env = env_for(&doc_dir, &fake_bin);
    let cli_base = Path::new(cli_env.get("BACKLOT_STATE_DIR").unwrap());
    let doc_base = Path::new(doc_env.get("BACKLOT_STATE_DIR").unwrap());

    let (stalled_seq, user_tell_seq, recast_card_seq, new_card_seq) =
        seed_full_world(cli_base, &cli_env);
    let (doc_stalled_seq, doc_user_tell_seq, doc_recast_card_seq, doc_new_card_seq) =
        seed_full_world(doc_base, &doc_env);
    assert_eq!(stalled_seq, doc_stalled_seq);
    assert_eq!(user_tell_seq, doc_user_tell_seq);
    assert_eq!(recast_card_seq, doc_recast_card_seq);
    assert_eq!(new_card_seq, doc_new_card_seq);
    let setup_card_seq = recast_card_seq + 1;

    let (core, stage, c_client, c_ep) = rt.block_on(async {
        let core = FakeCore::spawn(doc_dir.join("hc.sock")).await;
        let mut stage = Stage::for_test(core.client().await);
        stage.base = doc_base.to_path_buf();
        stage.env_vars = doc_env.vars.clone();
        let c_client = core.client().await;
        let c_ep = core.endpoint();
        (core, stage, c_client, c_ep)
    });
    let _ = &core;
    let mut consumer = Consumer::load(&stage.base, crate::live::now_ms());

    macro_rules! run_doc {
        ($ty:ty, $action:expr) => {{
            let cli_c = c_client.clone();
            let ep_c = c_ep.clone();
            let act = $action;
            rt.block_on(async {
                let fut = tokio::spawn(async move {
                    doc_client::send_typed::<$ty>(
                        &cli_c,
                        Some(&ep_c),
                        &act,
                        None,
                        Duration::from_secs(2),
                    )
                    .await
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
        }};
    }

    // 1. tell
    cli_ok(
        &cli_env,
        &[
            "tell",
            "--as",
            "user",
            "--studio",
            "acme-shop",
            "producer",
            "--text",
            "hello",
        ],
    );
    let tell_res: TellResult = run_doc!(
        TellResult,
        ReqAction::Tell {
            studio: "acme-shop".into(),
            role: "producer".into(),
            text: "hello".into(),
        }
    );
    assert_eq!(tell_res.role, "producer");

    // 2. tell-hollywood (+ doc verifies `answers` marks still-queued tell seen)
    cli_ok(&cli_env, &["tell", "hollywood", "check line"]);
    let th_res: TellHollywoodResult = run_doc!(
        TellHollywoodResult,
        ReqAction::TellHollywood {
            text: "check line".into(),
            answers: vec![doc_user_tell_seq],
        }
    );
    assert_eq!(th_res.to, "hollywood");
    assert_eq!(th_res.marked_seen, vec![doc_user_tell_seq]);

    // 3. card-show (structured recast, setup identity transitions, and studio-new draft)
    let cli_show = cli_ok(&cli_env, &["card", "show", &format!("c{recast_card_seq}")]);
    assert!(cli_show.contains("producer:"));
    let show_recast: CardShowResult = run_doc!(
        CardShowResult,
        ReqAction::CardShow {
            id: recast_card_seq,
        }
    );
    let rec_id = show_recast.identity_change.unwrap();
    assert_eq!(rec_id.old.unwrap().model, "model-f");
    assert_eq!(rec_id.new.model, "model-alt");

    let show_setup: CardShowResult =
        run_doc!(CardShowResult, ReqAction::CardShow { id: setup_card_seq });
    let set_id = show_setup.identity_change.unwrap();
    assert_eq!(set_id.old.unwrap().model, "model-f");
    assert_eq!(set_id.new.model, "model-alt");

    let show_new: CardShowResult = run_doc!(
        CardShowResult,
        ReqAction::CardShow {
            id: doc_new_card_seq,
        }
    );
    let draft = show_new.studio_new.unwrap();
    assert_eq!(draft.studio, "beta-tools");
    assert!(draft.brief_text.unwrap().contains("Build CLI helpers"));

    // 4. card-apply & 5. card-cancel are person-only and refused over the queue.
    for (req_id, action) in [
        (
            "refuse-card-apply",
            ReqAction::CardApply {
                id: recast_card_seq,
            },
        ),
        (
            "refuse-card-cancel",
            ReqAction::CardCancel {
                id: setup_card_seq,
                reason: "hold off".into(),
            },
        ),
    ] {
        let cli_c = c_client.clone();
        let ep_c = c_ep.clone();
        let ack = rt.block_on(async {
            let fut = tokio::spawn(async move {
                doc_client::send_request(&cli_c, Some(&ep_c), &action, None, Duration::from_secs(2))
                    .await
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
        });
        assert!(!ack.ok, "{req_id}");
        assert_eq!(
            ack.error.as_deref(),
            Some(backlot_model::req_wire::CARD_QUEUE_REFUSAL)
        );
    }

    // 6. attention-clear (non-prompt stalled)
    cli_ok(
        &cli_env,
        &[
            "attention",
            "clear",
            "--as",
            "user",
            "--studio",
            "acme-shop",
            &stalled_seq.to_string(),
        ],
    );
    let ac_res: AttentionClearResult = run_doc!(
        AttentionClearResult,
        ReqAction::AttentionClear {
            studio: "acme-shop".into(),
            target: doc_stalled_seq,
        }
    );
    assert_eq!(ac_res.cleared_reason, "stalled");

    // 7. recast
    cli_ok(
        &cli_env,
        &[
            "recast",
            "--studio",
            "acme-shop",
            "--person",
            "producer",
            "--runner",
            "fake",
            "--model",
            "model-alt",
            "--effort",
            "medium",
            "--reason",
            "upgrade",
        ],
    );
    let rec_res: RecastResult = run_doc!(
        RecastResult,
        ReqAction::Recast {
            studio: "acme-shop".into(),
            person: "producer".into(),
            runner: "fake".into(),
            model: "model-alt".into(),
            effort: Some("medium".into()),
            permission_profile: None,
            reason: "upgrade".into(),
        }
    );
    assert_eq!(rec_res.new.model, "model-alt");

    // 8a. restart (studio role, fresh)
    cli_ok(
        &cli_env,
        &[
            "restart",
            "--as",
            "user",
            "--studio",
            "acme-shop",
            "--person",
            "producer",
            "--fresh",
            "--reason",
            "reset",
        ],
    );
    let rst_role: RestartResult = run_doc!(
        RestartResult,
        ReqAction::Restart {
            studio: Some("acme-shop".into()),
            person: "producer".into(),
            fresh: true,
            reason: "reset".into(),
        }
    );
    assert!(!rst_role.is_hollywood);

    // 8b. restart (hollywood)
    cli_ok(
        &cli_env,
        &[
            "restart",
            "--person",
            "hollywood",
            "--fresh",
            "--reason",
            "h-reset",
        ],
    );
    let rst_h: RestartResult = run_doc!(
        RestartResult,
        ReqAction::Restart {
            studio: None,
            person: "hollywood".into(),
            fresh: true,
            reason: "h-reset".into(),
        }
    );
    assert!(rst_h.is_hollywood);

    // 9. close (writer held) & 10. reopen (director closed with session)
    cli_ok(
        &cli_env,
        &[
            "close",
            "--as",
            "user",
            "--studio",
            "acme-shop",
            "--person",
            "writer",
        ],
    );
    let cl_res: CloseResult = run_doc!(
        CloseResult,
        ReqAction::Close {
            studio: "acme-shop".into(),
            person: "writer".into(),
        }
    );
    assert_eq!(cl_res.person, "writer");

    cli_ok(
        &cli_env,
        &[
            "reopen",
            "--as",
            "user",
            "--studio",
            "acme-shop",
            "--person",
            "director",
        ],
    );
    let ro_res: ReopenResult = run_doc!(
        ReopenResult,
        ReqAction::Reopen {
            studio: "acme-shop".into(),
            person: "director".into(),
        }
    );
    assert_eq!(ro_res.person, "director");

    // 11. task-priority, 12. task-pause, 13. task-resume, 14. task-abandon
    cli_ok(
        &cli_env,
        &[
            "priority",
            "--as",
            "user",
            "--studio",
            "acme-shop",
            "--task",
            "t1",
            "--level",
            "high",
        ],
    );
    let pr_res: TaskPriorityResult = run_doc!(
        TaskPriorityResult,
        ReqAction::TaskPriority {
            studio: "acme-shop".into(),
            task: "t1".into(),
            level: Priority::High,
        }
    );
    assert_eq!(pr_res.level, Priority::High);

    cli_ok(
        &cli_env,
        &[
            "task",
            "pause",
            "--as",
            "user",
            "--studio",
            "acme-shop",
            "--task",
            "t1",
            "--reason",
            "wait",
        ],
    );
    let pause_res: TaskStateResult = run_doc!(
        TaskStateResult,
        ReqAction::TaskPause {
            studio: "acme-shop".into(),
            task: "t1".into(),
            reason: "wait".into(),
        }
    );
    assert_eq!(pause_res.state, TaskStateChange::Paused);

    cli_ok(
        &cli_env,
        &[
            "task",
            "resume",
            "--as",
            "user",
            "--studio",
            "acme-shop",
            "--task",
            "t1",
            "--reason",
            "go",
        ],
    );
    let resume_res: TaskStateResult = run_doc!(
        TaskStateResult,
        ReqAction::TaskResume {
            studio: "acme-shop".into(),
            task: "t1".into(),
            reason: "go".into(),
        }
    );
    assert_eq!(resume_res.state, TaskStateChange::Resumed);

    cli_ok(
        &cli_env,
        &[
            "task",
            "abandon",
            "--as",
            "user",
            "--studio",
            "acme-shop",
            "--task",
            "t1",
            "--reason",
            "obsolete",
        ],
    );
    let ab_res: TaskStateResult = run_doc!(
        TaskStateResult,
        ReqAction::TaskAbandon {
            studio: "acme-shop".into(),
            task: "t1".into(),
            reason: "obsolete".into(),
        }
    );
    assert_eq!(ab_res.state, TaskStateChange::Abandoned);
    assert_eq!(ab_res.reason, "obsolete");

    // 15. hollywood-setup
    cli_ok(
        &cli_env,
        &[
            "hollywood",
            "setup",
            "--runner",
            "fake",
            "--model",
            "model-alt",
            "--effort",
            "high",
        ],
    );
    let setup_res: HollywoodSetupResult = run_doc!(
        HollywoodSetupResult,
        ReqAction::HollywoodSetup {
            runner: "fake".into(),
            model: "model-alt".into(),
            effort: Some("high".into()),
            key: None,
            default_cast: None,
        }
    );
    assert_eq!(setup_res.model, "model-alt");
    assert_eq!(
        fs::read_to_string(crate::config::config_path(&doc_env.vars)).unwrap(),
        fs::read_to_string(crate::config::config_path(&cli_env.vars)).unwrap(),
    );

    let (cli_events, _) = store::load_events(&store::studio_dir(cli_base, "acme-shop")).unwrap();
    let (doc_events, _) = store::load_events(&store::studio_dir(doc_base, "acme-shop")).unwrap();
    assert_eq!(cli_events.len(), doc_events.len());
    for (ce, de) in cli_events.iter().skip(10).zip(doc_events.iter().skip(10)) {
        assert_eq!(ce.by.role, "user");
        assert_eq!(ce.by.via, None);
        assert_eq!(de.by.role, "user");
        assert_eq!(de.by.via.as_deref(), Some("doc"));
        match (&ce.kind, &de.kind) {
            (EventKind::MessageQueued { to: ct, .. }, EventKind::MessageQueued { to: dt, .. }) => {
                assert_eq!(ct, dt)
            }
            _ => assert_eq!(ce.kind, de.kind),
        }
    }

    let (cli_hevents, _) = store::load_hevents(&store::hollywood_dir(cli_base)).unwrap();
    let (doc_hevents, _) = store::load_hevents(&store::hollywood_dir(doc_base)).unwrap();
    let doc_user_hevents: Vec<_> = doc_hevents
        .iter()
        .skip(5)
        .filter(|e| !matches!(e.kind, HollywoodKind::MessageSeen { .. }))
        .collect();
    let cli_user_hevents: Vec<_> = cli_hevents.iter().skip(5).collect();
    assert_eq!(cli_user_hevents.len(), doc_user_hevents.len());
    for (ce, de) in cli_user_hevents.iter().zip(doc_user_hevents.iter()) {
        assert_eq!(ce.by.role, "user");
        assert_eq!(ce.by.via, None);
        assert_eq!(de.by.role, "user");
        assert_eq!(de.by.via.as_deref(), Some("doc"));
        assert_eq!(ce.kind, de.kind);
    }
    assert!(doc_hevents.iter().any(|e| {
        matches!(e.kind, HollywoodKind::MessageSeen { target, .. } if target == doc_user_tell_seq)
            && e.by.role == "stage"
            && e.by.via.as_deref() == Some("doc")
    }));
}
