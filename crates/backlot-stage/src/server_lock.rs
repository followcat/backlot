//! Exclusive `server.lock`, liveness lease, and `server-pointer.json` tombstones
//! for single-server mode (`backlot server`).

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::cli::Fail;
use crate::store;

/// Default `hc` session managed by `backlot server`.
pub(crate) const DEFAULT_SESSION: &str = "backlot";
/// Advisory `flock(2)` file under the server state directory.
pub(crate) const LOCK_FILE: &str = "server.lock";
/// Human-readable liveness lease refreshed by a running `backlot-server`.
pub(crate) const LEASE_FILE: &str = "server-lease.json";
/// Tombstone written into each retired legacy `<core_id>` directory.
pub(crate) const POINTER_FILE: &str = "server-pointer.json";
/// Lease refresh cadence while the daemon loop runs.
pub(crate) const LEASE_RENEW_MS: u64 = 5_000;
/// Advertised lease TTL in `server-lease.json` (advisory; OS `flock` governs liveness).
pub(crate) const LEASE_TTL_MS: u64 = 20_000;

/// Metadata persisted in `server-lease.json` (and initially in `server.lock`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ServerLease {
    pub(crate) pid: u32,
    pub(crate) session: String,
    pub(crate) base: String,
    pub(crate) started_ms: u64,
    pub(crate) renewed_ms: u64,
    pub(crate) ttl_ms: u64,
}

/// Tombstone left in each retired `<core_id>` directory after `server adopt`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ServerPointer {
    pub(crate) version: u32,
    pub(crate) core_id: String,
    pub(crate) session: String,
    pub(crate) server_dir: String,
    pub(crate) adopted_ms: u64,
}

/// Information about an active `backlot-server` holding `server.lock`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunningServer {
    pub(crate) base: PathBuf,
    pub(crate) session: String,
    pub(crate) pid: Option<u32>,
}

/// Held exclusive lock + liveness lease for `backlot server run`.
pub(crate) struct ServerLockGuard {
    file: File,
    base: PathBuf,
    lease: ServerLease,
    last_renew: Instant,
}

impl ServerLockGuard {
    /// Acquire `server.lock` exclusively (non-blocking) and publish `server-lease.json`.
    pub(crate) fn acquire(base: &Path, session: &str) -> Result<Self, Fail> {
        fs::create_dir_all(base).map_err(|e| {
            Fail::refused(format!("create server state dir {}: {e}", base.display()))
        })?;
        let lock_path = base.join(LOCK_FILE);
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| Fail::refused(format!("open {}: {e}", lock_path.display())))?;

        if !try_flock(&file, true)? {
            let srv = read_running_from_dir(base);
            return Err(running_server_refusal(&srv));
        }

        let now = now_ms();
        let lease = ServerLease {
            pid: std::process::id(),
            session: session.to_string(),
            base: base.to_string_lossy().into_owned(),
            started_ms: now,
            renewed_ms: now,
            ttl_ms: LEASE_TTL_MS,
        };
        write_lock_metadata(&mut file, &lease, &lock_path)?;
        write_lease_file(base, &lease)?;

        Ok(Self {
            file,
            base: base.to_path_buf(),
            lease,
            last_renew: Instant::now(),
        })
    }

    /// Refresh `server-lease.json` if `LEASE_RENEW_MS` has elapsed.
    pub(crate) fn refresh_if_due(&mut self) {
        if self.last_renew.elapsed().as_millis() < u128::from(LEASE_RENEW_MS) {
            return;
        }
        self.lease.renewed_ms = now_ms();
        if write_lease_file(&self.base, &self.lease).is_ok() {
            self.last_renew = Instant::now();
        }
    }
}

impl Drop for ServerLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.base.join(LEASE_FILE));
        let _ = unlock_file(&self.file);
    }
}

/// Directory storing single-server state (`~/.local/state/backlot/server` or `BACKLOT_STATE_DIR`).
pub(crate) fn server_dir(vars: &BTreeMap<String, String>) -> PathBuf {
    pinned_base(vars).unwrap_or_else(|| store::root_dir(vars).join("server"))
}

