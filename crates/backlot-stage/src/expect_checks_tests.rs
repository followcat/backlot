//! Fail-first tests for `expect` / `min_matches` / `expect_failed`
//! and preflight error/tail/timeout behavior in `backlot-stage`.
//! Neutral fixtures only (`acme-shop`, `actor-a`, `/home/user/acme-shop`).

use std::collections::BTreeMap;
use std::fs;
use std::time::Duration;

use crate::checkrun;
use crate::checks;
use crate::checkwork;
use crate::cli::{Env, Fail, run};
use crate::cli_testkit::*;
use crate::store;

fn setup_studio_with_checks(check_toml: &str) -> (Scratch, Env, std::path::PathBuf) {
    let (scratch, env) = scratch();
    let repo = scratch.files.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("f.txt"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);

    let cast = fixture(
        &scratch,
        "cast.toml",
        &format!("design_gate = \"off\"\n{CAST_TOML}\n\n{check_toml}\n"),
    );
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast}",
            repo.display()
        ),
    );
    (scratch, env, repo)
}

fn deliver_one(scratch: &Scratch, env: &Env, repo: &std::path::Path) {
    let req = fixture(scratch, "req.md", "# req\nscan\n");
    let acc = fixture(scratch, "acc.md", "# acc\nmust scan >= 1 file\n");
    ok(
        env,
        &format!("req revise --file {req} --summary first --as producer"),
    );
    ok(
        env,
        &format!("task new --title scan --kind feature --acceptance-file {acc} --as director"),
    );
    ok(
        env,
        "cast --task t1 --actor actor-a --reason fits --as director",
    );
    let work = store::studio_dir(&scratch.dir, "acme-shop").join("work/t1");
    fs::create_dir_all(work.parent().unwrap()).unwrap();
    git(
        repo,
        &[
            "worktree",
            "add",
            "-b",
            "backlot/acme-shop/t1",
            work.to_str().unwrap(),
            "HEAD",
        ],
    );
    let a = commit_in(&work, "one");
    let b = commit_in(&work, "two");
    let rep = fixture(scratch, "d1.md", "done\n");
    ok(
        env,
        &format!("deliver --commits {a}..{b} --report {rep} --as actor-t1-c1 --task t1"),
    );
}

