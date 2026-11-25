//! Round-1 review tests: fail-closed `git push` & global option allowlist,
//! `remote.<name>.mirror` / `push.recurseSubmodules` config hardening, and
//! glob refspec handling.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use hc_proto::client::Client;
use hc_proto::codec::Reader;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::cli::Env;
use crate::cli_testkit::{CAST_TOML, git, ok};
use crate::daemon::Stage;
use crate::guard_git_env::verify_hook_integrity;
use crate::guard_git_tests::{argv, setup_git_studio, studio_policy};
use crate::guard_rules::{GuardVerdict, classify_command};
use crate::hcard_outward_git::verify_local_git_clean;
use crate::store;
use backlot_model::hollywood::CardState;

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

#[test]
fn review1_abbreviated_and_unknown_push_options_are_refused_and_allowlist_passes() {
    let gs = setup_git_studio(CAST_TOML);
    let policy = studio_policy(&gs.sdir);

    for args in [
        &["push", "--mir", "origin"][..],
        &["push", "--del", "origin", "feat/login"][..],
        &["push", "--no-verif", "origin", "feat/login"][..],
        &["push", "--force-w", "origin", "feat/login"][..],
        &[
            "push",
            "--force-w=refs/heads/feat/login",
            "origin",
            "feat/login",
        ][..],
        &["push", "--force-i", "origin", "feat/login"][..],
        &["push", "--exe=cat", "origin", "feat/login"][..],
        &["push", "--rec=cat", "origin", "feat/login"][..],
        &["push", "--upl=cat", "origin", "feat/login"][..],
        &["push", "--pru", "origin", "feat/login"][..],
        &["push", "--no-verif", "--mir", "origin"][..],
        &["push", "--unknown-flag", "origin", "feat/login"][..],
        &["push", "--quiet=yes", "origin", "feat/login"][..],
        &["push", "--signed", "origin", "feat/login"][..],
        &["push", "--signed=if-asked", "origin", "feat/login"][..],
        &["push", "--recurse-submodules", "origin", "feat/login"][..],
        &[
            "push",
            "--recurse-submodules=on-demand",
            "origin",
            "feat/login",
        ][..],
        &["push", "--recurse-submodules=only", "origin", "feat/login"][..],
        &["push", "-x", "origin", "feat/login"][..],
        &["push", "-ux", "origin", "feat/login"][..],
        &["push", "-qf", "origin", "feat/login"][..],
        &["push", "-qd", "origin", "feat/login"][..],
        &["push", "--push-option"][..],
        &["push", "--repo"][..],
        &["push", "-o"][..],
        &["--exec-path=/tmp", "status"][..],
        &["--exec-p=/tmp", "push", "origin", "feat/login"][..],
    ] {
        let v = classify_command("git", &argv(args), &policy, &gs.work, &gs.env);
        assert!(
            matches!(v, GuardVerdict::Refuse { .. }),
            "expected Refuse for {args:?}, got {v:?}"
        );
    }

    // Aliases carrying abbreviated flags or overridden global --exec-path stay refused.
    git(&gs.repo, &["config", "alias.pmir", "push --mir"]);
    git(&gs.repo, &["config", "alias.pplain", "push"]);
    assert!(matches!(
        classify_command(
            "git",
            &argv(&["pmir", "origin"]),
            &policy,
            &gs.work,
            &gs.env
        ),
        GuardVerdict::Refuse { .. }
    ));
    assert!(matches!(
        classify_command(
            "git",
            &argv(&["--exec-path=/tmp", "pplain", "origin", "feat/login"]),
            &policy,
            &gs.work,
            &gs.env
        ),
        GuardVerdict::Refuse { .. }
    ));

    // Allowlisted exact long options, harmless negations, and allowed short clusters still Allow.
    for args in [
        &["push", "--set-upstream", "origin", "feat/login"][..],
        &["push", "--quiet", "origin", "feat/login"][..],
        &["push", "--verbose", "origin", "feat/login"][..],
        &["push", "--progress", "origin", "feat/login"][..],
        &["push", "--no-progress", "origin", "feat/login"][..],
        &["push", "--dry-run", "origin", "feat/login"][..],
        &["push", "--porcelain", "origin", "feat/login"][..],
        &["push", "--atomic", "origin", "feat/login"][..],
        &["push", "--no-atomic", "origin", "feat/login"][..],
        &["push", "--ipv4", "origin", "feat/login"][..],
        &["push", "--ipv6", "origin", "feat/login"][..],
        &["push", "--push-option", "ci.skip", "origin", "feat/login"][..],
        &["push", "--push-option=ci.skip", "origin", "feat/login"][..],
        &["push", "--no-push-option", "origin", "feat/login"][..],
        &["push", "--repo", "origin"][..],
        &["push", "--repo=origin"][..],
        &[
            "push",
            "--no-quiet",
            "--no-verbose",
            "--no-dry-run",
            "origin",
            "feat/login",
        ][..],
        &[
            "push",
            "--no-signed",
            "--signed=false",
            "origin",
            "feat/login",
        ][..],
        &["push", "--recurse-submodules=no", "origin", "feat/login"][..],
        &["push", "--recurse-submodules=check", "origin", "feat/login"][..],
        &["push", "--no-recurse-submodules", "origin", "feat/login"][..],
        &["push", "--verify", "origin", "feat/login"][..],
        &[
            "push",
            "-u",
            "-q",
            "-v",
            "-n",
            "-4",
            "-6",
            "origin",
            "feat/login",
        ][..],
        &["push", "-quvn46", "origin", "feat/login"][..],
        &["push", "-o", "ci.skip", "origin", "feat/login"][..],
        &["push", "-oci.skip", "origin", "feat/login"][..],
        &["push", "-qo", "ci.skip", "origin", "feat/login"][..],
        &["push", "-qoci.skip", "origin", "feat/login"][..],
    ] {
        let v = classify_command("git", &argv(args), &policy, &gs.work, &gs.env);
        assert_eq!(v, GuardVerdict::Allow, "expected Allow for {args:?}");
    }

    // Card-classified allowlisted options stay Card.
    for args in [
        &["push", "--tags", "origin", "feat/login"][..],
        &["push", "--follow-tags", "origin", "feat/login"][..],
        &["push", "--all", "origin"][..],
        &["push", "--branches", "origin"][..],
    ] {
        let v = classify_command("git", &argv(args), &policy, &gs.work, &gs.env);
        assert!(
            matches!(v, GuardVerdict::Card { .. }),
            "expected Card for {args:?}, got {v:?}"
        );
    }

    let v_unk = classify_command(
        "git",
        &argv(&["push", "--unknown-flag", "origin", "feat/login"]),
        &policy,
        &gs.work,
        &gs.env,
    );
    assert!(
        matches!(&v_unk, GuardVerdict::Refuse { rule, .. } if rule.contains("unknown git push option")),
        "got {v_unk:?}"
    );
}

