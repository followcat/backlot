//! Opt-in diagnostics snapshot. With `HC_TUI_DIAG=<path>` set, SIGUSR1
//! writes one JSON snapshot of the loop, the panes and the connections to
//! `<path>`, and a clean exit writes a final one. A snapshot holds counters
//! and protocol ids only: no terminal text, titles, paths, host names or
//! targets. Unset, nothing here runs and no file is touched.

mod snapshot;
#[cfg(test)]
mod tests;

pub use snapshot::Sources;

use std::ffi::{OsStr, OsString};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::Relaxed;
use std::thread::JoinHandle;
use std::time::Instant;

use hc_proto::endpoint::Env;
use serde_json::Value;
use signal_hook::consts::SIGUSR1;
use signal_hook::iterator::{Handle, Signals};

/// The variable that turns diagnostics on; its value is the snapshot path.
pub const ENV: &str = "HC_TUI_DIAG";

/// The snapshot format version, bumped when a key changes meaning.
pub const SCHEMA: u64 = 1;

fn path_from(env: &Env) -> Option<PathBuf> {
    env.get(OsStr::new(ENV))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Whether `env` asks for diagnostics.
pub fn requested(env: &Env) -> bool {
    path_from(env).is_some()
}

struct Shared {
    path: PathBuf,
    sources: Sources,
    started: Instant,
    seq: AtomicU64,
}

impl Shared {
    fn snapshot(&self) -> Value {
        let uptime = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.sources.snapshot(uptime)
    }

    /// Writes a fresh snapshot to a private temporary file beside the path,
    /// then renames it over the path so a reader never sees a partial file.
    fn write(&self) -> io::Result<()> {
        let seq = self.seq.fetch_add(1, Relaxed);
        let dir = match self.path.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => dir,
            _ => Path::new("."),
        };
        let name = self.path.file_name().unwrap_or(OsStr::new("snapshot"));
        let mut temp_name = OsString::from(".");
        temp_name.push(name);
        temp_name.push(format!(".{}.{seq}.tmp", std::process::id()));
        let temp = dir.join(temp_name);
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp)?;
            serde_json::to_writer(&mut file, &self.snapshot())?;
            file.write_all(b"\n")?;
            drop(file);
            std::fs::rename(&temp, &self.path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result
    }
}

/// One diagnostics session: the snapshot path, what it reads and, when
/// started from the environment, the SIGUSR1 writer that serves it.
pub struct Diag {
    shared: Arc<Shared>,
    signals: Option<(Handle, JoinHandle<()>)>,
}

impl Diag {
    /// A session that writes to `path` on request, installing nothing.
    pub fn new(path: PathBuf, sources: Sources) -> Diag {
        Diag {
            shared: Arc::new(Shared {
                path,
                sources,
                started: Instant::now(),
                seq: AtomicU64::new(0),
            }),
            signals: None,
        }
    }

    /// The session `env` asks for, with SIGUSR1 writing snapshots. `None`
    /// when unset, so nothing is installed. A failed install also leaves
    /// diagnostics off rather than failing the client.
    pub fn start(env: &Env, sources: Sources) -> Option<Diag> {
        let mut diag = Diag::new(path_from(env)?, sources);
        let mut signals = Signals::new([SIGUSR1]).ok()?;
        let handle = signals.handle();
        let shared = Arc::clone(&diag.shared);
        let thread = std::thread::Builder::new()
            .name("hc-diag-signal".into())
            .spawn(move || {
                for _ in signals.forever() {
                    let _ = shared.write();
                }
            })
            .ok()?;
        diag.signals = Some((handle, thread));
        Some(diag)
    }

    /// The snapshot as JSON now.
    pub fn snapshot(&self) -> Value {
        self.shared.snapshot()
    }

    /// Writes a snapshot now.
    pub fn write(&self) -> io::Result<()> {
        self.shared.write()
    }

    /// A clean exit: writes the final snapshot and stops the SIGUSR1 writer.
    pub fn finish(self) {
        let _ = self.write();
    }
}

impl Drop for Diag {
    fn drop(&mut self) {
        if let Some((handle, thread)) = self.signals.take() {
            handle.close();
            let _ = thread.join();
        }
    }
}