fn run_owed_checks_once(sdir: &std::path::Path, env: &Env) {
    let mut stage_vars = env.vars.clone();
    stage_vars
        .entry("PATH".into())
        .or_insert_with(|| "/usr/bin:/bin".into());
    let state = store::load_state(sdir).unwrap();
    let owed = checks::owed(&state).expect("expected an owed check");
    let mut git_fn = crate::workareas::real_git;
    let spawn_fn = &checkrun::real_spawn;
    let mut running = checkwork::start(
        &mut git_fn,
        spawn_fn,
        &state,
        sdir,
        &stage_vars,
        &owed,
        1000,
    )
    .expect("start owed check");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let out = loop {
        if let Some(outcome) = running.poll(2000) {
            break outcome;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "check timed out in test"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    checkwork::finish(&mut git_fn, sdir, &state, running, out).expect("finish check");
}

#[test]
fn expect_false_green_with_exit_zero_fails_check_and_blocks_accepted_review() {
    let (scratch, env, repo) = setup_studio_with_checks(
        r#"[[checks]]
name = "scan"
run = "echo files_scanned=0"
expect = "files_scanned=[1-9]"
"#,
    );
    deliver_one(&scratch, &env, &repo);
    let sdir = store::studio_dir(&scratch.dir, "acme-shop");
    run_owed_checks_once(&sdir, &env);

    let state = store::load_state(&sdir).unwrap();
    let t1 = state.tasks.get("t1").unwrap();
    let rec = t1.checked_of(1).next().expect("checked record");
    assert_eq!(
        rec.exit_code,
        Some(0),
        "must preserve real exit_code=0, never fabricate non-zero"
    );
    assert!(
        !rec.passed(),
        "exit 0 with missed expect must fail check (no false green)"
    );

    let refuse = refused(
        &env,
        "review --task t1 --delivery 1 --verdict accepted --conclusion green --as director",
    );
    assert!(
        refuse.contains("scan"),
        "review refusal must name failing check: {refuse}"
    );
}

#[test]
fn bad_expect_regex_and_zero_min_matches_are_usage_errors() {
    let (scratch, env) = scratch();
    let bad_re = fixture(
        &scratch,
        "bad-re.toml",
        &format!("{CAST_TOML}\n\n[[checks]]\nname = \"scan\"\nrun = \"true\"\nexpect = \"[\"\n"),
    );
    let err = run(
        &[
            "studio".into(),
            "new".into(),
            "--title".into(),
            "Shop".into(),
            "--repo".into(),
            "/home/user/acme-shop".into(),
            "--cast".into(),
            bad_re,
        ],
        &env,
    )
    .expect_err("invalid expect regex must fail at studio new");
    assert!(matches!(err, Fail::Usage(_)), "got {err:?}");

    let (scratch2, env2) = crate::cli_testkit::scratch();
    let bad_min = fixture(
        &scratch2,
        "bad-min.toml",
        &format!(
            "{CAST_TOML}\n\n[[checks]]\nname = \"scan\"\nrun = \"true\"\nexpect = \"ok\"\nmin_matches = 0\n"
        ),
    );
    let err2 = run(
        &[
            "studio".into(),
            "new".into(),
            "--title".into(),
            "Shop".into(),
            "--repo".into(),
            "/home/user/acme-shop".into(),
            "--cast".into(),
            bad_min,
        ],
        &env2,
    )
    .expect_err("min_matches = 0 must fail at studio new");
    assert!(matches!(err2, Fail::Usage(_)), "got {err2:?}");
}

#[test]
fn multi_match_expect_requires_min_matches() {
    let (scratch, mut env, repo) = setup_studio_with_checks(
        r#"[[checks]]
name = "multi"
run = "printf 'ok\n'"
expect = "^ok$"
min_matches = 2
"#,
    );
    env.vars = BTreeMap::from([
        (
            "BACKLOT_STATE_DIR".into(),
            scratch.dir.to_string_lossy().into_owned(),
        ),
        ("BACKLOT_STUDIO".into(), "acme-shop".into()),
        ("PATH".into(), "/usr/bin:/bin".into()),
    ]);
    deliver_one(&scratch, &env, &repo);
    let sdir = store::studio_dir(&scratch.dir, "acme-shop");
    run_owed_checks_once(&sdir, &env);

    let state = store::load_state(&sdir).unwrap();
    let rec = state
        .tasks
        .get("t1")
        .unwrap()
        .checked_of(1)
        .next()
        .expect("checked record");
    assert_eq!(rec.exit_code, Some(0));
    assert!(
        !rec.passed(),
        "only 1 match when min_matches=2 must fail check"
    );
}

#[test]
fn preflight_env_strips_secrets_and_applies_director_guard_before_running() {
    let dir = crate::test_support::TestDir::new("preflight-env");
    let role_dir = dir.join("roles/director");
    let exe_dir = dir.join("exe");
    fs::create_dir_all(&exe_dir).unwrap();
    fs::write(exe_dir.join("backlot"), "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(exe_dir.join("backlot"), fs::Permissions::from_mode(0o755)).unwrap();
    }
    crate::guard::prepare_role(&role_dir, exe_dir.to_str().unwrap()).unwrap();

    let stage_env = BTreeMap::from([
        ("BACKLOT_STUDIO".into(), "acme-shop".into()),
        ("BACKLOT_ROLE".into(), "stage".into()),
        ("HC_SESSION".into(), "sec-session".into()),
        ("GITHUB_TOKEN".into(), "ghp_secret".into()),
        ("CARGO_HOME".into(), "/opt/cargo".into()),
        (
            "PATH".into(),
            format!(
                "{}/roles/actor-t1-c1/bin:{}:/usr/bin:/bin",
                dir.display(),
                exe_dir.display()
            ),
        ),
    ]);
    let env_list = crate::checks_preflight::build_director_preflight_env(
        &dir,
        &stage_env,
        &["CARGO_HOME".into()],
    )
    .expect("build preflight env");
    let map: BTreeMap<String, String> = env_list.into_iter().collect();
    assert!(!map.contains_key("BACKLOT_STUDIO"));
    assert!(!map.contains_key("BACKLOT_ROLE"));
    assert!(!map.contains_key("HC_SESSION"));
    assert!(!map.contains_key("GITHUB_TOKEN"));
    assert_eq!(
        map.get("CARGO_HOME").map(String::as_str),
        Some("/opt/cargo")
    );
    let path = map.get("PATH").expect("PATH present");
    assert!(
        path.starts_with(&format!("{}:", role_dir.join("bin").display())),
        "director guard bin must lead PATH: {path}"
    );
    assert_eq!(map.get("GIT_CONFIG_COUNT").map(String::as_str), Some("1"));
    assert_eq!(
        map.get("GIT_CONFIG_KEY_0").map(String::as_str),
        Some("core.hooksPath")
    );
    assert_eq!(
        map.get("GIT_CONFIG_VALUE_0").map(String::as_str),
        Some(role_dir.join("git-hooks").to_str().unwrap())
    );
}

#[test]
fn design_write_preflight_refuses_nonzero_expect_miss_timeout_and_bad_slot_without_events() {
    let (scratch, env) = scratch();
    let repo = scratch.files.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("f.txt"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);

    let cast = fixture(
        &scratch,
        "cast.toml",
        &format!(
            "design_gate = \"required\"\n{CAST_TOML}\n\n[[checks]]\nname = \"cast_unit\"\nrun = \"true\"\n"
        ),
    );
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast}",
            repo.display()
        ),
    );
    let sdir = store::studio_dir(&scratch.dir, "acme-shop");
    let work = sdir.join("work/integration");
    fs::create_dir_all(work.parent().unwrap()).unwrap();
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "backlot/acme-shop/integration",
            work.to_str().unwrap(),
            "HEAD",
        ],
    );
    let req = fixture(&scratch, "req.md", "# Req\n");
    ok(
        &env,
        &format!("req revise --file {req} --summary v1 --as producer"),
    );
    ok(&env, "design open --as producer");
    let w1 = fixture(&scratch, "w1.md", "Proposal.\n");
    ok(
        &env,
        &format!("design write --slot proposal --file {w1} --as writer"),
    );
    let before_count = store::load_events(&sdir).unwrap().0.len();

    let mut stage_env = Env {
        vars: env.vars.clone(),
    };
    stage_env.vars.insert("PATH".into(), "/usr/bin:/bin".into());

    // 1. Non-zero exit returns output tail and appends nothing.
    let d1 = fixture(&scratch, "d1.md", "Buildability.\n");
    let fail_toml = fixture(
        &scratch,
        "fail.toml",
        "[[checks]]\nname = \"broken\"\nrun = \"echo oops_line_tail; exit 7\"\n",
    );
    let err = run(
        &[
            "design".into(),
            "write".into(),
            "--slot".into(),
            "buildability".into(),
            "--file".into(),
            d1.clone(),
            "--checks-file".into(),
            fail_toml,
            "--as".into(),
            "director".into(),
        ],
        &stage_env,
    )
    .unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("oops_line_tail") && msg.contains("exited 7"),
        "{msg}"
    );
    assert_eq!(store::load_events(&sdir).unwrap().0.len(), before_count);

    // 2. Missed expect with exit 0 refuses without appending.
    let miss_toml = fixture(
        &scratch,
        "miss.toml",
        "[[checks]]\nname = \"scan\"\nrun = \"echo files_scanned=0\"\nexpect = \"files_scanned=[1-9]\"\n",
    );
    let err = run(
        &[
            "design".into(),
            "write".into(),
            "--slot".into(),
            "buildability".into(),
            "--file".into(),
            d1.clone(),
            "--checks-file".into(),
            miss_toml,
            "--as".into(),
            "director".into(),
        ],
        &stage_env,
    )
    .unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("files_scanned=0") && msg.contains("matched 0 (min 1)"),
        "{msg}"
    );
    assert_eq!(store::load_events(&sdir).unwrap().0.len(), before_count);

    // 3. Collision with `cast_unit` refuses.
    let collide_toml = fixture(
        &scratch,
        "collide.toml",
        "[[checks]]\nname = \"cast_unit\"\nrun = \"true\"\n",
    );
    let err = run(
        &[
            "design".into(),
            "write".into(),
            "--slot".into(),
            "buildability".into(),
            "--file".into(),
            d1.clone(),
            "--checks-file".into(),
            collide_toml,
            "--as".into(),
            "director".into(),
        ],
        &stage_env,
    )
    .unwrap_err();
    assert!(format!("{err:?}").contains("cast_unit"), "{err:?}");

    // 4. Short timeout refuses cleanly.
    let timeout_toml = fixture(
        &scratch,
        "timeout.toml",
        "[[checks]]\nname = \"slow\"\nrun = \"sleep 3\"\ntimeout_s = 1\n",
    );
    let err = run(
        &[
            "design".into(),
            "write".into(),
            "--slot".into(),
            "buildability".into(),
            "--file".into(),
            d1,
            "--checks-file".into(),
            timeout_toml,
            "--as".into(),
            "director".into(),
        ],
        &stage_env,
    )
    .unwrap_err();
    assert!(format!("{err:?}").contains("timed out after 1s"), "{err:?}");
    assert_eq!(store::load_events(&sdir).unwrap().0.len(), before_count);
}

