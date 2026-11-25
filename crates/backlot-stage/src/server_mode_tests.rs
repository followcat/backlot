//! Fail-first tests for B3 single-server mode: `server run`,
//! `server start|stop`, locks & leases, `stage` exclusion, and `studio new --remote`.

use std::fs;
use std::time::{Duration, Instant};

use crate::cli::{self, Env, Fail};
use crate::cli_testkit::CAST_TOML;
use crate::server_fake_core::{FakeHcCore, base_env};
use crate::test_support::TestDir;

fn args(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn studio_new_with_remote_refuses_cleanly_naming_phase_2() {
    let dir = TestDir::new("bls-rem");
    let cast = dir.join("cast.toml");
    fs::write(&cast, CAST_TOML).unwrap();
    let cast_s = cast.to_string_lossy().into_owned();
    let env = Env {
        vars: base_env(&dir),
    };

    for case in [
        vec![
            "studio",
            "new",
            "--title",
            "Shop",
            "--repo",
            "/home/user/acme-shop",
            "--cast",
            &cast_s,
            "--remote",
            "host-a",
        ],
        vec!["studio", "new", "--interactive", "--remote", "host-a"],
        vec!["studio", "new", "--remote", "host-a"],
        vec!["--remote", "host-a", "studio", "new", "--interactive"],
    ] {
        let err = cli::run(&args(&case), &env).unwrap_err();
        let msg = err.message().to_string();
        assert_eq!(
            msg, "remote studio placement is Phase 2; create studios on the server host",
            "case={case:?}"
        );
    }
}

#[test]
fn server_help_lists_subcommands() {
    let env = Env {
        vars: base_env(&TestDir::new("bls-help")),
    };
    let out = cli::run(&args(&["server", "--help"]), &env).expect("server --help");
    for sub in ["run", "start", "stop", "adopt", "service"] {
        assert!(out.contains(sub), "help missing {sub}: {out}");
    }
}

#[test]
fn server_run_locks_writes_lease_runs_loop_and_blocks_stage_and_second_server() {
    let dir = TestDir::new("bls-run");
    let core = FakeHcCore::spawn(dir.join("hc.sock"), "core-a");
    let mut vars = base_env(&dir);
    vars.insert(
        "HC_SOCKET".into(),
        core.socket.to_string_lossy().into_owned(),
    );
    // Explicitly NOT setting BACKLOT_STAGE=1 — `server run` must work directly.
    assert!(!vars.contains_key("BACKLOT_STAGE"));

    let bg_env = Env { vars: vars.clone() };
    let handle = std::thread::spawn(move || cli::run(&args(&["server", "run"]), &bg_env));

    let server_dir = dir.join("xdg-state/backlot/server");
    let lease_path = server_dir.join("server-lease.json");
    let deadline = Instant::now() + Duration::from_secs(5);
    while (!lease_path.is_file() || !server_dir.join("live.json").is_file())
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        lease_path.is_file(),
        "server-lease.json missing under {}",
        server_dir.display()
    );
    assert!(
        server_dir.join("server.lock").is_file(),
        "server.lock missing under {}",
        server_dir.display()
    );
    assert!(server_dir.join("studios").is_dir());
    assert!(server_dir.join("hollywood").is_dir());
    assert!(server_dir.join("archive").is_dir());
    assert!(server_dir.join("live.json").is_file());

    let lease_val: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&lease_path).unwrap()).unwrap();
    assert_eq!(lease_val["session"], "backlot");
    assert!(lease_val["pid"].as_u64().unwrap() > 0);
    assert!(lease_val["ttl_ms"].as_u64().unwrap() >= 5_000);

    // Second `server run` refuses in any session on the same host.
    let env2 = Env { vars: vars.clone() };
    let second = cli::run(&args(&["server", "run", "--session", "other"]), &env2).unwrap_err();
    let msg2 = second.message();
    assert!(
        !msg2.contains('\n') && msg2.contains("backlot-server") && msg2.contains("backlot"),
        "unexpected second server run error: {msg2:?}"
    );

    // `stage start` in any local session refuses cleanly naming running server & session.
    let stage_err = cli::run(&args(&["stage", "start"]), &env2).unwrap_err();
    let stage_msg = stage_err.message();
    assert!(
        !stage_msg.contains('\n')
            && stage_msg.contains("backlot-server")
            && stage_msg.contains("backlot"),
        "unexpected stage start error: {stage_msg:?}"
    );

    // `stage run` with BACKLOT_STAGE=1 refuses identically.
    let mut vars_stage = vars.clone();
    vars_stage.insert("BACKLOT_STAGE".into(), "1".into());
    let run_err = cli::run(&args(&["stage", "run"]), &Env { vars: vars_stage }).unwrap_err();
    assert!(
        run_err.message().contains("backlot-server") && run_err.message().contains("backlot"),
        "unexpected stage run error: {}",
        run_err.message()
    );

    drop(core);
    let _ = handle.join().unwrap();
}

