//! `studio new --dry-run` end to end (D19): a valid draft passes
//! with nothing written, and each invalid kind refuses with a
//! message that says what to fix — plus the runner probes and the
//! `studio-new` card's own draft gate.

use std::fs;
use std::path::Path;

use crate::cli::Env;
use crate::cli_testkit::*;
use crate::runners;
use crate::store;

/// A cast whose three fixed roles and one candidate all run on
/// `fake` — the only runner whose launch file ships with the repo.
const FAKE_CAST: &str = r#"
mode = "standard"
rework_cap = 2

[producer]
runner = "fake"
model = "model-p"
effort = "high"
permission_profile = "standard"

[director]
runner = "fake"
model = "model-d"
effort = "high"
permission_profile = "standard"

[writer]
runner = "fake"
model = "model-w"
effort = "medium"
permission_profile = "standard"

[[candidates]]
name = "actor-a"
runner = "fake"
model = "model-a"
effort = "high"
permission_profile = "worktree"
"#;

/// The `FAKE_CAST` shape for `cmd` — `effort` is a flag `cmd`
/// does not carry; the roles keep the required `model` field.
const CMD_CAST: &str = r#"
[producer]
runner = "cmd"
model = "model-p"
permission_profile = "standard"

[director]
runner = "cmd"
model = "model-d"
permission_profile = "standard"

[writer]
runner = "cmd"
model = "model-w"
permission_profile = "standard"

[[candidates]]
name = "actor-a"
runner = "cmd"
model = "model-a"
permission_profile = "worktree"
"#;

/// A shell where no `BACKLOT_STUDIO` pins the name — `studio new`
/// and `studio-new` cards name their target with `--studio`.
fn free(env: &Env) -> Env {
    let mut vars = env.vars.clone();
    vars.remove("BACKLOT_STUDIO");
    Env { vars }
}

/// A real git repository under `files/` — the dry run asks git
/// itself, so a scratch path is not enough.
fn repo(scratch: &Scratch) -> String {
    let dir = scratch.files.join("repo");
    fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-b", "main"]);
    dir.to_string_lossy().into_owned()
}

#[test]
fn a_valid_draft_passes_and_writes_nothing() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", FAKE_CAST);
    let repo = repo(&scratch);
    let out = ok(
        &env,
        &format!("studio new --title Shop --repo {repo} --cast {cast} --dry-run"),
    );
    assert!(out.contains("dry run"), "{out}");
    assert!(out.contains("nothing recorded"), "{out}");
    // Nothing recorded, nothing created — not even the directory.
    assert!(!scratch.dir.join("studios").exists());
    assert!(!scratch.dir.join("hollywood").exists());
}

#[test]
fn each_refusal_kind_says_what_to_fix() {
    let (scratch, env) = scratch();
    let env = free(&env);
    let cast = fixture(&scratch, "cast.toml", FAKE_CAST);
    let repo = repo(&scratch);
    let dry = |extra: &str| {
        format!("studio new --title Shop --repo {repo} --cast {cast} --dry-run {extra}")
    };

    // The overseer's own name is reserved (D13).
    let msg = refused(&env, &dry("--studio hollywood"));
    assert!(msg.contains("reserved"), "{msg}");

    // A name that is taken already.
    let cast2 = fixture(&scratch, "cast2.toml", CAST_TOML);
    ok(
        &env,
        &format!("studio new --title S --repo /home/user/acme-shop --cast {cast2} --studio taken"),
    );
    let msg = refused(&env, &dry("--studio taken"));
    assert!(msg.contains("already exists"), "{msg}");

    // The path holds no git repository.
    let plain = fixture(&scratch, "notrepo", "x");
    let msg = refused(
        &env,
        &format!("studio new --title S --repo {plain} --cast {cast} --dry-run --studio fresh-a"),
    );
    assert!(msg.contains("no git repository"), "{msg}");

    // The cast file does not exist — a usage error, not a refusal.
    let msg = usage(
        &env,
        &format!(
            "studio new --title S --repo {repo} --cast {scratch}/none.toml --dry-run --studio fresh-b",
            scratch = scratch.files.display()
        ),
    );
    assert!(msg.contains("--cast"), "{msg}");
}

