// Derived from herdr-core crates/herdr-core/src/{client,machines}.rs and
// platform/unix.rs at ec8adcdf (Apache-2.0).
//
//! One open connection to a core: a Unix socket locally, or one owned ssh
//! child remotely. Gives the codec [`Reader`]/[`Writer`] its read and write
//! halves; the ssh child's lifetime is owned here. On Unix every remote
//! stream rides one shared login per target through a private
//! control-socket directory prepared here (10-minute persist; askpass
//! prompts are disabled), and stderr is kept as a bounded diagnostic tail.

mod control;

use crate::endpoint::{Endpoint, Remote};
use std::collections::VecDeque;
use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::pin::Pin;
use std::process::ExitStatus;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

/// The read half a codec [`Reader`](crate::codec::Reader) wraps.
pub type ReadHalf = Pin<Box<dyn AsyncRead + Send>>;
/// The write half a codec [`Writer`](crate::codec::Writer) wraps.
pub type WriteHalf = Pin<Box<dyn AsyncWrite + Send>>;

/// Bytes of stderr kept for diagnostics: a tail, never the whole stream, so
/// a chatty ssh cannot grow memory or block the pipe.
const STDERR_TAIL: usize = 8 * 1024;

/// How long `close` lets a remote `hc connect` exit on its own before the
/// child is killed.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);

/// How long `close` lets the stderr drain task end on pipe EOF after the
/// child is dead. A descendant process can outlive the child while holding
/// the stderr write end open — that must not hang the close.
const STDERR_DRAIN: Duration = Duration::from_secs(5);

/// How long an unexpected-EOF close still lets a child that is already
/// dying on its own report its real status — real ssh writes its stderr
/// diagnostic and `exit(255)` within microseconds of closing stdout, and
/// that status must beat the kill — before `start_kill` reaps a hung
/// child that ignored stdin EOF.
const EOF_EXIT_GRACE: Duration = Duration::from_millis(100);

/// One open connection.
pub struct Connection {
    /// Protocol bytes from the core (ssh stdout or the socket).
    pub read: ReadHalf,
    /// Protocol bytes to the core (ssh stdin or the socket).
    pub write: WriteHalf,
    backend: Backend,
}

enum Backend {
    Local,
    Ssh(Box<Ssh>),
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let backend = match self.backend {
            Backend::Local => "local",
            Backend::Ssh(_) => "ssh",
        };
        f.debug_struct("Connection")
            .field("backend", &backend)
            .finish_non_exhaustive()
    }
}

/// The owned pieces of an ssh pipe: the child, its stderr drain task and the
/// bounded tail that task fills.
struct Ssh {
    target: String,
    child: Option<Child>,
    stderr_task: Option<JoinHandle<()>>,
    tail: Arc<Mutex<VecDeque<u8>>>,
}

/// What `close` reports when it ends cleanly.
#[derive(Debug, Default)]
pub struct Closed {
    /// The ssh child's exit status, where there is a child.
    pub status: Option<ExitStatus>,
    /// What the child's stderr left in the tail, lossy-decoded.
    pub stderr_tail: String,
}

