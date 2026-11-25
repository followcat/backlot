//! Slice 3 tests: non-blocking outward card execution with clean person env,
//! log files under `outward/c<n>.log`, `outward.completed`, role `"outward"`
//! notices, `card.stale` on drift, `card.failed` on planted config/forged
//! `Refuse` cards without executing, `card.cancelled` -> `Denied`, and timeouts.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use backlot_model::EventBy;
use backlot_model::event::OutwardOutcome;
use backlot_model::hollywood::{CardAction, CardBase, CardState, HollywoodKind, NewHollywoodEvent};
use hc_proto::client::Client;
use hc_proto::codec::Reader;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::checkrun::{CheckChild, PollOut};
use crate::cli::Env;
use crate::cli_testkit::{CAST_TOML, fixture, git, ok, scratch};
use crate::daemon::Stage;
use crate::guard::{apply_env, prepare_role};
use crate::notices;
use crate::store::{self, HPending};

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

fn setup_studio() -> (
    crate::cli_testkit::Scratch,
    Env,
    Env,
    PathBuf,
    PathBuf,
    PathBuf,
    PathBuf,
) {
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
    fs::write(work.join("a"), "feature\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-m", "feat commit"]);

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
    let user_env = Env {
        vars: [(
            "BACKLOT_STATE_DIR".to_string(),
            scratch.dir.to_string_lossy().into_owned(),
        )]
        .into_iter()
        .collect(),
    };
    (scratch, env, user_env, bare, work, sdir, role_dir)
}