#[test]
fn unsupported_and_uninstalled_runners_are_named() {
    let (scratch, env) = scratch();
    let repo = repo(&scratch);
    // `runner-x` has no adapter.
    let cast = fixture(
        &scratch,
        "bad-runner.toml",
        &FAKE_CAST.replace("runner = \"fake\"", "runner = \"runner-x\""),
    );
    let msg = refused(
        &env,
        &format!("studio new --title S --repo {repo} --cast {cast} --dry-run"),
    );
    assert!(msg.contains("runner-x"), "{msg}");
    assert!(msg.contains("not supported"), "{msg}");

    // `claude` is a real runner but its program is not on this PATH.
    let mut vars = env.vars.clone();
    vars.insert("PATH".into(), "/nonexistent-dir".into());
    let no_path = Env { vars };
    let cast = fixture(
        &scratch,
        "claude.toml",
        &FAKE_CAST.replace("runner = \"fake\"", "runner = \"claude\""),
    );
    let msg = refused(
        &no_path,
        &format!("studio new --title S --repo {repo} --cast {cast} --dry-run"),
    );
    assert!(msg.contains("claude"), "{msg}");
    assert!(msg.contains("not installed"), "{msg}");
}

#[test]
fn a_cap_that_seats_nobody_is_a_usage_error() {
    let (scratch, env) = scratch();
    let repo = repo(&scratch);
    // All three fixed roles run on fake; a cap of 2 seats the
    // producer and the director but leaves no slot for anyone else.
    let cast = fixture(
        &scratch,
        "capped.toml",
        &format!("{FAKE_CAST}\n[limits]\nfake = 2\n"),
    );
    let msg = usage(
        &env,
        &format!("studio new --title S --repo {repo} --cast {cast} --dry-run"),
    );
    assert!(msg.contains("unseated") || msg.contains("no slot"), "{msg}");
}

