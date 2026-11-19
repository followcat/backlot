//! `studio new --mode` and experimental-mode tests (R14): the flag
//! resolves over the cast file's `mode`, `status` flags only an
//! experimental studio, and one may dispatch a task before any
//! formal spec.

use backlot_model::event::*;

use crate::cli::{Fail, run};
use crate::cli_testkit::*;

/// `--mode experimental` on `studio new` records the experimental
/// mode on `studio.created`; `--mode standard` is the default.
#[test]
fn studio_new_mode_flag() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo /home/user/acme-shop --cast {cast} --mode experimental"
        ),
    );
    let (events, _) =
        crate::store::load_events(&crate::store::studio_dir(&scratch.dir, "acme-shop")).unwrap();
    match &events[0].kind {
        EventKind::StudioCreated { mode, .. } => {
            assert_eq!(*mode, StudioMode::Experimental);
        }
        other => panic!("wrong event: {other:?}"),
    }
}

/// `mode = "experimental"` in the cast file lands the same way —
/// and a CLI `--mode` wins over the TOML when both are given.
#[test]
fn studio_new_mode_from_toml_and_cli_precedence() {
    let (scratch, env) = scratch();
    let toml = CAST_TOML.replacen("mode = \"standard\"", "mode = \"experimental\"", 1);
    let cast = fixture(&scratch, "cast-exp.toml", &toml);
    ok(
        &env,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast}"),
    );
    let (events, _) =
        crate::store::load_events(&crate::store::studio_dir(&scratch.dir, "acme-shop")).unwrap();
    match &events[0].kind {
        EventKind::StudioCreated { mode, .. } => {
            assert_eq!(*mode, StudioMode::Experimental);
        }
        other => panic!("wrong event: {other:?}"),
    }

    // The CLI flag overrides the TOML: --mode standard wins.
    let (scratch2, env2) = crate::cli_testkit::scratch();
    let cast2 = fixture(&scratch2, "cast-exp.toml", &toml);
    ok(
        &env2,
        &format!(
            "studio new --title Shop --repo /home/user/acme-shop --cast {cast2} --mode standard"
        ),
    );
    let (events, _) =
        crate::store::load_events(&crate::store::studio_dir(&scratch2.dir, "acme-shop")).unwrap();
    match &events[0].kind {
        EventKind::StudioCreated { mode, .. } => {
            assert_eq!(*mode, StudioMode::Standard);
        }
        other => panic!("wrong event: {other:?}"),
    }
}

/// An unrecognized `--mode` is a usage error (exit 2).
#[test]
fn studio_new_bad_mode_is_usage_error() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    match run(
        &[
            "studio".into(),
            "new".into(),
            "--title".into(),
            "t".into(),
            "--repo".into(),
            "/home/user/acme-shop".into(),
            "--cast".into(),
            cast,
            "--mode".into(),
            "bogus".into(),
        ],
        &env,
    ) {
        Err(Fail::Usage(m)) => assert!(m.contains("bad --mode"), "{m}"),
        other => panic!("expected usage error, got {other:?}"),
    }
}

/// `status` flags an experimental studio's header; a standard one
/// is byte-identical to what it always was.
#[test]
fn status_flags_only_the_experimental_studio() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo /home/user/acme-shop --cast {cast} --mode experimental"
        ),
    );
    let status = ok(&env, "status");
    assert!(status.contains("[experimental]"), "{status}");

    let (scratch2, env2) = crate::cli_testkit::scratch();
    let cast2 = fixture(&scratch2, "cast.toml", CAST_TOML);
    ok(
        &env2,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast2}"),
    );
    let status = ok(&env2, "status");
    assert!(!status.contains("[experimental]"), "{status}");
    assert!(status.contains("(Standard)"), "{status}");
}

/// R14: an experimental studio dispatches straight at the
/// exploration — `task new` needs no recorded requirement first;
/// a standard studio still refuses it.
#[test]
fn experimental_studio_dispatches_without_a_spec() {
    let (scratch, env) = scratch();
    let cast = fixture(&scratch, "cast.toml", CAST_TOML);
    ok(
        &env,
        &format!(
            "studio new --title Shop --repo /home/user/acme-shop --cast {cast} --mode experimental"
        ),
    );
    let acc = fixture(&scratch, "acc.md", "# acceptance\n");
    ok(
        &env,
        &format!("task new --title probe --kind research --acceptance-file {acc} --as director"),
    );
    let status = ok(&env, "status");
    assert!(status.contains("t1"), "{status}");

    let (scratch2, env2) = crate::cli_testkit::scratch();
    let cast2 = fixture(&scratch2, "cast.toml", CAST_TOML);
    ok(
        &env2,
        &format!("studio new --title Shop --repo /home/user/acme-shop --cast {cast2}"),
    );
    let acc2 = fixture(&scratch2, "acc.md", "# acceptance\n");
    let err = refused(
        &env2,
        &format!("task new --title probe --kind research --acceptance-file {acc2} --as director"),
    );
    assert!(err.contains("requirement"), "{err}");
}
