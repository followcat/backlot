//! S9e item 3 — the cast file's `[limits]` table: a zero cap is a
//! usage error and set values land on `studio.created` for the
//! reconcile slot count.

use crate::cli_testkit::*;

/// `fake = 0` never reaches the ledger — a zero cap seats nobody,
/// and a runner is either limited by a real number or absent.
#[test]
fn a_zero_limit_is_a_usage_error() {
    let (scratch, env) = scratch();
    let repo = shared_repo(&scratch);
    let cast = fixture(
        &scratch,
        "cast.toml",
        &format!("{CAST_TOML}\n[limits]\nfake = 0\n"),
    );
    let msg = usage(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast}",
            repo.display()
        ),
    );
    assert!(msg.contains("limits.fake"), "{msg}");
}

/// A set cap lands on `studio.created` — `status --json` shows the
/// reduced `studio.limits` the slot count reads. A runner with no
/// entry stays unlimited.
#[test]
fn set_limits_land_on_studio_created() {
    let (scratch, env) = scratch();
    let repo = shared_repo(&scratch);
    let cast = fixture(
        &scratch,
        "cast.toml",
        &format!("{CAST_TOML}\n[limits]\nfake = 2\ndevin = 1\n"),
    );
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast}",
            repo.display()
        ),
    );
    let out = ok(&env, "status --json");
    assert!(out.contains("\"fake\": 2"), "{out}");
    assert!(out.contains("\"devin\": 1"), "{out}");
}
