//! A cancelled `Attach::connect` owns the armed open cleanup: pending
//! calls fail, both halves drop and the ssh child is killed and reaped —
//! proven with `ESRCH`, not just a missing process. Fake `ssh` programs
//! are tiny scripts in scratch dirs; every wait is bounded.
#![cfg(unix)]

mod common;

use common::attach::params;
use common::fakecore;
use hc_proto::client::{Attach, AttachConn, AttachError};
use hc_proto::endpoint::{Endpoint, Remote};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(15);

async fn timeout<F: std::future::Future>(f: F) -> F::Output {
    tokio::time::timeout(DEADLINE, f)
        .await
        .expect("deadline exceeded")
}

/// The shared fake-ssh script and the files it publishes.
struct Fixtures {
    dir: PathBuf,
    pidfile: PathBuf,
    attach_seen: PathBuf,
}

/// Written once per process before any test spawns a child — a forked
/// child elsewhere can briefly inherit a script's open writer fd, making
/// a later `exec` fail with ETXTBSY; every spawn path calls `fake()`
/// first, which only returns after the synchronized init finished all
/// writes.
fn fixtures() -> &'static Fixtures {
    static FIXTURES: OnceLock<Fixtures> = OnceLock::new();
    FIXTURES.get_or_init(|| {
        let dir = fixture_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let pidfile = dir.join("ssh.pid");
        let attach_seen = dir.join("attach.seen");
        // Publishes its pid first — before hello — so a fast cancellation
        // can never kill it before the file exists; answers hello once
        // (advertising terminal.attach), then reads one byte of the framed
        // attach request and publishes the attach-stage marker before
        // sleeping: seeing the marker proves the client already sent the
        // attach RPC, which only happens inside the armed WireCells
        // window (the call runs after the tasks it owns spawn).
        let reply = serde_json::to_string(&json!({
            "jsonrpc": "2.0", "id": 0,
            "result": fakecore::hello_result(fakecore::ALL_METHODS),
        }))
        .unwrap();
        let path = dir.join("ssh-attach-sleep");
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf '%s' $$ > '{pid}'\nIFS= read -r line\nprintf '%s\\n' '{reply}'\ndd bs=1 count=1 of=/dev/null 2>/dev/null\nprintf x > '{seen}'\nexec sleep 60\n",
                pid = pidfile.display(),
                seen = attach_seen.display(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        // stdout closes before any hello reply, then the child ignores
        // stdin EOF: a hung ssh the EOF-grace close must reap promptly.
        let write_eof = |name: &str, answer_hello: bool| {
            let path = dir.join(name);
            let hello = if answer_hello {
                format!("IFS= read -r line\nprintf '%s\\n' '{reply}'\n")
            } else {
                String::new()
            };
            std::fs::write(
                &path,
                format!(
                    "#!/bin/sh\nprintf '%s' $$ > '{pid}'\n{hello}exec 1>&-\nexec sleep 60\n",
                    pid = dir.join(format!("{name}.pid")).display(),
                ),
            )
            .unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        write_eof("attach-eof-hello", false);
        // Answers hello framed, then closes stdout and ignores stdin EOF —
        // the shared wire's reader EOF must reap the hung child promptly.
        write_eof("attach-eof-live", true);
        unsafe {
            libc::atexit(remove_fixtures);
        }
        Fixtures {
            dir,
            pidfile,
            attach_seen,
        }
    })
}

fn fixture_dir() -> PathBuf {
    std::env::temp_dir().join(format!("hc-tui-attach-{}", std::process::id()))
}

extern "C" fn remove_fixtures() {
    let _ = std::fs::remove_dir_all(fixture_dir());
}

/// The path to the shared fake-ssh script.
fn fake(name: &str) -> PathBuf {
    fixtures().dir.join(name)
}

/// `kill(pid, 0)` + `ESRCH` — portable "dead and reaped" proof.
fn reaped(pid: i32) -> bool {
    let err = unsafe { libc::kill(pid, 0) };
    err == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

async fn deadline_wait(done: impl Fn() -> bool) {
    timeout(async {
        while !done() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
}

/// Reads a case's own `{name}.pid` file once its script published it —
/// the returned pid is captured immutably so a reaping poll can never be
/// redirected by a parallel test's child.
async fn pid_of(case: &str) -> i32 {
    let path = fixtures().dir.join(format!("{case}.pid"));
    deadline_wait(|| path.exists()).await;
    std::fs::read_to_string(path)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

/// Aborts the spawned connect if the test fails before awaiting it —
/// a leaked task would hold the ssh child past the test.
struct AbortOnDrop(Option<tokio::task::JoinHandle<()>>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if let Some(task) = &self.0 {
            task.abort();
        }
    }
}

#[tokio::test]
async fn cancelled_open_kills_and_reaps_the_ssh_child() {
    let pidfile = fixtures().pidfile.clone();
    let marker = fixtures().attach_seen.clone();
    let ssh = fake("ssh-attach-sleep");
    let endpoint = endpoint();
    let mut opening = AbortOnDrop(Some(tokio::spawn(async move {
        let p = params();
        let _ = Attach::connect(&endpoint, Some(ssh.as_os_str()), &p).await;
    })));
    // The fake only publishes its marker after reading a byte of the
    // framed attach request — the armed WireCells window is definitely
    // live, unlike a fixed-delay cancel that could land during hello.
    deadline_wait(|| marker.exists()).await;
    let start = std::time::Instant::now();
    let task = opening.0.take().unwrap();
    task.abort();
    // Awaiting after abort means the future's destructors ran: the armed
    // WireCells cleanup already killed the child before this returns.
    let cancelled = timeout(task).await;
    assert!(
        cancelled
            .expect_err("the open task had to be cancelled")
            .is_cancelled(),
        "the open task had to be cancelled"
    );
    let pid: i32 = std::fs::read_to_string(&pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    deadline_wait(|| reaped(pid)).await;
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "cancelled open waited near CLOSE_TIMEOUT: {:?}",
        start.elapsed()
    );
}

#[tokio::test]
async fn a_stdout_eof_open_failure_kills_the_child_promptly() {
    // The child closes stdout without answering hello, then ignores
    // stdin EOF: the failed open must not wait the full close timeout —
    // the EOF-grace close kills and reaps it promptly.
    let ssh = fake("attach-eof-hello");
    let conn = AttachConn::new(endpoint(), Some(ssh.into_os_string()));
    let start = std::time::Instant::now();
    let err = timeout(conn.attach(&params())).await.unwrap_err();
    assert!(matches!(err, AttachError::Open(_)), "{err:?}");
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "open EOF waited on a hung child: {:?}",
        start.elapsed()
    );
    let pid = pid_of("attach-eof-hello").await;
    deadline_wait(|| reaped(pid)).await;
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "child {pid} was still running: {:?}",
        start.elapsed()
    );
}

#[tokio::test]
async fn a_live_wire_eof_reports_attach_failure_promptly() {
    // hello succeeded, then the child closes stdout and ignores stdin
    // EOF: `diagnose` awaits the spawned backend close, so a hung child
    // must die inside the EOF grace or attach error reporting blocks.
    let ssh = fake("attach-eof-live");
    let conn = AttachConn::new(endpoint(), Some(ssh.into_os_string()));
    let start = std::time::Instant::now();
    let err = timeout(conn.attach(&params())).await.unwrap_err();
    assert!(matches!(err, AttachError::Open(_)), "{err:?}");
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "diagnose waited on a hung child: {:?}",
        start.elapsed()
    );
    let pid = pid_of("attach-eof-live").await;
    deadline_wait(|| reaped(pid)).await;
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "child {pid} was still running: {:?}",
        start.elapsed()
    );
}

fn endpoint() -> Endpoint {
    Endpoint::Remote(Remote {
        target: "test-host".into(),
        session: "test".into(),
    })
}