#[test]
fn server_run_honors_backlot_state_dir_and_ignores_stale_unlocked_lease() {
    let dir = TestDir::new("bls-stale");
    let custom = dir.join("custom-srv");
    fs::create_dir_all(&custom).unwrap();
    // Simulate a leftover lease + lock file from a crashed (SIGKILLed) server.
    fs::write(
        custom.join("server.lock"),
        "{\"pid\":999999,\"session\":\"old\"}",
    )
    .unwrap();
    fs::write(
        custom.join("server-lease.json"),
        "{\"pid\":999999,\"session\":\"old\",\"base\":\"/tmp\",\"started_ms\":1,\"renewed_ms\":9999999999999,\"ttl_ms\":20000}",
    )
    .unwrap();

    let core = FakeHcCore::spawn(dir.join("hc.sock"), "core-stale");
    let mut vars = base_env(&dir);
    vars.insert("BACKLOT_STATE_DIR".into(), custom.to_string_lossy().into());
    vars.insert("HC_SOCKET".into(), core.socket.to_string_lossy().into());

    let bg_env = Env { vars };
    let handle = std::thread::spawn(move || {
        cli::run(&args(&["server", "run", "--session", "work"]), &bg_env)
    });

    let deadline = Instant::now() + Duration::from_secs(5);
    while !custom.join("live.json").is_file() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        custom.join("live.json").is_file(),
        "daemon did not start under BACKLOT_STATE_DIR"
    );
    let lease: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(custom.join("server-lease.json")).unwrap())
            .unwrap();
    assert_eq!(lease["session"], "work");
    assert_eq!(
        lease["pid"].as_u64().unwrap(),
        u64::from(std::process::id())
    );

    drop(core);
    let _ = handle.join().unwrap();
}

#[test]
fn server_start_stop_mirrors_stage_start_stop_and_defaults_session_to_backlot() {
    let dir = TestDir::new("bls-ctl");
    let core = FakeHcCore::spawn(dir.join("hc.sock"), "core-ctl");
    let mut vars = base_env(&dir);
    vars.insert("HC_SOCKET".into(), core.socket.to_string_lossy().into());
    let env = Env { vars };

    let started = cli::run(&args(&["server", "start"]), &env).expect("server start");
    assert!(started.contains("backlot-server"), "{started}");
    let ran = core.ran();
    assert_eq!(ran.len(), 1);
    assert_eq!(ran[0].name.as_deref(), Some("backlot-server"));
    assert_eq!(
        ran[0].labels.get("backlot.role").map(String::as_str),
        Some("stage")
    );
    assert_eq!(
        ran[0].labels.get("herdr.tab").map(String::as_str),
        Some("server")
    );
    assert!(
        ran[0].argv.ends_with(&[
            "server".to_string(),
            "run".to_string(),
            "--session".to_string(),
            "backlot".to_string(),
        ]),
        "unexpected argv: {:?}",
        ran[0].argv
    );

    // Idempotent second `server start` on same session.
    let again = cli::run(&args(&["server", "start"]), &env).expect("idempotent server start");
    assert!(again.contains("already running"), "{again}");
    assert_eq!(core.ran().len(), 1);

    // `server stop` kills & rms only `backlot-server`, leaving other terminals alone.
    let stopped = cli::run(&args(&["server", "stop"]), &env).expect("server stop");
    assert!(stopped.contains("stopped"), "{stopped}");
    assert_eq!(core.killed(), vec!["t1"]);
    assert_eq!(core.rms(), vec!["t1"]);
    assert!(core.terminals().is_empty());

    let stopped_again = cli::run(&args(&["server", "stop"]), &env).expect("idempotent stop");
    assert!(stopped_again.contains("already stopped"), "{stopped_again}");
}

#[test]
fn server_start_with_custom_session_forwards_session() {
    let dir = TestDir::new("bls-sess");
    let core = FakeHcCore::spawn(dir.join("hc.sock"), "core-sess");
    let mut vars = base_env(&dir);
    vars.insert("HC_SOCKET".into(), core.socket.to_string_lossy().into());
    let env = Env { vars };

    let res = cli::run(&args(&["server", "start", "--session", "ci"]), &env);
    assert!(res.is_ok(), "unexpected result: {res:?}");
    let ran = core.ran();
    assert_eq!(ran.len(), 1);
    assert!(
        ran[0].argv.ends_with(&[
            "server".to_string(),
            "run".to_string(),
            "--session".to_string(),
            "ci".to_string(),
        ]),
        "{:?}",
        ran[0].argv
    );

    // Calling stop on a stopped session returns Ok, while usage errors are rejected.
    let bad = cli::run(&args(&["server", "stop", "--bogus"]), &env).unwrap_err();
    assert!(matches!(bad, Fail::Usage(_)), "{bad:?}");
}
