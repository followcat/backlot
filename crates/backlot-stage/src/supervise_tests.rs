//! The restart policy's clock-injected tests and the command-line
//! surface `stage watch --restart` and `stage service` add.

use super::*;
use crate::args::{Cmd, parse};
use crate::cli::{Env, Fail, check_stage_caller};

fn args(argv: &str) -> Vec<String> {
    argv.split_whitespace().map(String::from).collect()
}

#[test]
fn the_backoff_steps_two_four_eight_sixteen_thirty() {
    let mut p = RestartPolicy::new();
    assert_eq!(p.observe(false, 0), RestartStep::Start);
    assert_eq!(p.observe(false, 1_999), RestartStep::Wait);
    assert_eq!(p.observe(false, 2_000), RestartStep::Start);
    assert_eq!(p.observe(false, 5_999), RestartStep::Wait);
    assert_eq!(p.observe(false, 6_000), RestartStep::Start);
    assert_eq!(p.observe(false, 13_999), RestartStep::Wait);
    assert_eq!(p.observe(false, 14_000), RestartStep::Start);
    assert_eq!(p.observe(false, 29_999), RestartStep::Wait);
    assert_eq!(p.observe(false, 30_000), RestartStep::Start);
    // The fifth attempt spent the budget, so the cap only shows once
    // the window frees: the next start waits the last 30 s (the
    // sixth attempt's own delay would be the same 30 s).
    assert_eq!(p.observe(false, 59_999), RestartStep::Paused);
    assert_eq!(p.observe(false, 600_001), RestartStep::Start);
    assert_eq!(p.observe(false, 629_999), RestartStep::Wait);
    assert_eq!(p.observe(false, 630_001), RestartStep::Start);
}

#[test]
fn five_in_ten_minutes_pauses_once_and_recovers() {
    let mut p = RestartPolicy::new();
    for at in [0, 2_000, 6_000, 14_000, 30_000] {
        assert_eq!(p.observe(false, at), RestartStep::Start, "at {at}");
    }
    // The budget is spent: announce the pause once, then silence.
    assert_eq!(p.observe(false, 31_000), RestartStep::Paused);
    assert_eq!(p.observe(false, 32_000), RestartStep::Wait);
    assert_eq!(p.observe(false, 300_000), RestartStep::Wait);
    // The first attempt leaves the window one millisecond after
    // ten minutes: a slot frees and the restart runs again.
    assert_eq!(p.observe(false, 600_001), RestartStep::Start);
}

#[test]
fn the_stage_coming_back_ends_the_pause() {
    let mut p = RestartPolicy::new();
    for at in [0, 2_000, 6_000, 14_000, 30_000] {
        assert_eq!(p.observe(false, at), RestartStep::Start);
    }
    assert_eq!(p.observe(false, 31_000), RestartStep::Paused);
    // Live again on its own: no more pause announcements.
    assert_eq!(p.observe(true, 40_000), RestartStep::Wait);
    assert_eq!(p.observe(true, 41_000), RestartStep::Wait);
}

#[test]
fn a_stable_stage_resets_the_backoff() {
    let mut p = RestartPolicy::new();
    assert_eq!(p.observe(false, 0), RestartStep::Start);
    assert_eq!(p.observe(false, 2_000), RestartStep::Start);
    // The restart worked; a full minute live resets the backoff.
    assert_eq!(p.observe(true, 3_000), RestartStep::Wait);
    assert_eq!(p.observe(true, 63_000), RestartStep::Wait);
    // Dead again: an immediate start, then the 2-second first step
    // — not the 8-second one the old count would have asked for.
    assert_eq!(p.observe(false, 64_000), RestartStep::Start);
    assert_eq!(p.observe(false, 65_999), RestartStep::Wait);
    assert_eq!(p.observe(false, 66_000), RestartStep::Start);
}

#[test]
fn watch_parses_restart_and_the_target() {
    let (common, cmd) = parse(&args(
        "stage watch --restart --target bl-acme-shop-producer",
    ))
    .unwrap();
    assert_eq!(common.task, None);
    match cmd {
        Cmd::StageWatch { target, restart } => {
            assert!(restart);
            assert_eq!(target.as_deref(), Some("bl-acme-shop-producer"));
        }
        other => panic!("wrong command: {}", name(&other)),
    }
    let (_, cmd) = parse(&args("stage watch")).unwrap();
    match cmd {
        Cmd::StageWatch { target, restart } => {
            assert!(!restart, "restart defaults off");
            assert_eq!(target, None);
        }
        other => panic!("wrong command: {}", name(&other)),
    }
}

#[test]
fn service_parses_print_write_and_session() {
    let (_, cmd) = parse(&args("stage service")).unwrap();
    match cmd {
        Cmd::StageService { write } => assert!(!write, "the default is --print"),
        other => panic!("wrong command: {}", name(&other)),
    }
    let (common, cmd) = parse(&args("stage service --print --session run1")).unwrap();
    assert_eq!(common.session.as_deref(), Some("run1"));
    assert!(matches!(cmd, Cmd::StageService { write: false }));
    let (_, cmd) = parse(&args("stage service --write")).unwrap();
    assert!(matches!(cmd, Cmd::StageService { write: true }));
    // Both modes at once is a usage error, exit 2.
    let err = match parse(&args("stage service --print --write")) {
        Err(e) => e,
        Ok(_) => panic!("--print --write parsed"),
    };
    assert!(matches!(err, Fail::Usage(_)), "{err:?}");
    assert!(err.message().contains("--print"), "{}", err.message());
    // A systemd --user unit runs on the local host.
    let err = match parse(&args("stage service --remote host-b")) {
        Err(e) => e,
        Ok(_) => panic!("--remote parsed for a service unit"),
    };
    assert!(matches!(err, Fail::Usage(_)), "{err:?}");
    assert!(err.message().contains("--remote"), "{}", err.message());
}

#[test]
fn working_roles_cannot_run_the_stage_or_its_service() {
    let env = |role: &str| Env {
        vars: [("BACKLOT_ROLE".to_string(), role.to_string())]
            .into_iter()
            .collect(),
    };
    for role in ["actor-t1-c1", "writer", "hollywood"] {
        let refused = check_stage_caller(&Default::default(), &env(role));
        assert!(matches!(refused, Err(Fail::Refused(_))), "{role}");
    }
    for role in ["producer", "director", "user"] {
        assert!(check_stage_caller(&Default::default(), &env(role)).is_ok());
    }
}

/// The variant's name, for a readable failure.
fn name(cmd: &Cmd) -> &'static str {
    match cmd {
        Cmd::StageWatch { .. } => "StageWatch",
        Cmd::StageService { .. } => "StageService",
        _ => "other",
    }
}