#[test]
fn review1_mirror_and_recurse_submodules_config_refused_in_guard_and_stage() {
    let gs = setup_git_studio(CAST_TOML);
    let policy = studio_policy(&gs.sdir);
    let clean = Env {
        vars: [(
            "PATH".into(),
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        )]
        .into_iter()
        .collect(),
    };

    // 1. remote.origin.mirror = true refuses push + verify_hook_integrity + verify_local_git_clean.
    git(&gs.repo, &["config", "remote.origin.mirror", "true"]);
    assert!(matches!(
        classify_command(
            "git",
            &argv(&["push", "origin"]),
            &policy,
            &gs.work,
            &gs.env
        ),
        GuardVerdict::Refuse { .. }
    ));
    assert!(verify_hook_integrity(&gs.work, &gs.env, Some(&gs.role_dir)).is_err());
    assert!(verify_local_git_clean(&gs.work, &clean).is_err());

    // Valueless `[remote "exfil"] mirror` in `.git/config` is also refused.
    let cfg_path = gs.repo.join(".git/config");
    let orig_cfg = fs::read_to_string(&cfg_path).unwrap();
    fs::write(
        &cfg_path,
        format!("{orig_cfg}\n[remote \"exfil\"]\n\tmirror\n"),
    )
    .unwrap();
    assert!(verify_hook_integrity(&gs.work, &gs.env, Some(&gs.role_dir)).is_err());
    assert!(verify_local_git_clean(&gs.work, &clean).is_err());
    fs::write(&cfg_path, orig_cfg).unwrap();

    // remote.origin.mirror = false is harmless.
    git(&gs.repo, &["config", "remote.origin.mirror", "false"]);
    assert_eq!(
        classify_command(
            "git",
            &argv(&["push", "origin", "feat/login"]),
            &policy,
            &gs.work,
            &gs.env
        ),
        GuardVerdict::Allow
    );
    git(&gs.repo, &["config", "--unset", "remote.origin.mirror"]);

    // 2. push.recurseSubmodules = on-demand | only refuses push + both verify checks.
    for bad in ["on-demand", "only"] {
        git(&gs.repo, &["config", "push.recurseSubmodules", bad]);
        assert!(
            matches!(
                classify_command(
                    "git",
                    &argv(&["push", "origin", "feat/login"]),
                    &policy,
                    &gs.work,
                    &gs.env
                ),
                GuardVerdict::Refuse { .. }
            ),
            "expected Refuse for push.recurseSubmodules={bad}"
        );
        assert!(verify_hook_integrity(&gs.work, &gs.env, Some(&gs.role_dir)).is_err());
        assert!(verify_local_git_clean(&gs.work, &clean).is_err());
    }

    // Benign values check | no stay Allowed.
    for ok_val in ["no", "check"] {
        git(&gs.repo, &["config", "push.recurseSubmodules", ok_val]);
        assert_eq!(
            classify_command(
                "git",
                &argv(&["push", "origin", "feat/login"]),
                &policy,
                &gs.work,
                &gs.env
            ),
            GuardVerdict::Allow,
            "expected Allow for push.recurseSubmodules={ok_val}"
        );
        assert!(verify_hook_integrity(&gs.work, &gs.env, Some(&gs.role_dir)).is_ok());
        assert!(verify_local_git_clean(&gs.work, &clean).is_ok());
    }
    git(&gs.repo, &["config", "--unset", "push.recurseSubmodules"]);

    // submodule.recurse = true (with push.recurseSubmodules unset) acts as on-demand -> Refuse.
    git(&gs.repo, &["config", "submodule.recurse", "true"]);
    assert!(matches!(
        classify_command(
            "git",
            &argv(&["push", "origin", "feat/login"]),
            &policy,
            &gs.work,
            &gs.env
        ),
        GuardVerdict::Refuse { .. }
    ));
    git(&gs.repo, &["config", "--unset", "submodule.recurse"]);

    // 3. Person's Stage execution refuses applied card without pushing if mirror or recurseSubmodules was planted.
    let hdir = store::hollywood_dir(&gs.scratch.dir);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut stage = rt.block_on(dummy_stage(&gs.scratch.dir));
    stage.env_vars = BTreeMap::from([(
        "PATH".into(),
        std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
    )]);
    let user_env = Env {
        vars: [(
            "BACKLOT_STATE_DIR".into(),
            gs.scratch.dir.to_string_lossy().into_owned(),
        )]
        .into_iter()
        .collect(),
    };
    ok(
        &gs.env,
        "ask --run --why release_main -- git push origin HEAD:main",
    );
    stage.propose_outward(&hdir).unwrap();
    ok(&user_env, "card apply c1");
    git(&gs.work, &["config", "remote.origin.mirror", "true"]);
    stage.hcards(&hdir).unwrap();
    assert_eq!(
        store::load_hstate(&hdir).unwrap().cards[&1].state,
        CardState::Failed
    );
    git(&gs.work, &["config", "--unset", "remote.origin.mirror"]);

    ok(
        &gs.env,
        "ask --run --why release_main2 -- git push origin HEAD:main",
    );
    stage.propose_outward(&hdir).unwrap();
    let cid2 = *store::load_hstate(&hdir)
        .unwrap()
        .cards
        .keys()
        .max()
        .unwrap();
    ok(&user_env, &format!("card apply c{cid2}"));
    git(&gs.work, &["config", "push.recurseSubmodules", "on-demand"]);
    stage.hcards(&hdir).unwrap();
    assert_eq!(
        store::load_hstate(&hdir).unwrap().cards[&cid2].state,
        CardState::Failed
    );
}

#[test]
fn review1_glob_refspecs_become_card() {
    let gs = setup_git_studio(CAST_TOML);
    let policy = studio_policy(&gs.sdir);

    for rs in [
        "refs/heads/*:refs/heads/*",
        "refs/heads/feat/*:refs/heads/feat/*",
        "feat/*",
    ] {
        let v = classify_command(
            "git",
            &argv(&["push", "origin", rs]),
            &policy,
            &gs.work,
            &gs.env,
        );
        assert!(
            matches!(v, GuardVerdict::Card { .. }),
            "expected Card for glob refspec '{rs}', got {v:?}"
        );
    }
}
