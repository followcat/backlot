//! The cline adapter's argv and session pieces: the exact argv (fresh
//! and resume, every effort level, the targeted prompt and the
//! never-flags), the provider and effort refusals, and the manifest
//! lookup that finds a fresh launch's id. The prep files have their
//! own file (`cline_prep_tests.rs`). Fixtures are neutral: `alice`,
//! `/home/user`, `acme-shop`.

use std::collections::BTreeMap;

use super::cline::{provider, thinking};
use super::{ActorIdentity, LaunchCtx, PlanError, plan, session_in_argv};
use crate::test_support::TestDir;

pub(super) const MODEL: &str = "cline-pass/deepseek-v4.1-flash-1017";

pub(super) fn ident(model: &str, effort: Option<&str>, profile: &str) -> ActorIdentity {
    ActorIdentity {
        runner: "cline".into(),
        runner_version: None,
        model: model.into(),
        effort: effort.map(str::to_string),
        permission_profile: profile.into(),
    }
}

pub(super) fn ctx<'a>(
    env: &'a BTreeMap<String, String>,
    prompt: Option<&'a str>,
    resume: Option<&'a str>,
) -> LaunchCtx<'a> {
    LaunchCtx {
        terminal: "bl-s-t1",
        cwd: "/home/user/acme-shop",
        prompt,
        resume,
        studio_dir: "/home/user/state/studios/acme-shop",
        stage_env: env,
        role: "producer",
        checks: &[],
        answer_trust: true,
    }
}

pub(super) fn tmp(tag: &str) -> TestDir {
    TestDir::new(&format!("bl-cline-{tag}"))
}

/// A fresh launch: `-i` and `--yolo` always, the provider and the
/// model from the model id, `--thinking` for the effort, and the
/// first prompt last.
#[test]
fn fresh_argv() {
    let env = BTreeMap::new();
    let l = plan(
        &ident(MODEL, Some("medium"), "standard"),
        &ctx(&env, None, None),
    )
    .unwrap();
    assert_eq!(
        l.argv,
        [
            "cline",
            "-i",
            "--yolo",
            "-P",
            "cline-pass",
            "-m",
            MODEL,
            "--thinking",
            "medium"
        ]
    );
    // The first prompt is the positional argument, after the flags.
    let l = plan(
        &ident(MODEL, Some("none"), "standard"),
        &ctx(&env, Some("read the requirement"), None),
    )
    .unwrap();
    assert_eq!(l.argv.last().unwrap(), "read the requirement");
    assert_eq!(l.argv[7], "--thinking");
    assert_eq!(l.argv[8], "none");
    assert_eq!(l.session_id(), None, "cline mints the id");
}

/// Both permission profiles run the same argv — the hook, not a mode
/// flag, is cline's permission layer.
#[test]
fn both_profiles_plan_the_same_argv() {
    let env = BTreeMap::new();
    let a = plan(
        &ident(MODEL, Some("low"), "standard"),
        &ctx(&env, None, None),
    )
    .unwrap();
    let b = plan(
        &ident(MODEL, Some("low"), "worktree"),
        &ctx(&env, None, None),
    )
    .unwrap();
    assert_eq!(a.argv, b.argv);
}

/// A resume names the id right after `--yolo` and never carries the
/// positional prompt — cline ignores it, the stage sends the text
/// once the composer opens.
#[test]
fn resume_argv_has_no_prompt() {
    let env = BTreeMap::new();
    let l = plan(
        &ident(MODEL, Some("high"), "standard"),
        &ctx(&env, Some("go on"), Some("1794894104593_rurer")),
    )
    .unwrap();
    assert_eq!(
        l.argv,
        [
            "cline",
            "-i",
            "--yolo",
            "--id",
            "1794894104593_rurer",
            "-P",
            "cline-pass",
            "-m",
            MODEL,
            "--thinking",
            "high"
        ]
    );
    assert!(!l.argv.iter().any(|a| a == "go on"), "{:?}", l.argv);
}

/// Every effort level the model accepts, and every shape of argv the
/// adapter builds, carries `--yolo`: hub mode gives a role its own
/// environment back only there (S14a section 1).
#[test]
fn every_argv_carries_yolo() {
    let env = BTreeMap::new();
    for effort in backlot_model::caps::CLINE.effort.levels() {
        for profile in ["standard", "worktree", "readonly"] {
            for (prompt, resume) in [
                (None, None),
                (Some("first"), None),
                (Some("more"), Some("id-1")),
                (None, Some("id-1")),
            ] {
                let c = ctx(&env, prompt, resume);
                let l = plan(&ident(MODEL, Some(effort), profile), &c).unwrap();
                assert_eq!(
                    l.argv.iter().filter(|a| *a == "--yolo").count(),
                    1,
                    "{effort}/{profile}/{prompt:?}/{resume:?}: {:?}",
                    l.argv
                );
                assert_eq!(l.argv[1], "-i", "{:?}", l.argv);
            }
        }
    }
}