/// A transport failure. Strings carry the endpoint context (path, target,
/// program) — never protocol bytes and never credentials.
#[derive(Debug)]
pub enum Error {
    /// The socket could not be reached, or an existing pipe broke.
    Io(io::Error),
    /// The ssh executable would not start.
    Spawn { program: String, source: io::Error },
    /// The ssh child exited nonzero (auth refused, no hc there, dead link).
    Exit {
        target: String,
        status: ExitStatus,
        stderr_tail: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Spawn { program, source } => write!(f, "cannot run {program}: {source}"),
            Error::Exit {
                target,
                status,
                stderr_tail,
            } => {
                write!(f, "ssh to {target} exited with {status}")?;
                let tail = stderr_tail.trim();
                if !tail.is_empty() {
                    write!(f, ": {tail}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) | Error::Spawn { source: e, .. } => Some(e),
            Error::Exit { .. } => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error::Io(e)
    }
}

/// Opens a connection for a resolved [`Endpoint`]. `ssh_program` overrides
/// the `ssh` name — tests inject a scripted helper; production passes `None`.
/// Each call opens its own ssh child; on Unix the child's argv asks ssh to
/// share one login per target through [`control::control_dir`]'s socket, so
/// a burst of streams costs one TCP/key exchange instead of one each.
pub async fn connect(
    endpoint: &Endpoint,
    ssh_program: Option<&OsStr>,
) -> Result<Connection, Error> {
    match endpoint {
        Endpoint::Local(local) => unix(&local.socket).await,
        Endpoint::Remote(remote) => ssh(remote, ssh_program.unwrap_or(OsStr::new("ssh"))).await,
    }
}

/// Connects a Unix socket at `path` (local endpoint only).
#[cfg(unix)]
async fn unix(path: &Path) -> Result<Connection, Error> {
    let stream = tokio::net::UnixStream::connect(path).await.map_err(|e| {
        Error::Io(io::Error::new(
            e.kind(),
            format!("connect {}: {e}", path.display()),
        ))
    })?;
    let (read, write) = stream.into_split();
    Ok(Connection {
        read: Box::pin(read),
        write: Box::pin(write),
        backend: Backend::Local,
    })
}

/// Runs `ssh <target> 'hc connect …'` as an owned child: stdin/stdout are the
/// protocol pipe, stderr drains into a bounded tail so a flood cannot
/// deadlock the pipe. Batch mode plus askpass disabled means ssh can only
/// fail, never prompt our UI.
async fn ssh(remote: &Remote, program: &OsStr) -> Result<Connection, Error> {
    let control = control::control_dir();
    let mut cmd = Command::new(program);
    cmd.args(remote.argv(control.as_deref()))
        .env_remove("SSH_ASKPASS")
        .env("SSH_ASKPASS_REQUIRE", "never")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let display = program.to_string_lossy().into_owned();
    let mut child = cmd.spawn().map_err(|source| Error::Spawn {
        program: display,
        source,
    })?;
    let read = Box::pin(child.stdout.take().expect("stdout piped")) as ReadHalf;
    let write = Box::pin(child.stdin.take().expect("stdin piped")) as WriteHalf;
    let mut stderr = child.stderr.take().expect("stderr piped");
    let tail = Arc::new(Mutex::new(VecDeque::new()));
    let stderr_task = {
        let tail = Arc::clone(&tail);
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            while let Ok(n @ 1..) = stderr.read(&mut buf).await {
                let mut tail = tail.lock().unwrap_or_else(|p| p.into_inner());
                tail.extend(&buf[..n]);
                while tail.len() > STDERR_TAIL {
                    tail.pop_front();
                }
            }
        })
    };
    let ssh = Ssh {
        target: remote.target.clone(),
        child: Some(child),
        stderr_task: Some(stderr_task),
        tail,
    };
    Ok(Connection {
        read,
        write,
        backend: Backend::Ssh(Box::new(ssh)),
    })
}

impl Connection {
    /// The codec halves and, for ssh, the owned child guard. Splitting hands
    /// the caller the I/O while [`Guard`] keeps the child's lifetime.
    pub fn split(
        self,
    ) -> (
        crate::codec::Reader<ReadHalf>,
        crate::codec::Writer<WriteHalf>,
        Guard,
    ) {
        let Connection {
            read,
            write,
            backend,
        } = self;
        let guard = match backend {
            Backend::Local => Guard { ssh: None },
            Backend::Ssh(ssh) => Guard { ssh: Some(*ssh) },
        };
        (
            crate::codec::Reader::new(read),
            crate::codec::Writer::new(write),
            guard,
        )
    }

    /// The same connection with its transport reads and writes counted
    /// into `stats`.
    pub fn counted(self, stats: &Arc<crate::stats::ConnStats>) -> Connection {
        use crate::stats::Counted;
        Connection {
            read: Box::pin(Counted::new(self.read, Arc::clone(stats))),
            write: Box::pin(Counted::new(self.write, Arc::clone(stats))),
            backend: self.backend,
        }
    }

