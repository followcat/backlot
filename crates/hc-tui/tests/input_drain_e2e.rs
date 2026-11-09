//! Opt-in real-core end-to-end for the input-drain close budget: the
//! actual `hc-tui --remote` binary in a PTY, a fake `ssh` that runs
//! `hc connect` locally, and ten one-tab scratch terminals. A held
//! function key pressed on each visited wire pins its parked route, so
//! detach bursts one drain per route back to back and the ninth
//! overflows the close budget. A dropped route may lose queued input;
//! the client must still exit cleanly, the way the reference's
//! revoked connection recovers through the normal lifecycle.
//!
//! Ignored by default: `cargo test` never touches a real core. Run
//! explicitly with
//!
//! ```sh
//! HC_TUI_TEST_CORE=$(command -v hc) \
//!     cargo test -p hc-tui --test input_drain_e2e -- --ignored --test-threads=1
//! ```

#![cfg(unix)]

#[path = "common/pty.rs"]
mod pty;
#[path = "../../hc-proto/tests/common/scratch.rs"]
mod scratch;

use scratch::ScratchCore;

use hc_proto::client::Client;
use hc_proto::endpoint::Endpoint;
use hc_proto::terminal::RunParams;
use hc_vt::Vt;
use pty::{Pty, screen};

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const COLS: u16 = 120;
const ROWS: u16 = 34;
const DEADLINE: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(40);

/// xterm encodings for F1..F9 — nine distinct physical presses, so
/// each visited wire keeps its own held key and forwarded lease.
const FKEYS: [&[u8]; 9] = [
    b"\x1bOP",
    b"\x1bOQ",
    b"\x1bOR",
    b"\x1bOS",
    b"\x1b[15~",
    b"\x1b[17~",
    b"\x1b[18~",
    b"\x1b[19~",
    b"\x1b[20~",
];

fn tui_bin() -> &'static str {
    env!("CARGO_BIN_EXE_hc-tui")
}

