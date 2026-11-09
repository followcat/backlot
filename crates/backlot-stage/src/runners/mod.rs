//! Runner adapters: an actor identity becomes argv, environment and a
//! way to learn the session id — one file per runner. `plan` is pure:
//! it lays out the launch. Anything it cannot support is a `PlanError`
//! the stage records as `attention.raised`, as is a `[runner_env]`
//! config that does not parse. At most one real runner launches per
//! tick ([`decide_start`]); `fake` is exempt.

pub mod claude;
mod codex;
mod devin;
mod fake;
pub mod opencode;

use std::collections::BTreeMap;
use std::path::PathBuf;

use backlot_model::ActorIdentity;

/// How to start one role's terminal program.
#[derive(Debug, Clone, PartialEq)]
pub struct Launch {
    pub argv: Vec<String>,
    /// Runner-specific environment on top of the role's.
    pub env: BTreeMap<String, String>,
    /// How the role's session id becomes known.
    pub session: Session,
}

/// Where the session id a start records comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum Session {
    /// The adapter chose it at launch; `role.started` carries it.
    Chosen(String),
    /// The runner assigns it; the stage looks for it after launch
    /// and records `role.session`.
    PostLaunch,
}

impl Launch {
    /// The session id to record at launch, when the adapter chose one.
    pub fn session_id(&self) -> Option<String> {
        match &self.session {
            Session::Chosen(id) => Some(id.clone()),
            Session::PostLaunch => None,
        }
    }
}

/// What a launch needs besides the actor identity.
pub struct LaunchCtx<'a> {
    /// The role's terminal name (`bl-<studio>-<role>`).
    pub terminal: &'a str,
    /// The working directory the terminal gets.
    pub cwd: &'a str,
    /// The first prompt: inline text for most runners; for `devin`,
    /// the path of a file the caller already wrote (`--prompt-file`).
    pub prompt: Option<&'a str>,
    /// A session id to resume instead of starting fresh.
    pub resume: Option<&'a str>,
    /// The stage's own environment, for adapters that scrub
    /// inherited markers.
    pub stage_env: &'a BTreeMap<String, String>,
}

/// A role that cannot start, as `attention.raised` records it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanError {
    /// The attention's reason — one of the constants below.
    pub reason: &'static str,
    /// Its detail.
    pub detail: String,
}

/// The runner has no adapter.
pub const RUNNER_NOT_SUPPORTED: &str = "runner not supported";
/// The runner's table has no row for this permission profile.
pub const PROFILE_NOT_SUPPORTED: &str = "permission profile not supported";
/// `<state base>/config.toml` did not parse as `[runner_env]` maps.
pub const CONFIG_MALFORMED: &str = "config malformed";

impl PlanError {
    fn runner(runner: &str, ctx: &LaunchCtx) -> Self {
        PlanError {
            reason: RUNNER_NOT_SUPPORTED,
            detail: format!(
                "runner '{runner}' is not supported yet (role terminal '{}' would need it)",
                ctx.terminal
            ),
        }
    }

    fn profile(profile: &str) -> Self {
        PlanError {
            reason: PROFILE_NOT_SUPPORTED,
            detail: format!("unknown permission profile '{profile}'"),
        }
    }

    fn config(detail: String) -> Self {
        PlanError {
            reason: CONFIG_MALFORMED,
            detail,
        }
    }
}

/// `decide_start`'s verdict for one pending role.
#[derive(Debug, PartialEq)]
pub enum StartDecision {
    /// One attention record (the caller dedups open ones per role).
    Attention(PlanError),
    /// A real launch already went out this tick; retry next tick —
    /// reconcile re-derives the start.
    Defer,
    /// `terminal.run` with this argv and the merged runner env.
    Launch {
        launch: Launch,
        /// The `[runner_env.<runner>]` map from config.toml.
        runner_env: BTreeMap<String, String>,
    },
}

/// The tick's decision for one pending start. Plan and config errors
/// do not spend the launch slot — only a start that can actually run
/// takes it, so a broken role never starves the others.
pub fn decide_start(
    planned: Result<Launch, PlanError>,
    runner: &str,
    runner_env: Result<BTreeMap<String, String>, String>,
    real_slot_free: &mut bool,
) -> StartDecision {
    let launch = match planned {
        Ok(l) => l,
        Err(e) => return StartDecision::Attention(e),
    };
    let runner_env = match runner_env {
        Ok(env) => env,
        Err(detail) => return StartDecision::Attention(PlanError::config(detail)),
    };
    if runner != "fake" {
        if !*real_slot_free {
            return StartDecision::Defer;
        }
        *real_slot_free = false;
    }
    StartDecision::Launch { launch, runner_env }
}