    /// Ends the connection: the write side closes first (remote `hc connect`
    /// exits on stdin EOF), then the ssh child exits or is killed and its
    /// stderr tail is collected.
    pub async fn close(self) -> Result<Closed, Error> {
        let Connection { write, backend, .. } = self;
        drop(write);
        match backend {
            Backend::Local => Ok(Closed::default()),
            Backend::Ssh(ssh) => ssh.finish().await,
        }
    }
}

/// What [`Connection::split`] leaves of the backend: the owned ssh child or
/// nothing (a local socket dies with its halves). Dropping it ends and reaps
/// the child.
pub struct Guard {
    ssh: Option<Ssh>,
}

impl Guard {
    /// What the child's stderr collected so far, lossy-decoded.
    pub fn stderr_tail(&self) -> String {
        self.ssh.as_ref().map_or_else(String::new, Ssh::tail)
    }

    /// Graceful close for a split connection: the [`Connection::close`]
    /// equivalent for callers that took the codec halves. Drop the writer
    /// half first — the stdin EOF is what makes a remote `hc connect` exit
    /// on its own — then this waits for the child (bounded, killing it if
    /// needed) and reports its status and stderr tail.
    pub async fn close(self) -> Result<Closed, Error> {
        match self.ssh {
            None => Ok(Closed::default()),
            Some(ssh) => ssh.finish().await,
        }
    }

    /// Prompt close after an unexpected reader EOF or a failed open: the
    /// peer is already gone or never answered, so unlike [`close`](Self::close)
    /// the child gets only [`EOF_EXIT_GRACE`] to report a natural exit —
    /// enough for a dying ssh's `exit(255)` and stderr tail — before it is
    /// killed and reaped.
    pub(crate) async fn close_eof(self) -> Result<Closed, Error> {
        match self.ssh {
            None => Ok(Closed::default()),
            Some(ssh) => ssh.finish_with(EOF_EXIT_GRACE).await,
        }
    }
}

impl Ssh {
    fn tail(&self) -> String {
        let tail = self.tail.lock().unwrap_or_else(|p| p.into_inner());
        String::from_utf8_lossy(&tail.iter().copied().collect::<Vec<u8>>()).into_owned()
    }

    /// Orderly end: wait for the child up to [`CLOSE_TIMEOUT`], else kill
    /// it; then let the stderr task end on the pipe's EOF, bounded by
    /// [`STDERR_DRAIN`] — a descendant can outlive the child holding the
    /// write end. The handle stays owned by `self` while it is awaited, so
    /// dropping this future mid-await still lets [`Drop`] abort the task.
    async fn finish(self) -> Result<Closed, Error> {
        self.finish_with(CLOSE_TIMEOUT).await
    }

    /// `finish` with `timeout` as the child's natural-exit budget: on
    /// expiry the child is killed, then reaped; the stderr drain bound is
    /// unchanged. A short `timeout` still reports a child that was already
    /// exiting on its own — its waitpid lands well inside the budget.
    async fn finish_with(mut self, timeout: Duration) -> Result<Closed, Error> {
        let mut child = self.child.take().expect("child present until finish");
        let status = match tokio::time::timeout(timeout, child.wait()).await {
            Ok(status) => status.map_err(Error::Io)?,
            Err(_) => {
                child.start_kill().map_err(Error::Io)?;
                child.wait().await.map_err(Error::Io)?
            }
        };
        if let Some(task) = self.stderr_task.as_mut()
            && tokio::time::timeout(STDERR_DRAIN, &mut *task)
                .await
                .is_err()
        {
            task.abort();
        }
        let stderr_tail = self.tail();
        if status.success() {
            Ok(Closed {
                status: Some(status),
                stderr_tail,
            })
        } else {
            Err(Error::Exit {
                target: self.target.clone(),
                status,
                stderr_tail,
            })
        }
    }
}

impl Drop for Ssh {
    /// Any path that abandons the pipe — drop, early return, a cancelled
    /// future — kills the child and stops the stderr task. Inside a runtime
    /// a reaper waits on the pid; outside one `kill_on_drop` still applies
    /// and the process is reaped with this one.
    fn drop(&mut self) {
        if let Some(task) = self.stderr_task.take() {
            task.abort();
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            if let Ok(rt) = tokio::runtime::Handle::try_current() {
                rt.spawn(async move {
                    let _ = child.wait().await;
                });
            }
        }
    }
}
