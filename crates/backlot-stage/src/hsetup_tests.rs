//! `hollywood setup` (R22, D14) and `card show` (D19): the setup
//! command writes `[hollywood]` into the user's config and keeps the
//! rest of the file byte for byte; every bad input is refused before
//! the file is touched. `card show` renders a card's header,
//! proposer and — for a `studio-new` card — the draft's cast and
//! brief.
//!
//! The `fake` runner's install check passes in the workspace (its
//! `tools/fake-runner.sh` ships with the source); every refusal here
//! uses a PATH pointed at an empty scratch dir.

use std::fs;

use crate::cli::Env;
use crate::cli_testkit::*;
use crate::hwood_tests::{argv, argv_ok, running};

fn refused(env: &Env, parts: &[&str]) -> String {
    match argv(env, parts) {
        Err(crate::cli::Fail::Refused(m)) => m,
        other => panic!("expected refusal on {parts:?}, got {other:?}"),
    }
}

fn usage_err(env: &Env, parts: &[&str]) -> String {
    match argv(env, parts) {
        Err(crate::cli::Fail::Usage(m)) => m,
        other => panic!("expected a usage error on {parts:?}, got {other:?}"),
    }
}

/// The scratch's env plus a config root of its own — setup writes
/// under `XDG_CONFIG_HOME`, never the real one.
fn cfg_env(env: &Env) -> Env {
    let mut vars = env.vars.clone();
    vars.insert(
        "XDG_CONFIG_HOME".into(),
        format!("{}/config-home", env.get("BACKLOT_STATE_DIR").unwrap()),
    );
    Env { vars }
}

fn cfg_path(env: &Env) -> std::path::PathBuf {
    crate::config::config_path(&env.vars)
}

#[test]
fn setup_writes_the_table_and_keeps_the_rest_byte_for_byte() {
    let (_scratch, env) = scratch();
    let env = cfg_env(&env);
    // A config that already holds other sections.
    let path = cfg_path(&env);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let before = "# the user's file\n[panel]\nkey = \"y\"\n";
    fs::write(&path, before).unwrap();

    let out = argv_ok(
        &env,
        &[
            "hollywood",
            "setup",
            "--runner",
            "fake",
            "--model",
            "model-h",
            "--effort",
            "high",
            "--key",
            "alt+y",
        ],
    );
    assert!(out.contains("[hollywood] written"), "{out}");

    let text = fs::read_to_string(&path).unwrap();
    // Everything above the new table survived byte for byte.
    assert!(text.starts_with(before), "{text}");
    let cfg = crate::config::hollywood(&env.vars).unwrap().unwrap();
    assert_eq!(cfg.runner, "fake");
    assert_eq!(cfg.model, "model-h");
    assert_eq!(cfg.effort.as_deref(), Some("high"));
    assert_eq!(cfg.key.as_deref(), Some("alt+y"));
}

#[test]
fn setup_replaces_an_existing_table_only() {
    let (_scratch, env) = scratch();
    let env = cfg_env(&env);
    let path = cfg_path(&env);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let before = concat!(
        "[panel]\nkey = \"y\"\n\n",
        "[hollywood]\nrunner = \"fake\"\nmodel = \"old\"\neffort = \"low\"\n\n",
        "[runner_env.devin]\nPROXY = \"http://proxy:8080\"\n",
    );
    fs::write(&path, before).unwrap();

    argv_ok(
        &env,
        &[
            "hollywood",
            "setup",
            "--runner",
            "fake",
            "--model",
            "model-h",
            "--effort",
            "high",
        ],
    );
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("[panel]\nkey = \"y\""), "{text}");
    assert!(text.contains("PROXY = \"http://proxy:8080\""), "{text}");
    assert!(!text.contains("old"), "{text}");
    let cfg = crate::config::hollywood(&env.vars).unwrap().unwrap();
    assert_eq!(cfg.model, "model-h");
}

