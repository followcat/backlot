//! Transport tests: a real unix socket in a temporary dir, and scripted
//! fake-ssh helpers — no real ssh, no core, no hc session. Every blocking
//! path runs under a deadline so a broken test fails instead of hanging.

#![cfg(unix)]

use hc_proto::codec::{Reader, Writer};
use hc_proto::endpoint::{self, Endpoint, Env, Local};
use hc_proto::transport::{self, Error, ReadHalf, WriteHalf};
use std::collections::BTreeMap;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(15);

/// A unique scratch dir under the system temp dir, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        // The per-process counter keeps parallel tests apart even on a
        // reused name; `create_dir` claims the path atomically — a
        // leftover directory is retried, never adopted.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "hc-tui-test-{name}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Scratch(path),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("create scratch dir {}: {e}", path.display()),
            }
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Every fake-ssh script, written once per process before any test spawns
/// a child. A forked child elsewhere in the process can briefly inherit a
/// script's open writer fd, making a later `exec` fail with ETXTBSY; every
/// spawn path calls `fake()`/`fixtures()` first, which only returns after
/// the single synchronized init finished all writes.
fn fixtures() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = fixture_dir();
        std::fs::create_dir_all(&dir).unwrap();
        for (name, body) in SCRIPTS {
            let path = dir.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        unsafe {
            libc::atexit(remove_fixtures);
        }
        dir
    })
    .clone()
}

fn fixture_dir() -> PathBuf {
    std::env::temp_dir().join(format!("hc-tui-fake-ssh-{}", std::process::id()))
}

extern "C" fn remove_fixtures() {
    let _ = std::fs::remove_dir_all(fixture_dir());
}

/// The scripted fake-ssh bodies, all fixed at process start.
const SCRIPTS: &[(&str, &str)] = &[
    // Protocol pipe: echo stdin back on stdout.
    ("echo", "exec cat\n"),
    // Argv probe: one stdout line per argument, then the pipe.
    ("argv", "for a in \"$@\"; do echo \"$a\"; done\nexec cat\n"),
    // Auth-style failure: a stderr message, then nonzero exit.
    (
        "exit",
        "echo 'Permission denied (publickey)' >&2\nexit 42\n",
    ),
    // ~75 KB stderr flood — beyond the tail bound and the pipe buffer —
    // then the pipe.
    (
        "flood",
        "i=0; while [ $i -lt 5000 ]; do echo \"flood-line-$i\" >&2; i=$((i+1)); done\nexec cat\n",
    ),
    // Reports its own pid on stderr, then pipes.
    ("pid", "echo $$ >&2\nexec cat\n"),
    // Parent pid on stderr; a backgrounded descendant reports its own pid
    // on stdout (the first protocol line), closes its stdout, then beats on
    // stderr every 0.1s — it dies on the first broken-pipe write, which is
    // exactly when the drain task's read end disappears. Bounded to ~60s
    // so a failed test cannot leave it running. Parent then pipes.
    (
        "holder",
        "echo $$ >&2\nsh -c 'echo $$; exec 1>&-; i=0; while [ $i -lt 600 ]; do echo beat >&2 || exit 3; sleep 0.1; i=$((i+1)); done' &\nexec cat\n",
    ),
];

/// The path to a shared fake-ssh script.
fn fake(name: &str) -> PathBuf {
    fixtures().join(name)
}

/// True when `pid` is gone *and reaped*: a zombie still answers
/// `kill(pid, 0)`, so `ESRCH` proves the process is fully gone. Portable
/// across unix — no `/proc` assumptions.
fn reaped(pid: u32) -> bool {
    let err = unsafe { libc::kill(pid as i32, 0) };
    err == -1 && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
}

/// Kills a test helper process and waits until it is dead and reaped.
async fn kill_and_reap(pid: u32) {
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
    deadline_wait(move || reaped(pid)).await;
}

fn remote_endpoint(target: &str, session: &str) -> Endpoint {
    Endpoint::Remote(endpoint::remote(target, session).unwrap())
}

