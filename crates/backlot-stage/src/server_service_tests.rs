//! Fail-first tests for `backlot server service [--print | --write] [--session <s>]`.

use std::fs;

use crate::cli::{self, Env, Fail};
use crate::server_fake_core::base_env;
use crate::test_support::TestDir;

fn args(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn server_service_print_uses_restart_always_restart_sec_2_and_default_session_backlot() {
    let dir = TestDir::new("blsvc-p");
    let env = Env {
        vars: base_env(&dir),
    };
    let out = cli::run(&args(&["server", "service", "--print"]), &env).expect("service --print");
    assert!(out.contains("[Unit]"), "{out}");
    assert!(out.contains("Restart=always"), "{out}");
    assert!(out.contains("RestartSec=2"), "{out}");
    assert!(out.contains("server run --session backlot"), "{out}");
    assert!(!out.contains("BACKLOT_STAGE"), "{out}");
    assert!(!dir.join("xdg-config/systemd").exists());
}

#[test]
fn server_service_write_writes_default_and_named_units_with_state_dir() {
    let dir = TestDir::new("blsvc-w");
    let env = Env {
        vars: base_env(&dir),
    };

    let out = cli::run(&args(&["server", "service", "--write"]), &env).expect("service --write");
    let default_unit = dir.join("xdg-config/systemd/user/backlot-server.service");
    assert!(default_unit.is_file(), "missing {}", default_unit.display());
    assert!(
        !dir.join("xdg-config/systemd/user/backlot-server-backlot.service")
            .exists()
    );
    let content = fs::read_to_string(&default_unit).unwrap();
    assert!(content.contains("Restart=always"), "{content}");
    assert!(content.contains("RestartSec=2"), "{content}");
    assert!(
        content.contains("server run --session backlot"),
        "{content}"
    );
    assert!(out.contains("systemctl --user daemon-reload"), "{out}");
    assert!(
        out.contains("systemctl --user enable --now backlot-server.service"),
        "{out}"
    );

    // Custom session + BACKLOT_STATE_DIR override.
    let mut vars2 = base_env(&dir);
    vars2.insert("BACKLOT_STATE_DIR".into(), "/var/lib/backlot srv".into());
    let out2 = cli::run(
        &args(&["server", "service", "--write", "--session", "ci"]),
        &Env { vars: vars2 },
    )
    .expect("named service --write");
    let ci_unit = dir.join("xdg-config/systemd/user/backlot-server-ci.service");
    assert!(ci_unit.is_file(), "missing {}", ci_unit.display());
    let ci_content = fs::read_to_string(&ci_unit).unwrap();
    assert!(
        ci_content.contains("server run --session ci"),
        "{ci_content}"
    );
    assert!(
        ci_content.contains("Environment=\"BACKLOT_STATE_DIR=/var/lib/backlot srv\""),
        "{ci_content}"
    );
    assert!(
        out2.contains("systemctl --user enable --now backlot-server-ci.service"),
        "{out2}"
    );
}

#[test]
fn server_service_refuses_bad_arguments_and_remote() {
    let dir = TestDir::new("blsvc-bad");
    let env = Env {
        vars: base_env(&dir),
    };

    let both = cli::run(&args(&["server", "service", "--print", "--write"]), &env).unwrap_err();
    assert!(matches!(both, Fail::Usage(_)), "{both:?}");

    let bad_sess = cli::run(&args(&["server", "service", "--session", "a/b"]), &env).unwrap_err();
    assert!(matches!(bad_sess, Fail::Usage(_)), "{bad_sess:?}");

    let remote = cli::run(&args(&["server", "service", "--remote", "host-a"]), &env).unwrap_err();
    assert!(matches!(remote, Fail::Usage(_)), "{remote:?}");
}
