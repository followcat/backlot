//! Unit tests for `server_cfg`, hub-vs-remote endpoint resolution, and
//! `--server` CLI/TUI parsing.

use std::collections::BTreeMap;
use std::fs;

use hc_proto::endpoint::{Endpoint, Env as ProtoEnv, Sys};

use crate::args;
use crate::server_cfg::{
    DEFAULT_SERVER_SESSION, SERVER_ENV, ServerTarget, is_local_host_with, parse_target, resolve,
    resolve_endpoint_with,
};
use crate::test_support::TestDir;

fn env_with_config(dir: &TestDir, toml: Option<&str>) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    vars.insert(
        "XDG_CONFIG_HOME".into(),
        dir.as_ref().to_string_lossy().into_owned(),
    );
    vars.insert("HOME".into(), dir.as_ref().to_string_lossy().into_owned());
    if let Some(body) = toml {
        let cfg = dir.join("backlot").join("config.toml");
        fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        fs::write(cfg, body).unwrap();
    }
    vars
}

#[test]
fn cli_flag_beats_env_and_config() {
    let dir = TestDir::new("blsrv-cli");
    let mut vars = env_with_config(
        &dir,
        Some("[client]\nserver = \"host-c\"\nsession = \"cfg-sess\"\n"),
    );
    vars.insert(SERVER_ENV.into(), "host-b/env-sess".into());

    let got = resolve(Some("host-a/cli-sess"), &vars).unwrap();
    assert_eq!(
        got,
        Some(ServerTarget {
            host: "host-a".into(),
            session: "cli-sess".into(),
        })
    );

    // Explicit `--server <host>` without `/<session>` defaults to `backlot`,
    // never inheriting `[client] session`.
    let got_default = resolve(Some("host-a"), &vars).unwrap();
    assert_eq!(
        got_default,
        Some(ServerTarget {
            host: "host-a".into(),
            session: DEFAULT_SERVER_SESSION.into(),
        })
    );
}

#[test]
fn env_beats_config() {
    let dir = TestDir::new("blsrv-env");
    let mut vars = env_with_config(
        &dir,
        Some("[client]\nserver = \"host-c\"\nsession = \"cfg-sess\"\n"),
    );
    vars.insert(SERVER_ENV.into(), "host-b/env-sess".into());

    let got = resolve(None, &vars).unwrap();
    assert_eq!(
        got,
        Some(ServerTarget {
            host: "host-b".into(),
            session: "env-sess".into(),
        })
    );

    // Empty `BACKLOT_SERVER` does not block `[client]` config fallback.
    vars.insert(SERVER_ENV.into(), String::new());
    let from_cfg = resolve(None, &vars).unwrap();
    assert_eq!(
        from_cfg,
        Some(ServerTarget {
            host: "host-c".into(),
            session: "cfg-sess".into(),
        })
    );
}

#[test]
fn config_client_section_with_and_without_session_and_other_tables() {
    let dir = TestDir::new("blsrv-cfg");
    let vars = env_with_config(
        &dir,
        Some(
            "[panel]\nsort = \"urgent\"\n\n[client]\nserver = \"alice@host-a\"\n\n[hollywood]\nrunner = \"claude\"\n",
        ),
    );
    assert_eq!(
        resolve(None, &vars).unwrap(),
        Some(ServerTarget {
            host: "alice@host-a".into(),
            session: DEFAULT_SERVER_SESSION.into(),
        })
    );

    // `[client]` with no `server` leaves behavior unchanged (`None`).
    let dir_empty = TestDir::new("blsrv-empty");
    let vars_empty = env_with_config(&dir_empty, Some("[client]\nsession = \"backlot\"\n"));
    assert_eq!(resolve(None, &vars_empty).unwrap(), None);

    // Missing config file also yields `None`.
    let dir_missing = TestDir::new("blsrv-missing");
    let vars_missing = env_with_config(&dir_missing, None);
    assert_eq!(resolve(None, &vars_missing).unwrap(), None);
}

#[test]
fn parse_target_validates_host_and_session() {
    assert_eq!(
        parse_target("host-a", None).unwrap(),
        ServerTarget {
            host: "host-a".into(),
            session: "backlot".into(),
        }
    );
    assert_eq!(
        parse_target("alice@host-a/work", None).unwrap(),
        ServerTarget {
            host: "alice@host-a".into(),
            session: "work".into(),
        }
    );
    for bad in [
        "",
        "   ",
        "/backlot",
        "host-a/",
        "host-a/a/b",
        "host a/backlot",
    ] {
        assert!(
            parse_target(bad, None).is_err(),
            "accepted bad target '{bad}'"
        );
    }
}

