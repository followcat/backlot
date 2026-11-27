//! Cross-session studio resolution: a `--studio` name not under the
//! resolved base is searched across the sibling core directories —
//! the CLI finds the studio for you. Exactly one sibling match wins,
//! none keeps the "no studio" refusal, and several refuse with the
//! core ids so `--session` can disambiguate. `BACKLOT_STATE_DIR`,
//! `--session` and `--remote` pin the base and skip the scan.

use std::fs;

use crate::args::parse;
use crate::cli::{Env, Fail};
use crate::cli_testkit::*;
use crate::test_support::TestDir;

/// `backlot/<core>` under `root`'s fake XDG state home.
fn core(root: &TestDir, id: &str) -> std::path::PathBuf {
    root.join("backlot").join(id)
}

/// The plain-shell environment: an XDG state home, no
/// `BACKLOT_STATE_DIR` pin, no `--session`.
fn shell_env(root: &TestDir) -> Env {
    let mut vars = std::collections::BTreeMap::new();
    vars.insert("XDG_STATE_HOME".into(), root.to_string_lossy().into_owned());
    Env { vars }
}

/// `studio new` under `base` via the pinned-env path — the way the
/// studio would have been created inside its own session.
fn make_studio(root: &TestDir, core_dir: &std::path::Path, name: &str, title: &str) {
    let mut vars = std::collections::BTreeMap::new();
    vars.insert(
        "BACKLOT_STATE_DIR".into(),
        core_dir.to_string_lossy().into_owned(),
    );
    let env = Env { vars };
    let cast_dir = root.join("files");
    fs::create_dir_all(&cast_dir).unwrap();
    let cast = cast_dir.join(format!("cast-{name}.toml"));
    fs::write(&cast, CAST_TOML).unwrap();
    ok(
        &env,
        &format!(
            "studio new --title {title} --repo /home/user/{name} --cast {} --studio {name}",
            cast.display()
        ),
    );
}

fn argv(s: &str) -> Vec<String> {
    s.split(' ').map(String::from).collect()
}

/// The resolved base a named studio lands on — `Err` is the refusal.
fn resolved(env: &Env, base: &std::path::Path, args: &str) -> Result<std::path::PathBuf, Fail> {
    let (common, _) = parse(&argv(args)).unwrap();
    crate::cli::resolve_base(env, &common, base, "orchard")
}

#[test]
fn a_studio_on_another_core_resolves_for_reads_and_writes() {
    let root = TestDir::new("backlot-xdg");
    let core_a = core(&root, "core-a");
    let core_b = core(&root, "core-b");
    make_studio(&root, &core_b, "orchard", "Orchard");
    let env = shell_env(&root);

    // A write command resolved against core-a lands on core-b's log.
    let (common, cmd) = parse(&argv(
        "studio finish --studio orchard --reason done --force --as user",
    ))
    .unwrap();
    let out = crate::cli::exec_event(common, cmd, &env, &core_a).unwrap();
    assert!(out.contains("studio.finished"), "{out}");
    let state = crate::store::load_state(&core_b.join("studios/orchard")).unwrap();
    assert!(
        state.finished.is_some(),
        "the event reached core-b: {state:?}"
    );
    assert!(
        !core_a.join("studios/orchard").exists(),
        "nothing was created under core-a"
    );

    // A read resolves it the same way.
    let (common, cmd) = parse(&argv("status --studio orchard --json")).unwrap();
    let out = crate::hwood_read::read(&common, &cmd, &env, &core_a)
        .expect("status is a read")
        .unwrap();
    assert!(out.contains("\"finished\""), "{out}");
}

#[test]
fn several_cores_holding_the_name_refuse_with_the_list() {
    let root = TestDir::new("backlot-xdg");
    let core_a = core(&root, "core-a");
    make_studio(&root, &core(&root, "core-b"), "orchard", "Orchard");
    make_studio(&root, &core(&root, "core-c"), "orchard", "Orchard");
    let env = shell_env(&root);

    let m = resolved(&env, &core_a, "status --studio orchard").unwrap_err();
    let m = m.message();
    assert!(m.contains("several"), "{m}");
    assert!(m.contains("core-b") && m.contains("core-c"), "{m}");
    assert!(m.contains("--session"), "{m}");
}

#[test]
fn no_match_anywhere_keeps_the_plain_refusal() {
    let root = TestDir::new("backlot-xdg");
    fs::create_dir_all(core(&root, "core-a")).unwrap();
    make_studio(&root, &core(&root, "core-b"), "other", "Other");
    let env = shell_env(&root);

    let m = resolved(&env, &core(&root, "core-a"), "status --studio orchard").unwrap_err();
    assert_eq!(m.message(), "no studio 'orchard'");
}

#[test]
fn a_pinned_base_never_scans_siblings() {
    let root = TestDir::new("backlot-xdg");
    let core_a = core(&root, "core-a");
    make_studio(&root, &core(&root, "core-b"), "orchard", "Orchard");

    // `BACKLOT_STATE_DIR` pins — role terminals carry it.
    let mut env = shell_env(&root);
    env.vars.insert(
        "BACKLOT_STATE_DIR".into(),
        core_a.to_string_lossy().into_owned(),
    );
    let m = resolved(&env, &core_a, "status --studio orchard").unwrap_err();
    assert_eq!(m.message(), "no studio 'orchard'");

    // `--session` and `--remote` pin too — they already say which core.
    let env = shell_env(&root);
    for args in [
        "status --studio orchard --session other",
        "status --studio orchard --remote host-a",
    ] {
        let m = resolved(&env, &core_a, args).unwrap_err();
        assert_eq!(m.message(), "no studio 'orchard'", "{args}");
    }
}