/// Write a PATH stub and probe-exec it until the file is reliably
/// executable: a fork elsewhere in the process can briefly pin the
/// just-written script's writer fd and a racing `exec` fails with
/// ETXTBSY. The `STUB_PROBE` line makes each probe run a no-op.
fn write_executable(path: &Path, body: &str) {
    fs::write(
        path,
        format!("#!/bin/sh\n[ -n \"$STUB_PROBE\" ] && exit 0\n{body}"),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    for _ in 0..200 {
        match Command::new(path)
            .env("STUB_PROBE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Ok(_) => return,
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => panic!("cannot exec stub {}: {e}", path.display()),
        }
    }
    panic!("stub {} never became executable", path.display());
}

/// The environment `hc-tui --remote` sees: the scratch fixture's own
/// plus the stub dir ahead of PATH.
fn tui_env(scratch: &ScratchCore, stub: &Path) -> Vec<(String, String)> {
    let root = |dir: &str| scratch.root.join(dir).to_string_lossy().into_owned();
    let Endpoint::Local(local) = &scratch.endpoint else {
        panic!("scratch endpoint is always local");
    };
    vec![
        ("PATH".into(), format!("{}:/usr/bin:/bin", stub.display())),
        ("LANG".into(), "C.UTF-8".into()),
        ("TERM".into(), "xterm-256color".into()),
        ("HOME".into(), root("home")),
        ("XDG_STATE_HOME".into(), root("state")),
        ("XDG_CONFIG_HOME".into(), root("config")),
        ("XDG_RUNTIME_DIR".into(), root("run")),
        ("HC_SESSION".into(), scratch.session.clone()),
        (
            "HC_SOCKET".into(),
            local.socket.to_string_lossy().into_owned(),
        ),
    ]
}

async fn connect(scratch: &ScratchCore) -> Client {
    Client::connect(&scratch.endpoint, None::<&std::ffi::OsStr>)
        .await
        .expect("connect to scratch core")
}

/// One scratch terminal on its own tab: prints a mark once, then a
/// `cat` stays live so the wire keeps accepting input.
async fn run_tab(scratch: &ScratchCore, client: &Client, i: usize) {
    client
        .terminal_run(RunParams {
            argv: vec![
                "sh".into(),
                "-c".into(),
                format!("printf 'MARK-{i}\\n'; exec cat"),
            ],
            name: Some(format!("drain-{i}")),
            cwd: Some(scratch.work.to_string_lossy().into_owned()),
            labels: BTreeMap::from([
                ("herdr.space".into(), "main".into()),
                ("herdr.tab".into(), format!("tab{i}")),
                ("herdr.seen".into(), "0".into()),
            ]),
            ..RunParams::default()
        })
        .await
        .unwrap_or_else(|e| panic!("terminal.run drain-{i}: {e}"));
}

/// Pump for `dur` — keeps the screen and raw log current while the
/// app processes keys and repaints.
async fn settle(tui: &Pty, vt: &mut Vt, log: &mut Vec<u8>, dur: Duration) {
    let deadline = Instant::now() + dur;
    while Instant::now() < deadline {
        tui.pump_log(vt, log);
        tokio::time::sleep(POLL).await;
    }
}

/// The `MARK-N` text of whichever terminal's pane is showing — the
/// mark sits at row zero, so it is visible exactly while its wire
/// owns the pane.
fn mark_on_screen(vt: &Vt) -> Option<String> {
    let text = screen(vt);
    let at = text.find("MARK-")?;
    let digits: String = text[at + 5..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    (!digits.is_empty()).then(|| format!("MARK-{digits}"))
}

/// Pump until a wire different from `previous` shows its mark — the
/// new attach painted. Bounded, always.
async fn until_fresh_mark(
    tui: &Pty,
    vt: &mut Vt,
    log: &mut Vec<u8>,
    previous: Option<&str>,
) -> String {
    let deadline = Instant::now() + DEADLINE;
    loop {
        tui.pump_log(vt, log);
        if let Some(mark) = mark_on_screen(vt)
            && Some(mark.as_str()) != previous
        {
            return mark;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for a fresh pane mark; screen was:\n{}",
            screen(vt)
        );
        tokio::time::sleep(POLL).await;
    }
}

#[tokio::test]
#[ignore = "needs HC_TUI_TEST_CORE: a real pinned herdr-core binary"]
async fn detach_with_many_held_parked_routes_exits_cleanly() {
    let scratch = ScratchCore::spawn().await;
    let client = connect(&scratch).await;

    // The stub dir: `hc` (the pinned binary) and a fake `ssh` that runs
    // the remote command (its last argument) locally, with the
    // protocol stream passing straight through.
    let stub = scratch.root.join("stub");
    fs::create_dir(&stub).unwrap();
    std::os::unix::fs::symlink(scratch::core_binary(), stub.join("hc")).unwrap();
    write_executable(
        &stub.join("ssh"),
        "for last; do :; done\nexec sh -c \"$last\"\n",
    );

    // Ten terminals, one tab each — `prefix+n` cycles through them.
    for i in 0..10 {
        run_tab(&scratch, &client, i).await;
    }

    let mut tui = Pty::spawn(
        Path::new(tui_bin()),
        &["--remote", "host-a"],
        &tui_env(&scratch, &stub),
        &scratch.work,
        COLS,
        ROWS,
    );
    let mut vt = Vt::new(COLS, ROWS, 4096);
    let mut log = Vec::new();
    let deadline = Instant::now() + DEADLINE;
    while !screen(&vt).contains("agents") {
        tui.pump_log(&mut vt, &mut log);
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the initial UI render; screen was:\n{}",
            screen(&vt)
        );
        tokio::time::sleep(POLL).await;
    }

    // Visit nine tabs, pressing a distinct held function key on each:
    // the press forwards, xterm never reports a release, so the wire
    // keeps its held key and forwarded lease and stays parked. Detach
    // then starts all nine drains at once behind the live wire.
    let mut previous = until_fresh_mark(&tui, &mut vt, &mut log, None).await;
    for fkey in FKEYS {
        settle(&tui, &mut vt, &mut log, Duration::from_millis(150)).await;
        tui.type_keys(fkey);
        settle(&tui, &mut vt, &mut log, Duration::from_millis(120)).await;
        tui.type_keys(b"\x02n"); // prefix+n: next tab
        previous = until_fresh_mark(&tui, &mut vt, &mut log, Some(&previous)).await;
    }
    assert!(
        previous.starts_with("MARK-"),
        "the tenth wire painted: {previous}"
    );
    settle(&tui, &mut vt, &mut log, Duration::from_millis(400)).await;

    // Detach: the drain burst evicts the over-budget route — input
    // loss on that route, never a client failure.
    tui.type_keys(b"\x02q"); // prefix+q
    let deadline = Instant::now() + DEADLINE;
    let status = loop {
        tui.pump_log(&mut vt, &mut log);
        match tui.child.try_wait().expect("try_wait") {
            Some(status) => break status,
            None => {
                assert!(
                    Instant::now() < deadline,
                    "hc-tui did not exit after detach"
                );
                tokio::time::sleep(POLL).await;
            }
        }
    };
    assert!(
        status.success(),
        "hc-tui exited with {status}; tail: {}",
        String::from_utf8_lossy(&log[log.len().saturating_sub(2000)..])
    );
    assert!(
        !String::from_utf8_lossy(&log).contains("Input drain limit exceeded"),
        "the drain budget ended the client"
    );

    drop(client);
    drop(tui);
    scratch.finish().expect("scratch teardown");
}
