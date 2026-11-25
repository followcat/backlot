//! B1b client side: the remote CLI reads cast/ask content on the
//! client, never canonicalizes a hub repo, and sends a destructive
//! commit's dry run and token in one invocation.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use backlot_model::req_wire::{RES_DOC_NAME, ReqAction, ResDoc};
use serde_json::Value;

use crate::args;
use crate::cli::{Env, Fail};
use crate::cli_testkit::{CAST_TOML, fixture, ok, shared_area, shared_repo};
use crate::daemon::Stage;
use crate::doc_req::Consumer;
use crate::doc_req_fake_core::FakeCore;
use crate::remote_doc_cli::{MappedCmd, map_cmd};
use crate::store;
use crate::test_support::TestDir;

fn env() -> Env {
    Env::default()
}

fn mapped(line: Vec<String>, env: &Env, stdin: Option<&str>) -> MappedCmd {
    let (common, cmd) = args::parse(&line).unwrap();
    map_cmd(&common, &cmd, env, stdin).unwrap()
}

fn action(line: Vec<String>, env: &Env, stdin: Option<&str>) -> ReqAction {
    match mapped(line, env, stdin) {
        MappedCmd::Action(a) => a,
        MappedCmd::NeedsStudio(make) => make("acme-shop".into()),
        MappedCmd::Unsupported => panic!("unexpected unsupported verb"),
    }
}

fn words(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_owned).collect()
}

#[test]
fn studio_new_reads_cast_and_ask_locally_and_keeps_the_repo_spelling() {
    let dir = TestDir::new("blops-client");
    let cast = dir.join("cast.toml");
    let ask = dir.join("ask.md");
    fs::write(&cast, "cast body").unwrap();
    fs::write(&ask, "ask body").unwrap();
    let repo = dir.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let spelled = format!("{}/../{}", repo.display(), "repo");

    let act = action(
        vec![
            "studio".into(),
            "new".into(),
            "--studio".into(),
            "acme-shop".into(),
            "--title".into(),
            "Shop".into(),
            "--repo".into(),
            spelled.clone(),
            "--cast".into(),
            cast.to_string_lossy().into_owned(),
            "--ask-file".into(),
            ask.to_string_lossy().into_owned(),
        ],
        &env(),
        None,
    );
    let ReqAction::StudioNew {
        repo, cast, ask, ..
    } = act
    else {
        panic!("expected studio-new");
    };
    assert_eq!(repo, spelled, "the client must not canonicalize --repo");
    assert_eq!(cast, "cast body");
    assert_eq!(ask.as_deref(), Some("ask body"));

    let act = action(
        vec![
            "studio".into(),
            "new".into(),
            "--studio".into(),
            "acme-shop".into(),
            "--title".into(),
            "Shop".into(),
            "--repo".into(),
            "/home/user/acme-shop".into(),
            "--cast".into(),
            "-".into(),
        ],
        &env(),
        Some("cast from stdin"),
    );
    let ReqAction::StudioNew { cast, .. } = act else {
        panic!("expected studio-new");
    };
    assert_eq!(cast, "cast from stdin");
}

#[test]
fn studio_finish_maps_and_stays_the_persons_verb() {
    let act = action(
        words("studio finish --studio acme-shop --reason done --force"),
        &env(),
        None,
    );
    assert!(matches!(act, ReqAction::StudioFinish { .. }));

    let (common, cmd) = args::parse(&words(
        "studio finish --studio acme-shop --reason done --as writer",
    ))
    .unwrap();
    let err = match map_cmd(&common, &cmd, &env(), None) {
        Err(e) => e,
        Ok(_) => panic!("expected a refusal"),
    };
    assert!(err.message().contains("expected user"), "{}", err.message());

    let (scratch, env) = crate::cli_testkit::scratch();
    let repo = shared_repo(&scratch);
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast}",
            repo.display()
        ),
    );
    let err =
        crate::cli::run(&words("studio finish --studio acme-shop --as writer"), &env).unwrap_err();
    assert_eq!(err.message(), crate::commands::FINISH_IS_THE_PERSONS_CALL);
}