#[test]
fn is_local_host_identifies_hub_vs_remote() {
    assert!(is_local_host_with(
        "localhost",
        Some("host-b"),
        Some("alice")
    ));
    assert!(is_local_host_with(
        "127.0.0.1",
        Some("host-b"),
        Some("alice")
    ));
    assert!(is_local_host_with("::1", Some("host-b"), Some("alice")));
    assert!(is_local_host_with("[::1]", Some("host-b"), Some("alice")));
    assert!(is_local_host_with("host-a", Some("host-a"), Some("alice")));
    assert!(is_local_host_with("HOST-A", Some("host-a"), Some("alice")));
    assert!(is_local_host_with(
        "host-a",
        Some("host-a.example.com"),
        Some("alice")
    ));
    assert!(is_local_host_with(
        "host-a.example.com",
        Some("host-a"),
        Some("alice")
    ));
    assert!(is_local_host_with(
        "alice@host-a",
        Some("host-a"),
        Some("alice")
    ));

    assert!(!is_local_host_with(
        "bob@host-a",
        Some("host-a"),
        Some("alice")
    ));
    assert!(!is_local_host_with("host-b", Some("host-a"), Some("alice")));
    assert!(!is_local_host_with(
        "host-a.other.net",
        Some("host-a.example.com"),
        Some("alice")
    ));
}

#[test]
fn endpoint_uses_local_on_hub_and_remote_off_hub() {
    let mut env = ProtoEnv::default();
    env.insert("XDG_RUNTIME_DIR".into(), "/tmp/run-test".into());
    let target = ServerTarget {
        host: "host-a".into(),
        session: "backlot".into(),
    };
    let on_hub = resolve_endpoint_with(&target, &env, &Sys, Some("host-a"), Some("alice")).unwrap();
    assert!(matches!(on_hub, Endpoint::Local(ref l) if l.session == "backlot"));

    let off_hub =
        resolve_endpoint_with(&target, &env, &Sys, Some("host-b"), Some("alice")).unwrap();
    assert!(matches!(
        off_hub,
        Endpoint::Remote(ref r) if r.target == "host-a" && r.session == "backlot"
    ));
}

#[test]
fn args_parse_accepts_leading_and_trailing_server_flag() {
    let (common, _) = args::parse(&[
        "--server".into(),
        "host-a/backlot".into(),
        "tell".into(),
        "--as".into(),
        "user".into(),
        "--studio".into(),
        "acme-shop".into(),
        "producer".into(),
        "--text".into(),
        "hi".into(),
    ])
    .expect("leading --server parses");
    assert_eq!(common.studio.as_deref(), Some("acme-shop"));
    assert_eq!(common.server.as_deref(), Some("host-a/backlot"));

    let (common_eq, _) = args::parse(&[
        "tell".into(),
        "--server=host-a".into(),
        "--as".into(),
        "user".into(),
        "--studio".into(),
        "acme-shop".into(),
        "producer".into(),
        "--text".into(),
        "hi".into(),
    ])
    .expect("trailing --server= parses");
    assert_eq!(common_eq.server.as_deref(), Some("host-a"));
}

#[test]
fn command_word_skips_leading_common_flags() {
    assert_eq!(
        crate::cli::command_word(&["--server".into(), "host-a/backlot".into(), "status".into(),]),
        Some(2)
    );
    assert_eq!(
        crate::cli::command_word(&["--server=host-a".into(), "tell".into()]),
        Some(1)
    );
    assert_eq!(
        crate::cli::command_word(&["--server".into(), "host-a/backlot".into()]),
        None
    );
}

#[test]
fn resolve_os_matches_resolve_precedence() {
    let dir = TestDir::new("blsrv-os");
    let _vars = env_with_config(
        &dir,
        Some("[client]\nserver = \"host-c\"\nsession = \"cfg-s\"\n"),
    );
    let mut env = ProtoEnv::default();
    env.insert(
        "XDG_CONFIG_HOME".into(),
        dir.as_ref().as_os_str().to_os_string(),
    );
    env.insert("HOME".into(), dir.as_ref().as_os_str().to_os_string());
    assert_eq!(
        crate::server_cfg::resolve_os(None, &env).unwrap(),
        Some(ServerTarget {
            host: "host-c".into(),
            session: "cfg-s".into(),
        })
    );
    env.insert(SERVER_ENV.into(), "host-b/env-s".into());
    assert_eq!(
        crate::server_cfg::resolve_os(None, &env).unwrap(),
        Some(ServerTarget {
            host: "host-b".into(),
            session: "env-s".into(),
        })
    );
    assert_eq!(
        crate::server_cfg::resolve_os(Some("host-a"), &env).unwrap(),
        Some(ServerTarget {
            host: "host-a".into(),
            session: DEFAULT_SERVER_SESSION.into(),
        })
    );
}