#[test]
fn applied_outward_card_executes_async_push_clean_env_writes_log_and_notifies_role() {
    let (scratch, role_env, user_env, bare, work, sdir, role_dir) = setup_studio();
    let hdir = store::hollywood_dir(&scratch.dir);
    let _tokio = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut stage = _tokio.block_on(dummy_stage(&scratch.dir));

    // Poison stage env with role markers, GIT_CONFIG_*, and `<role>/bin` on PATH.
    let sys_path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
    stage.env_vars = BTreeMap::from([
        (
            "PATH".into(),
            format!("{}:{sys_path}", role_dir.join("bin").display()),
        ),
        ("HOME".into(), scratch.files.to_string_lossy().into_owned()),
        ("BACKLOT_ROLE".into(), "actor-t1-c1".into()),
        ("BACKLOT_STUDIO".into(), "acme-shop".into()),
        ("BACKLOT_TERMINAL".into(), "bl-acme-shop-actor-t1-c1".into()),
        ("GIT_CONFIG_COUNT".into(), "1".into()),
        ("GIT_CONFIG_KEY_0".into(), "core.sshCommand".into()),
        ("GIT_CONFIG_VALUE_0".into(), "false".into()),
    ]);

    ok(
        &role_env,
        "ask --run --why release_main -- git push origin HEAD:main",
    );
    stage.propose_outward(&hdir).unwrap();
    let head = git(&work, &["rev-parse", "HEAD"]);
    ok(&user_env, "card apply c1");

    // Drive `hcards` until the async child finishes (non-blocking poll).
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        stage.hcards(&hdir).unwrap();
        let st = store::load_hstate(&hdir).unwrap().cards[&1].state;
        if st != CardState::Applied {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let hstate = store::load_hstate(&hdir).unwrap();
    assert_eq!(
        hstate.cards[&1].state,
        CardState::Done,
        "{:?}",
        hstate.cards[&1]
    );
    assert_eq!(git(&bare, &["rev-parse", "refs/heads/main"]), head);
    assert!(
        sdir.join("outward/c1.log").is_file(),
        "missing outward/c1.log"
    );

    let sstate = store::load_state(&sdir).unwrap();
    assert!(sstate.outward_completions.iter().any(|c| {
        c.card == 1
            && c.outcome == OutwardOutcome::Succeeded
            && c.exit_code == Some(0)
            && c.log_file.as_deref() == Some("outward/c1.log")
    }));
    let owed = notices::owed(&sstate, &sdir);
    assert!(
        owed.iter().any(|n| {
            n.to == "actor-t1-c1"
                && n.label == "outward"
                && n.body.contains("git push origin HEAD:main")
        }),
        "{owed:?}"
    );
}

#[test]
fn applied_outward_drift_planted_config_and_forged_refuse_never_execute() {
    let (scratch, role_env, user_env, bare, work, sdir, _role_dir) = setup_studio();
    let hdir = store::hollywood_dir(&scratch.dir);
    let _tokio = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut stage = _tokio.block_on(dummy_stage(&scratch.dir));
    stage.env_vars = BTreeMap::from([(
        "PATH".into(),
        std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
    )]);

    // 1. Moved HEAD after `card apply` -> `card.stale` + `OutwardOutcome::Stale`.
    ok(
        &role_env,
        "ask --run --why drift_case -- git push origin HEAD:main",
    );
    stage.propose_outward(&hdir).unwrap();
    ok(&user_env, "card apply c1");
    fs::write(work.join("drift"), "x\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-m", "drift"]);
    stage.hcards(&hdir).unwrap();
    assert_eq!(
        store::load_hstate(&hdir).unwrap().cards[&1].state,
        CardState::Stale
    );
    assert!(
        store::load_state(&sdir)
            .unwrap()
            .outward_completions
            .iter()
            .any(|c| c.card == 1 && c.outcome == OutwardOutcome::Stale)
    );

    // 2. Planted `core.sshCommand` / `core.hooksPath` in local repo -> `card.failed` without running.
    let marker = scratch.files.join("planted-ran");
    ok(
        &role_env,
        "ask --run --why cfg_case -- git push origin HEAD:main",
    );
    stage.propose_outward(&hdir).unwrap();
    let card2 = *store::load_hstate(&hdir)
        .unwrap()
        .cards
        .keys()
        .max()
        .unwrap();
    ok(&user_env, &format!("card apply c{card2}"));
    git(
        &work,
        &[
            "config",
            "--local",
            "core.sshCommand",
            &format!("touch {}", marker.display()),
        ],
    );
    stage.hcards(&hdir).unwrap();
    assert!(!marker.exists());
    assert_eq!(
        store::load_hstate(&hdir).unwrap().cards[&card2].state,
        CardState::Failed
    );
    git(&work, &["config", "--local", "--unset", "core.sshCommand"]);

    // 3. Forged `Refuse` card (`git push --force`) planted in `hollywood/events.jsonl` -> `card.failed` without running.
    let bare_main_before = git(&bare, &["rev-parse", "refs/heads/main"]);
    let forged = NewHollywoodEvent::new(
        EventBy::role("stage"),
        HollywoodKind::CardProposed {
            action: CardAction::Outward {
                studio: "acme-shop".into(),
                role: "actor-t1-c1".into(),
                source_seq: 999,
                argv: vec![
                    "git".into(),
                    "push".into(),
                    "--force".into(),
                    "origin".into(),
                    "HEAD:main".into(),
                ],
                cwd: work.to_string_lossy().into_owned(),
                repo: scratch.files.join("repo").to_string_lossy().into_owned(),
                remote_url: bare.to_string_lossy().into_owned(),
                rule: "force push".into(),
                commits: Vec::new(),
                commits_total: 0,
            },
            why: "forged force push".into(),
            base: CardBase::GitHead {
                repo: scratch.files.join("repo").to_string_lossy().into_owned(),
                head_oid: git(&work, &["rev-parse", "HEAD"]),
                remote_ref: "refs/heads/main".into(),
                remote_oid: bare_main_before.clone(),
            },
        },
    );
    let card3 = store::append_hollywood(&hdir, |_, _| {
        Ok::<_, std::convert::Infallible>(HPending {
            event: forged,
            files: Vec::new(),
        })
    })
    .unwrap()
    .seq;
    ok(&user_env, &format!("card apply c{card3}"));
    stage.hcards(&hdir).unwrap();
    assert_eq!(
        store::load_hstate(&hdir).unwrap().cards[&card3].state,
        CardState::Failed
    );
    assert_eq!(
        git(&bare, &["rev-parse", "refs/heads/main"]),
        bare_main_before
    );
}

