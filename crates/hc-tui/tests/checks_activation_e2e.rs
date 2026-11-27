//! Binary-level end-to-end tests for proposal preflight security (`role.refused`
//! on `git push` and `git -C /outside` under director's guard, stripped `BACKLOT_*`,
//! `<role_dir>/bin` first on `PATH`), transactional activation on `design sign`,
//! `--no-checks` / `--skip`, origin-aware re-signing, and `studio check add|remove`.
//! Neutral fixtures only (`acme-shop`, `actor-a`, `alice`, `host-a`).

mod studio_kit;
mod support;

use std::fs;
use std::path::PathBuf;

use studio_kit::*;

fn init_studio_with_worktree(s: &Scratch, extra_cast: &str) -> (PathBuf, PathBuf, PathBuf) {
    let bare = s.root.join("files/bare.git");
    let repo = s.root.join("files/repo");
    fs::create_dir_all(&bare).unwrap();
    fs::create_dir_all(&repo).unwrap();
    git(&bare, &["init", "--bare", "-b", "main"]);
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("README.md"), "acme shop\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "init"]);
    git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);

    let cast = s.fixture(
        "cast.toml",
        &format!("design_gate = \"required\"\n{CAST_TOML}\n\n{extra_cast}\n"),
    );
    ok(
        s,
        &[
            "studio",
            "new",
            "--title",
            "Acme Shop",
            "--repo",
            repo.to_str().unwrap(),
            "--cast",
            &cast,
        ],
    );
    let sdir = s.state.join("studios/acme-shop");
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
    (bare, repo, work)
}

fn open_to_buildability(s: &Scratch, req_summary: &str, proposal_name: &str) {
    let req = s.fixture(
        &format!("req-{proposal_name}.md"),
        &format!("# {req_summary}\n"),
    );
    ok(
        s,
        &[
            "req",
            "revise",
            "--file",
            &req,
            "--summary",
            req_summary,
            "--as",
            "producer",
        ],
    );
    ok(s, &["design", "open", "--as", "producer"]);
    let p = s.fixture(&format!("prop-{proposal_name}.md"), "Proposal text.\n");
    ok(
        s,
        &[
            "design", "write", "--slot", "proposal", "--file", &p, "--as", "writer",
        ],
    );
}