/// The runner's plan for `identity` in `ctx`, or why it cannot run.
pub fn plan(identity: &ActorIdentity, ctx: &LaunchCtx) -> Result<Launch, PlanError> {
    match identity.runner.as_str() {
        "fake" => fake::plan(identity, ctx),
        "claude" => claude::plan(identity, ctx),
        "codex" => codex::plan(identity, ctx),
        "devin" => devin::plan(identity, ctx),
        "opencode" => opencode::plan(identity, ctx),
        other => Err(PlanError::runner(other, ctx)),
    }
}

/// The session id derivable without the terminal: `fake` from the
/// name. Chosen-at-launch ids (claude, opencode) sit in the terminal's
/// argv — [`claude::session_in_argv`], [`opencode::session_in_argv`]
/// find them there; codex and devin ids are found after launch.
pub fn session_id(identity: &ActorIdentity, terminal: &str) -> Option<String> {
    match identity.runner.as_str() {
        "fake" => Some(format!("fake-{terminal}")),
        _ => None,
    }
}

/// The session id a runner chose at launch, read back from a live
/// terminal's argv — how an adopted claude or opencode role recovers
/// its id without a `role.started` to carry it.
pub fn session_in_argv(runner: &str, argv: &[String]) -> Option<String> {
    match runner {
        "claude" => claude::session_in_argv(argv),
        "opencode" => opencode::session_in_argv(argv),
        _ => None,
    }
}

/// The workspace's `tools/` directory, resolved at build time from
/// this crate's manifest. The fake runner ships with the source.
fn tools_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(|p| p.join("tools"))
        .unwrap_or_else(|| PathBuf::from("tools"))
}

/// Sixteen bytes of entropy: `/dev/urandom` where it exists, else a
/// hash of time, pid and a counter — uniqueness only, never secrecy.
fn random_16() -> [u8; 16] {
    #[cfg(unix)]
    {
        use std::io::Read;
        let mut b = [0u8; 16];
        if std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut b))
            .is_ok()
        {
            return b;
        }
    }
    fallback_16()
}

fn fallback_16() -> [u8; 16] {
    use std::hash::{Hash, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seed = || {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (
            nanos,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        )
            .hash(&mut h);
        h.finish().to_le_bytes()
    };
    let (a, b) = (seed(), seed());
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&a);
    out[8..].copy_from_slice(&b);
    out
}

/// A v4 UUID from `random_16` — no dependency.
fn uuid_v4() -> String {
    let mut b = random_16();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7],
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    )
}

#[cfg(test)]
mod argv_tests;
#[cfg(test)]
mod tests {
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
            stage_env: env,
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
        let mut slot = true;
        // fake is exempt and never spends the slot.
        let first = plan(&actor("fake"), &ctx("bl-s-t1", &env));
        assert!(matches!(
            decide_start(first, "fake", Ok(BTreeMap::new()), &mut slot),
            StartDecision::Launch { .. }
        ));
        assert!(slot);
        // The first real runner takes the slot; the second defers.
        let d1 = decide_start(
            launch_for("claude"),
            "claude",
            Ok(BTreeMap::new()),
            &mut slot,
        );
        assert!(matches!(d1, StartDecision::Launch { .. }));
        assert!(!slot);
        let d2 = decide_start(launch_for("codex"), "codex", Ok(BTreeMap::new()), &mut slot);
        assert_eq!(d2, StartDecision::Defer);
        // Next tick the slot is free again.
        let mut slot = true;
        let d3 = decide_start(launch_for("codex"), "codex", Ok(BTreeMap::new()), &mut slot);
        assert!(matches!(d3, StartDecision::Launch { .. }));
    }

    /// Errors never spend the slot: a plan refusal and a malformed
    /// config leave the launch for a later start in the same tick.
    #[test]
    fn a_broken_role_does_not_spend_the_slot() {
        let mut slot = true;
        let refused = decide_start(
            Err(PlanError::profile("nowhere")),
            "claude",
            Ok(BTreeMap::new()),
            &mut slot,
        );
        assert!(matches!(refused, StartDecision::Attention(_)));
        assert!(slot);
        let malformed = decide_start(
            launch_for("claude"),
            "claude",
            Err("bad toml".into()),
            &mut slot,
        );
        match malformed {
            StartDecision::Attention(e) => assert_eq!(e.reason, CONFIG_MALFORMED),
            other => panic!("expected attention, got {other:?}"),
        }
        assert!(slot);
        let d = decide_start(launch_for("codex"), "codex", Ok(BTreeMap::new()), &mut slot);
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
}
