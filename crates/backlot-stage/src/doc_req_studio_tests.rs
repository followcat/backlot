//! B1b: `studio new`, its dry run and the confirm-token flow through
//! `plugin.backlot.req`.

use std::fs;

use backlot_model::req_results::{StudioFinishResult, StudioNewResult, StudioPlan};
use backlot_model::req_wire::ReqAction;

use crate::cli::Env;
use crate::cli_testkit::{self, CAST_TOML, fixture, ok, shared_area, shared_repo};
use crate::delete::DeleteReport;
use crate::doc_req_studio_seed::{FAKE_CAST, Server, base_of, err_line, load};
use crate::doc_req_verbs_seed::{env_for, make_fake_runner};
use crate::store;
use crate::test_support::TestDir;

#[test]
fn studio_new_and_finish_record_the_local_cli_lines_and_events() {
    let cli_dir = TestDir::new("blops-new-cli");
    let doc_dir = TestDir::new("blops-new-doc");
    let cli_cast = cli_dir.join("cast.toml");
    let cli_ask = cli_dir.join("ask.md");
    fs::write(&cli_cast, FAKE_CAST).unwrap();
    fs::write(&cli_ask, "first thing\n").unwrap();
    let cli_env = env_for(&cli_dir, &make_fake_runner(&cli_dir));
    let doc_env = env_for(&doc_dir, &make_fake_runner(&doc_dir));

    let local = ok(
        &cli_env,
        &format!(
            "studio new --studio acme-shop --title Shop --repo /home/user/acme-shop \
             --cast {} --ask-file {}",
            cli_cast.display(),
            cli_ask.display()
        ),
    );

    let mut server = Server::start(&doc_env);
    let made: StudioNewResult = server.typed(
        &ReqAction::StudioNew {
            id: "acme-shop".into(),
            title: "Shop".into(),
            repo: "/home/user/acme-shop".into(),
            cast: FAKE_CAST.into(),
            mode: None,
            init: false,
            no_git: false,
            ask: Some("first thing\n".into()),
            dry_run: false,
        },
        None,
    );
    assert_eq!(made.seq, Some(1));
    assert_eq!(made.ask_seq, Some(2));
    assert_eq!(crate::studio_fmt::studio_new(&made), local);

    let local_finish = ok(
        &cli_env,
        "studio finish --studio acme-shop --reason done --force --as user",
    );
    let finished: StudioFinishResult = server.typed(
        &ReqAction::StudioFinish {
            studio: "acme-shop".into(),
            reason: "done".into(),
            force: true,
        },
        None,
    );
    assert_eq!(crate::studio_fmt::studio_finish(&finished), local_finish);

    let cli_events = load(&cli_env, "acme-shop");
    let doc_events = load(&doc_env, "acme-shop");
    assert_eq!(cli_events.len(), 3);
    assert_eq!(doc_events.len(), cli_events.len());
    for (cli, doc) in cli_events.iter().zip(doc_events.iter()) {
        match (&cli.kind, &doc.kind) {
            (
                backlot_model::EventKind::MessageQueued {
                    to: cli_to,
                    file: cli_file,
                    ..
                },
                backlot_model::EventKind::MessageQueued {
                    to: doc_to,
                    file: doc_file,
                    ..
                },
            ) => {
                assert_eq!(cli_to, doc_to);
                let read = |env: &Env, file: &str| {
                    fs::read_to_string(store::studio_dir(&base_of(env), "acme-shop").join(file))
                        .unwrap()
                };
                assert_eq!(
                    read(&cli_env, cli_file.as_deref().unwrap()),
                    read(&doc_env, doc_file.as_deref().unwrap()),
                    "the staged copy carries the same content"
                );
            }
            _ => assert_eq!(cli.kind, doc.kind),
        }
        assert_eq!(doc.by.role, "user");
        assert_eq!(doc.by.via.as_deref(), Some("doc"));
    }
}