#[test]
fn studio_new_interactive_is_not_a_server_verb() {
    let (common, cmd) = args::parse(&words("studio new --interactive --studio acme-shop")).unwrap();
    let err = match map_cmd(&common, &cmd, &env(), None) {
        Err(e) => e,
        Ok(_) => panic!("expected a refusal"),
    };
    assert!(err.message().contains("runs where the terminal is"));
}

#[test]
fn studio_new_remote_placement_stays_phase_two() {
    let dir = TestDir::new("blops-remote");
    let env = Env {
        vars: std::collections::BTreeMap::from([(
            "XDG_CONFIG_HOME".into(),
            dir.as_ref().to_string_lossy().into_owned(),
        )]),
    };
    let err = crate::cli_run::run_stdin_with_host(
        &words(
            "studio new --studio acme-shop --title Shop --repo /home/user/acme-shop \
             --cast /home/user/cast.toml --remote host-b",
        ),
        &env,
        None,
        Some("host-a"),
        Some("alice"),
    )
    .unwrap_err();
    assert!(
        err.message().contains("remote studio placement is Phase 2"),
        "{}",
        err.message()
    );
}

struct Harness {
    rt: tokio::runtime::Runtime,
    core: FakeCore,
    stage: Stage,
    consumer: Consumer,
}

impl Harness {
    fn start(env: &Env) -> Self {
        let rt = crate::link::runtime().unwrap();
        let base = PathBuf::from(env.get("BACKLOT_STATE_DIR").unwrap());
        fs::create_dir_all(&base).unwrap();
        let (core, stage) = rt.block_on(async {
            let core = FakeCore::spawn(base.join("hc.sock")).await;
            let mut stage = Stage::for_test(core.client().await);
            stage.base = base.clone();
            stage.env_vars = env.vars.clone();
            (core, stage)
        });
        let consumer = Consumer::load(&stage.base, crate::live::now_ms());
        Harness {
            rt,
            core,
            stage,
            consumer,
        }
    }

    fn run(&mut self, action: &ReqAction, json: bool) -> Result<String, Fail> {
        let core = self.core.clone();
        let act = action.clone();
        let stage = &self.stage;
        let consumer = &mut self.consumer;
        self.rt.block_on(async {
            let fut = tokio::spawn(async move {
                let client = core.client().await;
                let ep = core.endpoint();
                crate::remote_doc_cli::execute_client_with(
                    &client,
                    Some(&ep),
                    &act,
                    Duration::from_secs(2),
                    json,
                )
                .await
            });
            loop {
                if fut.is_finished() {
                    return fut.await.unwrap();
                }
                let _ = consumer
                    .process(stage, crate::live::now_ms())
                    .await
                    .unwrap();
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
    }
}

#[test]
fn a_destructive_commit_sends_its_dry_run_then_the_token() {
    let (scratch, env) = crate::cli_testkit::scratch();
    let repo = shared_repo(&scratch);
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast}",
            repo.display()
        ),
    );
    shared_area(&scratch, "integration");
    ok(&env, "studio finish --reason done --force --as user");

    let mut harness = Harness::start(&env);
    let out = harness
        .run(
            &ReqAction::StudioDelete {
                studio: Some("acme-shop".into()),
                dry_run: false,
                force: false,
            },
            false,
        )
        .unwrap();
    assert!(out.contains("deleted"), "{out}");
    assert!(!store::studio_dir(&scratch.dir, "acme-shop").exists());

    let (_, val) = harness.core.doc(RES_DOC_NAME).unwrap();
    let res: ResDoc = serde_json::from_value(val).unwrap();
    assert_eq!(res.acks.len(), 2, "dry run plus commit");
    assert!(res.acks.iter().all(|a| a.ok), "{:?}", res.acks);
    let dry: Value = res.acks[0].result.clone().unwrap();
    assert!(dry["confirm_token"].is_string());
    assert_eq!(dry["report"]["dry_run"], true);
}
