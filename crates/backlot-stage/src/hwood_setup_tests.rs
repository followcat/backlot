//! Self-setup (`CardAction::Setup`) tests across CLI, config write,
//! staleness, and `hplan`.

use std::fs;
use std::path::PathBuf;

use backlot_model::hollywood::{HOLLYWOOD_TERMINAL, HollywoodKind, RoleMotion, apply};

use crate::cli::Env;
use crate::cli_testkit::*;
use crate::config::{HollywoodCfg, config_path};
use crate::hstage::{HMove, hplan, identity};
use crate::hwood_tests::{argv, argv_ok};

fn cfg_env(env: &Env) -> Env {
    let mut vars = env.vars.clone();
    vars.insert(
        "XDG_CONFIG_HOME".into(),
        format!("{}/config-home", env.get("BACKLOT_STATE_DIR").unwrap()),
    );
    Env { vars }
}

fn cfg_path(env: &Env) -> PathBuf {
    config_path(&env.vars)
}

fn refused(env: &Env, parts: &[&str]) -> String {
    match argv(env, parts) {
        Err(crate::cli::Fail::Refused(m)) => m,
        other => panic!("expected refusal on {parts:?}, got {other:?}"),
    }
}

fn read_start(path: &std::path::Path) -> Option<String> {
    let text = fs::read_to_string(path).unwrap();
    let val: toml::Value = toml::from_str(&text).unwrap();
    val.get("hollywood")?
        .get("start")?
        .as_str()
        .map(str::to_string)
}

#[test]
fn setup_preserves_start_and_extra_keys_and_lists_agy() {
    let (scratch, env) = scratch();
    let env = cfg_env(&env);
    let path = cfg_path(&env);
    fs::create_dir_all(path.parent().unwrap()).unwrap();

    // Unsupported error includes agy too.
    let mut no_path_vars = env.vars.clone();
    no_path_vars.insert("PATH".into(), scratch.files.to_string_lossy().into_owned());
    let msg = refused(
        &Env { vars: no_path_vars },
        &["hollywood", "setup", "--runner", "runner-x", "--model", "m"],
    );
    assert!(msg.contains("agy"), "{msg}");

    let before = concat!(
        "[panel]\nkey = \"y\"\n\n",
        "[hollywood]\n",
        "start = \"expanded\"\n",
        "runner = \"fake\"\n",
        "model = \"model-a\"\n",
        "effort = \"low\"\n",
        "key = \"alt+h\"\n",
        "default_cast = \"/home/user/cast.toml\"\n",
        "custom_flag = 7\n\n",
        "[hollywood.extra]\nnote = \"keep\"\n",
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
            "model-b",
            "--effort",
            "xhigh",
        ],
    );
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(read_start(&path).as_deref(), Some("expanded"));
    let parsed = crate::config::hollywood(&env.vars).unwrap().unwrap();
    assert_eq!(parsed.runner, "fake");
    assert_eq!(parsed.model, "model-b");
    assert_eq!(parsed.effort.as_deref(), Some("xhigh"));
    assert_eq!(parsed.key.as_deref(), Some("alt+h"));
    assert_eq!(parsed.default_cast.as_deref(), Some("/home/user/cast.toml"));
    assert!(text.contains("custom_flag = 7"), "{text}");
    assert!(
        text.contains("[hollywood.extra]\nnote = \"keep\""),
        "{text}"
    );

    // Switching to cursor (no effort setting) drops `effort` while keeping `start` and other keys.
    let stub = scratch.files.join("cursor-agent");
    fs::write(&stub, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut stub_vars = env.vars.clone();
    stub_vars.insert("PATH".into(), scratch.files.to_string_lossy().into_owned());
    let stub_env = Env { vars: stub_vars };
    argv_ok(
        &stub_env,
        &["hollywood", "setup", "--runner", "cursor", "--model", "c1"],
    );
    assert_eq!(read_start(&path).as_deref(), Some("expanded"));
    let parsed2 = crate::config::hollywood(&stub_env.vars).unwrap().unwrap();
    assert_eq!(parsed2.runner, "cursor");
    assert_eq!(parsed2.model, "c1");
    assert_eq!(parsed2.effort, None);
    assert_eq!(parsed2.key.as_deref(), Some("alt+h"));
}

