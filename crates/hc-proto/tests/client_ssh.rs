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
use std::path::PathBuf;
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(15);

async fn timeout<F: std::future::Future>(f: F) -> F::Output {
    tokio::time::timeout(DEADLINE, f)
        .await
        .expect("deadline exceeded")
}

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("hc-tui-hello-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }
    /// A fresh executable fake-ssh script; the writer is closed before
    /// anything can spawn it.
    fn script(&self, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = self.0.join(name);
        std::fs::write(&path, body).unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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
    let dir = Dir::new("exit");
    let ssh = dir.script(
        "ssh-exit",
        "#!/bin/sh\necho 'Permission denied (publickey).' >&2\nexit 13\n",
    );
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
    let dir = Dir::new("rpc");
    // Answers hello with a structured error, then dies nonzero: the
    // Rpc error must survive, not become "ssh exited 42".
    let ssh = dir.script(
        "ssh-rpc-error",
        "#!/bin/sh\nIFS= read -r line\nprintf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":0,\"error\":{\"code\":-32000,\"message\":\"unsupported_generation\",\"data\":{\"kind\":\"unsupported_generation\",\"hint\":\"speak generation 1\"}}}'\nexit 42\n",
    );
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
    let dir = Dir::new("cancel");
    let pidfile = dir.0.join("ssh.pid");
    // Answers hello, records its pid, then never speaks again.
    let reply = serde_json::to_string(&json!({
        "jsonrpc": "2.0", "id": 0,
        "result": fakecore::hello_result(&["terminal.list"]),
    }))
    .unwrap();
    let script = format!(
        "#!/bin/sh\nIFS= read -r line\nprintf '%s\\n' '{reply}'\nprintf '%s' $$ > '{}'\nexec sleep 60\n",
        pidfile.display()
    );
    let ssh = dir.script("ssh-sleep", &script);
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
    let dir = Dir::new("eof");
    // Replies garbage, then waits for stdin EOF to exit 42: if the codec
    // halves were not dropped before `guard.close`, the child only dies
    // at the 10s kill timeout.
    let ssh = dir.script(
        "ssh-garbage",
        "#!/bin/sh\nprintf 'garbage\\n'\ncat >/dev/null\nexit 42\n",
    );
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
    let dir = Dir::new("gen");
    let reply = serde_json::to_string(&json!({
        "jsonrpc": "2.0", "id": 0,
        "result": fakecore::hello_result(fakecore::ALL_METHODS),
    }))
    .unwrap()
    .replace("\"generation\":1", "\"generation\":2");
    let script = format!("#!/bin/sh\nIFS= read -r line\nprintf '%s\\n' '{reply}'\nexit 9\n");
    let ssh = dir.script("ssh-gen2", &script);
    let err = timeout(Client::connect(&endpoint(), Some(ssh.as_os_str())))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Generation(_)), "{err:?}");
}
