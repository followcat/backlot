//! The studio state directory and the append-only event log.
//!
//! Layout (ledger.md): `<base>/studios/<studio>/events.jsonl` plus
//! `events.lock` for the flock, and `req/`, `plan/`, `reports/`,
//! `messages/` holding copies of file content. Appends run under the
//! lock: assign the next `seq`, write one line, fsync. A torn last
//! line from a killed process is ignored on read and truncated before
//! the next append.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use backlot_model::NewEvent;
use backlot_model::event::Event;
use backlot_model::state::{StudioState, replay};
use backlot_model::validate::{Refusal, validate};

/// A file a command wants copied into the studio directory; `dest` is
/// relative to the studio dir so the ledger never points elsewhere.
pub struct PendingFile {
    pub src: PathBuf,
    pub dest: String,
}

/// What an append builds inside the lock: the event and its file copies.
pub struct Pending {
    pub event: NewEvent,
    pub files: Vec<PendingFile>,
}

/// The last log line was not newline-terminated or could not be parsed;
/// middle lines are never tolerated.
#[derive(Debug)]
pub struct Corrupt {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for Corrupt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "events.jsonl line {}: {}", self.line, self.message)
    }
}

/// Append/read failures. `Build` carries the command's own error type.
#[derive(Debug)]
pub enum StoreError<E> {
    Io(io::Error),
    Corrupt(Corrupt),
    Refused(Refusal),
    Build(E),
}

impl<E> From<io::Error> for StoreError<E> {
    fn from(e: io::Error) -> Self {
        StoreError::Io(e)
    }
}

impl<E> From<Refusal> for StoreError<E> {
    fn from(e: Refusal) -> Self {
        StoreError::Refused(e)
    }
}

/// The per-core base directory: `$BACKLOT_STATE_DIR`, else
/// `$XDG_STATE_HOME/backlot/default`, else `~/.local/state/backlot/default`.
/// Keying by core id arrives with the stage (S2).
pub fn base_dir(env: &BTreeMap<String, String>) -> PathBuf {
    if let Some(d) = env.get("BACKLOT_STATE_DIR") {
        return PathBuf::from(d);
    }
    if let Some(x) = env.get("XDG_STATE_HOME") {
        return PathBuf::from(x).join("backlot/default");
    }
    let home = env
        .get("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local/state/backlot/default")
}

/// `studios/<name>` under the base directory.
pub fn studio_dir(base: &Path, name: &str) -> PathBuf {
    base.join("studios").join(name)
}

/// All known studio names under the base directory.
pub fn studios(base: &Path) -> io::Result<Vec<String>> {
    let mut out = Vec::new();
    let dir = base.join("studios");
    match fs::read_dir(&dir) {
        Ok(entries) => {
            for e in entries {
                let e = e?;
                if e.file_type()?.is_dir() && e.path().join("events.jsonl").exists() {
                    out.push(e.file_name().to_string_lossy().into_owned());
                }
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    out.sort();
    Ok(out)
}

/// A sane file/dir name component: keep a small charset.
pub fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    if s.is_empty() || s.starts_with('.') {
        format!("x{s}")
    } else {
        s
    }
}

/// Parse log bytes. Returns the events and whether a torn tail was
/// ignored. A newline-terminated line that does not parse is corrupt.
pub fn parse_log(bytes: &[u8]) -> Result<(Vec<Event>, bool), Corrupt> {
    let terminated = bytes.ends_with(b"\n");
    let text = String::from_utf8_lossy(bytes);
    let mut events = Vec::new();
    let mut torn = false;
    let mut lines = text.split('\n').enumerate().peekable();
    while let Some((i, line)) = lines.next() {
        if line.is_empty() {
            continue;
        }
        if lines.peek().is_none() && !terminated {
            // Unterminated tail: torn write, ignored on read.
            torn = true;
            continue;
        }
        match serde_json::from_str::<Event>(line) {
            Ok(e) => events.push(e),
            Err(err) => {
                return Err(Corrupt {
                    line: i + 1,
                    message: err.to_string(),
                });
            }
        }
    }
    Ok((events, torn))
}

/// Read `events.jsonl`; a missing file reads as an empty log.
pub fn load_events(dir: &Path) -> Result<(Vec<Event>, bool), StoreError<()>> {
    let path = dir.join("events.jsonl");
    match fs::read(&path) {
        Ok(bytes) => parse_log(&bytes).map_err(StoreError::Corrupt),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok((Vec::new(), false)),
        Err(e) => Err(StoreError::Io(e)),
    }
}

/// Replay a studio directory into its state.
pub fn load_state(dir: &Path) -> Result<StudioState, StoreError<()>> {
    let (events, _) = load_events(dir)?;
    Ok(replay(&events))
}

struct LockGuard(File);

/// Take the append lock on `events.lock` (created if missing).
fn take_lock(dir: &Path) -> io::Result<LockGuard> {
    let f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(dir.join("events.lock"))?;
    #[cfg(unix)]
    {
        let rc = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    #[cfg(not(unix))]
    {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "flock requires unix",
        ));
    }
    Ok(LockGuard(f))
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

/// Append one event under the lock. `build` sees the replayed state
/// and the seq it will get; it may stage file copies. Order: truncate
/// a torn tail, build + validate, copy files, write + fsync.
pub fn append<E, F>(dir: &Path, build: F) -> Result<Event, StoreError<E>>
where
    F: FnOnce(&StudioState, u64) -> Result<Pending, E>,
{
    fs::create_dir_all(dir)?;
    let _lock = take_lock(dir)?;
    let events_path = dir.join("events.jsonl");
    let bytes = match fs::read(&events_path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(StoreError::Io(e)),
    };
    // Truncate a torn tail before anything else uses the log.
    let keep = if bytes.is_empty() || bytes.ends_with(b"\n") {
        bytes.len() as u64
    } else {
        bytes
            .iter()
            .rposition(|b| *b == b'\n')
            .map(|p| (p + 1) as u64)
            .unwrap_or(0)
    };
    if keep < bytes.len() as u64 {
        OpenOptions::new()
            .write(true)
            .open(&events_path)?
            .set_len(keep)?;
    }
    let (events, _) = parse_log(&bytes[..keep as usize]).map_err(StoreError::Corrupt)?;
    let state = replay(&events);
    let next_seq = events.iter().map(|e| e.seq).max().unwrap_or(0) + 1;
    let pending = build(&state, next_seq).map_err(StoreError::Build)?;
    validate(&state, &pending.event)?;

    for pf in &pending.files {
        let dest = dir.join(&pf.dest);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&pf.src, &dest)?;
    }

    let event = pending.event.stamp(
        next_seq,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    );
    let mut line = serde_json::to_string(&event)
        .map_err(|e| StoreError::Io(io::Error::new(io::ErrorKind::InvalidData, e)))?;
    line.push('\n');
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&events_path)?;
    f.write_all(line.as_bytes())?;
    f.sync_data()?;
    Ok(event)
}
