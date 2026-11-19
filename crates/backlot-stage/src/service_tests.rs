//! The `stage service` unit renderer and writer: exact unit
//! contents, quoting, `BACKLOT_STATE_DIR` passthrough, and a
//! `--write` confined to a scratch `XDG_CONFIG_HOME`.

use std::collections::BTreeMap;
use std::path::Path;

use super::*;
use crate::args::Common;
use crate::cli::Env;
use crate::test_support::TestDir;

fn env_with(pairs: &[(&str, &str)]) -> Env {
    Env {
        vars: pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    }
}

#[test]
fn the_default_unit_keeps_the_stage_alive() {
    let text = render(Path::new("/home/user/bin/backlot"), None, None, UNIT);
    assert!(text.starts_with("# Backlot stage supervisor"));
    assert!(text.contains(
        "systemctl --user daemon-reload && systemctl --user enable --now backlot-stage.service"
    ));
    assert!(text.contains("[Unit]\nDescription=Backlot stage supervisor"));
    assert!(text.contains("After=default.target"));
    assert!(text.contains("[Service]\nType=simple"));
    assert!(text.contains("ExecStart=/home/user/bin/backlot stage watch --restart\n"));
    assert!(text.contains("Restart=on-failure"));
    assert!(text.contains("RestartSec=5"));
    assert!(text.contains("Environment=\"PATH=/home/user/bin:/usr/local/bin:/usr/bin:/bin\""));
    assert!(text.contains("[Install]\nWantedBy=default.target"));
    assert!(!text.contains("BACKLOT_STATE_DIR"), "nothing was set");
    assert!(!text.contains("--session"), "no session was named");
}

#[test]
fn a_session_names_its_own_unit() {
    let unit = unit_name(Some("run1")).unwrap();
    assert_eq!(unit, "backlot-stage-run1.service");
    let text = render(Path::new("/opt/backlot"), None, Some("run1"), &unit);
    assert!(text.contains("ExecStart=/opt/backlot stage watch --restart --session run1\n"));
    assert!(text.contains("enable --now backlot-stage-run1.service"));
}

#[test]
fn an_exec_path_with_spaces_is_quoted() {
    let text = render(Path::new("/opt/My Tools/backlot"), None, None, UNIT);
    assert!(
        text.contains("ExecStart=\"/opt/My Tools/backlot\" stage watch --restart\n"),
        "{text}"
    );
    // PATH keeps the same directory, inside its own quoted value.
    assert!(text.contains("Environment=\"PATH=/opt/My Tools:/usr/local/bin:/usr/bin:/bin\""));
}

#[test]
fn a_state_dir_rides_along_in_the_environment() {
    let text = render(
        Path::new("/home/user/bin/backlot"),
        Some("/home/user/state/backlot/core-1"),
        None,
        UNIT,
    );
    assert!(
        text.contains("Environment=\"BACKLOT_STATE_DIR=/home/user/state/backlot/core-1\"\n"),
        "{text}"
    );
}

#[test]
fn unsafe_session_names_are_rejected() {
    for bad in ["", "a/b", "a b", "a\nb", "a;rm", &"x".repeat(49)] {
        assert!(
            matches!(unit_name(Some(bad)), Err(Fail::Usage(_))),
            "{bad:?} was accepted"
        );
    }
    for good in ["default", "run-1", "a.b_c"] {
        assert!(unit_name(Some(good)).is_ok(), "{good:?} was refused");
    }
}

#[test]
fn print_writes_nothing() {
    let dir = TestDir::new("backlot-service-test");
    let env = env_with(&[("XDG_CONFIG_HOME", dir.to_str().unwrap())]);
    let out = run(&Common::default(), &env, false).unwrap();
    assert!(out.contains("[Service]"), "{out}");
    assert!(!dir.join("systemd").exists(), "--print touched the disk");
}

#[test]
fn write_lands_under_xdg_config_home_with_the_printed_commands() {
    let dir = TestDir::new("backlot-service-test");
    let env = env_with(&[
        ("XDG_CONFIG_HOME", dir.to_str().unwrap()),
        ("BACKLOT_STATE_DIR", "/home/user/state"),
    ]);
    let out = run(&Common::default(), &env, true).unwrap();
    let path = dir.join("systemd/user/backlot-stage.service");
    assert!(path.is_file(), "{out}");
    assert!(out.contains(&format!("wrote {}", path.display())), "{out}");
    assert!(
        out.contains("systemctl --user enable --now backlot-stage.service"),
        "{out}"
    );
    let written = std::fs::read_to_string(&path).unwrap();
    let exe = std::env::current_exe().unwrap();
    assert_eq!(
        written,
        render(&exe, Some("/home/user/state"), None, UNIT),
        "the file and stdout disagree"
    );
    assert!(written.contains("Environment=\"BACKLOT_STATE_DIR=/home/user/state\""));
}

#[test]
fn a_session_writes_its_own_unit_file() {
    let dir = TestDir::new("backlot-service-test");
    let env = env_with(&[("XDG_CONFIG_HOME", dir.to_str().unwrap())]);
    let common = Common {
        session: Some("run1".into()),
        ..Common::default()
    };
    let out = run(&common, &env, true).unwrap();
    assert!(
        dir.join("systemd/user/backlot-stage-run1.service")
            .is_file()
    );
    assert!(
        out.contains("enable --now backlot-stage-run1.service"),
        "{out}"
    );
}

#[test]
fn home_is_the_fallback_config_root() {
    let dir = TestDir::new("backlot-service-test");
    let env = env_with(&[("HOME", dir.to_str().unwrap())]);
    assert_eq!(
        config_dir(&env).unwrap(),
        dir.join(".config"),
        "the default config root is wrong"
    );
    let empty = Env {
        vars: BTreeMap::new(),
    };
    assert!(matches!(config_dir(&empty), Err(Fail::Refused(_))));
}
