//! `hello` failures over a real ssh child keep the child's own
//! diagnostics, a real protocol answer is never masked by cleanup
//! status, and a cancelled close still reaps the child. Fake `ssh`
//! programs are tiny scripts in scratch dirs; every wait is bounded.
#![cfg(unix)]

mod common;

use common::fakecore;
use hc_proto::client::{CallError, Client, Error};
use hc_proto::endpoint::{Endpoint, Remote};
use hc_proto::jsonrpc::error_kind;
use hc_proto::terminal::ListParams;
use hc_proto::transport;
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

/// The shared fake-ssh scripts and the pid file `ssh-sleep` publishes.
struct Fixtures {
    dir: PathBuf,
    pidfile: PathBuf,
}

/// Every fake-ssh script, written once per process before any test spawns
/// a child. A forked child elsewhere in the process can briefly inherit a
/// script's open writer fd, making a later `exec` fail with ETXTBSY; every
/// spawn path calls `fake()`/`fixtures()` first, which only returns after
/// the single synchronized init finished all writes.
fn fixtures() -> &'static Fixtures {
    static FIXTURES: OnceLock<Fixtures> = OnceLock::new();
    FIXTURES.get_or_init(|| {
        let dir = fixture_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let pidfile = dir.join("ssh.pid");
        let write = |name: &str, body: String| {
            let path = dir.join(name);
            std::fs::write(&path, body).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        // Auth-style failure: a stderr message, then nonzero exit.
        write(
            "ssh-exit",
            "#!/bin/sh\necho 'Permission denied (publickey).' >&2\nexit 13\n".into(),
        );
        // A structured hello error, then nonzero exit.
        write(
            "ssh-rpc-error",
            "#!/bin/sh\nIFS= read -r line\nprintf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":0,\"error\":{\"code\":-32000,\"message\":\"unsupported_generation\",\"data\":{\"kind\":\"unsupported_generation\",\"hint\":\"speak generation 1\"}}}'\nexit 42\n".into(),
        );
        // Publishes its pid first — before hello — so a fast cancellation
        // can never kill it before the file exists; answers hello once.
        let reply = serde_json::to_string(&json!({
            "jsonrpc": "2.0", "id": 0,
            "result": fakecore::hello_result(&["terminal.list"]),
        }))
        .unwrap();
        write(
            "ssh-sleep",
            format!(
                "#!/bin/sh\nprintf '%s' $$ > '{}'\nIFS= read -r line\nprintf '%s\\n' '{reply}'\nexec sleep 60\n",
                pidfile.display()
            ),
        );
        // Garbage instead of a hello reply, then waits on stdin EOF.
        write(
            "ssh-garbage",
            "#!/bin/sh\nprintf 'garbage\\n'\ncat >/dev/null\nexit 42\n".into(),
        );
        // A generation-2 hello reply, then nonzero exit.
        let reply = serde_json::to_string(&json!({
            "jsonrpc": "2.0", "id": 0,
            "result": fakecore::hello_result(fakecore::ALL_METHODS),
        }))
        .unwrap()
        .replace("\"generation\":1", "\"generation\":2");
        write(
            "ssh-gen2",
            format!("#!/bin/sh\nIFS= read -r line\nprintf '%s\\n' '{reply}'\nexit 9\n"),
        );
        unsafe {
            libc::atexit(remove_fixtures);
        }
        Fixtures { dir, pidfile }
    })
}

fn fixture_dir() -> PathBuf {
    std::env::temp_dir().join(format!("hc-tui-hello-{}", std::process::id()))
}

extern "C" fn remove_fixtures() {
    let _ = std::fs::remove_dir_all(fixture_dir());
}

/// The path to a shared fake-ssh script.
fn fake(name: &str) -> PathBuf {
    fixtures().dir.join(name)
}

fn endpoint() -> Endpoint {
    Endpoint::Remote(Remote {
        target: "test-host".into(),
        session: "test".into(),
    })
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

#[tokio::test]
async fn a_dead_childs_status_and_stderr_survive_hello_eof() {
    let ssh = fake("ssh-exit");
    let err = timeout(Client::connect(&endpoint(), Some(ssh.as_os_str())))
        .await
        .unwrap_err();
    let Error::Transport(transport::Error::Exit {
        status,
        stderr_tail,
        ..
    }) = err
    else {
        panic!("expected ssh Exit diagnostics, got {err:?}");
    };
    assert_eq!(status.code(), Some(13));
    assert!(stderr_tail.contains("Permission denied"), "{stderr_tail}");
}

#[tokio::test]
async fn a_protocol_error_is_not_masked_by_cleanup() {
    // Answers hello with a structured error, then dies nonzero: the
    // Rpc error must survive, not become "ssh exited 42".
    let ssh = fake("ssh-rpc-error");
    let err = timeout(Client::connect(&endpoint(), Some(ssh.as_os_str())))
        .await
        .unwrap_err();
    let Error::Rpc(e) = err else {
        panic!("expected the core's Rpc error, got {err:?}");
    };
    assert!(e.is_kind(error_kind::UNSUPPORTED_GENERATION));
}

#[tokio::test]
async fn cancelled_close_reaps_the_ssh_child() {
    let pidfile = fixtures().pidfile.clone();
    let ssh = fake("ssh-sleep");
    let client = timeout(Client::connect(&endpoint(), Some(ssh.as_os_str())))
        .await
        .unwrap();
    let survivor = client.clone();

    let pending = tokio::spawn({
        let client = client.clone();
        async move { client.terminal_list(ListParams::default()).await }
    });
    timeout(async {
        while client.in_flight() == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await;
    // Cancel the close mid-shutdown: the guard was already owned, so
    // its drop kills and reaps the child even though a clone lives.
    let close = tokio::spawn(client.close());
    tokio::task::yield_now().await;
    close.abort();

    let err = timeout(pending).await.unwrap().unwrap_err();
    assert!(matches!(err, CallError::Closed), "{err:?}");
    assert!(survivor.is_closed());
    let pid: i32 = std::fs::read_to_string(&pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    deadline_wait(|| reaped(pid)).await;
}

#[tokio::test]
async fn hello_failure_eof_stdin_before_guard_close() {
    // Replies garbage, then waits for stdin EOF to exit 42: if the codec
    // halves were not dropped before `guard.close`, the child only dies
    // at the 10s kill timeout.
    let ssh = fake("ssh-garbage");
    let start = std::time::Instant::now();
    let err = timeout(Client::connect(&endpoint(), Some(ssh.as_os_str())))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Shape(_)), "{err:?}");
    assert!(
        start.elapsed() < Duration::from_secs(8),
        "stdin EOF let the child exit promptly: {:?}",
        start.elapsed()
    );
}

#[tokio::test]
async fn a_generation_mismatch_is_not_masked_by_cleanup() {
    let ssh = fake("ssh-gen2");
    let err = timeout(Client::connect(&endpoint(), Some(ssh.as_os_str())))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Generation(_)), "{err:?}");
}