/// Connects to a fake-ssh script and returns the codec halves plus guard.
async fn connect_fake(
    script: &str,
    target: &str,
) -> (Reader<ReadHalf>, Writer<WriteHalf>, transport::Guard) {
    let conn = transport::connect(
        &remote_endpoint(target, "default"),
        Some(fake(script).as_os_str()),
    )
    .await
    .unwrap();
    conn.split()
}

/// Collects the argument vector an `argv` fake printed: one line per
/// argument, ending at the remote `hc connect` line (the last argument).
async fn fake_argv(reader: &mut Reader<ReadHalf>) -> Vec<String> {
    timeout(async {
        let mut args = Vec::new();
        loop {
            let line = reader.read_line().await.unwrap().unwrap();
            let done = line.starts_with(b"hc connect");
            args.push(String::from_utf8(line).unwrap());
            if done {
                break args;
            }
        }
    })
    .await
}

/// Reads the pid a `pid`/`holder` fake printed on its stderr.
async fn child_pid(guard: &transport::Guard) -> u32 {
    timeout(async {
        loop {
            let tail = guard.stderr_tail();
            if let Some(pid) = tail.lines().next().and_then(|l| l.trim().parse().ok()) {
                break pid;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
}

#[tokio::test]
async fn unix_socket_carries_lines_both_ways() {
    let scratch = Scratch::new("unix-echo");
    let path = scratch.0.join("echo.sock");
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    let echo = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (mut r, mut w) = stream.into_split();
        tokio::io::copy(&mut r, &mut w).await.unwrap();
    });

    let mut env: Env = BTreeMap::new();
    env.insert("HC_SOCKET".into(), path.as_os_str().into());
    let endpoint = endpoint::resolve(&env, None, None, &endpoint::Sys).unwrap();
    let Endpoint::Local(local) = &endpoint else {
        panic!("expected local endpoint");
    };
    assert_eq!(local.socket, path);
    assert_eq!(local.holder_socket, path.with_extension("holder.sock"));

    let conn = transport::connect(&endpoint, None).await.unwrap();
    let (mut reader, mut writer, _guard) = conn.split();
    timeout(async {
        writer.write_line(b"{\"id\":1}").await.unwrap();
        let line = reader.read_line().await.unwrap().unwrap();
        assert_eq!(line, b"{\"id\":1}");
    })
    .await;
    echo.abort();
}

#[tokio::test]
async fn missing_unix_socket_is_an_io_error() {
    let scratch = Scratch::new("unix-missing");
    let endpoint = Endpoint::Local(Local {
        session: "default".into(),
        socket: scratch.0.join("nope.sock"),
        holder_socket: scratch.0.join("nope.holder.sock"),
        state: scratch.0.join("state"),
        config: scratch.0.join("config.toml"),
    });
    let err = transport::connect(&endpoint, None).await.unwrap_err();
    assert!(matches!(err, Error::Io(_)));
    assert!(err.to_string().contains("nope.sock"), "{err}");
}

#[tokio::test]
async fn fake_ssh_pipes_protocol_bytes() {
    let (mut reader, mut writer, _guard) = connect_fake("echo", "alice@host-b").await;
    timeout(async {
        writer
            .write_line(b"{\"jsonrpc\":\"2.0\",\"id\":1}")
            .await
            .unwrap();
        let line = reader.read_line().await.unwrap().unwrap();
        assert_eq!(line, b"{\"jsonrpc\":\"2.0\",\"id\":1}");
    })
    .await;
}

#[tokio::test]
async fn fake_ssh_gets_batch_argv_and_remote_line() {
    // The fake prints one stdout line per argument, then pipes.
    let conn = transport::connect(
        &remote_endpoint("alice@host-b", "o'clock"),
        Some(fake("argv").as_os_str()),
    )
    .await
    .unwrap();
    let (mut reader, writer, guard) = conn.split();
    let args = fake_argv(&mut reader).await;
    assert_eq!(args.len(), 17, "{args:?}");
    for (i, option) in [
        "BatchMode=yes",
        "ConnectTimeout=10",
        "ServerAliveInterval=15",
        "ServerAliveCountMax=4",
        "ControlMaster=auto",
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(args[1 + i * 2], "-o");
        assert_eq!(args[2 + i * 2], *option);
    }
    assert_eq!(args[0], "-T");
    assert_eq!(args[11], "-o");
    assert!(
        args[12].starts_with("ControlPath=") && args[12].ends_with("/%C"),
        "{:?}",
        args[12]
    );
    assert_eq!(args[13], "-o");
    assert_eq!(args[14], "ControlPersist=600");
    assert_eq!(args[15], "alice@host-b");
    assert_eq!(args[16], r"hc connect --session 'o'\''clock'");
    // Guard::close: stdin EOF lets the fake exit, status and tail come back.
    drop(writer);
    let closed = timeout(guard.close()).await.unwrap();
    assert!(closed.status.unwrap().success(), "the fake exits 0");
}

#[tokio::test]
async fn remote_streams_share_one_control_path_in_a_private_dir() {
    // Two opens must name the same ControlPath socket, under a dir this
    // layer created private (0700).
    let open_args = |target: &'static str| async move {
        let (mut reader, writer, guard) = connect_fake("argv", target).await;
        let args = fake_argv(&mut reader).await;
        drop(writer);
        timeout(guard.close()).await.unwrap();
        args
    };
    let control_path = |args: &[String]| {
        args.iter()
            .find_map(|a| a.strip_prefix("ControlPath="))
            .expect("ControlPath option")
            .to_string()
    };
    let first = open_args("alice@host-b").await;
    let second = open_args("alice@host-b").await;
    assert!(first.iter().any(|a| a == "ControlMaster=auto"));
    assert!(first.iter().any(|a| a == "ControlPersist=600"));
    let (a, b) = (control_path(&first), control_path(&second));
    assert_eq!(a, b, "every stream names the same shared socket");
    let dir = PathBuf::from(a.strip_suffix("/%C").expect("ControlPath ends /%C"));
    assert!(
        dir.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("hc-tui-ssh"),
        "{dir:?}"
    );
    let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700, "{dir:?} must be private");
}

