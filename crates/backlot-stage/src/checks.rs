//! The stage's own checks (S9b part 2). The cast declares them
//! (`[[checks]]`, `check_env`); `studio new` vets the declaration so
//! a bad name or a dangerous environment variable never reaches the
//! ledger.

use std::collections::BTreeSet;

use backlot_model::event::CheckDecl;
use serde::Deserialize;

use crate::cli::Fail;

/// One `[[checks]]` stanza of the cast file.
#[derive(Debug, Deserialize)]
pub struct CastCheck {
    pub name: String,
    pub run: String,
    #[serde(default = "default_timeout_s")]
    pub timeout_s: u64,
}

fn default_timeout_s() -> u64 {
    600
}

/// A check name is a short lowercase slug: it keys `task.checked`
/// and names the log file under `checks/`.
fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// `check_env` hands names straight to a check running actor-written
/// code. Names that carry the stage's own identity (`BACKLOT_*`,
/// `HC_*`) or look like a proxy or a credential are refused at
/// `studio new` — the check gets only the build variables the cast
/// names outright.
fn env_ok(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    if up.starts_with("BACKLOT_") || up.starts_with("HC_") {
        return false;
    }
    !["PROXY", "TOKEN", "SECRET", "PASSWORD", "CREDENTIAL", "KEY"]
        .iter()
        .any(|bad| up.contains(bad))
}

/// Vet the cast's check declaration for `studio new`; answers the
/// values recorded on `studio.created`.
pub fn declarations(
    check_env: &[String],
    checks: &[CastCheck],
) -> Result<(Vec<String>, Vec<CheckDecl>), Fail> {
    let mut names = BTreeSet::new();
    for check in checks {
        if !name_ok(&check.name) {
            return Err(Fail::usage(format!(
                "check name '{}' must match [a-z0-9_-]+",
                check.name
            )));
        }
        if !names.insert(&check.name) {
            return Err(Fail::usage(format!(
                "check name '{}' is declared twice",
                check.name
            )));
        }
        if check.run.trim().is_empty() {
            return Err(Fail::usage(format!(
                "check '{}' has an empty run command",
                check.name
            )));
        }
        if check.timeout_s == 0 {
            return Err(Fail::usage(format!(
                "check '{}' timeout_s must be at least 1",
                check.name
            )));
        }
    }
    for name in check_env {
        if !env_ok(name) {
            return Err(Fail::usage(format!(
                "check_env name '{name}' may not pass a stage identity, proxy or credential to a check"
            )));
        }
    }
    Ok((
        check_env.to_vec(),
        checks
            .iter()
            .map(|c| CheckDecl {
                name: c.name.clone(),
                run: c.run.clone(),
                timeout_s: c.timeout_s,
            })
            .collect(),
    ))
}