#[test]
fn setup_refuses_every_bad_input_without_touching_the_file() {
    let (scratch, env) = scratch();
    let env = cfg_env(&env);
    let path = cfg_path(&env);
    // No PATH at all → nothing is installed but fake (whose script
    // ships in tools/, off PATH).
    let mut empty_path = env.vars.clone();
    empty_path.insert("PATH".into(), scratch.files.to_string_lossy().into_owned());
    let no_path = Env { vars: empty_path };

    // An unsupported runner — the dry run's own message.
    let msg = refused(
        &no_path,
        &["hollywood", "setup", "--runner", "runner-x", "--model", "m"],
    );
    assert!(msg.contains("not supported"), "{msg}");
    // A supported runner that is not installed.
    let msg = refused(
        &no_path,
        &[
            "hollywood",
            "setup",
            "--runner",
            "claude",
            "--model",
            "m",
            "--effort",
            "high",
        ],
    );
    assert!(msg.contains("not installed"), "{msg}");
    // The cast file's effort rule holds for the overseer too.
    let msg = refused(
        &env,
        &["hollywood", "setup", "--runner", "fake", "--model", "m"],
    );
    assert!(msg.contains("needs an effort"), "{msg}");
    // `cursor` takes no effort — it needs `cursor-agent` installed
    // first, so the scratch PATH gets a stub that answers the probe.
    let stub = scratch.files.join("cursor-agent");
    fs::write(&stub, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let msg = refused(
        &no_path,
        &[
            "hollywood",
            "setup",
            "--runner",
            "cursor",
            "--model",
            "m",
            "--effort",
            "high",
        ],
    );
    assert!(msg.contains("takes no effort"), "{msg}");
    // A bare `--key` cannot type into programs — it needs a modifier
    // or the `prefix+` form.
    let msg = usage_err(
        &env,
        &[
            "hollywood",
            "setup",
            "--runner",
            "fake",
            "--model",
            "m",
            "--effort",
            "high",
            "--key",
            "y",
        ],
    );
    assert!(msg.contains("bare key"), "{msg}");
    // Missing flag values are usage errors.
    let msg = crate::hwood_tests::argv(&env, &["hollywood", "setup", "--runner", "fake"])
        .expect_err("missing --model must fail");
    assert!(msg.message().contains("--model"), "{msg:?}");

    assert!(!path.exists(), "a refusal still wrote the file");
}

/// `--default-cast` keeps the table's cast template through a
/// reconfigure — the TUI's dialog forwards it verbatim so a
/// runner/model change loses nothing.
#[test]
fn setup_carries_the_default_cast_flag() {
    let (_scratch, env) = scratch();
    let env = cfg_env(&env);
    argv_ok(
        &env,
        &[
            "hollywood",
            "setup",
            "--runner",
            "fake",
            "--model",
            "model-h",
            "--effort",
            "high",
            "--default-cast",
            "/home/user/casts/main.toml",
        ],
    );
    let cfg = crate::config::hollywood(&env.vars).unwrap().unwrap();
    assert_eq!(
        cfg.default_cast.as_deref(),
        Some("/home/user/casts/main.toml")
    );
}

#[test]
fn setup_is_the_users_never_a_role_terminals() {
    let (_scratch, env) = scratch();
    let env = cfg_env(&env);
    let mut vars = env.vars.clone();
    vars.insert("BACKLOT_ROLE".into(), "actor-t1-c1".into());
    let role_env = Env { vars };
    let msg = refused(
        &role_env,
        &[
            "hollywood",
            "setup",
            "--runner",
            "fake",
            "--model",
            "m",
            "--effort",
            "high",
        ],
    );
    assert!(msg.contains("BACKLOT_ROLE"), "{msg}");
}

/// A cast the draft check accepts — every role on `fake`, whose
/// launch file ships with the repo.
const FAKE_CAST: &str = r#"
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

#[test]
fn card_show_renders_a_studio_new_cards_draft() {
    let (scratch, env) = scratch();
    running(&scratch, &env);
    // Hollywood's own shell: no BACKLOT_STUDIO pin — a `studio-new`
    // card names a studio that does not exist yet.
    let env = crate::hwood_tests::hwood_env(&env);
    let cast = fixture(&scratch, "cast-beta.toml", FAKE_CAST);
    let brief = fixture(&scratch, "brief-beta.md", "# the beta brief\nship it\n");
    // `propose` re-validates the draft's repo — a real one.
    let repo = scratch.files.join("repo-beta");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    fs::write(repo.join("f"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-m", "base"]);
    argv_ok(
        &env,
        &[
            "hollywood",
            "propose",
            "studio-new",
            "--studio",
            "beta-tools",
            "--title",
            "Beta tools",
            "--repo",
            repo.to_str().unwrap(),
            "--cast",
            &cast,
            "--brief",
            &brief,
            "--why",
            "a second shop",
            "--as",
            "hollywood",
        ],
    );

    let out = argv_ok(&env, &["card", "show", "c1"]);
    assert!(out.contains("c1 open"), "{out}");
    assert!(out.contains("studio-new beta-tools"), "{out}");
    assert!(out.contains("a second shop"), "{out}");
    assert!(out.contains("proposed by hollywood"), "{out}");
    // The draft's cast — roles and candidates — and its brief.
    assert!(out.contains("producer: fake/model-p/high"), "{out}");
    assert!(out.contains("actor-a (fake/model-a/high)"), "{out}");
    assert!(out.contains("rework_cap 2"), "{out}");
    assert!(out.contains("# the beta brief"), "{out}");
    assert!(out.contains("ship it"), "{out}");
}

#[test]
fn card_show_reads_a_plain_card_and_refuses_an_unknown_id() {
    let (scratch, env) = scratch();
    running(&scratch, &env);
    argv_ok(
        &env,
        &[
            "hollywood",
            "propose",
            "pause",
            "--studio",
            "acme-shop",
            "--task",
            "t1",
            "--why",
            "looping",
            "--as",
            "hollywood",
        ],
    );
    let out = argv_ok(&env, &["card", "show", "1"]);
    assert!(out.contains("c1 open"), "{out}");
    assert!(out.contains("pause t1"), "{out}");
    let msg = refused(&env, &["card", "show", "c9"]);
    assert!(msg.contains("no card 'c9'"), "{msg}");
}