fn pinned_base(vars: &BTreeMap<String, String>) -> Option<PathBuf> {
    vars.get("BACKLOT_STATE_DIR")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// `true` when `dir` holds a retired `server-pointer.json` tombstone.
pub(crate) fn is_pointer_dir(dir: &Path) -> bool {
    dir.join(POINTER_FILE).is_file()
}

/// Read `dir/server-pointer.json` if present and valid.
pub(crate) fn read_pointer(dir: &Path) -> Option<ServerPointer> {
    let text = fs::read_to_string(dir.join(POINTER_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Atomically write `dir/server-pointer.json`.
pub(crate) fn write_pointer(dir: &Path, ptr: &ServerPointer) -> Result<(), Fail> {
    fs::create_dir_all(dir)
        .map_err(|e| Fail::refused(format!("create core dir {}: {e}", dir.display())))?;
    let json = serde_json::to_string_pretty(ptr)
        .map_err(|e| Fail::refused(format!("encode {POINTER_FILE}: {e}")))?;
    atomic_write(&dir.join(POINTER_FILE), format!("{json}\n").as_bytes())
}

/// Return a retired `server-pointer.json` if `BACKLOT_STATE_DIR` or `core_id` points to one.
pub(crate) fn check_retired_core(
    vars: &BTreeMap<String, String>,
    core_id: Option<&str>,
) -> Option<ServerPointer> {
    if let Some(pinned) = pinned_base(vars)
        && let Some(ptr) = read_pointer(&pinned)
    {
        return Some(ptr);
    }
    if let Some(id) = core_id {
        let dir = store::root_dir(vars).join(store::sanitize(id));
        if let Some(ptr) = read_pointer(&dir) {
            return Some(ptr);
        }
    }
    None
}

/// Probe whether a live `backlot-server` holds `server.lock` on this machine.
pub(crate) fn running_server(vars: &BTreeMap<String, String>) -> Option<RunningServer> {
    let mut candidates = vec![store::root_dir(vars).join("server")];
    if let Some(pinned) = pinned_base(vars)
        && !candidates.contains(&pinned)
    {
        candidates.push(pinned);
    }
    for dir in candidates {
        let lock_path = dir.join(LOCK_FILE);
        let Ok(file) = File::open(&lock_path) else {
            continue;
        };
        match try_flock(&file, false) {
            Ok(true) => {
                // Lock is unheld: previous server exited or was killed; stale lease is ignored.
                let _ = unlock_file(&file);
            }
            Ok(false) => {
                return Some(read_running_from_dir(&dir));
            }
            Err(_) => {}
        }
    }
    None
}

/// Exact single-line refusal when a second server run collides with an active server.
pub(crate) fn running_server_refusal(srv: &RunningServer) -> Fail {
    match srv.pid {
        Some(pid) => Fail::refused(format!(
            "backlot-server is already running (--session {}, pid {pid})",
            srv.session
        )),
        None => Fail::refused(format!(
            "backlot-server is already running (--session {})",
            srv.session
        )),
    }
}

/// Exact single-line refusal for `backlot stage start|run` when single-server mode is active
/// or the core was retired by `server adopt`.
pub(crate) fn refuse_if_server_or_retired(
    vars: &BTreeMap<String, String>,
    core_id: Option<&str>,
) -> Result<(), Fail> {
    if let Some(ptr) = check_retired_core(vars, core_id) {
        return Err(Fail::refused(format!(
            "managed by backlot-server (--session {})",
            ptr.session
        )));
    }
    if let Some(srv) = running_server(vars) {
        return Err(Fail::refused(format!(
            "backlot-server is already running (--session {}); cannot start backlot-stage",
            srv.session
        )));
    }
    Ok(())
}

/// Resolve the base directory for local commands/daemons when `BACKLOT_STATE_DIR` is unset:
/// route to `server/` when this core was retired or `backlot-server` is active; otherwise keep `<root>/<core_id>`.
pub(crate) fn effective_base(vars: &BTreeMap<String, String>, core_id: &str) -> PathBuf {
    if let Some(pinned) = pinned_base(vars) {
        return pinned;
    }
    let root = store::root_dir(vars);
    let core = root.join(store::sanitize(core_id));
    let server = root.join("server");
    if is_pointer_dir(&core) || running_server(vars).is_some() {
        return server;
    }
    if !core.exists()
        && (server.join("studios").exists()
            || server.join("hollywood").exists()
            || server.join("config.toml").exists())
    {
        return server;
    }
    core
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn read_running_from_dir(dir: &Path) -> RunningServer {
    for path in [dir.join(LEASE_FILE), dir.join(LOCK_FILE)] {
        if let Ok(mut f) = File::open(&path) {
            let mut buf = String::new();
            if f.read_to_string(&mut buf).is_ok()
                && let Ok(lease) = serde_json::from_str::<ServerLease>(&buf)
            {
                return RunningServer {
                    base: dir.to_path_buf(),
                    session: lease.session,
                    pid: Some(lease.pid),
                };
            }
        }
    }
    RunningServer {
        base: dir.to_path_buf(),
        session: DEFAULT_SESSION.to_string(),
        pid: None,
    }
}

fn write_lock_metadata(file: &mut File, lease: &ServerLease, path: &Path) -> Result<(), Fail> {
    let payload = serde_json::to_string(lease)
        .map_err(|e| Fail::refused(format!("encode lock metadata: {e}")))?;
    file.set_len(0)
        .and_then(|()| file.seek(SeekFrom::Start(0)))
        .and_then(|_| file.write_all(payload.as_bytes()))
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.flush())
        .map_err(|e| Fail::refused(format!("write {}: {e}", path.display())))
}

fn write_lease_file(base: &Path, lease: &ServerLease) -> Result<(), Fail> {
    let payload = serde_json::to_string_pretty(lease)
        .map_err(|e| Fail::refused(format!("encode lease: {e}")))?;
    atomic_write(&base.join(LEASE_FILE), format!("{payload}\n").as_bytes())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), Fail> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("tmp"),
        std::process::id()
    ));
    fs::write(&tmp, bytes).map_err(|e| Fail::refused(format!("write {}: {e}", tmp.display())))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        Fail::refused(format!("rename {}: {e}", path.display()))
    })
}

#[cfg(unix)]
fn try_flock(file: &File, exclusive: bool) -> Result<bool, Fail> {
    use std::os::unix::io::AsRawFd;
    let op = if exclusive {
        libc::LOCK_EX | libc::LOCK_NB
    } else {
        libc::LOCK_SH | libc::LOCK_NB
    };
    loop {
        let rc = unsafe { libc::flock(file.as_raw_fd(), op) };
        if rc == 0 {
            return Ok(true);
        }
        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        if err.raw_os_error() == Some(libc::EWOULDBLOCK) || err.raw_os_error() == Some(libc::EAGAIN)
        {
            return Ok(false);
        }
        return Err(Fail::refused(format!("flock {LOCK_FILE}: {err}")));
    }
}

#[cfg(not(unix))]
fn try_flock(_file: &File, _exclusive: bool) -> Result<bool, Fail> {
    Err(Fail::refused("server.lock requires unix flock"))
}

#[cfg(unix)]
fn unlock_file(file: &File) -> std::io::Result<()> {
    use std::os::unix::io::AsRawFd;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn unlock_file(_file: &File) -> std::io::Result<()> {
    Ok(())
}
