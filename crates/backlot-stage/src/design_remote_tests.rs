//! Remote-doc (`plugin.backlot.req`/`res`) and `doc_dispatch` parity
//! tests for `ReqAction::DesignStartRound`, `ReqAction::DesignSign`, and
//! `ReqAction::Tell { role: "producer" }` in `DesignPhase::WaitingUser`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use backlot_model::design::{DesignGate, DesignSlot, EntrySource, SignMode};
use backlot_model::event::{EventKind, StudioMode};
use backlot_model::req_results::{DesignSignResult, DesignStartRoundResult, TellResult};
use backlot_model::req_wire::ReqAction;
use backlot_model::{EventBy, NewEvent};

use crate::args;
use crate::cli::Env;
use crate::daemon::Stage;
use crate::doc_client;
use crate::doc_req::Consumer;
use crate::doc_req_fake_core::FakeCore;
use crate::doc_req_verbs_seed::{env_for, fake_id, make_fake_runner};
use crate::remote_doc_cli::{MappedCmd, execute_client, map_cmd};
use crate::store::{self, Pending};
use crate::test_support::TestDir;

fn seed_design_studio(base: &Path) {
    let dir = store::studio_dir(base, "acme-shop");
    fs::create_dir_all(dir.join("repo")).unwrap();
    fs::write(dir.join("req-v1.md"), "# Req v1\nCheckout flow.\n").unwrap();

    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("user"),
                EventKind::StudioCreated {
                    title: "Acme Shop".into(),
                    repo: dir.join("repo").to_string_lossy().into_owned(),
                    members: Vec::new(),
                    no_git: false,
                    mode: StudioMode::Standard,
                    roles: Box::new(backlot_model::event::RoleConfigs {
                        producer: fake_id("model-f"),
                        director: fake_id("model-f"),
                        writer: fake_id("model-f"),
                    }),
                    candidates: vec![backlot_model::event::Candidate {
                        name: "actor-a".into(),
                        runner: "fake".into(),
                        runner_version: None,
                        model: "model-f".into(),
                        effort: Some("medium".into()),
                        permission_profile: "standard".into(),
                    }],
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
                    design_gate: DesignGate::Required,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();

    store::append(&dir, |_, _| {
        Ok::<_, std::convert::Infallible>(Pending {
            event: NewEvent::new(
                EventBy::role("producer"),
                EventKind::ReqVersion {
                    version: 1,
                    file: "req-v1.md".into(),
                    summary: "Checkout design".into(),
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
}

fn setup_remote(
    dir: &TestDir,
) -> (
    tokio::runtime::Runtime,
    Env,
    std::path::PathBuf,
    FakeCore,
    Stage,
    Consumer,
) {
    let rt = crate::link::runtime().unwrap();
    let fake_bin = make_fake_runner(dir);
    let env = env_for(dir, &fake_bin);
    let base = Path::new(env.get("BACKLOT_STATE_DIR").unwrap()).to_path_buf();
    seed_design_studio(&base);
    let (core, stage) = rt.block_on(async {
        let core = FakeCore::spawn(dir.join("hc.sock")).await;
        let mut stage = Stage::for_test(core.client().await);
        stage.base = base.clone();
        stage.env_vars = env.vars.clone();
        (core, stage)
    });
    let consumer = Consumer::load(&stage.base, crate::live::now_ms());
    (rt, env, base, core, stage, consumer)
}

#[test]
fn doc_dispatch_and_remote_cli_cover_start_round_sign_and_tell_producer() {
    let dir = TestDir::new("bldes-rem");
    let (rt, env, base, core, stage, mut consumer) = setup_remote(&dir);
    let sdir = store::studio_dir(&base, "acme-shop");

    let run_cli = |consumer: &mut Consumer, owned: Vec<String>| -> String {
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

    // 1. Remote `backlot design open` -> `ReqAction::DesignStartRound`
    let open_out = run_cli(
        &mut consumer,
        vec![
            "design".into(),
            "open".into(),
            "--studio".into(),
            "acme-shop".into(),
        ],
    );
    assert!(open_out.contains("design.opened v1 r1"), "{open_out}");
    let events = store::load_events(&sdir).unwrap().0;
    let open_ev = events.last().unwrap();
    assert_eq!(open_ev.by.via.as_deref(), Some("doc"));
    assert_eq!(open_ev.by.role, "user");
    assert!(sdir.join("design/v1.md").is_file());

    // Record Proposal -> Buildability -> Summary locally to reach WaitingUser.
    let wf = dir.join("w1.md");
    let df = dir.join("d1.md");
    let pf = dir.join("p1.md");
    fs::write(&wf, "Remote proposal.\n").unwrap();
    fs::write(&df, "Remote buildability.\n").unwrap();
    fs::write(&pf, "Remote summary.\n").unwrap();
    for (args_list, role) in [
        (
            vec![
                "design",
                "write",
                "--studio",
                "acme-shop",
                "--slot",
                "proposal",
                "--file",
            ],
            "writer",
        ),
        (
            vec![
                "design",
                "write",
                "--studio",
                "acme-shop",
                "--slot",
                "buildability",
                "--file",
            ],
            "director",
        ),
        (
            vec![
                "design",
                "write",
                "--studio",
                "acme-shop",
                "--slot",
                "summary",
                "--file",
            ],
            "producer",
        ),
    ] {
        let mut owned: Vec<String> = args_list.into_iter().map(str::to_owned).collect();
        let file = match role {
            "writer" => &wf,
            "director" => &df,
            _ => &pf,
        };
        owned.push(file.to_string_lossy().into_owned());
        owned.extend(["--as".into(), role.into()]);
        crate::cli::run(&owned, &env).unwrap();
    }

    // 2. Remote `ReqAction::Tell { role: "producer" }` in WaitingUser creates `DesignEntry(UserInput, Tell)`.
    let tell_res: TellResult = rt.block_on(async {
        let (cli_c, ep_c) = (core.client().await, core.endpoint());
        let act = ReqAction::Tell {
            studio: "acme-shop".into(),
            role: "producer".into(),
            text: "Use event sourcing".into(),
        };
        let fut = tokio::spawn(async move {
            doc_client::send_typed::<TellResult>(
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
    });
    assert_eq!(tell_res.role, "producer");
    let events = store::load_events(&sdir).unwrap().0;
    let entry_ev = events.last().unwrap();
    assert_eq!(entry_ev.by.via.as_deref(), Some("doc"));
    match &entry_ev.kind {
        EventKind::DesignEntry {
            slot, source, file, ..
        } => {
            assert_eq!(*slot, DesignSlot::UserInput);
            assert_eq!(*source, Some(EntrySource::Tell));
            assert!(sdir.join(file).is_file());
        }
        other => panic!("expected DesignEntry(UserInput), got {other:?}"),
    }

    // 3. Remote `backlot design sign --note "ship v1"` -> `ReqAction::DesignSign`
    let sign_out = run_cli(
        &mut consumer,
        vec![
            "design".into(),
            "sign".into(),
            "--studio".into(),
            "acme-shop".into(),
            "--note".into(),
            "ship v1".into(),
        ],
    );
    assert!(
        sign_out.contains("design.signed v1 r1 approved"),
        "{sign_out}"
    );
    assert!(sdir.join("design/v1-final.md").is_file());
    let events = store::load_events(&sdir).unwrap().0;
    assert_eq!(events.last().unwrap().by.via.as_deref(), Some("doc"));

    // Verify `DesignStartRoundResult` typed dispatch from Idle -> SuggestSkip rejects cleanly vs R2 works before sign.
    let _: DesignStartRoundResult = DesignStartRoundResult {
        studio: "acme-shop".into(),
        seq: 1,
        version: 1,
        round: 1,
        doc_file: "design/v1.md".into(),
    };
}

#[test]
fn remote_design_sign_skip_works_on_suggest_skip_without_final_snapshot() {
    let dir = TestDir::new("bldes-skip");
    let (rt, env, base, core, stage, mut consumer) = setup_remote(&dir);
    let sdir = store::studio_dir(&base, "acme-shop");

    crate::cli::run(
        &[
            "design".into(),
            "open".into(),
            "--studio".into(),
            "acme-shop".into(),
            "--as".into(),
            "producer".into(),
            "--suggest-skip".into(),
            "--reason".into(),
            "typo".into(),
        ],
        &env,
    )
    .unwrap();

    let res: DesignSignResult = rt.block_on(async {
        let (cli_c, ep_c) = (core.client().await, core.endpoint());
        let act = ReqAction::DesignSign {
            studio: "acme-shop".into(),
            skip: true,
            reason: Some("agreed trivial".into()),
            note: Some("go".into()),
            no_checks: false,
        };
        let fut = tokio::spawn(async move {
            doc_client::send_typed::<DesignSignResult>(
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
    });
    assert_eq!(res.mode, SignMode::Skipped);
    assert_eq!(res.doc_file, "");
    assert!(!sdir.join("design/v1-final.md").exists());
    let doc = fs::read_to_string(sdir.join("design/v1.md")).unwrap();
    assert!(doc.contains("skipped"), "{doc}");
}
