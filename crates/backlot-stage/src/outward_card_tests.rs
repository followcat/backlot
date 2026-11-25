//! Slice 2 tests: `role.refused (Card)` and `backlot ask --run`
//! card proposal without `[hollywood]`, `gh pr create` remote head-branch
//! precondition & staleness, `StudioSummary.outward_cards`, and `card show`.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use backlot_model::event::EventKind;
use backlot_model::hollywood::{CardAction, CardBase, CardState};
use backlot_model::summary::summary_with_running;
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::cli::Env;
use crate::cli_testkit::{CAST_TOML, fixture, git, ok, refused, scratch};
use crate::daemon::Stage;
use crate::guard::{apply_env, prepare_role, run_in_with_stdin};
use crate::store;

async fn dummy_stage(base: &Path) -> Stage {
    let (core_side, client_side) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        let (read, mut write) = tokio::io::split(core_side);
        let mut reader = Reader::new(read);
        let hello = reader.read_line().await.unwrap().unwrap();
        let req: Value = serde_json::from_slice(&hello).unwrap();
        let reply = serde_json::to_vec(&json!({
            "jsonrpc": "2.0", "id": req["id"],
            "result": {
                "generation": 1, "version": "fake-core 0.0",
                "framed": true,
                "core": {"id": "core-1", "name": "fake"},
                "methods": [], "features": [],
            }
        }))
        .unwrap();
        write.write_all(&reply).await.unwrap();
        write.write_all(b"\n").await.unwrap();
        while let Ok(Some(_)) = reader.read_frame().await {}
    });
    let (read, write) = tokio::io::split(client_side);
    let client = Client::open(Box::pin(read), Box::pin(write), None)
        .await
        .unwrap();
    let mut stage = Stage::for_test(client);
    stage.base = base.to_path_buf();
    stage.state_dir = base.to_string_lossy().into_owned();
    stage.path_env = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
    stage
}

fn setup_git_studio() -> (crate::cli_testkit::Scratch, Env, PathBuf, PathBuf, PathBuf) {
    let (scratch, mut env) = scratch();
    let bare = scratch.files.join("bare.git");
    let repo = scratch.files.join("repo");
    fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare", "-b", "main"]);
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("f"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);
    git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(&repo, &["push", "-u", "origin", "main"]);
    git(&bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git(&repo, &["fetch", "origin"]);
    git(&repo, &["remote", "set-head", "origin", "main"]);

    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast}",
            repo.display()
        ),
    );
    let sdir = store::studio_dir(&scratch.dir, "acme-shop");
    let work = sdir.join("work/t1");
    fs::create_dir_all(work.parent().unwrap()).unwrap();
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "feat/login",
            work.to_str().unwrap(),
        ],
    );
    fs::write(work.join("a"), "one\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-m", "add login model"]);
    fs::write(work.join("b"), "two\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-m", "add login view"]);

    let role_dir = sdir.join("roles/actor-t1-c1");
    let exe_dir = scratch.files.join("exe");
    fs::create_dir_all(&exe_dir).unwrap();
    let mock_backlot = exe_dir.join("backlot");
    fs::write(&mock_backlot, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&mock_backlot, fs::Permissions::from_mode(0o755)).unwrap();
    prepare_role(&role_dir, exe_dir.to_str().unwrap()).unwrap();
    env.vars.insert("BACKLOT_ROLE".into(), "actor-t1-c1".into());
    env.vars.insert("BACKLOT_TASK".into(), "t1".into());
    let base_path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
    apply_env(
        &mut env.vars,
        &role_dir,
        exe_dir.to_str().unwrap(),
        &base_path,
        None,
    );
    (scratch, env, bare, work, sdir)
}

