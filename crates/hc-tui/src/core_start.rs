//! One-shot `hc server start` when a local endpoint's socket does not
//! answer: never for `--remote`, never when `HC_SOCKET` names a core
//! someone else manages, and never more than once per process — the
//! watcher's ordinary reconnect picks the core up. The caller kicks it
//! only after terminal setup succeeded, so a run with no terminal still
//! fails first and starts nothing.

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use hc_proto::endpoint::{Endpoint, Env, Local};
use tokio::sync::oneshot;

/// The official core installer, quoted in the missing-`hc` notice.
const INSTALL_URL: &str = "https://releases.herdr.dev/core/install.sh";

/// A notice for the app to show: title and body.
pub type Notice = (String, String);

/// The one-shot report. Resolves to `Some(notice)` when the start
/// failed, `None` when a core was already there, the socket failed in a
/// way a fresh core can't fix, or `hc server start` reported success.
pub type Report = oneshot::Receiver<Option<Notice>>;

/// Once per process — latched only on an actual start, so a check that
/// found a live core doesn't count.
static STARTED: AtomicBool = AtomicBool::new(false);

/// Starts a local core when the resolved socket doesn't answer.
/// `None` — nothing running, nothing to report — for a remote
/// endpoint or a set `HC_SOCKET`.
pub fn kick(endpoint: &Endpoint, env: &Env) -> Option<Report> {
    kick_with(endpoint, env, OsStr::new("hc"))
}

/// `kick` with the `hc` program named explicitly — tests hand in a
/// fake by path instead of rewriting `PATH` for the whole process.
fn kick_with(endpoint: &Endpoint, env: &Env, hc: &OsStr) -> Option<Report> {
    let Endpoint::Local(local) = endpoint else {
        return None; // remote: the core there is the remote side's job
    };
    if env
        .get(OsStr::new("HC_SOCKET"))
        .is_some_and(|v| !v.is_empty())
    {
        return None; // a specific core someone else manages
    }
    let local = local.clone();
    let hc = hc.to_os_string();
    let (tx, rx) = oneshot::channel();
    tokio::spawn(async move {
        let _ = tx.send(check_and_start(&local, &hc).await);
    });
    Some(rx)
}

/// The check itself: probe the socket, then `hc server start` once.
/// `Some(notice)` only when the start should have worked and didn't.
async fn check_and_start(local: &Local, hc: &OsStr) -> Option<Notice> {
    if !dead(&local.socket).await {
        return None;
    }
    if STARTED.swap(true, Ordering::SeqCst) {
        return None; // this process already started a core
    }
    start(local, hc).await
}

/// Spawns `hc server start` for the session and waits it out:
/// `Some(notice)` on a spawn or exit failure, `None` on success.
async fn start(local: &Local, hc: &OsStr) -> Option<Notice> {
    let mut command = tokio::process::Command::new(hc);
    command
        .args(["server", "start"])
        // `hc server start` has no --session flag — it resolves its
        // session from the environment — so the resolved name is set
        // explicitly: a `--session` override or a missing `HC_SESSION`
        // would otherwise start the wrong (default) core.
        .env("HC_SESSION", &local.session)
        .env_remove("HC_SOCKET")
        .stdin(Stdio::null())
        .stdout(Stdio::null()) // a child's chatter must never reach the TUI
        .stderr(Stdio::piped());
    let child = match command.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Some((
                "hc isn't installed".into(),
                format!("install it: curl -fsSL {INSTALL_URL} | sh"),
            ));
        }
        Err(e) => return Some(("Core start failed".into(), format!("hc server start: {e}"))),
    };
    // `hc server start` bounds itself (spawn + a few seconds of socket
    // wait), so the wait needs no deadline here; the child survives the
    // client's exit so a started core outlives detach.
    match child.wait_with_output().await {
        Ok(output) if output.status.success() => None,
        Ok(output) => Some((
            "Core start failed".into(),
            first_line(&output.stderr)
                .unwrap_or_else(|| format!("hc server start exited with {}", output.status)),
        )),
        Err(e) => Some(("Core start failed".into(), format!("hc server start: {e}"))),
    }
}

/// `true` only for the connect failures a fresh core fixes — a missing
/// socket file or a refused (stale) one. A live listener, a wedged one
/// (connect never returns) or a permission error means something is
/// there, so nothing is started.
async fn dead(socket: &Path) -> bool {
    match socket_connect(socket).await {
        Ok(()) => false,
        Err(e) => matches!(
            e.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
        ),
    }
}

/// One bounded connect to the socket. The timeout keeps a listener
/// that never accepts from stalling the task; it maps to an error that
/// `dead` treats as "something answers".
#[cfg(unix)]
async fn socket_connect(socket: &Path) -> io::Result<()> {
    match tokio::time::timeout(
        Duration::from_secs(2),
        tokio::net::UnixStream::connect(socket),
    )
    .await
    {
        Ok(result) => result.map(|_| ()),
        Err(_) => Err(io::Error::from(io::ErrorKind::TimedOut)),
    }
}

/// The Unix-socket probe is the whole point; off Unix there is no
/// local socket to start a core behind.
#[cfg(not(unix))]
async fn socket_connect(_socket: &Path) -> io::Result<()> {
    Ok(())
}

