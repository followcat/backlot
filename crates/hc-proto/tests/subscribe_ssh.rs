//! The subscription owns its ssh child: a drop, a *failed* read and a
//! cancelled open all kill and reap it — proven with `ESRCH`, not just a
//! missing process — and `close` first releases the child's stdout so a
//! peer blocked on a full pipe dies on EPIPE instead of holding the
//! close until the kill deadline. Fake `ssh` programs are tiny scripts
//! in scratch dirs; every wait is bounded.
#![cfg(unix)]

mod common;

use common::fakecore;
use common::subscribe::{self, DEADLINE, params};
use hc_proto::endpoint::{Endpoint, Remote};
use hc_proto::subscribe::{SubscribeError, Subscription};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

async fn timeout<F: std::future::Future>(f: F) -> F::Output {
    tokio::time::timeout(DEADLINE, f)
        .await
        .expect("deadline exceeded")
}

/// Written once per process before any test spawns a child — a forked
/// child elsewhere can briefly inherit a script's open writer fd, making
/// a later `exec` fail with ETXTBSY; every spawn path calls `fake()`
/// first, which only returns after the synchronized init finished all
/// writes.
fn fixtures() -> &'static PathBuf {
    static FIXTURES: OnceLock<PathBuf> = OnceLock::new();
    FIXTURES.get_or_init(|| {
        let dir = fixture_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let hello = reply(0, fakecore::hello_result_framed(fakecore::ALL_METHODS, &[], false));
        let sub = reply(
            1,
            subscribe::snapshot_value(5, vec![common::minimal_terminal()]),
        );
        // Every case publishes its OWN `{name}.pid` — the tests run in
        // parallel and must never read another child's file — and `$$`
        // survives `exec`, so the pid is the process the test later
        // proves reaped. The opener ends after the hello reply.
        let opener = |name: &str| {
            format!(
                "#!/bin/sh\nprintf '%s' $$ > '{}.pid'\nIFS= read -r line\nprintf '%s\\n' '{hello}'\n",
                dir.join(name).display()
            )
        };
        // `ssh-sub-sleep`: answers hello and subscribe, then sleeps.
        script(
            &dir.join("ssh-sub-sleep"),
            &format!("{opener}IFS= read -r line\nprintf '%s\\n' '{sub}'\nexec sleep 60\n", opener = opener("ssh-sub-sleep")),
        );
        // `ssh-sub-bad`: subscribes, sends one malformed line, then sleeps.
        script(
            &dir.join("ssh-sub-bad"),
            &format!("{opener}IFS= read -r line\nprintf '%s\\n' '{sub}'\nprintf '%s\\n' '{{oops'\nexec sleep 60\n", opener = opener("ssh-sub-bad")),
        );
        // `ssh-sub-flood`: subscribes, then floods a stdout nobody reads —
        // the child blocks on the full pipe until its write end dies.
        script(
            &dir.join("ssh-sub-flood"),
            &format!("{opener}IFS= read -r line\nprintf '%s\\n' '{sub}'\nexec yes\n", opener = opener("ssh-sub-flood")),
        );
        // `ssh-sub-stage`: answers hello, reads one byte of the subscribe
        // request, publishes ITS OWN marker, then sleeps unanswered — the
        // marker proves the subscribe call was in flight on cancel.
        script(
            &dir.join("ssh-sub-stage"),
            &format!("{opener}dd bs=1 count=1 of=/dev/null 2>/dev/null\nprintf x > '{seen}'\nexec sleep 60\n", opener = opener("ssh-sub-stage"), seen = dir.join("ssh-sub-stage.seen").display()),
        );
        unsafe {
            libc::atexit(remove_fixtures);
        }
        dir
    })
}

fn fixture_dir() -> PathBuf {
    std::env::temp_dir().join(format!("hc-tui-subscribe-{}", std::process::id()))
}

extern "C" fn remove_fixtures() {
    let _ = std::fs::remove_dir_all(fixture_dir());
}

fn reply(id: u64, result: serde_json::Value) -> String {
    serde_json::to_string(&json!({"jsonrpc": "2.0", "id": id, "result": result})).unwrap()
}