#[test]
fn the_probe_knows_supported_and_installed() {
    for runner in [
        "fake", "claude", "codex", "devin", "opencode", "cursor", "cmd",
    ] {
        assert!(runners::supported(runner), "{runner}");
    }
    assert!(!runners::supported("runner-x"));
    assert!(!runners::supported(""));

    // `fake` resolves through the repo's own tools/ script — it is
    // installed wherever the repo is.
    assert!(runners::installed("fake", None));
    assert!(!runners::installed("claude", Some("/nonexistent-dir")));
    assert!(!runners::installed("cmd", None));

    // A program file on the PATH value answers `installed`.
    let (scratch, _env) = scratch();
    let bin = scratch.files.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let prog = bin.join("cursor-agent");
    fs::write(&prog, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&prog, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = bin.to_string_lossy().into_owned();
    assert!(runners::installed("cursor", Some(&path)));
}

/// U5: a `[runner_env.<runner>]` PATH in the state's `config.toml`
/// counts toward installed — it is the PATH the role's terminal
/// would launch with, so the binary the wizard wrote there is
/// found.
#[test]
fn a_configured_runner_path_counts_as_installed() {
    let (scratch, env) = scratch();
    let env = free(&env);
    let repo = repo(&scratch);
    // A fake `claude` in a directory nothing's PATH knows.
    let bin = scratch.files.join("agent-bin");
    fs::create_dir_all(&bin).unwrap();
    let prog = bin.join("claude");
    fs::write(&prog, "#!/bin/sh\necho 2.1.330\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&prog, fs::Permissions::from_mode(0o755)).unwrap();
    }
    // The config reaches it — like the wizard's own write left it.
    fs::write(
        scratch.dir.join("config.toml"),
        format!("[runner_env.claude]\nPATH = \"{}\"\n", bin.display()),
    )
    .unwrap();
    let cast = fixture(
        &scratch,
        "claude.toml",
        &FAKE_CAST.replace("runner = \"fake\"", "runner = \"claude\""),
    );
    let mut vars = env.vars.clone();
    vars.insert("PATH".into(), "/nonexistent-dir".into());
    let env = Env { vars };
    let out = ok(
        &env,
        &format!("studio new --title S --repo {repo} --cast {cast} --dry-run --studio fresh-cfg"),
    );
    assert!(out.contains("dry run"), "{out}");
    // Without the config the same draft still refuses.
    fs::remove_file(scratch.dir.join("config.toml")).unwrap();
    let msg = refused(
        &env,
        &format!("studio new --title S --repo {repo} --cast {cast} --dry-run --studio other"),
    );
    assert!(msg.contains("not installed"), "{msg}");
}

/// The picker's offer is launchable (Ux): `cmd` reachable only
/// through a well-known dir — `runners --json` calls it detected,
/// the picker offers it — so a cast naming it passes `--dry-run`,
/// and the real `studio new` writes the `[runner_env]` PATH the
/// launch needs, exactly like the wizard's own write. The reported
/// defect: the picker offered `cmd`, the dry-run said "not
/// installed".
#[test]
fn a_known_dir_runner_passes_dry_run_and_gets_written() {
    let (scratch, env) = scratch();
    let env = free(&env);
    let repo = repo(&scratch);
    // `cmd` under a fake nvm dir: the well-known-dirs probe reaches
    // it, the inherited PATH does not.
    let home = scratch.files.join("home");
    let bin = home.join(".nvm/versions/node/v24.11.1/bin");
    fs::create_dir_all(&bin).unwrap();
    let cmd = bin.join("cmd");
    fs::write(&cmd, "#!/bin/sh\necho 1.2.3\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&cmd, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut vars = env.vars.clone();
    vars.insert("HOME".into(), home.to_string_lossy().into_owned());
    vars.insert("PATH".into(), "/nonexistent-dir".into());
    let env = Env { vars };
    let cast = fixture(&scratch, "cmd.toml", CMD_CAST);
    // The dry run agrees with the picker's offer — and preps the
    // machine like the real `new` does: the `[runner_env]` PATH the
    // launch needs is already written, never studio state.
    let out = ok(
        &env,
        &format!("studio new --title S --repo {repo} --cast {cast} --dry-run --studio cmd-a"),
    );
    assert!(out.contains("dry run"), "{out}");
    let text = fs::read_to_string(scratch.dir.join("config.toml")).unwrap();
    assert!(text.contains("[runner_env.cmd]"), "{text}");
    assert!(text.contains(&bin.display().to_string()), "{text}");
    assert!(!scratch.dir.join("studios").exists());
    // The real `new` records the studio — the user's own PATH line
    // is never rewritten.
    ok(
        &env,
        &format!("studio new --title S --repo {repo} --cast {cast} --studio cmd-b"),
    );
    let text2 = fs::read_to_string(scratch.dir.join("config.toml")).unwrap();
    assert_eq!(text, text2, "the write is idempotent");
}

/// The launch env wins, both directions: a `[runner_env]` PATH
/// that does not reach the binary refuses — even when the binary
/// sits on the inherited PATH, because the configured PATH is the
/// whole launch PATH — and the refusal names the PATH searched.
#[test]
fn a_configured_path_that_misses_is_the_refusal() {
    let (scratch, env) = scratch();
    let env = free(&env);
    let repo = repo(&scratch);
    // `cmd` on the inherited PATH, but the config pins a PATH that
    // does not reach it — the pin is authoritative.
    let bin = scratch.files.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let cmd = bin.join("cmd");
    fs::write(&cmd, "#!/bin/sh\necho 1.2.3\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&cmd, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let pinned = scratch.files.join("pinned");
    fs::create_dir_all(&pinned).unwrap();
    fs::write(
        scratch.dir.join("config.toml"),
        format!("[runner_env.cmd]\nPATH = \"{}\"\n", pinned.display()),
    )
    .unwrap();
    let mut vars = env.vars.clone();
    vars.insert("PATH".into(), bin.to_string_lossy().into_owned());
    let env = Env { vars };
    let cast = fixture(&scratch, "cmd.toml", CMD_CAST);
    let msg = refused(
        &env,
        &format!("studio new --title S --repo {repo} --cast {cast} --dry-run --studio cmd-c"),
    );
    assert!(msg.contains("runner 'cmd' is not installed"), "{msg}");
    assert!(msg.contains("searched PATH="), "{msg}");
    assert!(
        msg.contains(&pinned.display().to_string()),
        "the refusal names the configured PATH: {msg}"
    );
}

#[test]
fn a_studio_new_card_only_takes_a_passing_draft() {
    let (scratch, env) = scratch();
    let env = free(&env);
    let cast = fixture(&scratch, "cast.toml", FAKE_CAST);
    let brief = fixture(&scratch, "brief.md", "# brief\n");
    let repo = repo(&scratch);

    // A bad draft — the repo is not a git repository — never opens
    // a card.
    let plain = fixture(&scratch, "plain", "x");
    let msg = refused(
        &env,
        &format!(
            "hollywood propose studio-new --studio new-shop --title N \
             --repo {plain} --cast {cast} --brief {brief} --why w --as hollywood"
        ),
    );
    assert!(msg.contains("no git repository"), "{msg}");
    assert!(!scratch.dir.join("hollywood/events.jsonl").exists());

    // A draft that passes every check opens the card with the
    // `studio_absent` base.
    let out = ok(
        &env,
        &format!(
            "hollywood propose studio-new --studio new-shop --title N \
             --repo {repo} --cast {cast} --brief {brief} --why w --as hollywood"
        ),
    );
    assert!(out.contains("card.proposed c1"), "{out}");
    let (events, _) = store::load_hevents(&store::hollywood_dir(&scratch.dir)).unwrap();
    match &events[0].kind {
        backlot_model::hollywood::HollywoodKind::CardProposed { base, .. } => {
            assert_eq!(*base, backlot_model::hollywood::CardBase::StudioAbsent);
        }
        k => panic!("expected card.proposed, got {k:?}"),
    }

    // The card goes stale the moment the name is taken by any path.
    let cast2 = fixture(&scratch, "cast2.toml", CAST_TOML);
    ok(
        &env,
        &format!(
            "studio new --title N --repo /home/user/acme-shop --cast {cast2} --studio new-shop"
        ),
    );
    let out = ok(&env, "card apply c1");
    assert!(out.contains("card.stale c1"), "{out}");
}

/// D4: every path shape passes `--dry-run` — a workspace records its
/// members, a plain directory takes `--init` or `--no-git`, and a
/// plain directory without a choice is refused.
#[test]
fn each_path_shape_passes_or_names_its_choice() {
    let (scratch, env) = scratch();
    let env = free(&env);
    let cast = fixture(&scratch, "cast.toml", FAKE_CAST);

    // A workspace: two sibling repositories under one directory.
    let ws = scratch.files.join("mono");
    for name in ["beta-api", "web"] {
        let m = ws.join(name);
        fs::create_dir_all(&m).unwrap();
        git(&m, &["init", "-b", "main"]);
        fs::write(m.join("f"), "base\n").unwrap();
        git(&m, &["add", "."]);
        git(&m, &["commit", "-m", "base"]);
    }
    let ws = ws.to_string_lossy().into_owned();
    let out = ok(
        &env,
        &format!("studio new --title M --repo {ws} --cast {cast} --dry-run --studio mono-ws"),
    );
    assert!(out.contains("dry run"), "{out}");
    // And the real create records the members — none of them is
    // a synthetic parent repository.
    ok(
        &env,
        &format!("studio new --title M --repo {ws} --cast {cast} --studio mono-ws"),
    );
    let (events, _) = store::load_events(&store::studio_dir(&scratch.dir, "mono-ws")).unwrap();
    match &events[0].kind {
        backlot_model::EventKind::StudioCreated {
            members, no_git, ..
        } => {
            assert_eq!(members.len(), 2, "{members:?}");
            assert!(!no_git);
        }
        k => panic!("expected studio.created, got {k:?}"),
    }
    assert!(!Path::new(&ws).join(".git").exists());

    // A plain directory: refused without a choice, valid with either.
    let plain = scratch.files.join("notes");
    fs::create_dir_all(&plain).unwrap();
    let plain = plain.to_string_lossy().into_owned();
    let msg = refused(
        &env,
        &format!("studio new --title N --repo {plain} --cast {cast} --dry-run --studio plain-a"),
    );
    assert!(msg.contains("--init") && msg.contains("--no-git"), "{msg}");
    let out = ok(
        &env,
        &format!(
            "studio new --title N --repo {plain} --cast {cast} --init --dry-run --studio plain-b"
        ),
    );
    assert!(out.contains("dry run"), "{out}");
    let out = ok(
        &env,
        &format!(
            "studio new --title N --repo {plain} --cast {cast} --no-git --dry-run --studio plain-c"
        ),
    );
    assert!(out.contains("dry run"), "{out}");

    // An empty repository needs no flag at all — the initial commit
    // comes at launch.
    let empty = scratch.files.join("fresh");
    fs::create_dir_all(&empty).unwrap();
    git(&empty, &["init", "-b", "main"]);
    let empty = empty.to_string_lossy().into_owned();
    let out = ok(
        &env,
        &format!("studio new --title E --repo {empty} --cast {cast} --dry-run --studio empty-a"),
    );
    assert!(out.contains("dry run"), "{out}");
}

/// A declared `[[checks]]` cast refuses on a workspace or a no-git
/// binding — the stage's check worktree needs one repository (D10).
#[test]
fn declared_checks_need_a_single_repository() {
    let (scratch, env) = scratch();
    let env = free(&env);
    let cast = fixture(
        &scratch,
        "checks.toml",
        &format!(
            "{FAKE_CAST}\n[[checks]]\nname = \"unit\"\nrun = \"cargo test\"\ntimeout_s = 60\n"
        ),
    );
    let ws = scratch.files.join("mono");
    let m = ws.join("beta-api");
    fs::create_dir_all(&m).unwrap();
    git(&m, &["init", "-b", "main"]);
    fs::write(m.join("f"), "base\n").unwrap();
    git(&m, &["add", "."]);
    git(&m, &["commit", "-m", "base"]);
    let ws = ws.to_string_lossy().into_owned();
    let msg = refused(
        &env,
        &format!("studio new --title M --repo {ws} --cast {cast} --dry-run --studio ws-checks"),
    );
    assert!(msg.contains("checks"), "{msg}");

    let plain = scratch.files.join("notes");
    fs::create_dir_all(&plain).unwrap();
    let plain = plain.to_string_lossy().into_owned();
    let msg = refused(
        &env,
        &format!(
            "studio new --title N --repo {plain} --cast {cast} --no-git --dry-run --studio ng-checks"
        ),
    );
    assert!(msg.contains("checks"), "{msg}");
}
