//! The check process runner (S9b part 2): `sh -c <run>` in the
//! delivery's detached worktree, its own process group, stdin on
//! `/dev/null`, stdout and stderr merged into one bounded capture.
//! The stage polls it with `try_wait` from the tick; the only wait
//! is a bounded drain of an exited shell's last bytes. `Spawn` is
//! the injection seam: production runs [`real_spawn`], tests drive
//! scripted children.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The log cap: one MiB, keeping the end.
pub const LOG_CAP: usize = 1 << 20;

/// What a finished check answers: the exit (`None` when a signal
/// killed it) and everything it wrote, already capped.
pub struct PollOut {
    pub code: Option<i32>,
    pub output: Vec<u8>,
}

/// A spawned check the stage polls — never waits on. `term`/`kill`
/// reach the child's whole process group, so the tree dies with the
/// shell, not just `sh`.
pub trait CheckChild: Send {
    /// `Some` when the process finished; `output` holds its whole
    /// capture capped to [`LOG_CAP`] keeping the end.
    fn poll(&mut self) -> Option<PollOut>;
    /// SIGTERM the child's process group.
    fn term(&mut self);
    /// SIGKILL the child's process group.
    fn kill(&mut self);
}

/// What a spawn needs: the shell command, the directory, the exact
/// environment (already stripped by the caller).
pub struct CheckSpawn {
    pub run: String,
    pub dir: PathBuf,
    pub env: Vec<(String, String)>,
}

/// The spawner — [`real_spawn`] live, a fake in tests.
pub type Spawn = dyn Fn(&CheckSpawn) -> Result<Box<dyn CheckChild>, String> + Send + Sync;

/// A bounded sink: pushes keep only the last [`LOG_CAP`] bytes.
pub(crate) struct CappedBuf(pub Vec<u8>);

impl CappedBuf {
    pub(crate) fn push(&mut self, data: &[u8]) {
        self.0.extend_from_slice(data);
        let over = self.0.len().saturating_sub(LOG_CAP);
        if over > 0 {
            self.0.drain(..over);
        }
    }
}

/// The real child: a piped `sh` drained by a reader thread per
/// pipe into the shared capped buffer, so the tick's `try_wait`
/// never blocks on output either. Each reader reports its EOF on a
/// channel, so an exiting shell's last bytes still make the log.
struct RealChild {
    child: Child,
    out: Arc<Mutex<CappedBuf>>,
    readers: Vec<Receiver<()>>,
}

impl RealChild {
    /// Each reader reports EOF on its channel; the log waits at
    /// most two seconds for both. `true` when one was still blocked
    /// — what it had captured by then is all the log gets.
    fn drain_readers(&mut self) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut cut = false;
        for rx in self.readers.drain(..) {
            let left = deadline.saturating_duration_since(Instant::now());
            if rx.recv_timeout(left).is_err() {
                cut = true;
            }
        }
        cut
    }
}

impl CheckChild for RealChild {
    fn poll(&mut self) -> Option<PollOut> {
        let status = match self.child.try_wait() {
            Ok(Some(status)) => status,
            _ => return None,
        };
        // The shell is out: anything it left in its group (a
        // `server &`) would hold the pipes open — KILL the group,
        // then give the readers a bounded moment to drain.
        self.kill();
        let cut = self.drain_readers();
        let mut output = std::mem::take(&mut self.out.lock().unwrap().0);
        if cut {
            if !output.ends_with(b"\n") && !output.is_empty() {
                output.push(b'\n');
            }
            output.extend_from_slice(
                b"backlot: check output cut: a pipe reader did not drain in 2s\n",
            );
        }
        Some(PollOut {
            code: status.code(),
            output,
        })
    }

    fn term(&mut self) {
        signal_group(self.child.id(), libc::SIGTERM);
    }

    fn kill(&mut self) {
        signal_group(self.child.id(), libc::SIGKILL);
    }
}

/// Signal the whole group the child leads (`process_group(0)` made
/// it the leader, so its id is the group id).
fn signal_group(pid: u32, sig: libc::c_int) {
    unsafe {
        libc::kill(-(pid as i32), sig);
    }
}

/// Drain `pipe` into `sink` until EOF — one reader per end of the
/// child's output, both sharing the capped buffer. The returned
/// channel gets the reader's `()` at EOF.
fn drain(mut pipe: impl Read + Send + 'static, sink: Arc<Mutex<CappedBuf>>) -> Receiver<()> {
    let (tx, rx) = std::sync::mpsc::channel();
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

/// Production [`Spawn`]: `sh -c run` under the stripped environment,
/// `/dev/null` stdin, both pipes drained into the capped tail.
pub fn real_spawn(req: &CheckSpawn) -> Result<Box<dyn CheckChild>, String> {
    use std::os::unix::process::CommandExt;
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(&req.run)
        .current_dir(&req.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .envs(req.env.iter().cloned())
        // Its own process group: the timeout's TERM/KILL reach every
        // child the shell spawns, not just the shell.
        .process_group(0);
    let mut child = cmd.spawn().map_err(|e| format!("spawn sh -c: {e}"))?;
    let out = Arc::new(Mutex::new(CappedBuf(Vec::new())));
    let readers = vec![
        drain(
            child.stdout.take().expect("stdout is piped"),
            Arc::clone(&out),
        ),
        drain(
            child.stderr.take().expect("stderr is piped"),
            Arc::clone(&out),
        ),
    ];
    Ok(Box::new(RealChild {
        child,
        out,
        readers,
    }))
}