#[test]
fn preflight_blocks_git_push_and_outside_worktree_under_director_guard_and_strips_env() {
    let s = Scratch::new();
    let (_bare, _repo, _work) = init_studio_with_worktree(&s, "");
    open_to_buildability(&s, "req v1", "v1");

    let d1 = s.fixture("buildability.md", "Buildability notes.\n");

    // 1. `git push origin main` (protected branch) inside proposed check must hit
    // director's guard -> refused + `role.refused` by director.
    let push_toml = s.fixture(
        "checks-push.toml",
        r#"[[checks]]
name = "push_check"
run = "git push origin main"
timeout_s = 30
"#,
    );
    let push_out = run(
        &s,
        &[
            "design",
            "write",
            "--slot",
            "buildability",
            "--file",
            &d1,
            "--checks-file",
            &push_toml,
            "--as",
            "director",
        ],
    );
    assert!(
        !push_out.status.success(),
        "git push preflight must fail: {}",
        stdout(&push_out)
    );
    let events_1 = fs::read_to_string(s.events()).unwrap();
    assert!(
        events_1.contains(r#""kind":"role.refused""#) && events_1.contains(r#""role":"director""#),
        "expected role.refused by director on git push preflight:\n{events_1}"
    );
    assert!(
        !events_1.contains(r#""slot":"buildability""#),
        "failed preflight must append no design.entry"
    );

    // 2. `git -C /tmp status` (outside worktree) must hit director's guard -> refused + `role.refused`.
    let outside_toml = s.fixture(
        "checks-outside.toml",
        r#"[[checks]]
name = "outside_check"
run = "git -C /tmp status"
timeout_s = 30
"#,
    );
    let out_res = run(
        &s,
        &[
            "design",
            "write",
            "--slot",
            "buildability",
            "--file",
            &d1,
            "--checks-file",
            &outside_toml,
            "--as",
            "director",
        ],
    );
    assert!(
        !out_res.status.success(),
        "git -C /outside preflight must fail: {}",
        stdout(&out_res)
    );
    let events_2 = fs::read_to_string(s.events()).unwrap();
    let refusal_count = events_2
        .lines()
        .filter(|l| l.contains(r#""kind":"role.refused""#))
        .count();
    assert!(
        refusal_count >= 2,
        "expected second role.refused for git -C /outside:\n{events_2}"
    );

    // 3. Clean check with dirty outer env verifies BACKLOT_*, HC_*, *_TOKEN, and *_PROXY
    // are all stripped while `<role_dir>/bin` leads PATH.
    let clean_toml = s.fixture(
        "checks-clean.toml",
        r#"[[checks]]
name = "env_probe"
run = "env | grep -E '^(BA''CKLOT_|HC_|.*_TOKEN=|.*_PROXY=)' && exit 2; case \"$PATH\" in */roles/director/bin:*) echo env_ok=1 ;; *) echo bad_path=$PATH; exit 3 ;; esac"
expect = "env_ok=1"
timeout_s = 30
"#,
    );
    let dirty_out = command(
        &s,
        &[
            "design",
            "write",
            "--slot",
            "buildability",
            "--file",
            &d1,
            "--checks-file",
            &clean_toml,
            "--as",
            "director",
        ],
    )
    .env("BACKLOT_ROLE", "director")
    .env("BACKLOT_SECRET", "leak_secret")
    .env("HC_SESSION", "leak_hc_session")
    .env("HC_TOKEN", "leak_hc_token")
    .env("GITHUB_TOKEN", "leak_gh_token")
    .env("HTTP_PROXY", "http://127.0.0.1:9")
    .env("HTTPS_PROXY", "https://127.0.0.1:9")
    .output()
    .unwrap();
    assert!(
        dirty_out.status.success(),
        "clean check failed under dirty outer env: {}",
        stderr(&dirty_out)
    );
    let ok_msg = stdout(&dirty_out);
    assert!(ok_msg.contains("design.entry"), "{ok_msg}");
    assert!(
        s.state
            .join("studios/acme-shop/roles/director/bin/git")
            .is_file(),
        "director git shim must be installed"
    );
}

#[test]
fn preflight_blocks_git_push_origin_head_with_director_role_dir_and_role_refused() {
    let s = Scratch::new();
    let bare = s.root.join("files/bare.git");
    let repo = s.root.join("files/repo");
    fs::create_dir_all(&bare).unwrap();
    fs::create_dir_all(&repo).unwrap();
    git(&bare, &["init", "--bare", "-b", "main"]);
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("README.md"), "acme shop\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "init"]);

    // Studio with no recorded push remotes yet + director role dir + integration worktree.
    let cast = s.fixture(
        "cast.toml",
        &format!("design_gate = \"required\"\n{CAST_TOML}\n"),
    );
    ok(
        &s,
        &[
            "studio",
            "new",
            "--title",
            "Acme Shop",
            "--repo",
            repo.to_str().unwrap(),
            "--cast",
            &cast,
        ],
    );
    let sdir = s.state.join("studios/acme-shop");
    let work = sdir.join("work/integration");
    let role_bin = sdir.join("roles/director/bin");
    fs::create_dir_all(&role_bin).unwrap();
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
    git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);

    open_to_buildability(&s, "req v1", "v1");
    let d1 = s.fixture("buildability.md", "Buildability notes.\n");
    let push_head_toml = s.fixture(
        "checks-push-head.toml",
        r#"[[checks]]
name = "push_head"
run = "git push origin HEAD"
timeout_s = 30
"#,
    );
    let res = run(
        &s,
        &[
            "design",
            "write",
            "--slot",
            "buildability",
            "--file",
            &d1,
            "--checks-file",
            &push_head_toml,
            "--as",
            "director",
        ],
    );
    assert!(
        !res.status.success(),
        "`git push origin HEAD` preflight must be refused: {}",
        stdout(&res)
    );
    let events = fs::read_to_string(s.events()).unwrap();
    assert!(
        events.contains(r#""kind":"role.refused""#) && events.contains(r#""role":"director""#),
        "expected role.refused by director on `git push origin HEAD`:\n{events}"
    );
    assert!(
        !events.contains(r#""slot":"buildability""#),
        "refused `git push origin HEAD` preflight must append no design.entry:\n{events}"
    );

    // Second check `env` proves BACKLOT_*, HC_*, *_TOKEN, and *_PROXY are stripped.
    let env_toml = s.fixture(
        "checks-env.toml",
        r#"[[checks]]
name = "env_dump"
run = "out=$(env); echo \"$out\"; echo \"$out\" | grep -E '^(BA''CKLOT_|HC_|.*_TOKEN=|.*_PROXY=)' && exit 2; exit 0"
timeout_s = 30
"#,
    );
    let env_res = command(
        &s,
        &[
            "design",
            "write",
            "--slot",
            "buildability",
            "--file",
            &d1,
            "--checks-file",
            &env_toml,
            "--as",
            "director",
        ],
    )
    .env("BACKLOT_ROLE", "director")
    .env("BACKLOT_API_KEY", "secret")
    .env("HC_SESSION", "s1")
    .env("HC_TOKEN", "hc_tok")
    .env("MY_TOKEN", "tok")
    .env("ALL_PROXY", "socks5://127.0.0.1:9")
    .output()
    .unwrap();
    assert!(
        env_res.status.success(),
        "env check failed: {}",
        stderr(&env_res)
    );
    assert!(role_bin.join("git").is_file(), "director git shim exists");
}

#[test]
fn design_sign_activation_no_checks_skip_origin_preservation_and_studio_check_cli() {
    let s = Scratch::new();
    let (_bare, _repo, work) = init_studio_with_worktree(
        &s,
        r#"[[checks]]
name = "cast_unit"
run = "true"
timeout_s = 60
"#,
    );
    fs::write(work.join("dummy.txt"), "scanned\n").unwrap();

    // Non-user cannot `studio check add|remove`.
    let non_user = run(
        &s,
        &[
            "studio", "check", "add", "--name", "nope", "--run", "true", "--as", "director",
        ],
    );
    assert!(!non_user.status.success(), "director must not add checks");

    // User adds `user_check` with `--preflight`.
    ok(
        &s,
        &[
            "studio",
            "check",
            "add",
            "--name",
            "user_check",
            "--run",
            "echo user_ok=1",
            "--expect",
            "user_ok=1",
            "--preflight",
        ],
    );

    // Req v1: propose `prop_a` and `prop_b`, sign with `--no-checks`.
    open_to_buildability(&s, "req v1", "v1");
    let d1 = s.fixture("d1.md", "Buildability v1.\n");
    let sum1 = s.fixture("s1.md", "Summary v1.\n");
    let c1 = s.fixture(
        "c1.toml",
        r#"[[checks]]
name = "prop_a"
run = "echo a=1"
expect = "a=[1-9]"
timeout_s = 30

[[checks]]
name = "prop_b"
run = "echo b=1"
timeout_s = 45
"#,
    );
    ok(
        &s,
        &[
            "design",
            "write",
            "--slot",
            "buildability",
            "--file",
            &d1,
            "--checks-file",
            &c1,
            "--as",
            "director",
        ],
    );
    ok(
        &s,
        &[
            "design", "write", "--slot", "summary", "--file", &sum1, "--as", "producer",
        ],
    );
    ok(&s, &["design", "sign", "--no-checks"]);
    let json_after_no_checks = ok(&s, &["studio", "cast", "show", "--json"]);
    assert!(
        json_after_no_checks.contains("cast_unit")
            && json_after_no_checks.contains("user_check")
            && !json_after_no_checks.contains("prop_a"),
        "--no-checks must activate no proposed checks:\n{json_after_no_checks}"
    );

    // Req v2: propose `prop_a` & `prop_b`, sign normally -> both activated.
    open_to_buildability(&s, "req v2", "v2");
    ok(
        &s,
        &[
            "design",
            "write",
            "--slot",
            "buildability",
            "--file",
            &d1,
            "--checks-file",
            &c1,
            "--as",
            "director",
        ],
    );
    ok(
        &s,
        &[
            "design", "write", "--slot", "summary", "--file", &sum1, "--as", "producer",
        ],
    );
    ok(&s, &["design", "sign"]);
    let json_v2 = ok(&s, &["studio", "cast", "show", "--json"]);
    assert!(
        json_v2.contains("cast_unit")
            && json_v2.contains("user_check")
            && json_v2.contains("prop_a")
            && json_v2.contains("prop_b"),
        "expected activated proposed checks:\n{json_v2}"
    );

    // Req v3: propose only `prop_c`, sign -> replaces `prop_a`+`prop_b`, preserves `cast_unit`+`user_check`.
    open_to_buildability(&s, "req v3", "v3");
    let c2 = s.fixture(
        "c2.toml",
        r#"[[checks]]
name = "prop_c"
run = "echo c=1"
expect = "c=[1-9]"
timeout_s = 25
"#,
    );
    ok(
        &s,
        &[
            "design",
            "write",
            "--slot",
            "buildability",
            "--file",
            &d1,
            "--checks-file",
            &c2,
            "--as",
            "director",
        ],
    );
    ok(
        &s,
        &[
            "design", "write", "--slot", "summary", "--file", &sum1, "--as", "producer",
        ],
    );
    ok(&s, &["design", "sign"]);
    let json_v3 = ok(&s, &["studio", "cast", "show", "--json"]);
    assert!(
        json_v3.contains("cast_unit")
            && json_v3.contains("user_check")
            && json_v3.contains("prop_c")
            && !json_v3.contains("prop_a")
            && !json_v3.contains("prop_b"),
        "re-sign must replace only design checks:\n{json_v3}"
    );

    // Req v4: `--suggest-skip` + `design sign --skip` activates nothing and preserves all existing checks.
    let req4 = s.fixture("req4.md", "# tiny\n");
    ok(
        &s,
        &[
            "req",
            "revise",
            "--file",
            &req4,
            "--summary",
            "req v4",
            "--as",
            "producer",
        ],
    );
    ok(
        &s,
        &[
            "design",
            "open",
            "--suggest-skip",
            "--reason",
            "trivial",
            "--as",
            "producer",
        ],
    );
    ok(&s, &["design", "sign", "--skip"]);
    let json_v4 = ok(&s, &["studio", "cast", "show", "--json"]);
    assert!(
        json_v4.contains("cast_unit")
            && json_v4.contains("user_check")
            && json_v4.contains("prop_c"),
        "skip must preserve existing checks:\n{json_v4}"
    );

    // `studio check remove` removes `user_check` only.
    ok(&s, &["studio", "check", "remove", "--name", "user_check"]);
    let json_removed = ok(&s, &["studio", "cast", "show", "--json"]);
    assert!(
        !json_removed.contains("user_check")
            && json_removed.contains("cast_unit")
            && json_removed.contains("prop_c"),
        "studio check remove failed:\n{json_removed}"
    );
}