#[tokio::test]
async fn nonzero_exit_reports_status_and_stderr_tail() {
    let conn = transport::connect(
        &remote_endpoint("alice@host-b", "default"),
        Some(fake("exit").as_os_str()),
    )
    .await
    .unwrap();
    let err = timeout(conn.close()).await.unwrap_err();
    assert!(err.to_string().contains("alice@host-b"), "{err}");
    let Error::Exit {
        target,
        status,
        stderr_tail,
    } = err
    else {
        panic!("expected Exit, got {err:?}");
    };
    assert_eq!(target, "alice@host-b");
    assert_eq!(status.code(), Some(42));
    assert!(stderr_tail.contains("Permission denied"), "{stderr_tail}");
}

#[tokio::test]
async fn stderr_flood_neither_deadlocks_nor_grows_the_tail() {
    let (mut reader, mut writer, guard) = connect_fake("flood", "host-b").await;
    timeout(async {
        // While stderr floods, protocol bytes still flow both ways.
        writer.write_line(b"{\"id\":2}").await.unwrap();
        assert_eq!(reader.read_line().await.unwrap().unwrap(), b"{\"id\":2}");
    })
    .await;
    // Wait for the flood to drain: the tail fills and stays bounded.
    deadline_wait(|| guard.stderr_tail().len() >= 8 * 1024).await;
    let tail = guard.stderr_tail();
    assert!(tail.len() <= 8 * 1024, "tail grew past its bound");
    assert!(tail.contains("flood-line-"), "tail holds the last lines");
    drop((reader, writer, guard));
}

