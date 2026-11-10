//! The stage's own checks (S9b part 2). The cast declares them
//! (`[[checks]]`, `check_env`); `studio new` vets the declaration so
//! a bad name or a dangerous environment variable never reaches the
//! ledger. On the tick the stage runs each declared check once per
//! delivery — detached worktree, stripped environment, one process
//! at a time — and records `task.checked` with the capped log.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use backlot_model::event::CheckDecl;
use backlot_model::state::{StudioState, TaskStatus};
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
        // A `run` reaching for `BACKLOT_*` or an environment-clearing
        // flag is refused outright — the same words `touches_env`
        // flags on a prompt. A claude launch pre-allows each check
        // verbatim and could never be trusted to cover one.
        if crate::permit::touches_env(&check.run) {
            return Err(Fail::usage(format!(
                "check '{}' run may not reach for BACKLOT_*, env -i or --ignore-environment",
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

/// A check the ledger still owes: the next unrecorded declared
/// check on the latest delivery of a `delivered` task.
pub struct Owed {
    pub task: String,
    pub delivery: u32,
    /// The `task.delivered` event's seq — it keys the run dir and
    /// the log names, so two deliveries never share either.
    pub delivery_seq: u64,
    pub check: CheckDecl,
}

/// The next check to run, in task order then declared order. Only
/// the latest delivery of a task still `delivered` owes checks —
/// once it moves on nothing new starts, though a check already
/// running finishes and is recorded. Restarting the stage re-derives
/// this from the ledger, so a check with no `task.checked` reruns.
pub fn owed(state: &StudioState) -> Option<Owed> {
    let checks = &state.studio.as_ref()?.checks;
    if checks.is_empty() {
        return None;
    }
    for t in state.tasks.values() {
        if t.status != TaskStatus::Delivered {
            continue;
        }
        let Some(d) = t.deliveries.last() else {
            continue;
        };
        for c in checks {
            if !t.checked_of(d.n).any(|r| r.name == c.name) {
                return Some(Owed {
                    task: t.id.clone(),
                    delivery: d.n,
                    delivery_seq: d.seq,
                    check: c.clone(),
                });
            }
        }
    }
    None
}

/// Stage-owned environment names a check may see — only when the
/// stage has them set. Nothing `HC_*`, `BACKLOT_*` or proxy-shaped
/// ever crosses: the cast's `check_env` was vetted for those, and
/// this list never carries them.
const PASSTHROUGH: &[&str] = &[
    "PATH", "HOME", "LANG", "LC_ALL", "LC_CTYPE", "TERM", "USER", "LOGNAME", "TMPDIR",
];

/// The exact environment a check runs under: the passthrough list
/// plus the declared `check_env` names, valued from the stage's own
/// environment. A name the stage does not have is simply absent.
pub fn env_for(
    stage_env: &BTreeMap<String, String>,
    check_env: &[String],
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for name in PASSTHROUGH
        .iter()
        .copied()
        .chain(check_env.iter().map(|s| s.as_str()))
    {
        if seen.insert(name)
            && let Some(v) = stage_env.get(name)
        {
            out.push((name.to_string(), v.clone()));
        }
    }
    out
}

/// The detached worktree a delivery's checks run in.
pub fn run_dir(dir: &Path, delivery_seq: u64) -> PathBuf {
    dir.join("checks").join(format!("run-{delivery_seq}"))
}

/// The check's log path relative to the studio dir — the `output`
/// the `task.checked` event records.
pub fn log_rel(delivery_seq: u64, name: &str) -> String {
    format!("checks/{delivery_seq}-{name}.log")
}

/// The "Accepted with failing stage checks" section every report
/// copied into the ledger gains while such a task exists: task,
/// delivery, the failing names and the conclusion the review
/// recorded for them. `None` when nothing qualifies.
pub fn failing_section(state: &StudioState) -> Option<String> {
    let studio = state.studio.as_ref()?;
    let mut rows = Vec::new();
    for t in state.tasks.values() {
        if !matches!(
            t.status,
            TaskStatus::Accepted | TaskStatus::PartiallyAccepted
        ) {
            continue;
        }
        let Some(r) = t.reviews.last().filter(|r| {
            matches!(
                r.verdict,
                backlot_model::event::Verdict::Accepted
                    | backlot_model::event::Verdict::PartiallyAccepted
            )
        }) else {
            continue;
        };
        let failed = t.checks_failed(r.delivery, &studio.checks);
        if failed.is_empty() {
            continue;
        }
        rows.push(format!(
            "- {} delivery {}: {} — {}",
            t.id,
            r.delivery,
            failed.join(", "),
            r.conclusion
        ));
    }
    if rows.is_empty() {
        return None;
    }
    Some(format!(
        "\n\n## Accepted with failing stage checks\n\n{}\n",
        rows.join("\n")
    ))
}