/// The first non-empty trimmed line — what a failing `hc` printed
/// first, without the rest of its report.
fn first_line(stderr: &[u8]) -> Option<String> {
    String::from_utf8_lossy(stderr)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hc_proto::endpoint::Remote;
    use std::ffi::OsString;
    use std::path::PathBuf;

    /// A scratch directory that removes itself.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            let dir = std::env::temp_dir().join(format!(
                "hctui-corestart-{}-{:x}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn env_with(pairs: &[(&str, &str)]) -> Env {
        pairs
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect()
    }

    fn local(socket: &Path) -> Endpoint {
        Endpoint::Local(Local {
            session: "scratch".into(),
            socket: socket.to_path_buf(),
            holder_socket: socket.with_extension("holder.sock"),
            state: PathBuf::from("/scratch/state"),
            config: PathBuf::from("/scratch/config.toml"),
        })
    }

    /// A fake `hc`: appends `argv` plus `HC_SESSION` to `record`, then
    /// succeeds (`ok`) or fails (`fail`: two stderr lines, exit 1).
    /// The record path and mode are baked into the script — the child
    /// argv is `server start` exactly as production sends it.
    fn fake_hc(dir: &Path, mode: &str) -> (OsString, PathBuf) {
        let script = dir.join("hc");
        let record = dir.join("record");
        let tail = match mode {
            "fail" => "echo 'fake: first failure line' >&2\necho 'fake: second' >&2\nexit 1",
            _ => "exit 0",
        };
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf 'hc %s session=%s\\n' \"$*\" \"${{HC_SESSION-}}\" \
                 >> \"{}\"\n{tail}\n",
                record.display(),
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        (script.into_os_string(), record)
    }

    /// The only latch consumer in the suite: a dead socket runs
    /// `hc server start` with the resolved session in `HC_SESSION`,
    /// and a second check in the same process runs nothing.
    #[tokio::test]
    async fn a_dead_socket_runs_hc_server_start_once() {
        let scratch = Scratch::new();
        let socket = scratch.0.join("missing.sock");
        let (hc, record) = fake_hc(&scratch.0, "ok");
        let report = kick_with(&local(&socket), &env_with(&[]), &hc).unwrap();
        assert_eq!(report.await.unwrap(), None);
        let recorded = std::fs::read_to_string(&record).unwrap();
        assert_eq!(recorded.trim(), "hc server start session=scratch");
        // Once per process: the second check reports quietly and runs
        // nothing more.
        let second = kick_with(&local(&socket), &env_with(&[]), &hc).unwrap();
        assert_eq!(second.await.unwrap(), None);
        assert_eq!(std::fs::read_to_string(&record).unwrap(), recorded);
    }

    #[tokio::test]
    async fn a_remote_endpoint_never_starts_a_local_core() {
        let remote = Endpoint::Remote(Remote {
            target: "host-a".into(),
            session: "scratch".into(),
        });
        let scratch = Scratch::new();
        let (hc, _record) = fake_hc(&scratch.0, "ok");
        assert!(kick_with(&remote, &env_with(&[]), &hc).is_none());
    }

    #[tokio::test]
    async fn a_set_hc_socket_never_starts_a_core() {
        let scratch = Scratch::new();
        let socket = scratch.0.join("missing.sock");
        let (hc, record) = fake_hc(&scratch.0, "ok");
        let env = env_with(&[("HC_SOCKET", socket.to_str().unwrap())]);
        assert!(kick_with(&local(&socket), &env, &hc).is_none());
        assert!(!record.exists());
    }

    /// A listening socket answers: the probe sees it and nothing runs.
    #[tokio::test]
    async fn an_answering_socket_starts_nothing() {
        let scratch = Scratch::new();
        let socket = scratch.0.join("live.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        assert!(!dead(&socket).await);
    }

    /// Missing and refused sockets are both dead ends a fresh core
    /// fixes: a socket left behind with nothing listening refuses
    /// connects.
    #[tokio::test]
    async fn missing_or_refused_sockets_are_dead() {
        let scratch = Scratch::new();
        let missing = scratch.0.join("missing.sock");
        assert!(dead(&missing).await);
        drop(std::os::unix::net::UnixListener::bind(&missing).unwrap());
        assert!(dead(&missing).await);
    }

    #[tokio::test]
    async fn a_missing_hc_says_how_to_install() {
        let scratch = Scratch::new();
        let nowhere = scratch.0.join("no-such-hc");
        let local = Local {
            session: "scratch".into(),
            socket: scratch.0.join("missing.sock"),
            holder_socket: PathBuf::from("/scratch/holder.sock"),
            state: PathBuf::from("/scratch/state"),
            config: PathBuf::from("/scratch/config.toml"),
        };
        let (title, body) = start(&local, nowhere.as_os_str()).await.unwrap();
        assert!(title.contains("hc"));
        assert!(body.contains(INSTALL_URL), "body: {body}");
    }

    #[tokio::test]
    async fn a_failed_start_reports_its_first_error_line() {
        let scratch = Scratch::new();
        let (hc, _record) = fake_hc(&scratch.0, "fail");
        let local = Local {
            session: "scratch".into(),
            socket: scratch.0.join("missing.sock"),
            holder_socket: PathBuf::from("/scratch/holder.sock"),
            state: PathBuf::from("/scratch/state"),
            config: PathBuf::from("/scratch/config.toml"),
        };
        let (title, body) = start(&local, &hc).await.unwrap();
        assert_eq!(title, "Core start failed");
        assert_eq!(body, "fake: first failure line");
    }

    #[test]
    fn first_line_skips_blank_lines() {
        assert_eq!(
            first_line(b"\n  hello  \nsecond\n").as_deref(),
            Some("hello")
        );
        assert_eq!(first_line(b"\n\n"), None);
        assert_eq!(first_line(b""), None);
    }
}