#[test]
fn propose_setup_validates_and_card_show_lists_both_identities() {
    let (scratch, env) = scratch();
    let env = cfg_env(&env);
    let mut no_path_vars = env.vars.clone();
    no_path_vars.insert("PATH".into(), scratch.files.to_string_lossy().into_owned());
    let no_path = Env { vars: no_path_vars };

    // Unknown runner refused at birth.
    let msg = refused(
        &no_path,
        &[
            "hollywood",
            "propose",
            "setup",
            "--runner",
            "runner-x",
            "--model",
            "m",
            "--why",
            "bad runner",
            "--as",
            "hollywood",
        ],
    );
    assert!(msg.contains("not supported"), "{msg}");

    // Missing effort for claude refused at birth.
    let msg = refused(
        &no_path,
        &[
            "hollywood",
            "propose",
            "setup",
            "--runner",
            "claude",
            "--model",
            "claude-opus-5-5",
            "--why",
            "missing effort",
            "--as",
            "hollywood",
        ],
    );
    assert!(msg.contains("takes an effort"), "{msg}");

    // Bad cline model refused at birth.
    let msg = refused(
        &no_path,
        &[
            "hollywood",
            "propose",
            "setup",
            "--runner",
            "cline",
            "--model",
            "plain-model",
            "--effort",
            "high",
            "--why",
            "bad cline model",
            "--as",
            "hollywood",
        ],
    );
    assert!(msg.contains("cline-pass/"), "{msg}");

    // Initial config present, then valid proposal succeeds.
    let path = cfg_path(&env);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        "[hollywood]\nrunner = \"fake\"\nmodel = \"model-a\"\neffort = \"low\"\nstart = \"collapsed\"\n",
    )
    .unwrap();

    let out = argv_ok(
        &env,
        &[
            "hollywood",
            "propose",
            "setup",
            "--runner",
            "fake",
            "--model",
            "model-b",
            "--effort",
            "xhigh",
            "--why",
            "person asked",
            "--as",
            "hollywood",
        ],
    );
    assert!(out.contains("c1"), "{out}");

    let shown = argv_ok(&env, &["card", "show", "1"]);
    assert!(
        shown.contains("Hollywood: fake / model-a / low → fake / model-b / xhigh"),
        "{shown}"
    );
}

#[test]
fn card_apply_checks_setup_staleness_and_hplan_tracks_full_identity() {
    let (_scratch, env) = scratch();
    let env = cfg_env(&env);
    let path = cfg_path(&env);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        "[hollywood]\nrunner = \"fake\"\nmodel = \"model-a\"\neffort = \"low\"\n",
    )
    .unwrap();

    argv_ok(
        &env,
        &[
            "hollywood",
            "propose",
            "setup",
            "--runner",
            "fake",
            "--model",
            "model-b",
            "--effort",
            "high",
            "--why",
            "switch model",
            "--as",
            "hollywood",
        ],
    );

    // Config changes underneath -> apply records card.stale instead of applied.
    argv_ok(
        &env,
        &[
            "hollywood",
            "setup",
            "--runner",
            "fake",
            "--model",
            "model-c",
            "--effort",
            "low",
        ],
    );
    let res = argv_ok(&env, &["card", "apply", "1", "--as", "user"]);
    assert!(res.contains("stale"), "{res}");

    // Same runner but changed model or effort restarts live Hollywood in hplan.
    let mut cfg = HollywoodCfg {
        runner: "fake".into(),
        model: "model-b".into(),
        effort: Some("high".into()),
        key: None,
        default_cast: None,
    };
    let mut state = backlot_model::hollywood::HollywoodState::default();
    let old_actor = identity(&HollywoodCfg {
        runner: "fake".into(),
        model: "model-a".into(),
        effort: Some("high".into()),
        key: None,
        default_cast: None,
    });
    apply(
        &mut state,
        &backlot_model::testkit::hev(
            1,
            "stage",
            HollywoodKind::RoleState {
                state: RoleMotion::Started,
                terminal: Some(HOLLYWOOD_TERMINAL.into()),
                actor: Some(old_actor),
                session_id: Some("sess-a".into()),
                reason: None,
            },
        ),
    );
    assert_eq!(
        hplan(Some(&cfg), &state.role, true, 200, 0),
        vec![HMove::Close("recast"), HMove::Start]
    );
    // Changing effort on the same runner/model also restarts.
    cfg.model = "model-a".into();
    cfg.effort = Some("low".into());
    assert_eq!(
        hplan(Some(&cfg), &state.role, true, 200, 0),
        vec![HMove::Close("recast"), HMove::Start]
    );
    // Matching identity does not restart.
    cfg.effort = Some("high".into());
    assert_eq!(
        hplan(Some(&cfg), &state.role, true, 200, 0),
        Vec::<HMove>::new()
    );
}
