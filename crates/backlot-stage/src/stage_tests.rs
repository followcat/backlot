//! `stage start`'s `terminal.run` params, local and remote. The remote
//! shape is the fix for `s` on a remote endpoint: the stage terminal
//! runs on the core's host, where this machine's exe path, `--remote`
//! and `BACKLOT_STATE_DIR` all mean nothing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use hc_proto::endpoint::{Endpoint, Local, Remote};

use super::*;

fn env() -> Env {
    Env {
        vars: BTreeMap::from([
            ("BACKLOT_STATE_DIR".into(), "/home/alice/state".into()),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ]),
    }
}

fn local() -> Endpoint {
    Endpoint::Local(Local {
        session: "s1".into(),
        socket: PathBuf::from("/run/user/1000/hc/s.sock"),
        holder_socket: PathBuf::from("/run/user/1000/hc/s.holder.sock"),
        state: PathBuf::from("/home/alice/.local/state/hc/s"),
        config: PathBuf::from("/home/alice/.config/hc/config.toml"),
    })
}

fn remote() -> Endpoint {
    Endpoint::Remote(Remote {
        target: "host-a".into(),
        session: "s1".into(),
    })
}

#[test]
fn a_local_stage_runs_this_exe_with_the_state_dir() {
    let params = stage_params("/home/alice/.local/bin/backlot", &local(), &env());
    assert_eq!(
        params.argv,
        vec![
            "/home/alice/.local/bin/backlot",
            "stage",
            "run",
            "--session",
            "s1"
        ]
    );
    assert!(params.line.is_none());
    assert_eq!(
        params.env.get("BACKLOT_STATE_DIR").map(String::as_str),
        Some("/home/alice/state")
    );
    assert_eq!(
        params.env.get("BACKLOT_STAGE").map(String::as_str),
        Some("1")
    );
}

#[test]
fn a_remote_stage_runs_that_hosts_backlot_with_the_session_alone() {
    let params = stage_params("/home/alice/.local/bin/backlot", &remote(), &env());
    // No local path and no argv: the terminal's shell resolves the
    // host's own `backlot`.
    assert!(params.argv.is_empty(), "{:?}", params.argv);
    let line = params.line.as_deref().expect("a shell line");
    assert!(line.contains("command -v backlot"), "{line}");
    assert!(line.contains("backlot stage run --session 's1'"), "{line}");
    // `--remote` would loop back out; the target names nothing there.
    assert!(!line.contains("--remote"), "{line}");
    assert!(!line.contains("host-a"), "{line}");
    assert!(!line.contains("/home/alice"), "{line}");
    // The state dir is this machine's: meaningless on the core's host.
    assert!(
        !params.env.contains_key("BACKLOT_STATE_DIR"),
        "{:?}",
        params.env
    );
    assert_eq!(
        params.env.get("BACKLOT_STAGE").map(String::as_str),
        Some("1")
    );
}

#[test]
fn a_remote_stage_exits_127_when_the_host_has_no_backlot() {
    let params = stage_params("backlot", &remote(), &env());
    let line = params.line.expect("a shell line");
    assert!(line.starts_with("command -v backlot >/dev/null 2>&1 || exit 127;"));
}

#[test]
fn the_missing_backlot_message_names_the_target_and_the_fix() {
    assert_eq!(
        not_installed("host-a"),
        "backlot is not installed on host-a: install it there (curl -fsSL \
         https://github.com/followcat/backlot/releases/latest/download/install.sh | \
         BACKLOT_SKIP_CORE=1 sh) or run backlot on that host"
    );
}

