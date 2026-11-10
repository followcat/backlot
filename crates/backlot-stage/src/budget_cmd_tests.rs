//! S9e item 2 — the cast file's `task_budget_s`: zero is a usage
//! error and a set value lands on `studio.created` for the stage's
//! budget clock.

use crate::cli_testkit::*;

/// `task_budget_s = 0` never reaches the ledger — a zero budget
/// would flag every cast the tick it lands.
#[test]
fn a_zero_task_budget_is_a_usage_error() {
    let (scratch, env) = scratch();
    let repo = shared_repo(&scratch);
    let cast = fixture(
        &scratch,
        "cast.toml",
        &format!("task_budget_s = 0\n{CAST_TOML}"),
    );
    let msg = usage(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast}",
            repo.display()
        ),
    );
    assert!(msg.contains("task_budget_s"), "{msg}");
}

/// A set budget lands on `studio.created` — `status --json` shows
/// the reduced `studio.task_budget_s` the budget clock reads.
#[test]
fn a_set_task_budget_lands_on_studio_created() {
    let (scratch, env) = scratch();
    let repo = shared_repo(&scratch);
    let cast = fixture(
        &scratch,
        "cast.toml",
        &format!("task_budget_s = 30\n{CAST_TOML}"),
    );
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast}",
            repo.display()
        ),
    );
    let out = ok(&env, "status --json");
    assert!(out.contains("\"task_budget_s\": 30"), "{out}");
}
