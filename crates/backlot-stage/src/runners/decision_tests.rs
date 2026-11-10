//! Tests for the shared planning helpers: `decide_start` and its
//! slot, the unknown-runner refusal and the generated ids.

use super::*;

fn actor(runner: &str) -> ActorIdentity {
    ActorIdentity {
        runner: runner.into(),
        runner_version: None,
        model: "m".into(),
        effort: "high".into(),
        permission_profile: "standard".into(),
    }
}

fn ctx<'a>(terminal: &'a str, env: &'a BTreeMap<String, String>) -> LaunchCtx<'a> {
    LaunchCtx {
        terminal,
        cwd: "/home/user/acme-shop",
        prompt: None,
        resume: None,
        studio_dir: "/home/user/state/studios/acme-shop",
        stage_env: env,
        role: "actor-t1-c1",
        checks: &[],
    }
}

/// `fake` runs the shipped script and reports a derivable session.
#[test]
fn fake_plans_the_script_and_a_session() {
    let env = BTreeMap::new();
    let launch = plan(&actor("fake"), &ctx("bl-s-t1", &env)).unwrap();
    assert_eq!(launch.argv.len(), 1);
    assert!(launch.argv[0].ends_with("tools/fake-runner.sh"));
    assert!(
        std::path::Path::new(&launch.argv[0]).is_file(),
        "{}",
        launch.argv[0]
    );
    assert_eq!(launch.session_id().as_deref(), Some("fake-bl-s-t1"));
    assert_eq!(
        session_id(&actor("fake"), "bl-s-t1").as_deref(),
        Some("fake-bl-s-t1")
    );
}

/// Anything else explains itself instead of running.
#[test]
fn an_unknown_runner_is_a_plan_error() {
    let env = BTreeMap::new();
    let err = plan(&actor("kimi"), &ctx("bl-s-t1", &env)).unwrap_err();
    assert_eq!(err.reason, RUNNER_NOT_SUPPORTED);
    assert!(err.detail.contains("kimi"), "{err:?}");
    assert_eq!(session_id(&actor("kimi"), "bl-s-t1"), None);
}

fn launch_for(runner: &str) -> Result<Launch, PlanError> {
    Ok(Launch {
        argv: vec![runner.into()],
        env: BTreeMap::new(),
        session: Session::PostLaunch,
    })
}

/// Two pending real starts give one launch: the slot is spent by
/// the first that can run. `fake` never spends it, and neither a
/// plan error nor a malformed config does — a broken role cannot
/// starve the others.
#[test]
fn one_real_runner_launches_per_tick() {
    let env = BTreeMap::new();
    let none = BTreeSet::new();
    let mut slot = true;
    // fake is exempt and never spends the slot.
    let first = plan(&actor("fake"), &ctx("bl-s-t1", &env));
    assert!(matches!(
        decide_start(first, "fake", Ok(BTreeMap::new()), &mut slot, &none, "/cwd"),
        StartDecision::Launch { .. }
    ));
    assert!(slot);
    // The first real runner takes the slot; the second defers.
    let d1 = decide_start(
        launch_for("claude"),
        "claude",
        Ok(BTreeMap::new()),
        &mut slot,
        &none,
        "/cwd",
    );
    assert!(matches!(d1, StartDecision::Launch { .. }));
    assert!(!slot);
    let d2 = decide_start(
        launch_for("codex"),
        "codex",
        Ok(BTreeMap::new()),
        &mut slot,
        &none,
        "/cwd",
    );
    assert_eq!(d2, StartDecision::Defer);
    // Next tick the slot is free again.
    let mut slot = true;
    let d3 = decide_start(
        launch_for("codex"),
        "codex",
        Ok(BTreeMap::new()),
        &mut slot,
        &none,
        "/cwd",
    );
    assert!(matches!(d3, StartDecision::Launch { .. }));
}

/// A post-launch runner's second start in the same folder
/// defers — without spending the real slot — until the live
/// role's pair leaves `blocked`; another folder or runner, and
/// chosen-id runners, are unaffected.
#[test]
fn one_pending_post_launch_runner_per_folder() {
    let none = BTreeSet::new();
    let blocked = BTreeSet::from([("codex".to_string(), "/cwd".to_string())]);
    let mut slot = true;
    // Same runner, same folder: defers, slot untouched.
    let d = decide_start(
        launch_for("codex"),
        "codex",
        Ok(BTreeMap::new()),
        &mut slot,
        &blocked,
        "/cwd",
    );
    assert_eq!(d, StartDecision::Defer);
    assert!(slot, "a deferred start spends nothing");
    // Same runner, another folder: free to go.
    let d = decide_start(
        launch_for("codex"),
        "codex",
        Ok(BTreeMap::new()),
        &mut slot,
        &blocked,
        "/other",
    );
    assert!(matches!(d, StartDecision::Launch { .. }));
    // A chosen-id runner in the blocked folder is unaffected.
    let mut slot = true;
    let mut launch = launch_for("claude").unwrap();
    launch.session = Session::Chosen("s".into());
    let d = decide_start(
        Ok(launch),
        "claude",
        Ok(BTreeMap::new()),
        &mut slot,
        &blocked,
        "/cwd",
    );
    assert!(matches!(d, StartDecision::Launch { .. }));
    // And once the blocker clears (id found or attention), the
    // deferred start goes ahead on a fresh tick's slot.
    let mut slot = true;
    let d = decide_start(
        launch_for("codex"),
        "codex",
        Ok(BTreeMap::new()),
        &mut slot,
        &none,
        "/cwd",
    );
    assert!(matches!(d, StartDecision::Launch { .. }));
}

/// Errors never spend the slot: a plan refusal and a malformed
/// config leave the launch for a later start in the same tick.
#[test]
fn a_broken_role_does_not_spend_the_slot() {
    let none = BTreeSet::new();
    let mut slot = true;
    let refused = decide_start(
        Err(PlanError::profile("nowhere")),
        "claude",
        Ok(BTreeMap::new()),
        &mut slot,
        &none,
        "/cwd",
    );
    assert!(matches!(refused, StartDecision::Attention(_)));
    assert!(slot);
    let malformed = decide_start(
        launch_for("claude"),
        "claude",
        Err("bad toml".into()),
        &mut slot,
        &none,
        "/cwd",
    );
    match malformed {
        StartDecision::Attention(e) => assert_eq!(e.reason, CONFIG_MALFORMED),
        other => panic!("expected attention, got {other:?}"),
    }
    assert!(slot);
    let d = decide_start(
        launch_for("codex"),
        "codex",
        Ok(BTreeMap::new()),
        &mut slot,
        &none,
        "/cwd",
    );
    assert!(matches!(d, StartDecision::Launch { .. }));
}

/// Generated ids are v4-shaped UUIDs.
#[test]
fn uuid_v4_shape() {
    let u = uuid_v4();
    assert_eq!(u.len(), 36, "{u}");
    assert_eq!(&u[14..15], "4", "{u}");
    assert!(matches!(&u[19..20], "8" | "9" | "a" | "b"), "{u}");
    assert!(u.chars().all(|c| c.is_ascii_hexdigit() || c == '-'), "{u}");
}