#[tokio::test]
async fn close_completes_when_a_descendant_holds_stderr() {
    // The parent exits on stdin EOF but a backgrounded descendant keeps the
    // stderr write end open: the stderr drain must be bounded, not hung.
    let (mut reader, writer, guard) = connect_fake("holder", "host-b").await;
    let grandchild: u32 = timeout(async {
        // The first protocol line is the descendant's pid.
        let line = reader.read_line().await.unwrap().unwrap();
        String::from_utf8(line).unwrap().trim().parse().unwrap()
    })
    .await;
    let parent = child_pid(&guard).await;
    drop(writer); // stdin EOF -> the cat exits -> child dead
    let closed = timeout(guard.close()).await.unwrap();
    assert!(
        closed.status.unwrap().success(),
        "orderly exit despite held stderr"
    );
    assert!(reaped(parent), "child {parent} dead and reaped");
    kill_and_reap(grandchild).await;
}

#[tokio::test]
async fn cancelled_close_kills_the_child_and_drains_cleanup() {
    // Same held-stderr shape, but the close future is dropped mid-await:
    // Drop must still kill the child and abort the drain task.
    let (mut reader, writer, guard) = connect_fake("holder", "host-b").await;
    let grandchild: u32 = timeout(async {
        let line = reader.read_line().await.unwrap().unwrap();
        String::from_utf8(line).unwrap().trim().parse().unwrap()
    })
    .await;
    let parent = child_pid(&guard).await;
    drop(writer); // stdin EOF -> the cat exits -> child dead
    // Protocol EOF (descendant closed its stdout) proves the parent exited;
    // the still-open stderr keeps the drain pending.
    timeout(async {
        assert_eq!(reader.read_line().await.unwrap(), None);
    })
    .await;
    // Now close is guaranteed to sit in the stderr-drain await — the exact
    // spot where a taken-then-awaited handle would leak on cancellation.
    let mut close = Box::pin(guard.close());
    let early = tokio::time::timeout(Duration::from_millis(50), &mut close).await;
    assert!(early.is_err(), "close was still draining on held stderr");
    drop(close);
    deadline_wait(move || reaped(parent)).await;
    // The drain's abort closes the pipe's read end: the descendant's next
    // stderr beat hits EPIPE and it dies. A leaked drain (the old
    // take-then-await bug) would keep reading, the writer would keep
    // beating, and this wait would time out.
    deadline_wait(move || reaped(grandchild)).await;
}

#[tokio::test]
async fn dropped_pipe_kills_and_reaps_the_child() {
    let (_reader, _writer, guard) = connect_fake("pid", "host-b").await;
    let pid = child_pid(&guard).await;
    drop(guard);
    deadline_wait(move || reaped(pid)).await;
    assert!(reaped(pid), "child {pid} is dead and reaped");
}

#[tokio::test]
async fn cancelled_read_still_owns_the_child() {
    let (mut reader, _writer, guard) = connect_fake("pid", "host-b").await;
    // Read a line that never comes, cancel the read by timeout, drop all.
    let result = tokio::time::timeout(Duration::from_millis(100), reader.read_line()).await;
    assert!(result.is_err(), "the read really was still pending");
    let pid = child_pid(&guard).await;
    drop((reader, guard));
    deadline_wait(move || reaped(pid)).await;
}

#[tokio::test]
async fn missing_ssh_program_is_a_spawn_error() {
    // Even a spawn that must fail forks first — and a forked child can
    // inherit a script's open writer fd. Every spawn waits for fixtures.
    fixtures();
    let scratch = Scratch::new("ssh-missing");
    let missing = scratch.0.join("not-here-ssh");
    let err = transport::connect(
        &remote_endpoint("host-b", "default"),
        Some(missing.as_os_str()),
    )
    .await
    .unwrap_err();
    let Error::Spawn { program, .. } = err else {
        panic!("expected Spawn, got {err:?}");
    };
    assert!(program.contains("not-here-ssh"), "{program}");
}

/// Runs a future to completion inside [`DEADLINE`]; panics rather than hangs.
async fn timeout<F: std::future::Future>(f: F) -> F::Output {
    tokio::time::timeout(DEADLINE, f)
        .await
        .expect("deadline exceeded")
}

/// Polls `cond` until true or [`DEADLINE`].
async fn deadline_wait(cond: impl Fn() -> bool) {
    timeout(async {
        while !cond() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
}