/// The flags and words the adapter must never emit: an ignored hook
/// directory, a second auto-approve authority, the non-interactive
/// and hub modes, a relocated data dir, global writes, and every
/// subcommand (S14a section 2).
#[test]
fn never_flags() {
    let env = BTreeMap::new();
    for (prompt, resume) in [(None, None), (Some("first"), None), (None, Some("id-1"))] {
        let c = ctx(&env, prompt, resume);
        let l = plan(&ident(MODEL, Some("high"), "standard"), &c).unwrap();
        for flag in [
            "--hooks-dir",
            "--auto-approve",
            "-p",
            "--plan",
            "-z",
            "--zen",
            "--data-dir",
            "--json",
            "--acp",
            "--worktree",
            "--config",
        ] {
            assert!(!l.argv.iter().any(|a| a == flag), "{flag}: {:?}", l.argv);
        }
        for word in [
            "auth", "config", "doctor", "hub", "kanban", "schedule", "history", "update",
        ] {
            assert!(!l.argv.iter().any(|a| a == word), "{word}: {:?}", l.argv);
        }
    }
}

/// `none|low|medium|high|xhigh` map straight onto `--thinking`;
/// `xhigh` is the top level — there is no `max`.
#[test]
fn effort_levels_map() {
    let levels = backlot_model::caps::CLINE.effort.levels();
    assert_eq!(levels, ["none", "low", "medium", "high", "xhigh"]);
    for e in levels {
        assert_eq!(thinking(Some(e)).unwrap(), Some(*e));
    }
    assert_eq!(thinking(None).unwrap(), None, "no declared effort, no flag");
}

/// Anything else is a refusal naming the effort — never a guess.
#[test]
fn an_unknown_effort_is_refused() {
    for bad in ["max", "ultra", "High", "", "medium-high"] {
        let e = thinking(Some(bad)).unwrap_err();
        assert_eq!(e.reason, "effort not supported");
        assert!(e.detail.contains(bad), "{e:?}");
    }
    let env = BTreeMap::new();
    let e = plan(
        &ident(MODEL, Some("max"), "standard"),
        &ctx(&env, None, None),
    )
    .unwrap_err();
    assert_eq!(e.reason, "effort not supported");
}

/// The provider comes from the model id: `cline-pass/…` is the one
/// provider the adapter knows; any other model would need an
/// explicit provider and there is no config channel for one, so it
/// is refused with the model named.
#[test]
fn only_the_cline_pass_provider_is_known() {
    assert_eq!(provider(MODEL), Some("cline-pass"));
    assert_eq!(provider("cline-pass/x"), Some("cline-pass"));
    for bad in ["deepseek-v4.1-flash-1017", "cline-pass/", "other/model"] {
        assert_eq!(provider(bad), None, "{bad}");
    }
    let env = BTreeMap::new();
    let e = plan(
        &ident("deepseek-v4.1-flash-1017", Some("high"), "standard"),
        &ctx(&env, None, None),
    )
    .unwrap_err();
    assert_eq!(e.reason, "model not supported");
    assert!(e.detail.contains("deepseek-v4.1-flash-1017"), "{e:?}");
    assert!(e.detail.contains("cline-pass"), "{e:?}");
}

/// A profile no runner knows is refused before anything else.
#[test]
fn an_unknown_profile_is_refused() {
    let env = BTreeMap::new();
    let e = plan(
        &ident(MODEL, Some("high"), "nowhere"),
        &ctx(&env, None, None),
    )
    .unwrap_err();
    assert_eq!(e.reason, super::PROFILE_NOT_SUPPORTED);
    assert!(e.detail.contains("nowhere"), "{e:?}");
}

/// `--id <id>` reads back a resumed session, the only id a live
/// cline terminal's argv carries.
#[test]
fn argv_reads_back_the_resumed_id() {
    let env = BTreeMap::new();
    let l = plan(
        &ident(MODEL, Some("high"), "standard"),
        &ctx(&env, None, Some("1794894104593_rurer")),
    )
    .unwrap();
    assert_eq!(
        session_in_argv("cline", &l.argv).as_deref(),
        Some("1794894104593_rurer")
    );
    assert_eq!(
        session_in_argv("cline", &["cline".into(), "-i".into()]),
        None
    );
    assert_eq!(
        session_in_argv("cline", &["cline".into(), "--id".into()]),
        None,
        "a flag with no value names nothing"
    );
}

/// A refusal's detail names no path from the machine.
#[test]
fn refusal_details_name_no_private_data() {
    let env = BTreeMap::new();
    let e: PlanError = plan(
        &ident("plain-model", None, "standard"),
        &ctx(&env, None, None),
    )
    .unwrap_err();
    assert!(!e.detail.contains("/home/alice"), "{e:?}");
    assert!(!e.detail.contains("host-"), "{e:?}");
}