#[test]
fn role_refused_card_and_ask_run_create_deduped_outward_cards_without_hollywood() {
    let (scratch, env, bare, work, sdir) = setup_git_studio();
    let hdir = store::hollywood_dir(&scratch.dir);
    let _tokio = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let rt = _tokio.block_on(dummy_stage(&scratch.dir));

    // 1. Guarded `git push origin main` (Card) writes `role.refused { disposition: Card }`.
    let args: Vec<String> = ["git", "--", "push", "origin", "HEAD:main"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let (code, _) = run_in_with_stdin(&args, &env, &work, false, "");
    assert_eq!(code, ExitCode::from(126));

    // Proposing twice yields one deduped CardAction::Outward even without [hollywood].
    rt.propose_outward(&hdir).unwrap();
    rt.propose_outward(&hdir).unwrap();
    let hstate = store::load_hstate(&hdir).unwrap();
    assert_eq!(hstate.cards.len(), 1, "expected 1 proposed card");
    let c1 = &hstate.cards[&1];
    assert_eq!(c1.by, "stage");
    assert_eq!(c1.state, CardState::Open);
    match (&c1.action, &c1.base) {
        (
            CardAction::Outward {
                studio,
                role,
                remote_url,
                rule,
                commits,
                commits_total,
                ..
            },
            CardBase::GitHead { head_oid, .. },
        ) => {
            assert_eq!(studio, "acme-shop");
            assert_eq!(role, "actor-t1-c1");
            assert_eq!(remote_url, &bare.to_string_lossy());
            assert!(rule.contains("protected branch 'main'"), "{rule}");
            assert_eq!(*commits_total, 2);
            assert_eq!(commits.len(), 2);
            assert_eq!(head_oid, &git(&work, &["rev-parse", "HEAD"]));
        }
        other => panic!("unexpected card action/base: {other:?}"),
    }

    // 2. `backlot ask --run` validates Card verdict at CLI time and appends `outward.requested`.
    let bad_allow = refused(&env, "ask --run -- git push origin feat/login");
    assert!(
        bad_allow.contains("card") || bad_allow.contains("allow"),
        "{bad_allow}"
    );
    let bad_force = refused(&env, "ask --run -- git push --force origin feat/login");
    assert!(
        bad_force.contains("refuse") || bad_force.contains("card"),
        "{bad_force}"
    );

    ok(
        &env,
        "ask --run --why release_branch -- git push origin HEAD:release/v1",
    );
    // Single-string form `backlot ask --run "git push origin main"` also parses and validates.
    let single = crate::cli::run(
        &["ask".into(), "--run".into(), "git push origin main".into()],
        &env,
    )
    .unwrap();
    assert!(single.contains("outward.requested"), "{single}");
    rt.propose_outward(&hdir).unwrap();
    rt.propose_outward(&hdir).unwrap();
    let hstate = store::load_hstate(&hdir).unwrap();
    assert_eq!(hstate.cards.len(), 3);

    // 3. `StudioSummary.outward_cards` populates without [hollywood].
    let sstate = store::load_state(&sdir).unwrap();
    let mut doc = summary_with_running(
        &[("acme-shop".into(), sstate)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        100,
    );
    backlot_model::summary::attach_outward_cards(&mut doc, &hstate);
    assert_eq!(doc.hollywood, None);
    assert_eq!(doc.studios[0].outward_cards.len(), 3);
    assert_eq!(doc.studios[0].outward_cards[0].id, "c1");
    assert_eq!(doc.studios[0].outward_cards[0].commits_total, 2);

    // 4. `card list` and `card show c1` render full preview.
    let user_env = Env {
        vars: [(
            "BACKLOT_STATE_DIR".to_string(),
            scratch.dir.to_string_lossy().into_owned(),
        )]
        .into_iter()
        .collect(),
    };
    let listed = ok(&user_env, "card list");
    assert!(listed.contains("c1") && listed.contains("c2"), "{listed}");
    let shown = ok(&user_env, "card show c1");
    assert!(shown.contains("actor-t1-c1"), "{shown}");
    assert!(shown.contains("add login view"), "{shown}");
    assert!(
        shown.contains(&bare.to_string_lossy().into_owned()),
        "{shown}"
    );
    assert!(shown.contains("protected branch 'main'"), "{shown}");
}

#[test]
fn gh_pr_create_requires_pushed_head_on_recorded_remote_and_stales_on_drift() {
    let (scratch, env, _bare, work, sdir) = setup_git_studio();
    let hdir = store::hollywood_dir(&scratch.dir);
    let _tokio = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let rt = _tokio.block_on(dummy_stage(&scratch.dir));

    // 1. `feat/login` is NOT pushed yet -> `ask --run -- gh pr create` fails at proposal cleanly.
    ok(
        &env,
        "ask --run --why open_pr -- gh pr create --title Add --body x",
    );
    rt.propose_outward(&hdir).unwrap();
    assert!(
        store::load_hstate(&hdir).unwrap().cards.is_empty(),
        "must not propose card when head branch is unpushed"
    );
    let (events, _) = store::load_events(&sdir).unwrap();
    assert!(events.iter().any(|e| matches!(
        &e.kind,
        EventKind::OutwardCompleted {
            card: 0,
            output_tail,
            ..
        } if output_tail.contains("not pushed to recorded remote") && output_tail.contains("push it first")
    )));

    // 2. Push `feat/login` (auto-allowed via recorded origin) then request `gh pr create` again.
    git(&work, &["push", "-u", "origin", "feat/login"]);
    ok(
        &env,
        "ask --run --why open_pr -- gh pr create --title Add --body x",
    );
    rt.propose_outward(&hdir).unwrap();
    let hstate = store::load_hstate(&hdir).unwrap();
    assert_eq!(hstate.cards.len(), 1);
    let c = hstate.cards.values().next().unwrap();
    let cid = c.seq;

    // 3. Amend HEAD locally so remote branch lags HEAD -> `backlot card apply` marks `card.stale`.
    fs::write(work.join("c"), "three\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-m", "local drift commit"]);

    let user_env = Env {
        vars: [(
            "BACKLOT_STATE_DIR".to_string(),
            scratch.dir.to_string_lossy().into_owned(),
        )]
        .into_iter()
        .collect(),
    };
    let apply_out = ok(&user_env, &format!("card apply c{cid}"));
    assert!(apply_out.contains("card.stale"), "{apply_out}");
    assert_eq!(
        store::load_hstate(&hdir).unwrap().cards[&cid].state,
        CardState::Stale
    );
}
