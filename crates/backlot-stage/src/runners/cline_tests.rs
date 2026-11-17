//! The cline adapter's argv and session pieces: the exact argv (fresh
//! and resume, every effort level, the targeted prompt and the
//! never-flags), the provider and effort refusals, and the manifest
//! lookup that finds a fresh launch's id. The prep files have their
//! own file (`cline_prep_tests.rs`). Fixtures are neutral: `alice`,
//! `/home/user`, `acme-shop`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::cline::{EFFORTS, provider, session_at, session_in_argv, thinking};
use super::{ActorIdentity, LaunchCtx, PlanError, plan};

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
    }
}

fn tmp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bl-cline-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
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
    for effort in EFFORTS {
        for profile in ["standard", "worktree"] {
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
    assert_eq!(EFFORTS, ["none", "low", "medium", "high", "xhigh"]);
    for e in EFFORTS {
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
        session_in_argv(&l.argv).as_deref(),
        Some("1794894104593_rurer")
    );
    assert_eq!(session_in_argv(&["cline".into(), "-i".into()]), None);
    assert_eq!(
        session_in_argv(&["cline".into(), "--id".into()]),
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

fn manifest(root: &Path, id: &str, fields: serde_json::Value) {
    let dir = root.join(id);
    fs::create_dir_all(&dir).unwrap();
    let mut v = serde_json::json!({
        "version": 1,
        "session_id": id,
        "source": "cli",
        "cwd": "/home/user/acme-shop",
        "workspace_root": "/home/user/acme-shop",
        "interactive": true,
        "status": "running",
    });
    if let (Some(base), Some(extra)) = (v.as_object_mut(), fields.as_object()) {
        for (k, val) in extra {
            base.insert(k.clone(), val.clone());
        }
    }
    fs::write(dir.join(format!("{id}.json")), v.to_string()).unwrap();
}

/// The fixture process tree: engine pid → the terminal's pid.
fn tree(pairs: &[(u32, u32)]) -> impl Fn(u32) -> Option<u32> + use<> {
    let pairs: Vec<(u32, u32)> = pairs.to_vec();
    move |pid: u32| {
        pairs
            .iter()
            .find(|(p, _)| *p == pid)
            .map(|(_, parent)| *parent)
    }
}

/// The launch's own manifest is found: the role cwd, its pid under the
/// terminal, at or after the launch. Another folder's session, an
/// older one, a directory with no manifest and another process's
/// session are all skipped (S14a section 3).
#[test]
fn session_at_finds_the_launch_manifest() {
    let dir = tmp("sessions");
    let root = dir.join("sessions");
    fs::create_dir_all(&root).unwrap();
    let owner = 4241;
    let mine = "1794894104593_rurer";
    manifest(&root, mine, serde_json::json!({"pid": 4242}));
    // Another folder's session.
    manifest(
        &root,
        "1794894104594_other",
        serde_json::json!({"pid": 4200, "cwd": "/home/user/other-repo"}),
    );
    // A session from another process tree.
    manifest(
        &root,
        "1794894104595_stray",
        serde_json::json!({"pid": 9000}),
    );
    // A dir with no manifest at all, and one whose manifest is
    // corrupt.
    fs::create_dir_all(root.join("1794894104596_none")).unwrap();
    fs::create_dir_all(root.join("1794894104597_bad")).unwrap();
    fs::write(root.join("1794894104597_bad/bad.json"), "not json").unwrap();
    // A person's own session in the same folder.
    manifest(
        &root,
        "1794894104598_human",
        serde_json::json!({"pid": 9100, "source": "cli"}),
    );
    let taken = BTreeSet::new();
    let found = session_at(
        &root,
        "/home/user/acme-shop",
        1,
        &taken,
        Some(owner),
        &tree(&[(4242, 4241), (9000, 8000), (9100, 9200)]),
    );
    assert_eq!(found.as_deref(), Some(mine));
    // A taken id is skipped; the other folder's and other processes'
    // sessions stay out, so nothing is left.
    let taken = BTreeSet::from([mine.to_string()]);
    assert_eq!(
        session_at(
            &root,
            "/home/user/acme-shop",
            1,
            &taken,
            Some(owner),
            &tree(&[(4242, 4241), (9000, 8000), (9100, 9200)])
        ),
        None
    );
}

/// Manifests older than the launch are skipped: `started_ms` past
/// every file's time finds nothing. A manifest whose parent pid is
/// unknown is not rejected on a guess, and without an owner pid the
/// check is skipped entirely.
#[test]
fn session_at_respects_the_launch_time_and_the_pid() {
    let dir = tmp("sessions-time");
    let root = dir.join("sessions");
    fs::create_dir_all(&root).unwrap();
    let id = "1794894104593_rurer";
    manifest(&root, id, serde_json::json!({"pid": 4242}));
    let none = BTreeSet::new();
    // "Older start": every file precedes the launch.
    assert_eq!(
        session_at(
            &root,
            "/home/user/acme-shop",
            u64::MAX,
            &none,
            Some(4241),
            &tree(&[(4242, 4241)])
        ),
        None
    );
    // The parent is this role's terminal.
    assert_eq!(
        session_at(
            &root,
            "/home/user/acme-shop",
            1,
            &none,
            Some(4241),
            &tree(&[(4242, 4241)])
        )
        .as_deref(),
        Some(id)
    );
    // A different parent is another session.
    assert_eq!(
        session_at(
            &root,
            "/home/user/acme-shop",
            1,
            &none,
            Some(1111),
            &tree(&[(4242, 4241)])
        ),
        None
    );
    // A pid the lookup cannot resolve keeps the session: nothing
    // proves it is somebody else's.
    assert_eq!(
        session_at(
            &root,
            "/home/user/acme-shop",
            1,
            &none,
            Some(4241),
            &tree(&[])
        )
        .as_deref(),
        Some(id)
    );
    // Without the terminal's pid there is nothing to compare.
    assert_eq!(
        session_at(&root, "/home/user/acme-shop", 1, &none, None, &tree(&[])).as_deref(),
        Some(id)
    );
    // A missing store is no session.
    assert_eq!(
        session_at(
            &dir.join("gone"),
            "/home/user/acme-shop",
            1,
            &none,
            None,
            &tree(&[])
        ),
        None
    );
}

/// The store is the user's own `~/.cline/data/sessions`, and the pid
/// lookup reads a real process tree.
#[test]
fn the_store_and_the_pid_lookup() {
    assert_eq!(
        super::cline::sessions_root(Path::new("/home/user")),
        Path::new("/home/user/.cline/data/sessions")
    );
    // This test process's parent is a real pid.
    #[cfg(target_os = "linux")]
    {
        let parent = super::cline::parent_pid(std::process::id()).expect("/proc is readable");
        assert!(parent > 0, "{parent}");
        assert_eq!(
            super::cline::parent_pid(u32::MAX - 1),
            None,
            "no such process"
        );
    }
}
