//! Tests for eliminating noisy `role.refused` and `outward action`
//! events on benign read-only probes while preserving refusal and
//! recording of real writes, pushes, merges, comments, and outside
//! reaches.

use std::fs;
use std::io::Cursor;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use backlot_model::event::{EventKind, OUTWARD_ACTION, PushPolicy, RefusalDisposition};

use crate::cli::Env;
use crate::cli_testkit::{CAST_TOML, git};
use crate::guard::{apply_env, prepare_role, run_in};
use crate::guard_git::{GitContext, is_fast_path};
use crate::guard_git_tests::{GitStudio, argv, setup_git_studio, studio_policy};
use crate::guard_rules::{GuardVerdict, classify_command};
use crate::hook::run_io_with_delay;
use crate::runners::agy;
use crate::store;

fn env_for(gs: &GitStudio, role: &str, task: Option<&str>) -> (Env, PathBuf) {
    let role_dir = gs.sdir.join("roles").join(role);
    let exe_dir = gs.scratch.files.join("exe");
    prepare_role(&role_dir, exe_dir.to_str().unwrap()).unwrap();
    let fake_gh = exe_dir.join("gh");
    if !fake_gh.exists() {
        fs::write(&fake_gh, "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(&fake_gh, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut vars = gs.env.vars.clone();
    vars.insert("BACKLOT_STUDIO".into(), "acme-shop".into());
    vars.insert("BACKLOT_ROLE".into(), role.into());
    match task {
        Some(t) => {
            vars.insert("BACKLOT_TASK".into(), t.into());
        }
        None => {
            vars.remove("BACKLOT_TASK");
        }
    }
    vars.remove("GIT_CONFIG_COUNT");
    vars.remove("GIT_CONFIG_PARAMETERS");
    for i in 0..4 {
        vars.remove(&format!("GIT_CONFIG_KEY_{i}"));
        vars.remove(&format!("GIT_CONFIG_VALUE_{i}"));
    }
    let base_path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
    apply_env(
        &mut vars,
        &role_dir,
        exe_dir.to_str().unwrap(),
        &base_path,
        Some(&base_path),
    );
    (Env { vars }, role_dir)
}

fn add_integration_worktree(gs: &GitStudio) -> PathBuf {
    let integration = gs.sdir.join("work/integration");
    git(
        &gs.repo,
        &[
            "worktree",
            "add",
            "-b",
            "feat/integration",
            integration.to_str().unwrap(),
        ],
    );
    integration
}

fn run_agy(gs: &GitStudio, env: &Env, cwd: &Path, cmd: &str) -> serde_json::Value {
    let sig = gs.scratch.files.join("agy-signals.jsonl");
    let hargs = vec![
        "--runner".into(),
        "agy".into(),
        "--file".into(),
        sig.to_string_lossy().into_owned(),
    ];
    let payload = serde_json::json!({
        "conversationId": "conv-probe",
        "hookEventName": "PreToolUse",
        "toolCall": { "name": "run_command", "args": { "CommandLine": cmd } }
    })
    .to_string();
    let mut out = Vec::new();
    let (code, done) = run_io_with_delay(
        &hargs,
        env,
        cwd,
        Duration::from_millis(1500),
        None,
        &mut Cursor::new(payload.as_bytes()),
        &mut out,
    );
    done.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(code, ExitCode::SUCCESS);
    serde_json::from_slice(&out).unwrap()
}

fn refused_events(sdir: &Path) -> Vec<EventKind> {
    store::load_events(sdir)
        .unwrap()
        .0
        .into_iter()
        .filter(|e| matches!(e.kind, EventKind::RoleRefused { .. }))
        .map(|e| e.kind)
        .collect()
}

fn outward_attention_events(sdir: &Path) -> Vec<EventKind> {
    store::load_events(sdir)
        .unwrap()
        .0
        .into_iter()
        .filter(|e| matches!(&e.kind, EventKind::AttentionRaised { reason, .. } if reason == OUTWARD_ACTION))
        .map(|e| e.kind)
        .collect()
}

#[test]
fn benign_git_probes_and_studio_worktrees_allow_without_refusal() {
    let gs = setup_git_studio(CAST_TOML);
    let policy = studio_policy(&gs.sdir);
    let integration = add_integration_worktree(&gs);
    let prompts = gs.sdir.join("prompts");
    let messages = gs.sdir.join("messages");
    let non_repo_tmp = gs.scratch.files.join("pytest-tmp");
    for dir in [&prompts, &messages, &non_repo_tmp] {
        fs::create_dir_all(dir).unwrap();
    }

    let (actor_env, actor_rdir) = env_for(&gs, "actor-t1-c1", Some("t1"));
    for args in [
        &["--version"][..],
        &["-v"][..],
        &["version"][..],
        &["--help"][..],
        &["help"][..],
        &["rev-parse", "--show-toplevel"][..],
        &["rev-parse", "--git-dir"][..],
        &["rev-parse", "--git-common-dir"][..],
        &["rev-parse", "--is-inside-work-tree"][..],
        &["rev-parse", "--is-bare-repository"][..],
        &["rev-parse", "--show-prefix"][..],
    ] {
        for cwd in [&prompts, &messages, &non_repo_tmp] {
            assert_eq!(
                classify_command("git", &argv(args), &policy, cwd, &actor_env),
                GuardVerdict::Allow,
                "args={args:?} cwd={}",
                cwd.display()
            );
            let ctx = GitContext {
                policy: &policy,
                cwd,
                role_dir: Some(&actor_rdir),
                work_area: Some(&gs.work),
                repo_root: Some(&gs.repo),
                members: &[],
                env: &actor_env,
            };
            assert!(
                is_fast_path(&argv(args), &ctx),
                "expected fast path for {args:?} in {}",
                cwd.display()
            );
            let mut full = vec!["git".into(), "--".into()];
            full.extend(argv(args));
            let (code, msg) = run_in(&full, &actor_env, cwd, false);
            assert!(msg.is_none(), "unexpected refusal for {args:?}: {msg:?}");
            assert_ne!(code, ExitCode::from(126), "args={args:?}");
        }
    }
    assert_eq!(
        run_agy(&gs, &actor_env, &prompts, "git rev-parse --show-toplevel")["decision"],
        "allow"
    );

    // Producer/writer inspecting `work/integration`, and director inspecting `../t1` and `repo`.
    let (producer_env, _) = env_for(&gs, "producer", None);
    for args in [
        &[
            "-C",
            integration.to_str().unwrap(),
            "status",
            "--short",
            "--branch",
        ][..],
        &[
            "-C",
            integration.to_str().unwrap(),
            "log",
            "-1",
            "--oneline",
        ][..],
        &["-C", integration.to_str().unwrap(), "rev-parse", "HEAD"][..],
    ] {
        assert_eq!(
            classify_command("git", &argv(args), &policy, &integration, &producer_env),
            GuardVerdict::Allow,
            "producer read in integration failed: {args:?}"
        );
    }

    let (director_env, _) = env_for(&gs, "director", None);
    assert_eq!(
        classify_command(
            "git",
            &argv(&["-C", "../t1", "status", "--short", "--branch"]),
            &policy,
            &integration,
            &director_env
        ),
        GuardVerdict::Allow
    );
    assert_eq!(
        classify_command(
            "git",
            &argv(&["status", "--short", "--branch"]),
            &policy,
            &gs.repo,
            &director_env
        ),
        GuardVerdict::Allow
    );

    assert!(refused_events(&gs.sdir).is_empty());
    assert!(outward_attention_events(&gs.sdir).is_empty());
}

#[test]
fn benign_gh_read_probes_and_agy_settings_allow() {
    let gs = setup_git_studio(CAST_TOML);
    let policy = studio_policy(&gs.sdir);
    let (actor_env, _) = env_for(&gs, "actor-t1-c1", Some("t1"));

    for args in [
        &["pr", "view", "--json", "number,title,url,state"][..],
        &["pr", "list"][..],
        &["pr", "status"][..],
        &["pr", "diff"][..],
        &["pr", "checks"][..],
        &[
            "repo",
            "view",
            "acme/shop",
            "--json",
            "name,owner,visibility",
        ][..],
        &["repo", "list", "acme", "--limit", "20"][..],
        &["issue", "list", "-R", "acme/shop", "--limit", "10"][..],
        &["issue", "view", "1"][..],
        &["issue", "status"][..],
        &["release", "list"][..],
        &["release", "view", "v1.0"][..],
        &["gist", "list"][..],
        &["gist", "view", "abc123"][..],
        &["workflow", "run", "--help"][..],
        &["repo", "create", "--help"][..],
        &["--help"][..],
    ] {
        assert_eq!(
            classify_command("gh", &argv(args), &policy, &gs.work, &actor_env),
            GuardVerdict::Allow,
            "gh probe should Allow: {args:?}"
        );
        let mut full = vec!["gh".into(), "--".into()];
        full.extend(argv(args));
        let (code, msg) = run_in(&full, &actor_env, &gs.work, false);
        assert_eq!(
            code,
            ExitCode::SUCCESS,
            "gh run_in failed {args:?}: {msg:?}"
        );
    }

    assert_eq!(
        run_agy(
            &gs,
            &actor_env,
            &gs.work,
            "gh repo view acme/shop --json name"
        )["decision"],
        "allow"
    );
    assert_eq!(
        run_agy(&gs, &actor_env, &gs.work, "gh workflow run --help")["decision"],
        "allow"
    );

    let settings = String::from_utf8(agy::settings_content(None, None).unwrap()).unwrap();
    for coarse in [
        "command(gh repo)",
        "command(gh issue)",
        "command(gh release)",
        "command(gh api)",
    ] {
        assert!(!settings.contains(coarse), "{coarse} left in {settings}");
    }
    assert!(refused_events(&gs.sdir).is_empty());
    assert!(outward_attention_events(&gs.sdir).is_empty());
}

#[test]
fn suppressed_probe_refusals_emit_no_ledger_events() {
    let gs = setup_git_studio(CAST_TOML);
    let (actor_env, _) = env_for(&gs, "actor-t1-c1", Some("t1"));
    let policy = PushPolicy::default();

    // IDE git index/remote probes with `-c core.hooksPath=/dev/null` or `GIT_CONFIG_*` remain
    // refused by the guard, yet must not append `role.refused` or raise `outward action`.
    let ide_ls = argv(&[
        "git",
        "--",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.fsmonitor=",
        "ls-files",
        "--error-unmatch",
        "--",
        ".claude/settings.local.json",
    ]);
    assert!(matches!(
        classify_command("git", &ide_ls[2..], &policy, &gs.work, &actor_env),
        GuardVerdict::Refuse { .. }
    ));
    let (code, _) = run_in(&ide_ls, &actor_env, &gs.work, false);
    assert_eq!(code, ExitCode::from(126));

    let mut cfg_env = Env {
        vars: actor_env.vars.clone(),
    };
    cfg_env.vars.insert("GIT_CONFIG_COUNT".into(), "1".into());
    cfg_env
        .vars
        .insert("GIT_CONFIG_KEY_0".into(), "core.fsmonitor".into());
    cfg_env
        .vars
        .insert("GIT_CONFIG_VALUE_0".into(), String::new());
    let (code_cfg, _) = run_in(
        &argv(&["git", "--", "remote", "get-url", "origin"]),
        &cfg_env,
        &gs.work,
        false,
    );
    assert_eq!(code_cfg, ExitCode::from(126));

    let mut params_env = Env {
        vars: actor_env.vars.clone(),
    };
    params_env.vars.insert(
        "GIT_CONFIG_PARAMETERS".into(),
        "'core.alternateRefsCommand=true' 'alias.x=status'".into(),
    );
    let (code_params, _) = run_in(
        &argv(&["git", "--", "config", "--local", "--list"]),
        &params_env,
        &gs.work,
        false,
    );
    assert_eq!(code_params, ExitCode::from(126));

    // Uncontained read in `/tmp` without explicit `-C` / `GIT_DIR` escapes stays refused quietly.
    let tmp_dir = gs.scratch.files.join("unrelated-tmp");
    fs::create_dir_all(&tmp_dir).unwrap();
    git(&tmp_dir, &["init", "-b", "main"]);
    let (code_tmp, _) = run_in(
        &argv(&["git", "--", "--no-pager", "log", "-1", "--oneline"]),
        &actor_env,
        &tmp_dir,
        false,
    );
    assert_eq!(code_tmp, ExitCode::from(126));

    assert!(
        refused_events(&gs.sdir).is_empty(),
        "unexpected role.refused: {:?}",
        refused_events(&gs.sdir)
    );
    assert!(
        outward_attention_events(&gs.sdir).is_empty(),
        "unexpected outward action: {:?}",
        outward_attention_events(&gs.sdir)
    );
    assert_eq!(crate::guard_probes::read_suppressed_count(&gs.sdir), 4);
    let mut logged = std::collections::BTreeMap::new();
    let line = crate::daemon::Stage::flush_suppressed_in(&mut logged, "acme-shop", &gs.sdir);
    assert_eq!(
        line.as_deref(),
        Some("acme-shop: suppressed 4 read-only probe refusal(s) (total 4)")
    );
    assert_eq!(
        crate::daemon::Stage::flush_suppressed_in(&mut logged, "acme-shop", &gs.sdir),
        None
    );
}

#[test]
fn real_writes_pushes_merges_and_outside_reaches_still_refuse_and_record() {
    let gs = setup_git_studio(CAST_TOML);
    let integration = add_integration_worktree(&gs);
    let outside = gs.scratch.files.join("outside-repo");
    fs::create_dir_all(&outside).unwrap();
    git(&outside, &["init", "-b", "main"]);

    let (actor_env, _) = env_for(&gs, "actor-t1-c1", Some("t1"));
    let (director_env, _) = env_for(&gs, "director", None);

    // 1. Force push.
    assert_eq!(
        run_in(
            &argv(&["git", "--", "push", "--force", "origin", "feat/login"]),
            &actor_env,
            &gs.work,
            false
        )
        .0,
        ExitCode::from(126)
    );
    // 2. Protected push (Card).
    assert_eq!(
        run_in(
            &argv(&["git", "--", "push", "origin", "main"]),
            &actor_env,
            &gs.work,
            false
        )
        .0,
        ExitCode::from(126)
    );
    // 3. Explicit outside-reach git (`-C /outside` and `GIT_DIR=/outside/.git`).
    assert_eq!(
        run_in(
            &argv(&["git", "--", "-C", outside.to_str().unwrap(), "status"]),
            &actor_env,
            &gs.work,
            false
        )
        .0,
        ExitCode::from(126)
    );
    let mut esc_env = Env {
        vars: actor_env.vars.clone(),
    };
    esc_env.vars.insert(
        "GIT_DIR".into(),
        outside.join(".git").to_string_lossy().into_owned(),
    );
    assert_eq!(
        run_in(
            &argv(&["git", "--", "rev-parse", "--show-toplevel"]),
            &esc_env,
            &gs.work,
            false
        )
        .0,
        ExitCode::from(126)
    );
    // 4. Write into another studio worktree (`director` committing into `../t1`).
    assert_eq!(
        run_in(
            &argv(&["git", "--", "-C", "../t1", "commit", "-m", "hijack"]),
            &director_env,
            &integration,
            false
        )
        .0,
        ExitCode::from(126)
    );
    // 5. Hook override on push (`git -c core.hooksPath=/dev/null push ...`).
    assert_eq!(
        run_in(
            &argv(&[
                "git",
                "--",
                "-c",
                "core.hooksPath=/dev/null",
                "push",
                "origin",
                "feat/login"
            ]),
            &actor_env,
            &gs.work,
            false
        )
        .0,
        ExitCode::from(126)
    );
    // 6. `gh` writes (merge, comment, repo create, workflow run, POST api) + `ssh`.
    for (bin, args) in [
        ("gh", &["pr", "merge", "42"][..]),
        ("gh", &["pr", "comment", "1", "--body", "hi"][..]),
        ("gh", &["repo", "create", "acme-new", "--public"][..]),
        ("gh", &["issue", "comment", "1", "--body", "hi"][..]),
        ("gh", &["workflow", "run", "deploy.yml"][..]),
        ("gh", &["api", "-X", "POST", "repos/a/b/issues"][..]),
        ("ssh", &["host-a", "hostname"][..]),
    ] {
        let mut full = vec![bin.into(), "--".into()];
        full.extend(argv(args));
        assert_eq!(
            run_in(&full, &actor_env, &gs.work, false).0,
            ExitCode::from(126),
            "expected refuse for {bin} {args:?}"
        );
    }

    let events = refused_events(&gs.sdir);
    assert_eq!(
        events.len(),
        13,
        "all real refusals/cards recorded: {events:?}"
    );
    let cards = events
        .iter()
        .filter(|e| {
            matches!(
                e,
                EventKind::RoleRefused {
                    disposition: RefusalDisposition::Card,
                    ..
                }
            )
        })
        .count();
    assert_eq!(cards, 1);
    assert!(
        outward_attention_events(&gs.sdir).len() >= 11,
        "outward action attention raised for real refusals"
    );
}