#[test]
fn a_finished_start_is_judged_by_its_exit() {
    let term = |state: &str, exit: Option<i32>| hc_proto::terminal::Terminal {
        address: "t7".into(),
        name: Some(STAGE_NAME.into()),
        labels: BTreeMap::new(),
        argv: Vec::new(),
        cwd: "/home/user".into(),
        keep: false,
        state: state.into(),
        pid: None,
        exit_code: exit,
        signal: None,
        reason: Some("exited".into()),
        created_ms: 1,
        finished_ms: None,
        screen: "exact".into(),
        cols: 80,
        rows: 24,
        identity: String::new(),
        status: None,
        status_ms: None,
        records: BTreeMap::new(),
        turn: 0,
        reported: false,
        waiting_for_person: false,
        title: String::new(),
        parent: None,
        notify: false,
        die_with_parent: false,
        rm: false,
        template: None,
        first_message: None,
        submit: None,
        stop: None,
        paste_gap_ms: None,
        foreground: None,
        repository: None,
        activity: None,
        extra: Default::default(),
    };
    assert_eq!(probe_state(&term("live", None)), Probe::Live);
    assert_eq!(probe_state(&term("finished", Some(127))), Probe::Missing);
    assert_eq!(probe_state(&term("finished", Some(1))), Probe::Exited(1));
    assert_eq!(probe_state(&term("finished", None)), Probe::Exited(-1));
}

/// A scripted stage host: each probe pops the next state, `tick`
/// advances a fake clock by one poll interval, and `remove` records
/// the cleanup. No core and no sleeping.
struct FakeStage {
    probes: std::cell::RefCell<std::collections::VecDeque<Probe>>,
    elapsed: std::cell::Cell<Duration>,
    removed: std::cell::Cell<bool>,
}

impl FakeStage {
    fn new(probes: impl IntoIterator<Item = Probe>) -> Self {
        FakeStage {
            probes: std::cell::RefCell::new(probes.into_iter().collect()),
            elapsed: std::cell::Cell::new(Duration::ZERO),
            removed: std::cell::Cell::new(false),
        }
    }
}

impl StageHost for FakeStage {
    async fn probe(&self) -> Result<Probe, Fail> {
        Ok(self
            .probes
            .borrow_mut()
            .pop_front()
            .unwrap_or(Probe::Starting))
    }
    async fn remove(&self) {
        self.removed.set(true);
    }
    fn elapsed(&self) -> Duration {
        self.elapsed.get()
    }
    async fn tick(&self) {
        self.elapsed.set(self.elapsed.get() + POLL);
    }
}

/// `live, live, finished(127)` inside the settle window: the shell
/// exited before running `backlot`, so the host has none and the
/// finished terminal is removed.
#[tokio::test]
async fn a_live_then_missing_stage_is_removed() {
    let fake = FakeStage::new([Probe::Live, Probe::Live, Probe::Missing]);
    let err = await_remote_start(&fake, "host-a")
        .await
        .expect_err("missing backlot must not read as started");
    assert_eq!(err.message(), not_installed("host-a"));
    assert!(fake.removed.get(), "the finished terminal was not removed");
}

/// `live, finished(1)` is a stage that started and died: report the
/// exit and remove the record.
#[tokio::test]
async fn a_live_then_exited_stage_reports_the_code_and_is_removed() {
    let fake = FakeStage::new([Probe::Live, Probe::Exited(1)]);
    let err = await_remote_start(&fake, "host-a")
        .await
        .expect_err("a dead stage must not read as started");
    assert!(
        err.message().contains("exited with code 1"),
        "{}",
        err.message()
    );
    assert!(fake.removed.get(), "the finished terminal was not removed");
}

/// A stage that stays live through the settle window is started.
#[tokio::test]
async fn a_stage_live_through_the_settle_window_is_started() {
    let fake = FakeStage::new(std::iter::repeat_n(Probe::Live, 12));
    await_remote_start(&fake, "host-a")
        .await
        .expect("a live stage must start");
    assert!(
        fake.elapsed.get() >= SETTLE,
        "the start was accepted after {:?}, before the settle window",
        fake.elapsed.get()
    );
    assert!(!fake.removed.get(), "a live stage was removed");
}