fn script(path: &PathBuf, contents: &str) {
    std::fs::write(path, contents).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The path to a shared fake-ssh script; forces the barrier's init.
fn fake(name: &str) -> PathBuf {
    fixtures().join(name)
}

/// A case's own pid file — never shared with a parallel sibling.
fn pidfile(case: &str) -> PathBuf {
    fixtures().join(format!("{case}.pid"))
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

/// Reads a case's pid file ONCE, after that case's own readiness barrier
/// proved the script published it — the returned pid is captured
/// immutably so a reaping poll can never be redirected by a parallel
/// test's child.
async fn pid_of(case: &str) -> i32 {
    deadline_wait(|| pidfile(case).exists()).await;
    std::fs::read_to_string(pidfile(case))
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
async fn a_dropped_subscription_kills_and_reaps_the_ssh_child() {
    let ssh = fake("ssh-sub-sleep");
    let p = params();
    let sub = timeout(Subscription::connect(
        &endpoint(),
        Some(ssh.as_os_str()),
        &p,
    ))
    .await
    .expect("subscribe failed");
    // `connect` returning is this case's barrier: its script published
    // its own pid before answering hello.
    let pid = pid_of("ssh-sub-sleep").await;
    drop(sub);
    deadline_wait(|| reaped(pid)).await;
}

#[tokio::test]
async fn a_failed_read_shuts_down_the_ssh_child() {
    // The malformed line ends the subscription: the latched failure drops
    // both halves and the owned child — it cannot stay alive serving a
    // subscription that already failed.
    let ssh = fake("ssh-sub-bad");
    let p = params();
    let mut sub = timeout(Subscription::connect(
        &endpoint(),
        Some(ssh.as_os_str()),
        &p,
    ))
    .await
    .expect("subscribe failed");
    let pid = pid_of("ssh-sub-bad").await;
    let err = timeout(sub.recv())
        .await
        .expect_err("the malformed line had to fail");
    assert!(matches!(err, SubscribeError::Protocol(_)), "{err:?}");
    deadline_wait(|| reaped(pid)).await;
    // Latched: the failed subscription reports Closed, promptly.
    assert!(matches!(
        timeout(sub.recv()).await,
        Err(SubscribeError::Closed)
    ));
}

#[tokio::test]
async fn close_releases_a_child_blocked_on_unread_stdout() {
    // The child floods stdout; nobody reads after `close` — dropping the
    // read half first makes its writes die on EPIPE so `close` finishes
    // promptly instead of waiting out the 10 s kill deadline.
    let ssh = fake("ssh-sub-flood");
    let p = params();
    let sub = timeout(Subscription::connect(
        &endpoint(),
        Some(ssh.as_os_str()),
        &p,
    ))
    .await
    .expect("subscribe failed");
    let pid = pid_of("ssh-sub-flood").await;
    // Far inside CLOSE_TIMEOUT (10 s): only the released stdout makes
    // this fast — a kill-deadline path would fail the deadline.
    let _ = tokio::time::timeout(Duration::from_secs(4), sub.close())
        .await
        .expect("close hung on a stdout-blocked child");
    deadline_wait(|| reaped(pid)).await;
}

#[tokio::test]
async fn cancelled_open_kills_and_reaps_the_ssh_child() {
    let ssh = fake("ssh-sub-stage");
    let mut opening = AbortOnDrop(Some(tokio::spawn(async move {
        let p = params();
        let _ = Subscription::connect(&endpoint(), Some(ssh.as_os_str()), &p).await;
    })));
    // The fake only publishes its marker after reading a byte of the
    // subscribe request — the call is definitely in flight, not still
    // negotiating hello.
    deadline_wait(|| fixtures().join("ssh-sub-stage.seen").exists()).await;
    let pid = pid_of("ssh-sub-stage").await;
    let task = opening.0.take().unwrap();
    task.abort();
    let cancelled = timeout(task).await;
    assert!(
        cancelled
            .expect_err("the open task had to be cancelled")
            .is_cancelled()
    );
    deadline_wait(|| reaped(pid)).await;
}
