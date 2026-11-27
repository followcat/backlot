//! Outward-action guard classification, real-binary pass-through,
//! and ledger/attention recording (D3).

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::ExitCode;

use backlot_model::attention::AttentionClass;
use backlot_model::event::{EventKind, OUTWARD_ACTION};
use backlot_model::summary::summary;

use crate::cli::Env;
use crate::cli_testkit::{CAST_TOML, fixture, git, ok, scratch};
use crate::guard::{SHIM_MARKER, apply_env, find_real_binary, prepare_role, run_in};
use crate::guard_rules::refused_command;
use crate::runners::executable;
use crate::store;
use crate::test_support::TestDir;

fn sargs(cmd: &str) -> Vec<String> {
    if cmd.is_empty() {
        return Vec::new();
    }
    cmd.split(' ').map(String::from).collect()
}

fn write_exec(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

#[test]
fn shim_allow_and_refuse_cases() {
    let refuse = [
        ("git-push", "origin main"),
        ("git-push", ""),
        ("gh", "pr merge 12"),
        ("gh", "-R owner/repo pr close 5"),
        ("gh", "--repo=owner/repo release create v1.0"),
        ("gh", "repo create foo"),
        ("gh", "issue create --title bug"),
        ("gh", "gist create foo.txt"),
        ("gh", "secret set KEY"),
        ("gh", "workflow run deploy.yml"),
        ("gh", "api -X POST repos/a/b/issues"),
        ("gh", "api --method=DELETE repos/a/b"),
        ("gh", "api -XPUT repos/a/b"),
        ("gh", "api repos/a/b -f title=hi"),
        ("gh", "api repos/a/b -F count=1"),
        ("gh", "api repos/a/b --field a=b"),
        ("gh", "api repos/a/b --raw-field a=b"),
        ("gh", "api -X GET repos/a/b -f k=v"),
        ("hc", "update"),
        ("hc", "machine add host-b"),
        ("hc", "server stop"),
        ("hc", "--json server stop"),
        ("hc", "send t1 hello"),
        ("hc", "run --bg 'backlot card apply c1'"),
        ("hc", "exec t1 -- ls"),
        ("hc", "keys t1 ctrl-c"),
        ("hc", "connect --session backlot"),
        ("hc", "--on t1 send hi"),
        ("hc", "--session backlot exec t1 -- ls"),
        (
            "socat",
            "- UNIX-CONNECT:/run/user/1000/herdr-core/backlot/backlot.sock",
        ),
        (
            "socat",
            "STDIO UNIX-CONNECT:/home/alice/.local/state/herdr-core/backlot.sock",
        ),
        ("nc", "-U /run/user/1000/herdr-core/backlot/backlot.sock"),
        ("ncat", "-U $HC_SOCKET"),
        ("nc", "-U /tmp/queue-plugin.backlot.req.sock"),
        (
            "systemd-run",
            "--user --quiet sh -c 'backlot card apply c1'",
        ),
        ("tmux", "new -d sh -c 'backlot card apply c1'"),
        ("tmux", "new-session -d -- sh -c 'backlot card apply c1'"),
        ("tmux", "-L backlot-test new -d sleep 60"),
        ("backlot", "server stop"),
        ("backlot", "--session backlot server run"),
        ("backlot", "server adopt --stop-stages"),
        ("scp", "a.txt host-a:/tmp/a.txt"),
        ("scp", "host-a:/tmp/a.txt ."),
        ("rsync", "-avz src/ host-a:/dst/"),
        ("rsync", "user@host-a:/src/ ./dst"),
        ("rsync", "-e ssh src/ [::1]:/dst/"),
        ("ssh", "host-a ls"),
        ("ssh", "-p 2222 alice@host-a uname -a"),
        ("ssh", "-p22 alice@host-a uname -a"),
        ("ssh", "-i /tmp/key host-a id"),
        ("npm", "publish"),
        ("npm", "--access public publish"),
        ("cargo", "publish"),
        ("cargo", "+nightly publish --dry-run"),
    ];
    for (name, args) in refuse {
        assert!(
            refused_command(name, &sargs(args)).is_some(),
            "expected refuse for {name} {args}"
        );
    }

    let allow = [
        ("gh", "pr list"),
        ("gh", "pr view 42"),
        ("gh", "status"),
        ("gh", "auth status"),
        ("gh", "search prs --state open"),
        ("gh", "workflow list"),
        ("gh", "run view 123"),
        ("gh", "api repos/a/b/pulls"),
        ("gh", "api -X GET repos/a/b/pulls"),
        ("gh", "api --method get repos/a/b/pulls"),
        (
            "gh",
            "api --method=GET -H Accept:application/json repos/a/b",
        ),
        ("gh", "--help"),
        ("hc", "status"),
        ("hc", "list"),
        ("hc", "server status"),
        ("hc", "ls"),
        ("hc", "get backlot"),
        ("hc", "read backlot"),
        ("hc", "wait t1"),
        ("hc", "events"),
        ("hc", "templates"),
        ("hc", "schema"),
        ("hc", "file read a.txt"),
        ("socat", "-u /dev/null /tmp/plain.sock"),
        ("nc", "-l 9999"),
        ("socat", "TCP:host-a:80"),
        ("tmux", "ls"),
        ("screen", "-ls"),
        ("backlot", "status"),
        ("backlot", "server status"),
        ("rsync", "-av ./src ./dst"),
        ("rsync", "-av ./a:b /tmp/c:d"),
        ("ssh", "host-a"),
        ("ssh", "-p 22 alice@host-a"),
        ("ssh", "-V"),
        ("npm", "test"),
        ("npm", "run build"),
        ("cargo", "test -- publish"),
        ("cargo", "build --release"),
    ];
    for (name, args) in allow {
        assert_eq!(
            refused_command(name, &sargs(args)),
            None,
            "expected allow for {name} {args}"
        );
    }
}

#[test]
fn real_binary_resolution_skips_shim_dir_and_executes_real_tool() {
    let dir = TestDir::new("backlot-guard-real");
    let shim_dir = dir.join("roles/producer/bin");
    let real_dir = dir.join("usr-bin");
    let mark = dir.join("ran.txt");

    write_exec(
        &shim_dir.join("gh"),
        &format!("#!/bin/sh\n{SHIM_MARKER}\necho BAD_SHIM\nexit 99\n"),
    );
    write_exec(
        &real_dir.join("gh"),
        &format!(
            "#!/bin/sh\nprintf '%s' \"$*\" > '{}'\nexit 7\n",
            mark.display()
        ),
    );

    let path_env = format!("{}:{}", shim_dir.display(), real_dir.display());
    let resolved = find_real_binary("gh", Some(&path_env), Some(&shim_dir)).unwrap();
    assert_eq!(resolved, real_dir.join("gh"));

    let mut vars = BTreeMap::new();
    vars.insert("PATH".into(), path_env);
    let env = Env { vars };
    let args = ["gh", "--", "api", "repos/a/b"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    let (code, err) = run_in(&args, &env, &dir, false);
    assert!(err.is_none(), "unexpected refusal message: {err:?}");
    assert_eq!(code, ExitCode::from(7));
    assert_eq!(fs::read_to_string(&mark).unwrap(), "api repos/a/b");
}

#[test]
fn refusal_records_event_and_raises_outward_action_once_per_episode() {
    let (scratch, mut env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let sdir = store::studio_dir(&scratch.dir, "acme-shop");
    store::append(&sdir, |_, _| {
        Ok::<_, std::convert::Infallible>(store::Pending {
            event: backlot_model::NewEvent::new(
                backlot_model::EventBy::role("stage"),
                EventKind::RoleStarted {
                    role: "actor-t1-c1".into(),
                    terminal: Some("bl-acme-t1".into()),
                    actor: None,
                    session_id: None,
                    reason: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();
    env.vars.insert("BACKLOT_ROLE".into(), "actor-t1-c1".into());
    env.vars.insert("BACKLOT_TASK".into(), "t1".into());
    env.vars.insert("PATH".into(), "/usr/bin:/bin".into());

    let work = Path::new("/home/user/acme-shop");
    let argv: Vec<String> = ["gh", "--", "pr", "merge", "42"]
        .into_iter()
        .map(String::from)
        .collect();

    let (code, msg) = run_in(&argv, &env, work, false);
    assert_eq!(code, ExitCode::from(126));
    assert_eq!(
        msg.as_deref(),
        Some(
            "refused by backlot: gh pr merge 42 is the person's to run; ask with backlot ask (gh write)"
        )
    );

    // Repeat identical refusal inside same attention episode: appends another
    // role.refused, does NOT raise a second attention.
    let (code2, _) = run_in(&argv, &env, work, false);
    assert_eq!(code2, ExitCode::from(126));

    let sdir = store::studio_dir(&scratch.dir, "acme-shop");
    let (events, _) = store::load_events(&sdir).unwrap();
    let refused: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::RoleRefused { .. }))
        .collect();
    assert_eq!(refused.len(), 2);
    assert_eq!(
        refused[0].kind,
        EventKind::RoleRefused {
            role: "actor-t1-c1".into(),
            task: Some("t1".into()),
            command: "gh pr merge 42".into(),
            cwd: "/home/user/acme-shop".into(),
            argv: vec!["gh".into(), "pr".into(), "merge".into(), "42".into()],
            rule: Some("gh write".into()),
            disposition: backlot_model::event::RefusalDisposition::Refused,
        }
    );

    let raised: Vec<_> = events
        .iter()
        .filter(|e| {
            matches!(
                &e.kind,
                EventKind::AttentionRaised { reason, .. } if reason == OUTWARD_ACTION
            )
        })
        .collect();
    assert_eq!(raised.len(), 1);
    assert_eq!(
        raised[0].kind,
        EventKind::AttentionRaised {
            role: "actor-t1-c1".into(),
            reason: OUTWARD_ACTION.into(),
            detail: "gh pr merge 42".into(),
            needs_user: false,
            account: None,
            reset_at_ms: None,
        }
    );

    // Verify summary keeps `outward action` on the role's record as
    // `Info` and does not count it in the person's waiting tally.
    let state = store::load_state(&sdir).unwrap();
    let doc = summary(
        &[(String::from("acme-shop"), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        0,
    );
    assert_eq!(doc.studios[0].waiting, 0);
    let actor_row = doc.studios[0]
        .roles
        .iter()
        .find(|r| r.role == "actor-t1-c1")
        .unwrap();
    assert_eq!(actor_row.attention.as_deref(), Some(OUTWARD_ACTION));
    assert_eq!(actor_row.attention_class, Some(AttentionClass::Info));
    assert!(!actor_row.attention_user);
    assert_eq!(
        actor_row.attention_detail.as_deref(),
        Some("gh pr merge 42")
    );

    // Clear attention via user env, then refuse again: starts a new episode.
    let mut clear_env = Env::default();
    clear_env.vars.insert(
        "BACKLOT_STATE_DIR".into(),
        scratch.dir.to_string_lossy().into_owned(),
    );
    clear_env
        .vars
        .insert("BACKLOT_STUDIO".into(), "acme-shop".into());
    ok(
        &clear_env,
        &format!("attention clear {} --as user", raised[0].seq),
    );

    let (code3, _) = run_in(&argv, &env, work, false);
    assert_eq!(code3, ExitCode::from(126));
    let (events, _) = store::load_events(&sdir).unwrap();
    let raised_after: Vec<_> = events
        .iter()
        .filter(|e| {
            matches!(
                &e.kind,
                EventKind::AttentionRaised { reason, .. } if reason == OUTWARD_ACTION
            )
        })
        .collect();
    assert_eq!(raised_after.len(), 2);

    // Command text longer than 200 chars is capped at 200 chars.
    let long_arg = "x".repeat(250);
    let long_argv = vec!["gh".into(), "--".into(), "pr".into(), long_arg];
    let (code4, _) = run_in(&long_argv, &env, work, false);
    assert_eq!(code4, ExitCode::from(126));
    let (events, _) = store::load_events(&sdir).unwrap();
    let last_refused = events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::RoleRefused { command, .. } => Some(command.clone()),
            _ => None,
        })
        .next_back()
        .unwrap();
    assert!(last_refused.chars().count() <= 200);
    assert_eq!(last_refused.chars().count(), 200);
    let _ = AttentionClass::of(OUTWARD_ACTION, true);

    // Outside a studio ledger (e.g. Hollywood or un-pinned shell), refusal still prints and exits 126.
    let mut h_vars = BTreeMap::new();
    h_vars.insert(
        "BACKLOT_STATE_DIR".into(),
        scratch.dir.to_string_lossy().into_owned(),
    );
    h_vars.insert("BACKLOT_ROLE".into(), "hollywood".into());
    h_vars.insert("PATH".into(), "/usr/bin:/bin".into());
    let (hcode, hmsg) = run_in(
        &["scp".into(), "--".into(), "a".into(), "host-a:/tmp/".into()],
        &Env { vars: h_vars },
        work,
        false,
    );
    assert_eq!(hcode, ExitCode::from(126));
    assert!(
        hmsg.unwrap()
            .contains("refused by backlot: scp a host-a:/tmp/")
    );
}

/// The refusal is informational: `outward action` is `Info`, so the
/// studio's needs-you count stays 0 and the role's row carries it
/// without a "needs you" state. A permission prompt outside the work
/// area stays the user's — the two classes are distinct.
#[test]
fn a_refusal_is_info_and_permission_prompts_stay_the_users() {
    let (scratch, mut env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let sdir = store::studio_dir(&scratch.dir, "acme-shop");
    let started = |role: &str| {
        store::append(&sdir, |_, _| {
            Ok::<_, std::convert::Infallible>(store::Pending {
                event: backlot_model::NewEvent::new(
                    backlot_model::EventBy::role("stage"),
                    EventKind::RoleStarted {
                        role: role.into(),
                        terminal: Some("bl-acme-shop-writer".into()),
                        actor: None,
                        session_id: None,
                        reason: None,
                    },
                ),
                files: Vec::new(),
            })
        })
        .unwrap();
    };
    started("writer");
    env.vars.insert("BACKLOT_ROLE".into(), "writer".into());

    let work = Path::new("/home/user/acme-shop");
    let argv: Vec<String> = ["gh", "--", "pr", "merge", "42"]
        .into_iter()
        .map(String::from)
        .collect();
    let (code, _) = run_in(&argv, &env, work, false);
    assert_eq!(code, ExitCode::from(126));

    // A permission prompt the stage will not answer is the user's;
    // the refusal beside it is not.
    store::append(&sdir, |_, _| {
        Ok::<_, std::convert::Infallible>(store::Pending {
            event: backlot_model::NewEvent::new(
                backlot_model::EventBy::role("stage"),
                EventKind::AttentionRaised {
                    role: "writer".into(),
                    reason: backlot_model::event::PERMISSION_PROMPT.into(),
                    detail: "bash: rm -rf build; needs the user".into(),
                    needs_user: true,
                    account: None,
                    reset_at_ms: None,
                },
            ),
            files: Vec::new(),
        })
    })
    .unwrap();

    let state = store::load_state(&sdir).unwrap();
    // Both raises stand: the refusal is `Info`, the prompt `NeedsYou`.
    assert_eq!(
        AttentionClass::of(OUTWARD_ACTION, false),
        AttentionClass::Info
    );
    assert_eq!(
        AttentionClass::of(backlot_model::event::PERMISSION_PROMPT, true),
        AttentionClass::NeedsYou
    );
    let doc = summary(
        &[(String::from("acme-shop"), state)],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        0,
    );
    // Only the permission prompt counts toward the person's tally.
    assert_eq!(doc.studios[0].waiting, 1);
    let writer = doc.studios[0]
        .roles
        .iter()
        .find(|r| r.role == "writer")
        .unwrap();
    // The row shows the prompt that waits on the person, not the
    // informational refusal beside it.
    assert_eq!(
        writer.attention.as_deref(),
        Some("permission prompt — needs the user")
    );
    assert_eq!(writer.attention_class, Some(AttentionClass::NeedsYou));
    assert!(writer.attention_user);
}

#[test]
fn pre_push_refuses_in_role_env_allows_user_and_passes_through_repo_hooks() {
    let dir = TestDir::new("backlot-guard-git");
    let exe_dir = dir.join("exe");
    let role_dir = dir.join("state/studios/acme/roles/actor-t1-c1");
    let bare = dir.join("bare.git");
    let repo = dir.join("repo");
    let work = dir.join("work-t1");

    // Mock `backlot` exe verifying `guard git-push -- ...` invocation from pre-push hook
    // while passing `guard git -- ...` through to real git so pre-commit and pre-push run.
    write_exec(
        &exe_dir.join("backlot"),
        "#!/bin/sh\nif [ \"$1\" = \"guard\" ] && [ \"$2\" = \"git-push\" ]; then\n  echo \"refused by backlot: git push is the person's to run; ask with backlot ask\" >&2\n  exit 126\nfi\nif [ \"$1\" = \"guard\" ] && [ \"$2\" = \"git\" ]; then\n  shift 3\n  PATH=\"${PATH#*:}\" exec git \"$@\"\nfi\nexit 0\n",
    );
    prepare_role(&role_dir, exe_dir.to_str().unwrap()).unwrap();

    for cmd in [
        "git",
        "gh",
        "hc",
        "scp",
        "rsync",
        "ssh",
        "npm",
        "cargo",
        "socat",
        "nc",
        "ncat",
        "systemd-run",
        "tmux",
    ] {
        assert!(
            executable(&role_dir.join("bin").join(cmd)),
            "missing shim for {cmd}"
        );
    }
    assert!(executable(&role_dir.join("git-hooks/pre-push")));
    assert!(executable(&role_dir.join("git-hooks/pre-commit")));

    fs::create_dir_all(&bare).unwrap();
    git(&bare, &["init", "--bare", "-b", "main"]);
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("f"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);
    git(&repo, &["remote", "add", "origin", bare.to_str().unwrap()]);
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "backlot/acme/t1",
            work.to_str().unwrap(),
        ],
    );

    // 1. Repository's default `.git/hooks/pre-commit` still runs in role env (even from a worktree).
    let default_mark = dir.join("default-pre-commit.txt");
    write_exec(
        &repo.join(".git/hooks/pre-commit"),
        &format!("#!/bin/sh\necho ran > '{}'\n", default_mark.display()),
    );

    let mut role_env = BTreeMap::new();
    apply_env(
        &mut role_env,
        &role_dir,
        exe_dir.to_str().unwrap(),
        &format!("{}:/usr/bin:/bin", exe_dir.display()),
        None,
    );

    fs::write(work.join("f"), "c1\n").unwrap();
    let add = std::process::Command::new("git")
        .current_dir(&work)
        .env_clear()
        .envs(&role_env)
        .args(["add", "."])
        .status()
        .unwrap();
    assert!(add.success());
    let commit = std::process::Command::new("git")
        .current_dir(&work)
        .env_clear()
        .envs(&role_env)
        .args([
            "-c",
            "user.name=alice",
            "-c",
            "user.email=alice@host-a",
            "commit",
            "-m",
            "c1",
        ])
        .output()
        .unwrap();
    assert!(
        commit.status.success(),
        "{}",
        String::from_utf8_lossy(&commit.stderr)
    );
    assert!(
        default_mark.is_file(),
        "default .git/hooks/pre-commit must still pass through"
    );

    // 2. Repository's own `--local core.hooksPath` also passes through.
    let custom_dir = work.join(".custom-hooks");
    let custom_mark = dir.join("custom-pre-commit.txt");
    write_exec(
        &custom_dir.join("pre-commit"),
        &format!("#!/bin/sh\necho custom > '{}'\n", custom_mark.display()),
    );
    git(
        &repo,
        &["config", "--local", "core.hooksPath", ".custom-hooks"],
    );
    fs::write(work.join("f"), "c2\n").unwrap();
    let add2 = std::process::Command::new("git")
        .current_dir(&work)
        .env_clear()
        .envs(&role_env)
        .args(["add", "."])
        .status()
        .unwrap();
    assert!(add2.success());
    let commit2 = std::process::Command::new("git")
        .current_dir(&work)
        .env_clear()
        .envs(&role_env)
        .args([
            "-c",
            "user.name=alice",
            "-c",
            "user.email=alice@host-a",
            "commit",
            "-m",
            "c2",
        ])
        .output()
        .unwrap();
    assert!(
        commit2.status.success(),
        "{}",
        String::from_utf8_lossy(&commit2.stderr)
    );
    assert!(
        custom_mark.is_file(),
        "local core.hooksPath pre-commit must pass through"
    );

    // 3. `git push` inside role env is refused; bare remote stays empty.
    let push_role = std::process::Command::new("git")
        .current_dir(&work)
        .env_clear()
        .envs(&role_env)
        .args(["push", "origin", "backlot/acme/t1"])
        .output()
        .unwrap();
    assert!(!push_role.status.success());
    let push_err = String::from_utf8_lossy(&push_role.stderr);
    assert!(push_err.contains("refused by backlot:"), "{push_err}");
    assert_eq!(git(&bare, &["rev-list", "--all"]), "");

    // 4. User's own `git push` outside role env succeeds against the same bare remote.
    let push_user = std::process::Command::new("git")
        .current_dir(&work)
        .args(["push", "origin", "backlot/acme/t1"])
        .output()
        .unwrap();
    assert!(
        push_user.status.success(),
        "{}",
        String::from_utf8_lossy(&push_user.stderr)
    );
    assert!(!git(&bare, &["rev-list", "--all"]).is_empty());
}

/// The shim layer refuses each role escape with one line naming the
/// command and that it is the person's.
#[test]
fn shim_refuses_role_escapes_in_one_line() {
    let dir = TestDir::new("backlot-guard-escapes");
    let mut vars = BTreeMap::new();
    vars.insert("PATH".into(), "/usr/bin:/bin".into());
    let env = Env { vars };
    for (argv, needle) in [
        (
            vec!["hc", "--", "run", "--bg", "backlot card apply c1"],
            "hc run --bg backlot card apply c1 is the person's to run",
        ),
        (
            vec![
                "hc", "--", "exec", "t1", "--", "backlot", "card", "apply", "c1",
            ],
            "hc exec t1 -- backlot card apply c1 is the person's to run",
        ),
        (
            vec![
                "nc",
                "--",
                "-U",
                "/run/user/1000/herdr-core/backlot/backlot.sock",
            ],
            "nc -U /run/user/1000/herdr-core/backlot/backlot.sock is the person's to run",
        ),
        (
            vec!["socat", "--", "-", "UNIX-CONNECT:/tmp/hc.sock"],
            "socat - UNIX-CONNECT:/tmp/hc.sock is the person's to run",
        ),
        (
            vec!["systemd-run", "--", "--user", "true"],
            "systemd-run --user true is the person's to run",
        ),
        (
            vec!["tmux", "--", "new", "-d", "x"],
            "tmux new -d x is the person's to run",
        ),
        (
            vec!["backlot", "--", "server", "stop"],
            "backlot server stop is the person's to run",
        ),
    ] {
        let args: Vec<String> = argv.iter().map(|s| (*s).to_string()).collect();
        let (code, msg) = run_in(&args, &env, &dir, false);
        assert_eq!(code, ExitCode::from(126), "{argv:?}: {msg:?}");
        let msg = msg.expect("a refusal line");
        assert!(msg.contains(needle), "{argv:?} -> {msg}");
    }
}
