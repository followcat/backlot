//! `BACKLOT_FAKE_RUNNER` (R3-M16): a nonempty override wins over the
//! shipped `tools/fake-runner.sh` in launch planning, resolution and
//! `backlot runners --json`; unset or empty falls back to that script.

use std::collections::BTreeMap;
use std::path::Path;

use crate::cli::Env;
use crate::test_support::TestDir;

use super::detect::scope;
use super::probe::{FAKE_RUNNER_ENV, fake_runner_override, fake_runner_path};
use super::{LaunchCtx, detect_report, fake, resolve, tools_dir};
use backlot_model::ActorIdentity;

fn dir(name: &str) -> TestDir {
    TestDir::new(&format!("backlot-fake-runner-{name}"))
}

fn exe(dir: &Path) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("fake-runner.sh");
    std::fs::write(
        &path,
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 9.9.9; fi\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

fn env(vars: &[(&str, &str)]) -> Env {
    Env {
        vars: vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn identity() -> ActorIdentity {
    ActorIdentity {
        runner: "fake".into(),
        runner_version: None,
        model: "m".into(),
        effort: Some("high".into()),
        permission_profile: "standard".into(),
    }
}

fn ctx<'a>(env: &'a BTreeMap<String, String>) -> LaunchCtx<'a> {
    LaunchCtx {
        terminal: "bl-s-t1",
        cwd: "/home/user/acme-shop",
        prompt: None,
        resume: None,
        studio_dir: "/home/user/state/studios/acme-shop",
        stage_env: env,
        role: "actor-t1-c1",
        checks: &[],
    }
}

/// A nonempty override is the script path; empty and unset fall back
/// to the shipped `tools/fake-runner.sh`.
#[test]
fn the_path_prefers_a_nonempty_override_then_the_tools_script() {
    assert_eq!(
        fake_runner_path(Some("/opt/rig/tools/fake-runner.sh")),
        std::path::PathBuf::from("/opt/rig/tools/fake-runner.sh")
    );
    assert_eq!(
        fake_runner_path(Some("   ")),
        tools_dir().join("fake-runner.sh")
    );
    assert_eq!(
        fake_runner_path(Some("")),
        tools_dir().join("fake-runner.sh")
    );
    assert_eq!(fake_runner_path(None), tools_dir().join("fake-runner.sh"));
}

/// A nonempty stage value is returned; an empty one is skipped.
#[test]
fn the_override_reports_only_nonempty_values() {
    assert_eq!(
        fake_runner_override(Some("/opt/rig/tools/fake-runner.sh")).as_deref(),
        Some("/opt/rig/tools/fake-runner.sh")
    );
    assert_eq!(fake_runner_override(Some("  ")), None);
}

/// `plan` launches the stage environment's script ahead of `tools/`.
#[test]
fn plan_uses_the_stage_env_override() {
    let stage = BTreeMap::from([(
        FAKE_RUNNER_ENV.to_string(),
        "/opt/rig/tools/fake-runner.sh".to_string(),
    )]);
    let launch = fake::plan(&identity(), &ctx(&stage)).unwrap();
    assert_eq!(launch.argv[0], "/opt/rig/tools/fake-runner.sh");
    // No override: the shipped script, whatever the process env says.
    let bare = BTreeMap::new();
    let expected = fake_runner_path(fake_runner_override(None).as_deref());
    let launch = fake::plan(&identity(), &ctx(&bare)).unwrap();
    assert_eq!(launch.argv[0], expected.to_string_lossy());
}

/// Resolution honours the injected environment's override, and an
/// empty value falls back to the shipped script.
#[test]
fn resolve_uses_the_injected_override() {
    let root = dir("resolve");
    let script = exe(&root.join("tools"));
    let script_str = script.to_string_lossy().into_owned();
    let configured = env(&[(FAKE_RUNNER_ENV, &script_str)]);
    let found = resolve::launch_binary("fake", "", &scope(&configured)).unwrap();
    assert_eq!(found.path, script);

    let empty = env(&[(FAKE_RUNNER_ENV, "")]);
    let found = resolve::launch_binary("fake", "", &scope(&empty)).unwrap();
    assert_eq!(found.path, tools_dir().join("fake-runner.sh"));
}

/// `backlot runners --json` lists `fake` at the override's script.
#[test]
fn report_json_reads_fake_at_the_override() {
    let root = dir("json");
    let script = exe(&root.join("tools"));
    let script_str = script.to_string_lossy().into_owned();
    let configured = env(&[("PATH", "/nonexistent-dir"), (FAKE_RUNNER_ENV, &script_str)]);
    let out = detect_report::report_json(&configured).unwrap();
    let rows: serde_json::Value = serde_json::from_str(&out).unwrap();
    let fake = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["runner"] == "fake")
        .expect("a fake row");
    assert_eq!(fake["path"], script_str);
    assert_eq!(fake["version"], "9.9.9");
}
