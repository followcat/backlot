//! CLI parsing, role/offline guards, wait/cancel/status, and `setup`
//! live-Hollywood refusal tests for `backlot hollywood switch` (§2.4.3, Q1).

use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;

use backlot_model::EventBy;
use backlot_model::event::ActorIdentity;
use backlot_model::hollywood::{HOLLYWOOD, HollywoodKind, NewHollywoodEvent};
use backlot_model::req_results::{HollywoodSwitchResult, HollywoodSwitchStatusResult};

use crate::args::{self, Cmd, HollywoodCmd};
use crate::cli::{self, Env, Fail};
use crate::cli_testkit::*;
use crate::handoff::{HandoffPhase, load_handoff, save_handoff};
use crate::live::{Live, LiveRole, now_ms, write as write_live};
use crate::store;

fn setup_env_with_cfg() -> (Scratch, std::path::PathBuf, Env) {
    let (scratch, _) = scratch();
    let base = scratch.files.join("state");
    let mut env = Env::default();
    let cfg_home = scratch.files.join("config-home");
    let fake_bin = scratch.files.join("fake-runner");
    fs::create_dir_all(&cfg_home).unwrap();
    fs::write(&fake_bin, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&fake_bin, fs::Permissions::from_mode(0o755));
    }
    env.vars
        .insert("BACKLOT_STATE_DIR".into(), base.to_string_lossy().into());
    env.vars
        .insert("XDG_CONFIG_HOME".into(), cfg_home.to_string_lossy().into());
    env.vars.insert(
        "BACKLOT_FAKE_RUNNER".into(),
        fake_bin.to_string_lossy().into(),
    );
    env.vars
        .insert("PATH".into(), scratch.files.to_string_lossy().into());
    let backlot_cfg = cfg_home.join("backlot/config.toml");
    fs::create_dir_all(backlot_cfg.parent().unwrap()).unwrap();
    fs::write(
        &backlot_cfg,
        "[hollywood]\nrunner = \"fake\"\nmodel = \"model-a\"\neffort = \"low\"\n",
    )
    .unwrap();
    (scratch, base, env)
}

fn seed_live_stage_and_hollywood(base: &Path, live_hollywood: bool) {
    let hdir = store::hollywood_dir(base);
    if live_hollywood {
        store::append_hollywood(&hdir, |_, _| {
            Ok::<_, std::convert::Infallible>(store::HPending {
                event: NewHollywoodEvent::new(
                    EventBy::role("stage"),
                    HollywoodKind::RoleState {
                        state: backlot_model::hollywood::RoleMotion::Started,
                        terminal: Some("bl-hollywood".into()),
                        actor: Some(ActorIdentity {
                            runner: "fake".into(),
                            runner_version: None,
                            model: "model-a".into(),
                            effort: Some("low".into()),
                            permission_profile: "standard".into(),
                        }),
                        session_id: Some("sid-1".into()),
                        reason: Some("boot".into()),
                    },
                ),
                files: Vec::new(),
            })
        })
        .unwrap();
    }
    write_live(
        base,
        &Live {
            updated_ms: now_ms(),
            roles: vec![LiveRole {
                studio: HOLLYWOOD.into(),
                role: HOLLYWOOD.into(),
                work: Some(backlot_model::summary::Work::Waiting),
                doing: None,
                running: None,
                idle_since_ms: None,
            }],
            accounts: Vec::new(),
        },
    )
    .unwrap();
}

fn words(s: &[&str]) -> Vec<String> {
    s.iter().copied().map(String::from).collect()
}

