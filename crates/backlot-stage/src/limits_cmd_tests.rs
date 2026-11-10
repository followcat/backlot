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

/// A cap under the always-live fixed seats never starts: with
/// producer and director on `fake`, `fake = 1` seats no director
/// and `fake = 2` leaves the writer and every actor waiting
/// forever. Both are usage errors at `studio new`.
#[test]
fn a_cap_that_seats_nobody_else_is_a_usage_error() {
    let (scratch, env) = scratch();
    let repo = shared_repo(&scratch);
    // Producer, director, writer and a candidate all on `fake`.
    let all_fake = CAST_TOML
        .replace("runner-p", "fake")
        .replace("runner-d", "fake")
        .replace("runner-w", "fake")
        .replace("runner-a", "fake")
        .replace("runner-b", "fake");
    for (cap, needle) in [
        (1, "leaves a fixed role unseated"),
        (2, "leaves no slot for the writer or an actor"),
    ] {
        let cast = fixture(
            &scratch,
            &format!("cast{cap}.toml"),
            &format!("{all_fake}\n[limits]\nfake = {cap}\n"),
        );
        let msg = usage(
            &env,
            &format!(
                "studio new --title Shop --repo {} --cast {cast}",
                repo.display()
            ),
        );
        assert!(msg.contains(needle), "cap {cap}: {msg}");
        assert!(msg.contains("limits.fake"), "cap {cap}: {msg}");
    }
    // One seat past the fixed two is the smallest workable cap.
    let cast = fixture(
        &scratch,
        "cast3.toml",
        &format!("{all_fake}\n[limits]\nfake = 3\n"),
    );
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo {} --cast {cast}",
            repo.display()
        ),
    );
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
