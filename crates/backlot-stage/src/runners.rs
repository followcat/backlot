//! Runner adapters: an actor identity becomes argv, environment and a
//! session id. Only `fake` exists in S2 — a script that announces the
//! role and stays alive. Anything else is a plan error the stage
//! records as `attention.raised`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use backlot_model::ActorIdentity;

/// How to start one role's terminal program.
#[derive(Debug, Clone, PartialEq)]
pub struct Launch {
    pub argv: Vec<String>,
    /// Runner-specific environment on top of the role's.
    pub env: BTreeMap<String, String>,
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

/// The runner's plan for `identity` on `terminal`, or why it cannot
/// run yet (`detail` goes into `attention.raised`).
pub fn plan(identity: &ActorIdentity, terminal: &str) -> Result<Launch, String> {
    match identity.runner.as_str() {
        "fake" => Ok(Launch {
            argv: vec![
                tools_dir()
                    .join("fake-runner.sh")
                    .to_string_lossy()
                    .into_owned(),
            ],
            env: BTreeMap::new(),
        }),
        other => Err(format!(
            "runner '{other}' is not supported yet (role terminal '{terminal}' would need it)"
        )),
    }
}

/// The session id a runner reports for `terminal`, when it has one.
/// `fake` derives it from the terminal name so a recorded id survives
/// a role close.
pub fn session_id(identity: &ActorIdentity, terminal: &str) -> Option<String> {
    match identity.runner.as_str() {
        "fake" => Some(format!("fake-{terminal}")),
        _ => None,
    }
}

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

    /// `fake` runs the shipped script and reports a derivable session.
    #[test]
    fn fake_plans_the_script_and_a_session() {
        let launch = plan(&actor("fake"), "bl-s-t1").unwrap();
        assert_eq!(launch.argv.len(), 1);
        assert!(launch.argv[0].ends_with("tools/fake-runner.sh"));
        assert!(
            std::path::Path::new(&launch.argv[0]).is_file(),
            "{}",
            launch.argv[0]
        );
        assert_eq!(
            session_id(&actor("fake"), "bl-s-t1").as_deref(),
            Some("fake-bl-s-t1")
        );
    }

    /// Anything else explains itself instead of running.
    #[test]
    fn an_unknown_runner_is_a_plan_error() {
        let err = plan(&actor("claude"), "bl-s-t1").unwrap_err();
        assert!(err.contains("claude"), "{err}");
        assert!(err.contains("not supported"), "{err}");
        assert_eq!(session_id(&actor("claude"), "bl-s-t1"), None);
    }
}