#[test]
fn parse_switch_flags_and_mutual_exclusions() {
    let (_, cmd) = args::parse(&words(&[
        "hollywood",
        "switch",
        "--runner",
        "fake",
        "--model",
        "model-b",
        "--effort",
        "high",
        "--timeout",
        "30",
        "--force",
        "--no-wait",
        "--json",
    ]))
    .unwrap();
    assert!(matches!(
        cmd,
        Cmd::Hollywood(HollywoodCmd::Switch {
            ref runner, ref model, ref effort,
            timeout_s: Some(30), force: true, no_wait: true, json: true,
        }) if runner == "fake" && model == "model-b" && effort.as_deref() == Some("high")
    ));

    let (_, cmd) = args::parse(&words(&["hollywood", "switch", "--cancel", "--json"])).unwrap();
    assert!(matches!(
        cmd,
        Cmd::Hollywood(HollywoodCmd::SwitchCancel { json: true })
    ));

    let (_, cmd) = args::parse(&words(&["hollywood", "switch", "--status", "--json"])).unwrap();
    assert!(matches!(
        cmd,
        Cmd::Hollywood(HollywoodCmd::SwitchStatus { json: true })
    ));

    let (_, cmd) = args::parse(&words(&[
        "hollywood",
        "setup",
        "--runner",
        "fake",
        "--model",
        "model-b",
        "--effort",
        "high",
        "--force-offline",
    ]))
    .unwrap();
    assert!(matches!(
        cmd,
        Cmd::Hollywood(HollywoodCmd::Setup {
            force_offline: true,
            ..
        })
    ));

    for bad in [
        &["hollywood", "switch"][..],
        &["hollywood", "switch", "--runner", "fake"][..],
        &["hollywood", "switch", "--model", "m"][..],
        &["hollywood", "switch", "--cancel", "--status"][..],
        &["hollywood", "switch", "--cancel", "--runner", "fake"][..],
        &[
            "hollywood",
            "switch",
            "--runner",
            "fake",
            "--model",
            "m",
            "--timeout",
            "0",
        ][..],
    ] {
        assert!(
            args::parse(&words(bad)).is_err(),
            "expected usage error for {bad:?}"
        );
    }
}

#[test]
fn switch_and_setup_refused_inside_role_env() {
    let (_s, base, mut env) = setup_env_with_cfg();
    seed_live_stage_and_hollywood(&base, true);
    env.vars.insert("BACKLOT_ROLE".into(), "hollywood".into());

    for argv in [
        &[
            "hollywood",
            "switch",
            "--runner",
            "fake",
            "--model",
            "model-b",
            "--effort",
            "high",
            "--no-wait",
        ][..],
        &["hollywood", "switch", "--cancel"][..],
        &["hollywood", "switch", "--status"][..],
        &[
            "hollywood",
            "setup",
            "--runner",
            "fake",
            "--model",
            "model-b",
            "--effort",
            "high",
            "--force-offline",
        ][..],
    ] {
        let err = cli::run(&words(argv), &env).unwrap_err();
        assert!(
            matches!(err, Fail::Refused(_)),
            "expected Refused for {argv:?}, got {err:?}"
        );
    }
    assert!(
        load_handoff(&store::hollywood_dir(&base))
            .unwrap()
            .is_none()
    );
}

#[test]
fn switch_refuses_offline_stage_and_unchanged_identity() {
    let (_s, base, env) = setup_env_with_cfg();

    let err = cli::run(
        &words(&[
            "hollywood",
            "switch",
            "--runner",
            "fake",
            "--model",
            "model-b",
            "--effort",
            "high",
            "--no-wait",
        ]),
        &env,
    )
    .unwrap_err();
    assert!(
        err.message().contains("stage is not running") && err.message().contains("hollywood setup"),
        "{err:?}"
    );

    seed_live_stage_and_hollywood(&base, true);
    let err = cli::run(
        &words(&[
            "hollywood",
            "switch",
            "--runner",
            "fake",
            "--model",
            "model-a",
            "--effort",
            "low",
            "--no-wait",
        ]),
        &env,
    )
    .unwrap_err();
    assert!(err.message().contains("already"), "{err:?}");

    let ok = cli::run(
        &words(&[
            "hollywood",
            "switch",
            "--runner",
            "fake",
            "--model",
            "model-a",
            "--effort",
            "low",
            "--force",
            "--no-wait",
            "--json",
        ]),
        &env,
    )
    .unwrap();
    let parsed: HollywoodSwitchResult = serde_json::from_str(&ok).unwrap();
    assert_eq!(parsed.action, "queued");
}

