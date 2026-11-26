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
use backlot_model::hollywood::{self, HRefusal, HollywoodEvent, HollywoodState, NewHollywoodEvent};
use backlot_model::state::{StudioState, replay};
use backlot_model::validate::{Refusal, validate};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// A file a command wants copied into the studio directory; `dest` is
/// relative to the studio dir so the ledger never points elsewhere.
/// `append` is the stage's own annotation — bytes the source file
/// never carried, written after the copy lands.
pub struct PendingFile {
    pub src: PathBuf,
    pub dest: String,
    pub append: Option<String>,
}

/// What an append builds inside the lock: the event and its file copies.
pub struct Pending {
    pub event: NewEvent,
    pub files: Vec<PendingFile>,
}

/// The same for Hollywood's ledger.
pub struct HPending {
    pub event: NewHollywoodEvent,
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

/// Append/read failures. `Build` carries the command's own error
/// type, `Refused` the model's (`Refusal` for studios, `HRefusal`
/// for Hollywood).
#[derive(Debug)]
pub enum StoreError<E, V = Refusal> {
    Io(io::Error),
    Corrupt(Corrupt),
    Refused(V),
    Build(E),
}

impl<E, V> From<io::Error> for StoreError<E, V> {
    fn from(e: io::Error) -> Self {
        StoreError::Io(e)
    }
}

impl<E> From<Refusal> for StoreError<E> {
    fn from(e: Refusal) -> Self {
        StoreError::Refused(e)
    }
}

impl<E> From<HRefusal> for StoreError<E, HRefusal> {
    fn from(e: HRefusal) -> Self {
        StoreError::Refused(e)
    }
}

/// The parent every core's state lives under:
/// `$XDG_STATE_HOME/backlot` or `~/.local/state/backlot`.
pub fn root_dir(env: &BTreeMap<String, String>) -> PathBuf {
    if let Some(x) = env.get("XDG_STATE_HOME") {
        return PathBuf::from(x).join("backlot");
    }
    let home = env
        .get("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local/state/backlot")
}

/// The per-core base directory: `$BACKLOT_STATE_DIR` always wins;
/// otherwise `root_dir/<core id>` so scratch sessions stay apart.
pub fn base_dir(env: &BTreeMap<String, String>, core_id: &str) -> PathBuf {
    if let Some(d) = env.get("BACKLOT_STATE_DIR") {
        return PathBuf::from(d);
    }
    root_dir(env).join(sanitize(core_id))
}

/// `studios/<name>` under the base directory.
pub fn studio_dir(base: &Path, name: &str) -> PathBuf {
    base.join("studios").join(name)
}

/// `<base>/archive` holding archived studio directories (`<id>-<stamp>`).
pub fn archive_dir(base: &Path) -> PathBuf {
    base.join("archive")
}

/// Archived studio directories under `<base>/archive/`, sorted by
/// directory name.
pub fn archived_studios(base: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    let dir = archive_dir(base);
    match fs::read_dir(&dir) {
        Ok(entries) => {
            for e in entries {
                let e = e?;
                let path = e.path();
                if e.file_type()?.is_dir() && path.join("events.jsonl").exists() {
                    out.push((e.file_name().to_string_lossy().into_owned(), path));
                }
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Hollywood's own ledger directory, `<base>/hollywood/` — beside
/// `studios/`, never inside it, so `studios()` and every studio rule
/// pass over it (D13).
pub fn hollywood_dir(base: &Path) -> PathBuf {
    base.join("hollywood")
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

/// The migration a log type needs — studio lines and Hollywood's own
/// each name theirs (`backlot_model::migrate`).
pub type MigrateFn = fn(
    serde_json::Value,
) -> Result<(serde_json::Value, bool), backlot_model::migrate::MigrateError>;

/// Parse log bytes — the event type is the caller's: `Event` for a
/// studio ledger, `HollywoodEvent` for Hollywood's. Every line's
/// JSON value passes through `migrate` before it deserializes, so a
/// pre-versioning line (no `v`) loads transparently and a line
/// stamped by a newer backlot fails with the upgrade message instead
/// of an opaque serde error. Returns the events and whether a torn
/// tail was ignored. A newline-terminated line that does not parse
/// or migrate is corrupt.
pub fn parse_log<T: DeserializeOwned>(
    bytes: &[u8],
    migrate: MigrateFn,
) -> Result<(Vec<T>, bool), Corrupt> {
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
        let corrupt = |e: &dyn std::fmt::Display| Corrupt {
            line: i + 1,
            message: e.to_string(),
        };
        let val = match serde_json::from_str::<serde_json::Value>(line) {
            Ok(v) => v,
            Err(err) => return Err(corrupt(&err)),
        };
        let (val, _) = migrate(val).map_err(|e| corrupt(&e))?;
        match serde_json::from_value::<T>(val) {
            Ok(e) => events.push(e),
            Err(err) => return Err(corrupt(&err)),
        }
    }
    Ok((events, torn))
}

/// Read `events.jsonl`; a missing file reads as an empty log.
fn load<T: DeserializeOwned>(
    dir: &Path,
    migrate: MigrateFn,
) -> Result<(Vec<T>, bool), StoreError<()>> {
    let path = dir.join("events.jsonl");
    match fs::read(&path) {
        Ok(bytes) => parse_log(&bytes, migrate).map_err(StoreError::Corrupt),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok((Vec::new(), false)),
        Err(e) => Err(StoreError::Io(e)),
    }
}

/// Read a studio's `events.jsonl`.
pub fn load_events(dir: &Path) -> Result<(Vec<Event>, bool), StoreError<()>> {
    load(dir, backlot_model::migrate::migrate_event_value)
}

/// Read Hollywood's `events.jsonl`.
pub fn load_hevents(dir: &Path) -> Result<(Vec<HollywoodEvent>, bool), StoreError<()>> {
    load(dir, backlot_model::migrate::migrate_hollywood_value)
}

/// Replay a studio directory into its state.
pub fn load_state(dir: &Path) -> Result<StudioState, StoreError<()>> {
    let (events, _) = load_events(dir)?;
    Ok(replay(&events))
}

/// Replay Hollywood's directory into its state.
pub fn load_hstate(dir: &Path) -> Result<HollywoodState, StoreError<()>> {
    let (events, _) = load_hevents(dir)?;
    Ok(hollywood::replay(&events))
}

pub(crate) struct LockGuard(File);

/// Take the append lock on `events.lock` (created if missing).
pub(crate) fn take_lock(dir: &Path) -> io::Result<LockGuard> {
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

/// The `seq` accessor `append_locked` needs over both event types.
trait HasSeq {
    fn seq(&self) -> u64;
}

impl HasSeq for Event {
    fn seq(&self) -> u64 {
        self.seq
    }
}

impl HasSeq for HollywoodEvent {
    fn seq(&self) -> u64 {
        self.seq
    }
}

/// Append one or more events under the lock. `build` sees the replayed
/// state and the first seq it will get; it may stage file copies per
/// event. Order: truncate a torn tail, build, validate every event
/// against the state its predecessors leave, copy files, write + fsync.
/// A later event's `dest` names must use the seq it is stamped with.
/// `reduce`, `stamp` and `check` are the ledger's replay, stamping
/// and append-time rules — the shared core under `append` (studios)
/// and `append_hollywood`.
trait AppendGuard<V> {
    fn guard_state(&self) -> Result<(), V>;
}

impl AppendGuard<backlot_model::Refusal> for StudioState {
    fn guard_state(&self) -> Result<(), backlot_model::Refusal> {
        if self.unknown_kinds.is_empty() {
            Ok(())
        } else {
            Err(backlot_model::Refusal::NewerRecords(
                self.unknown_kinds.iter().cloned().collect(),
            ))
        }
    }
}

impl AppendGuard<backlot_model::hollywood::HRefusal> for backlot_model::hollywood::HollywoodState {
    fn guard_state(&self) -> Result<(), backlot_model::hollywood::HRefusal> {
        Ok(())
    }
}

struct AppendOps<R, St, C, A, Pre, Post> {
    reduce: R,
    stamp: St,
    check: C,
    apply: A,
    pre_copy: Pre,
    post_copy: Post,
    migrate: MigrateFn,
}

fn append_locked<Ev, New, S, E, V, R, St, C, A, Pre, Post>(
    dir: &Path,
    build: impl FnOnce(&S, u64) -> Result<Vec<(New, Vec<PendingFile>)>, E>,
    mut ops: AppendOps<R, St, C, A, Pre, Post>,
) -> Result<Vec<Ev>, StoreError<E, V>>
where
    Ev: DeserializeOwned + Serialize + HasSeq,
    S: AppendGuard<V>,
    R: Fn(&[Ev]) -> S,
    St: Fn(New, u64, u64) -> Ev,
    C: FnMut(&S, &New) -> Result<(), V>,
    A: FnMut(&mut S, &Ev),
    Pre: FnOnce(&[Ev], &[PendingFile]) -> Result<(), StoreError<E, V>>,
    Post: FnOnce(&S, &[Ev]) -> Result<(), StoreError<E, V>>,
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
    let (events, _) =
        parse_log(&bytes[..keep as usize], ops.migrate).map_err(StoreError::Corrupt)?;
    let mut state = (ops.reduce)(&events);
    state.guard_state().map_err(StoreError::Refused)?;
    let next_seq = events.iter().map(|e| e.seq()).max().unwrap_or(0) + 1;
    let built = build(&state, next_seq).map_err(StoreError::Build)?;

    // Validate and stamp in order; each event sees the state the ones
    // before it leave, so a proposal and its supersede can be checked
    // together. Files copy after everything is valid.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut stamped = Vec::with_capacity(built.len());
    let mut copies: Vec<PendingFile> = Vec::new();
    for (offset, (new, files)) in built.into_iter().enumerate() {
        let seq = next_seq + offset as u64;
        (ops.check)(&state, &new).map_err(StoreError::Refused)?;
        let event = (ops.stamp)(new, seq, now);
        (ops.apply)(&mut state, &event);
        stamped.push(event);
        copies.extend(files);
    }

    (ops.pre_copy)(&stamped, &copies)?;
    for pf in &copies {
        let dest = dir.join(&pf.dest);
        crate::design_doc::copy_pending_atomic(pf, &dest)?;
    }
    (ops.post_copy)(&state, &stamped)?;

    // One buffer, one `write_all`: a torn append can land a prefix of
    // the batch but never interleave or reorder within it, and a single
    // write before the fsync is the cheapest durable shape.
    let mut buf = Vec::new();
    for event in &stamped {
        serde_json::to_writer(&mut buf, event)
            .map_err(|e| StoreError::Io(io::Error::new(io::ErrorKind::InvalidData, e)))?;
        buf.push(b'\n');
    }
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&events_path)?;
    f.write_all(&buf)?;
    f.sync_data()?;
    Ok(stamped)
}

/// One studio event into `dir`'s `events.jsonl`.
pub fn append<E, F>(dir: &Path, build: F) -> Result<Event, StoreError<E>>
where
    F: FnOnce(&StudioState, u64) -> Result<Pending, E>,
{
    let mut events = append_many(dir, move |state, seq| build(state, seq).map(|p| vec![p]))?;
    Ok(events.remove(0))
}

/// Several studio events into `dir`'s `events.jsonl` under one lock —
/// e.g. `message.queued` paired atomically with `design.entry`.
pub fn append_many<E, F>(dir: &Path, build: F) -> Result<Vec<Event>, StoreError<E>>
where
    F: FnOnce(&StudioState, u64) -> Result<Vec<Pending>, E>,
{
    append_locked(
        dir,
        |state, seq| {
            build(state, seq).map(|v| {
                v.into_iter()
                    .map(|p| (p.event, p.files))
                    .collect::<Vec<_>>()
            })
        },
        AppendOps {
            reduce: replay,
            stamp: |new: NewEvent, seq, ts| new.stamp(seq, ts),
            check: validate,
            apply: backlot_model::state::apply,
            pre_copy: |stamped: &[Event], copies: &[PendingFile]| {
                crate::design_doc::verify_pending_entries(stamped, copies)
            },
            post_copy: |state: &StudioState, stamped: &[Event]| {
                crate::design_doc::sync_on_append(dir, state, stamped)
            },
            migrate: backlot_model::migrate::migrate_event_value,
        },
    )
}

/// One Hollywood event into Hollywood's own `events.jsonl` — the
/// overseer's ledger, its own seqs (D13).
pub fn append_hollywood<E, F>(
    dir: &Path,
    build: F,
) -> Result<HollywoodEvent, StoreError<E, HRefusal>>
where
    F: FnOnce(&HollywoodState, u64) -> Result<HPending, E>,
{
    let mut events =
        append_hollywood_many(dir, move |state, seq| build(state, seq).map(|p| vec![p]))?;
    Ok(events.remove(0))
}

/// Several Hollywood events under one lock — a proposal and the
/// `card.superseded` events that close the cards it replaces. Each
/// event is validated against what its predecessors leave, so the pair
/// can never land half-checked.
pub fn append_hollywood_many<E, F>(
    dir: &Path,
    build: F,
) -> Result<Vec<HollywoodEvent>, StoreError<E, HRefusal>>
where
    F: FnOnce(&HollywoodState, u64) -> Result<Vec<HPending>, E>,
{
    append_locked(
        dir,
        |state, seq| {
            build(state, seq).map(|v| {
                v.into_iter()
                    .map(|p| (p.event, p.files))
                    .collect::<Vec<_>>()
            })
        },
        AppendOps {
            reduce: hollywood::replay,
            stamp: |new: NewHollywoodEvent, seq, ts| new.stamp(seq, ts),
            check: hollywood::validate,
            apply: hollywood::apply,
            pre_copy: |_: &[HollywoodEvent], _: &[PendingFile]| Ok(()),
            post_copy: |_: &HollywoodState, _: &[HollywoodEvent]| Ok(()),
            migrate: backlot_model::migrate::migrate_hollywood_value,
        },
    )
}