#[test]
fn studio_new_dry_run_and_refusals_match_the_local_command() {
    let dir = TestDir::new("blops-dry");
    let env = env_for(&dir, &make_fake_runner(&dir));
    let repo = dir.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let cast = dir.join("cast.toml");
    let ask = dir.join("ask.md");
    fs::write(&cast, FAKE_CAST).unwrap();
    fs::write(&ask, "first thing\n").unwrap();
    let repo_arg = repo.display().to_string();

    let local = ok(
        &env,
        &format!(
            "studio new --dry-run --no-git --studio acme-shop --title Shop \
             --repo {repo_arg} --cast {} --ask-file {}",
            cast.display(),
            ask.display()
        ),
    );
    let mut server = Server::start(&env);
    let dry: StudioNewResult = server.typed(
        &ReqAction::StudioNew {
            id: "acme-shop".into(),
            title: "Shop".into(),
            repo: repo_arg.clone(),
            cast: FAKE_CAST.into(),
            mode: None,
            init: false,
            no_git: true,
            ask: Some("first thing\n".into()),
            dry_run: true,
        },
        None,
    );
    assert!(dry.dry_run && dry.seq.is_none());
    assert_eq!(crate::studio_fmt::studio_new(&dry), local);
    assert!(!store::studio_dir(&base_of(&env), "acme-shop").exists());

    ok(
        &env,
        &format!(
            "studio new --no-git --studio acme-shop --title Shop \
             --repo {repo_arg} --cast {}",
            cast.display()
        ),
    );
    let refused = cli_testkit::refused(
        &env,
        &format!(
            "studio new --no-git --studio acme-shop --title Shop \
             --repo {repo_arg} --cast {}",
            cast.display()
        ),
    );
    let dup = err_line(&server.send(
        &ReqAction::StudioNew {
            id: "acme-shop".into(),
            title: "Shop".into(),
            repo: repo_arg.clone(),
            cast: FAKE_CAST.into(),
            mode: None,
            init: false,
            no_git: true,
            ask: None,
            dry_run: false,
        },
        None,
    ));
    assert_eq!(dup, refused);

    let local_rel = cli_testkit::usage(
        &env,
        &format!(
            "studio new --title Shop --repo relative/shop --cast {}",
            cast.display()
        ),
    );
    let rel = err_line(&server.send(
        &ReqAction::StudioNew {
            id: "other-shop".into(),
            title: "Shop".into(),
            repo: "relative/shop".into(),
            cast: FAKE_CAST.into(),
            mode: None,
            init: false,
            no_git: false,
            ask: None,
            dry_run: false,
        },
        None,
    ));
    assert_eq!(rel.lines().next(), local_rel.lines().next());
}

#[test]
fn destructive_verbs_require_the_reviewed_token() {
    let (scratch, env) = cli_testkit::scratch();
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
    let mut server = Server::start(&env);

    let delete = |dry_run: bool| ReqAction::StudioDelete {
        studio: Some("acme-shop".into()),
        dry_run,
        force: false,
    };
    let plan: StudioPlan<DeleteReport> = server.typed(&delete(true), None);
    assert!(plan.report.dry_run);
    assert!(
        plan.report
            .removed_worktrees
            .contains(&"work/integration".to_string()),
        "{:?}",
        plan.report
    );
    assert_eq!(plan.confirm_token.len(), 64);
    assert!(plan.report.archive.contains("<stamp>"));

    assert_eq!(
        err_line(&server.send(&delete(false), None)),
        "confirm this from a dry run first"
    );
    assert_eq!(
        err_line(&server.send(&delete(false), Some("00".repeat(32)))),
        "the studio changed since you reviewed it; run it again"
    );
    let done: DeleteReport = server.typed(&delete(false), Some(plan.confirm_token.clone()));
    assert!(done.archived);
    assert!(!done.archive.contains("<stamp>"));

    // Restore with a new name needs the token too; a fresh archive
    // restored under its own name does not.
    let archived = store::archived_studios(&base_of(&env)).unwrap()[0]
        .0
        .clone();
    let restore = |dry_run: bool, name: Option<&str>| ReqAction::StudioRestore {
        archived: archived.clone(),
        name: name.map(str::to_string),
        dry_run,
    };
    let local_restore: serde_json::Value = serde_json::from_str(&ok(
        &env,
        &format!("studio restore {archived} --name acme-back --dry-run --json"),
    ))
    .unwrap();
    let plan: StudioPlan<crate::restore::RestoreReport> =
        server.typed(&restore(true, Some("acme-back")), None);
    assert_eq!(serde_json::to_value(&plan.report).unwrap(), local_restore);
    assert!(plan.report.renamed && plan.report.needs_confirm());
    assert_eq!(
        err_line(&server.send(&restore(false, Some("acme-back")), None)),
        "confirm this from a dry run first"
    );
    let restored: crate::restore::RestoreReport = server.typed(
        &restore(false, Some("acme-back")),
        Some(plan.confirm_token.clone()),
    );
    assert_eq!(restored.name, "acme-back");
    assert!(
        store::studio_dir(&base_of(&env), "acme-back")
            .join("events.jsonl")
            .is_file()
    );
}

#[test]
fn remote_studio_new_without_design_gate_defaults_to_required() {
    let dir = TestDir::new("blops-remote-design-gate");
    let env = env_for(&dir, &make_fake_runner(&dir));
    let mut server = Server::start(&env);
    let made: StudioNewResult = server.typed(
        &ReqAction::StudioNew {
            id: "acme-shop".into(),
            title: "Shop".into(),
            repo: "/home/user/acme-shop".into(),
            cast: FAKE_CAST.into(),
            mode: None,
            init: false,
            no_git: true,
            ask: None,
            dry_run: false,
        },
        None,
    );
    assert_eq!(made.seq, Some(1));
    let state = store::load_state(&store::studio_dir(&base_of(&env), "acme-shop")).unwrap();
    assert_eq!(
        state.studio.unwrap().design_gate,
        backlot_model::design::DesignGate::Required
    );
}
