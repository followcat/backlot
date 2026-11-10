//! Declaring stage checks in the cast file (S9b part 2, item 1):
//! `check_env` and `[[checks]]` reach `studio.created`; bad names,
//! duplicates, empty commands, zero timeouts and dangerous
//! environment names are usage errors that create nothing.

use crate::cli::{Env, Fail, run};
use crate::cli_testkit::*;
use crate::store;
use backlot_model::{Event, EventKind};

/// Cast TOML with the caller's `check_env` line and check stanzas.
/// `check_env` is a bare key: it belongs beside `mode`/`rework_cap`,
/// ahead of the first `[table]` header, or TOML files it under the
/// last `[[candidates]]` entry.
fn cast_with_checks(check_env: &str, checks: &str) -> String {
    let base = CAST_TOML.replacen("rework_cap = 2", &format!("rework_cap = 2\n{check_env}"), 1);
    format!("{base}\n{checks}\n")
}

/// `studio new` with a cast built from `cast_with_checks`; answers
/// the studio's events, or the failure message when refused.
fn new_studio(env: &Env, scratch: &Scratch, cast: &str) -> Result<Vec<Event>, String> {
    let cast_path = fixture(scratch, "cast-checks.toml", cast);
    let args: Vec<String> = [
        "studio",
        "new",
        "--title",
        "Shop",
        "--repo",
        "/home/user/acme-shop",
        "--cast",
        &cast_path,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    match run(&args, env) {
        Ok(_) => {
            let (events, _) = store::load_events(&store::studio_dir(&scratch.dir, "acme-shop"))
                .expect("load events");
            Ok(events)
        }
        Err(Fail::Usage(m) | Fail::Refused(m) | Fail::Core(m)) => Err(m),
    }
}

#[test]
fn declared_checks_reach_studio_created() {
    let (scratch, env) = scratch();
    let events = new_studio(
        &env,
        &scratch,
        &cast_with_checks(
            "check_env = [\"CARGO_HOME\", \"RUSTUP_HOME\"]",
            "[[checks]]\nname = \"unit\"\nrun = \"python3 -m unittest\"\n\n\
             [[checks]]\nname = \"smoke-test\"\nrun = \"sh smoke.sh\"\ntimeout_s = 30",
        ),
    )
    .expect("studio new");
    let [only] = events.as_slice() else {
        panic!("one event")
    };
    let EventKind::StudioCreated {
        check_env, checks, ..
    } = &only.kind
    else {
        panic!("studio.created")
    };
    assert_eq!(
        check_env,
        &["CARGO_HOME".to_string(), "RUSTUP_HOME".to_string()]
    );
    assert_eq!(checks.len(), 2);
    assert_eq!(checks[0].name, "unit");
    assert_eq!(checks[0].run, "python3 -m unittest");
    assert_eq!(checks[0].timeout_s, 600, "the default timeout is 600s");
    assert_eq!(checks[1].name, "smoke-test");
    assert_eq!(checks[1].timeout_s, 30);
}

#[test]
fn a_bad_check_name_is_a_usage_error() {
    for name in ["Unit", "has space", "dot.name", ""] {
        let (scratch, env) = scratch();
        let out = new_studio(
            &env,
            &scratch,
            &cast_with_checks(
                "",
                &format!("[[checks]]\nname = \"{name}\"\nrun = \"true\""),
            ),
        )
        .unwrap_err();
        assert!(out.contains("check"), "{out}");
        assert!(
            !store::studio_dir(&scratch.dir, "acme-shop").exists(),
            "the refused studio was still created"
        );
    }
}

#[test]
fn a_duplicate_check_name_is_a_usage_error() {
    let (scratch, env) = scratch();
    let out = new_studio(
        &env,
        &scratch,
        &cast_with_checks(
            "",
            "[[checks]]\nname = \"unit\"\nrun = \"a\"\n\n[[checks]]\nname = \"unit\"\nrun = \"b\"",
        ),
    )
    .unwrap_err();
    assert!(out.contains("unit"), "{out}");
}

#[test]
fn an_empty_run_is_a_usage_error() {
    let (scratch, env) = scratch();
    let out = new_studio(
        &env,
        &scratch,
        &cast_with_checks("", "[[checks]]\nname = \"unit\"\nrun = \"   \""),
    )
    .unwrap_err();
    assert!(out.contains("unit"), "{out}");
}

#[test]
fn a_zero_timeout_is_a_usage_error() {
    let (scratch, env) = scratch();
    let out = new_studio(
        &env,
        &scratch,
        &cast_with_checks(
            "",
            "[[checks]]\nname = \"unit\"\nrun = \"true\"\ntimeout_s = 0",
        ),
    )
    .unwrap_err();
    assert!(out.contains("timeout_s"), "{out}");
}

/// `check_env` names that would hand the stage's own identity or a
/// secret to the check are refused; the reason names the variable.
#[test]
fn a_dangerous_check_env_name_is_a_usage_error() {
    for var in [
        "BACKLOT_ROLE",
        "HC_SESSION",
        "HTTPS_PROXY",
        "api_token",
        "AWS_SECRET_KEY",
        "DB_PASSWORD",
        "client_credential",
        "PRIVATE_KEY",
    ] {
        let (scratch, env) = scratch();
        let out = new_studio(
            &env,
            &scratch,
            &cast_with_checks(
                &format!("check_env = [\"{var}\"]"),
                "[[checks]]\nname = \"unit\"\nrun = \"true\"",
            ),
        )
        .unwrap_err();
        assert!(out.contains(var), "{out}");
    }
}

/// Ordinary build variables pass through.
#[test]
fn ordinary_check_env_names_are_accepted() {
    let (scratch, env) = scratch();
    new_studio(
        &env,
        &scratch,
        &cast_with_checks(
            "check_env = [\"CARGO_HOME\", \"LANG_EXTRA\", \"CC\"]",
            "[[checks]]\nname = \"unit\"\nrun = \"true\"",
        ),
    )
    .expect("ordinary names accepted");
}