#[test]
fn cancelled_outward_card_records_denied_and_queues_role_notice() {
    let (scratch, role_env, user_env, _bare, _work, sdir, _role_dir) = setup_studio();
    let hdir = store::hollywood_dir(&scratch.dir);
    let _tokio = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let stage = _tokio.block_on(dummy_stage(&scratch.dir));

    ok(
        &role_env,
        "ask --run --why wait_release -- git push origin HEAD:main",
    );
    stage.propose_outward(&hdir).unwrap();
    ok(&user_env, "card cancel c1 --reason wait_for_qa");
    stage.hcards(&hdir).unwrap();
    stage.hcards(&hdir).unwrap();

    let sstate = store::load_state(&sdir).unwrap();
    let denied: Vec<_> = sstate
        .outward_completions
        .iter()
        .filter(|c| c.card == 1 && c.outcome == OutwardOutcome::Denied)
        .collect();
    assert_eq!(denied.len(), 1, "{:?}", sstate.outward_completions);
    let owed = notices::owed(&sstate, &sdir);
    assert!(
        owed.iter().any(|n| {
            n.to == "actor-t1-c1" && n.label == "outward" && n.body.contains("wait_for_qa")
        }),
        "{owed:?}"
    );
}

#[test]
fn non_blocking_poll_and_timeout_kills_slow_outward_child() {
    struct ScriptedChild {
        polls: Arc<Mutex<Vec<&'static str>>>,
        killed_after: bool,
    }
    impl CheckChild for ScriptedChild {
        fn poll(&mut self) -> Option<PollOut> {
            self.polls.lock().unwrap().push("poll");
            if self.killed_after {
                Some(PollOut {
                    code: None,
                    output: b"timed out line\n".to_vec(),
                })
            } else {
                None
            }
        }
        fn term(&mut self) {
            self.polls.lock().unwrap().push("term");
        }
        fn kill(&mut self) {
            self.polls.lock().unwrap().push("kill");
            self.killed_after = true;
        }
    }

    let (scratch, role_env, user_env, _bare, _work, sdir, _role_dir) = setup_studio();
    let hdir = store::hollywood_dir(&scratch.dir);
    let _tokio = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut stage = _tokio.block_on(dummy_stage(&scratch.dir));
    stage.env_vars = BTreeMap::from([(
        "PATH".into(),
        std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
    )]);
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev_clone = Arc::clone(&events);
    stage.spawn_outward = Box::new(move |_, _, _, _| {
        Ok(Box::new(ScriptedChild {
            polls: Arc::clone(&ev_clone),
            killed_after: false,
        }))
    });

    ok(
        &role_env,
        "ask --run --why slow_push -- git push origin HEAD:main",
    );
    stage.propose_outward(&hdir).unwrap();
    ok(&user_env, "card apply c1");

    // Tick at 1s: spawns without blocking; card remains Applied.
    stage.hcards_at(&hdir, 1_000).unwrap();
    assert_eq!(
        store::load_hstate(&hdir).unwrap().cards[&1].state,
        CardState::Applied
    );
    // Tick at 121s (>120s timeout): sends TERM.
    stage.hcards_at(&hdir, 121_000).unwrap();
    assert_eq!(
        store::load_hstate(&hdir).unwrap().cards[&1].state,
        CardState::Applied
    );
    // Tick at 126s (+5s after TERM): sends KILL -> next poll completes TimedOut.
    stage.hcards_at(&hdir, 126_000).unwrap();
    stage.hcards_at(&hdir, 126_100).unwrap();

    assert_eq!(
        store::load_hstate(&hdir).unwrap().cards[&1].state,
        CardState::Failed
    );
    assert!(
        store::load_state(&sdir)
            .unwrap()
            .outward_completions
            .iter()
            .any(|c| c.card == 1 && c.outcome == OutwardOutcome::TimedOut)
    );
    let trace = events.lock().unwrap();
    assert!(
        trace.contains(&"term") && trace.contains(&"kill"),
        "{trace:?}"
    );
}