#[test]
fn studio_cast_show_set_preserves_design_and_user_check_origins() {
    use backlot_model::event::CheckOrigin;

    let (scratch, env) = scratch();
    let repo = scratch.files.join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("f"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);

    let real_cast = crate::studio_cast_tests::REAL_CAST;
    let cast = fixture(
        &scratch,
        "cast.toml",
        &format!(
            "design_gate = \"required\"\n{real_cast}\n\n[[checks]]\nname = \"cast_unit\"\nrun = \"true\"\nexpect = \"^$\"\n"
        ),
    );
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast}",
            repo.display()
        ),
    );
    let sdir = store::studio_dir(&scratch.dir, "acme-shop");
    let work = sdir.join("work/integration");
    fs::create_dir_all(work.parent().unwrap()).unwrap();
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "backlot/acme-shop/integration",
            work.to_str().unwrap(),
            "HEAD",
        ],
    );

    ok(
        &env,
        "studio check add --name user_check --run true --expect ^$",
    );
    let req = fixture(&scratch, "req.md", "# v1\n");
    ok(
        &env,
        &format!("req revise --file {req} --summary v1 --as producer"),
    );
    ok(&env, "design open --as producer");
    let w1 = fixture(&scratch, "w1.md", "Proposal.\n");
    ok(
        &env,
        &format!("design write --slot proposal --file {w1} --as writer"),
    );
    let d1 = fixture(&scratch, "d1.md", "Build.\n");
    let cf = fixture(
        &scratch,
        "c1.toml",
        "[[checks]]\nname = \"prop_check\"\nrun = \"echo prop=1\"\nexpect = \"prop=1\"\nmin_matches = 1\n",
    );
    let mut dir_env = Env {
        vars: env.vars.clone(),
    };
    dir_env.vars.insert("PATH".into(), "/usr/bin:/bin".into());
    run(
        &[
            "design".into(),
            "write".into(),
            "--slot".into(),
            "buildability".into(),
            "--file".into(),
            d1,
            "--checks-file".into(),
            cf,
            "--as".into(),
            "director".into(),
        ],
        &dir_env,
    )
    .unwrap();
    let s1 = fixture(&scratch, "s1.md", "Summary.\n");
    ok(
        &env,
        &format!("design write --slot summary --file {s1} --as producer"),
    );
    ok(&env, "design sign");

    // `studio cast show` -> `studio cast set` without changes still says "nothing changes".
    let shown = ok(&env, "studio cast show");
    assert!(shown.contains("expect = \"prop=1\""), "{shown}");
    let shown_path = fixture(&scratch, "shown.toml", &shown);
    let msg = refused(&env, &format!("studio cast set --file {shown_path}"));
    assert!(msg.contains("nothing changes"), "{msg}");

    // Bumping `rework_cap = 3` keeps User & Design check origins intact.
    let edited = shown.replacen("rework_cap = 2", "rework_cap = 3", 1);
    let edited_path = fixture(&scratch, "edited.toml", &edited);
    ok(&env, &format!("studio cast set --file {edited_path}"));
    let state = store::load_state(&sdir).unwrap();
    let checks = &state.studio.unwrap().checks;
    assert_eq!(checks[0].origin, CheckOrigin::Cast);
    assert_eq!(checks[1].origin, CheckOrigin::User);
    assert_eq!(checks[2].origin, CheckOrigin::Design);
}