#[test]
fn switch_no_wait_status_and_cancel_lifecycle() {
    let (_s, base, env) = setup_env_with_cfg();
    seed_live_stage_and_hollywood(&base, true);

    let out = cli::run(
        &words(&[
            "hollywood",
            "switch",
            "--runner",
            "fake",
            "--model",
            "model-b",
            "--effort",
            "high",
            "--no-wait",
            "--json",
        ]),
        &env,
    )
    .unwrap();
    let queued: HollywoodSwitchResult = serde_json::from_str(&out).unwrap();
    assert_eq!(queued.action, "queued");
    assert_eq!(
        queued.handoff.as_ref().map(|h| h.phase.as_str()),
        Some("preparing")
    );

    let err = cli::run(
        &words(&[
            "hollywood",
            "switch",
            "--runner",
            "fake",
            "--model",
            "model-c",
            "--effort",
            "high",
            "--no-wait",
        ]),
        &env,
    )
    .unwrap_err();
    assert!(err.message().contains("already in progress"), "{err:?}");

    let status_raw =
        cli::run(&words(&["hollywood", "switch", "--status", "--json"]), &env).unwrap();
    let status: HollywoodSwitchStatusResult = serde_json::from_str(&status_raw).unwrap();
    assert!(status.active);
    assert_eq!(
        status.handoff.as_ref().map(|h| h.target_model.as_str()),
        Some("model-b")
    );

    let cancel_raw =
        cli::run(&words(&["hollywood", "switch", "--cancel", "--json"]), &env).unwrap();
    let cancel: HollywoodSwitchResult = serde_json::from_str(&cancel_raw).unwrap();
    assert_eq!(cancel.action, "cancel-requested");
    let rec = load_handoff(&store::hollywood_dir(&base)).unwrap().unwrap();
    assert!(rec.cancel_requested);
}

#[test]
fn switch_wait_commits_or_returns_error_on_rollback() {
    let (_s, base, env) = setup_env_with_cfg();
    seed_live_stage_and_hollywood(&base, true);
    let hdir = store::hollywood_dir(&base);

    let hdir_bg = hdir.clone();
    let handle = thread::spawn(move || {
        loop {
            if let Ok(Some(mut rec)) = load_handoff(&hdir_bg)
                && rec.phase == HandoffPhase::Preparing
            {
                rec.phase = HandoffPhase::Verifying;
                save_handoff(&hdir_bg, &rec).unwrap();
                thread::sleep(Duration::from_millis(40));
                rec.phase = HandoffPhase::Committed;
                rec.finished_ms = Some(now_ms());
                save_handoff(&hdir_bg, &rec).unwrap();
                break;
            }
            thread::sleep(Duration::from_millis(15));
        }
    });
    let out = cli::run(
        &words(&[
            "hollywood",
            "switch",
            "--runner",
            "fake",
            "--model",
            "model-b",
            "--effort",
            "high",
            "--timeout",
            "5",
            "--json",
        ]),
        &env,
    )
    .unwrap();
    handle.join().unwrap();
    let res: HollywoodSwitchResult = serde_json::from_str(&out).unwrap();
    assert_eq!(res.action, "committed");

    let hdir_bg2 = hdir.clone();
    let handle2 = thread::spawn(move || {
        loop {
            if let Ok(Some(mut rec)) = load_handoff(&hdir_bg2)
                && rec.phase == HandoffPhase::Preparing
            {
                rec.phase = HandoffPhase::RolledBack;
                rec.error = Some("candidate fake/model-c hit quota limit".into());
                rec.finished_ms = Some(now_ms());
                save_handoff(&hdir_bg2, &rec).unwrap();
                break;
            }
            thread::sleep(Duration::from_millis(15));
        }
    });
    let err = cli::run(
        &words(&[
            "hollywood",
            "switch",
            "--runner",
            "fake",
            "--model",
            "model-c",
            "--effort",
            "high",
            "--timeout",
            "5",
        ]),
        &env,
    )
    .unwrap_err();
    handle2.join().unwrap();
    assert!(err.message().contains("hit quota limit"), "{err:?}");
}

#[test]
fn setup_refuses_when_hollywood_live_unless_force_offline() {
    let (_s, base, env) = setup_env_with_cfg();
    seed_live_stage_and_hollywood(&base, true);
    let cfg_path = crate::config::config_path(&env.vars);
    let before = fs::read_to_string(&cfg_path).unwrap();

    let err = cli::run(
        &words(&[
            "hollywood",
            "setup",
            "--runner",
            "fake",
            "--model",
            "model-b",
            "--effort",
            "high",
        ]),
        &env,
    )
    .unwrap_err();
    assert!(err.message().contains("hollywood switch"), "{err:?}");
    assert_eq!(fs::read_to_string(&cfg_path).unwrap(), before);

    let ok = cli::run(
        &words(&[
            "hollywood",
            "setup",
            "--runner",
            "fake",
            "--model",
            "model-b",
            "--effort",
            "high",
            "--force-offline",
        ]),
        &env,
    )
    .unwrap();
    assert!(ok.contains("written to"), "{ok}");
    assert!(fs::read_to_string(&cfg_path).unwrap().contains("model-b"));
}
