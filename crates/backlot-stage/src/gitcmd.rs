//! Bounded git invocations: every work-area, delivery-range, wizard and
//! repo probe runs through [`real_git`], which spawns `git` in its own
//! process group, drains both pipes on reader threads and gives up after
//! [`GIT_TIMEOUT`]. A hung git must never block the stage tick for every
//! studio.
//!
//! On Unix the timeout escalates SIGTERM to the whole process group, then
//! SIGKILL after [`GRACE`], and reaps the child, so no grandchild lingers.
//! The failure reads `git timed out after <n>s: ...` — [`crate::workareas::err_line`]
//! puts that line first.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What one git invocation produced.
#[derive(Debug, Clone, PartialEq)]
pub struct GitOut {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// How the plan runs git: argv and cwd in, the process result out.
/// Injected so tests never spawn git; production uses [`real_git`].
pub type GitRun<'a> = dyn FnMut(&[&str], &str) -> GitOut + 'a;

/// The default bound for a production git step — long enough for a big
/// checkout, short enough that a hung git cannot hide the stage.
pub const GIT_TIMEOUT: Duration = Duration::from_secs(60);

/// How long the child gets after SIGTERM before SIGKILL.
const GRACE: Duration = Duration::from_secs(1);

/// How often the wait loop checks the child.
const POLL: Duration = Duration::from_millis(10);

/// How long an exited child's readers may take to reach EOF.
const DRAIN: Duration = Duration::from_secs(2);

/// One pipe keeps at most this much (the tail), so a sprawling git
/// output cannot grow the stage's memory.
const PIPE_CAP: usize = 1 << 20;

/// Production [`GitRun`] at the default bound.
pub fn real_git(argv: &[&str], cwd: &str) -> GitOut {
    real_git_with_timeout(argv, cwd, GIT_TIMEOUT)
}

/// Production [`GitRun`] with an explicit bound — the seam the timeout
/// tests drive directly.
pub fn real_git_with_timeout(argv: &[&str], cwd: &str, timeout: Duration) -> GitOut {
    run_bounded("git", argv, cwd, timeout)
}

/// A bounded sink keeping only the last [`PIPE_CAP`] bytes.
struct Capped(Vec<u8>);

impl Capped {
    fn push(&mut self, data: &[u8]) {
        self.0.extend_from_slice(data);
        let over = self.0.len().saturating_sub(PIPE_CAP);
        if over > 0 {
            self.0.drain(..over);
        }
    }
}

/// Drain `pipe` into `sink` until EOF; the channel gets `()` then. One
/// reader per pipe means a full pipe can never deadlock the wait loop.
fn drain(mut pipe: impl Read + Send + 'static, sink: Arc<Mutex<Capped>>) -> Receiver<()> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match pipe.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => sink.lock().unwrap().push(&buf[..n]),
            }
        }
        let _ = tx.send(());
    });
    rx
}

/// Spawn `program`, bound it by `timeout`, and collect both pipes. The
/// child leads its own process group on Unix so the timeout's signals
/// reach everything it started.
pub(crate) fn run_bounded(program: &str, argv: &[&str], cwd: &str, timeout: Duration) -> GitOut {
    let mut cmd = Command::new(program);
    cmd.args(argv)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            return GitOut {
                code: -1,
                stdout: String::new(),
                stderr: format!("{program}: {e}"),
            };
        }
    };
    let out = Arc::new(Mutex::new(Capped(Vec::new())));
    let err = Arc::new(Mutex::new(Capped(Vec::new())));
    let readers = vec![
        drain(
            child.stdout.take().expect("stdout is piped"),
            Arc::clone(&out),
        ),
        drain(
            child.stderr.take().expect("stderr is piped"),
            Arc::clone(&err),
        ),
    ];
    let (status, timed_out) = match wait_bounded(&mut child, timeout) {
        Ok(Some(status)) => (Some(status), false),
        Ok(None) => {
            // SIGTERM the group, give it a moment, then SIGKILL what
            // is left — and reap the direct child either way.
            signal_group(&mut child, Signal::Term);
            let status = match wait_bounded(&mut child, GRACE) {
                Ok(Some(status)) => Some(status),
                _ => {
                    signal_group(&mut child, Signal::Kill);
                    child.wait().ok()
                }
            };
            (status, true)
        }
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = drain_readers(readers, DRAIN);
            let stderr = String::from_utf8_lossy(&take(&err)).into_owned();
            return GitOut {
                code: -1,
                stdout: String::from_utf8_lossy(&take(&out)).into_owned(),
                stderr: format!("{program}: {e}{}", tail(&stderr)),
            };
        }
    };
    let cut = drain_readers(readers, DRAIN);
    let stdout = String::from_utf8_lossy(&take(&out)).into_owned();
    let mut stderr = String::from_utf8_lossy(&take(&err)).into_owned();
    if timed_out {
        let mut line = format!(
            "{program} timed out after {}s: {program} {}",
            secs(timeout),
            argv.join(" ")
        );
        if !stderr.trim().is_empty() {
            line.push('\n');
            line.push_str(stderr.trim_end());
        }
        if cut {
            line.push_str("\n(its pipes were still open when it was killed)");
        }
        stderr = line;
    }
    GitOut {
        code: if timed_out {
            -1
        } else {
            status.and_then(|s| s.code()).unwrap_or(-1)
        },
        stdout,
        stderr,
    }
}

/// `Some(status)` when the child exited within `timeout`; `None` when
/// the bound passed first. The direct child is reaped in both cases.
fn wait_bounded(
    child: &mut Child,
    timeout: Duration,
) -> std::io::Result<Option<std::process::ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(POLL);
    }
}

enum Signal {
    Term,
    Kill,
}

/// Signal the whole group the child leads — `process_group(0)` made its
/// id the group id, so every grandchild shares the target.
fn signal_group(child: &mut Child, sig: Signal) {
    #[cfg(unix)]
    {
        let signal = match sig {
            Signal::Term => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
        };
        unsafe {
            libc::kill(-(child.id() as i32), signal);
        }
    }
    #[cfg(not(unix))]
    {
        // No process groups here: the direct child is all we can reach.
        let _ = sig;
        let _ = child.kill();
    }
}

/// Wait for each reader's EOF, bounded; `true` when one was cut short.
fn drain_readers(readers: Vec<Receiver<()>>, bounded: Duration) -> bool {
    let deadline = Instant::now() + bounded;
    let mut cut = false;
    for rx in readers {
        let left = deadline.saturating_duration_since(Instant::now());
        if rx.recv_timeout(left).is_err() {
            cut = true;
        }
    }
    cut
}

/// Take the captured bytes out of `sink`.
fn take(sink: &Arc<Mutex<Capped>>) -> Vec<u8> {
    std::mem::take(&mut sink.lock().unwrap().0)
}

/// The stderr tail appended after a spawn/wait error.
fn tail(stderr: &str) -> String {
    if stderr.trim().is_empty() {
        String::new()
    } else {
        format!("\n{}", stderr.trim_end())
    }
}

/// A timeout as whole seconds when it is one, else one decimal place.
fn secs(d: Duration) -> String {
    let s = d.as_secs_f64();
    if s.fract() == 0.0 {
        format!("{}", s as u64)
    } else {
        format!("{s:.1}")
    }
}
