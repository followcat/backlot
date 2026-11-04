//! Real-core scratch fixture for the opt-in integration suite.
//!
//! Spawns `herdr-core serve` (the binary named by `HC_TUI_TEST_CORE`) on an
//! isolated session: temporary XDG/HOME roots, a short explicit `HC_SOCKET`,
//! a `hctui-test-*` session name and a pre-seeded neutral `host-a` identity.
//!
//! Teardown follows the core's own shutdown path: `server stop` under the
//! identical fixture environment stops the core *and* its detached
//! holders/programs (they `setsid` away and intentionally survive a killed
//! core — no signal to the serve can reach them). Unclean shutdowns —
//! the deliberate mid-test kill or a failed stop — are recovered by an
//! owned replacement `serve` that adopts the holders, then stops
//! normally. Every child wait is bounded. The root is removed on every
//! path — success, panic and test cancellation via drop.
//!
//! Tests must close/drop their clients and watchers *before* the fixture
//! drops: CLI `connect` autostarts a missing endpoint, and a reconnecting
//! watcher would otherwise race the final `server stop`.

#![allow(dead_code)]

use std::ffi::OsStr;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use hc_proto::client::Client;
use hc_proto::endpoint::{Endpoint, Local};

/// Deadline for the core to bind its socket and answer hello.
pub const READY: Duration = Duration::from_secs(20);
/// Bounded `server stop`: the smoke proves ~ms; this covers slow CI.
const STOP: Duration = Duration::from_secs(10);
/// Bounded reap after SIGKILL — teardown never blocks on a child.
const REAP: Duration = Duration::from_secs(5);

/// The pinned binary under test. An explicit real run must fail loudly
/// when the variable or file is missing — never silently skip.
pub fn core_binary() -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os("HC_TUI_TEST_CORE")
            .expect("HC_TUI_TEST_CORE is not set: point it at the pinned herdr-core binary"),
    );
    assert!(
        path.is_file(),
        "HC_TUI_TEST_CORE={path:?} does not exist; build it with tools/build-test-core.sh"
    );
    path
}

/// The isolated environment every fixture-owned process shares — serve,
/// `server stop` and `terminal.run` children all see only this.
fn fixture_env(root: &Path, session: &str, socket: &Path) -> Vec<(&'static str, String)> {
    let mut env = vec![
        ("PATH", "/usr/bin:/bin".into()),
        ("LANG", "C.UTF-8".into()),
        ("SHELL", "/bin/sh".into()),
        ("TERM", "xterm-256color".into()),
        ("HOME", "home".into()),
        ("XDG_STATE_HOME", "state".into()),
        ("XDG_CONFIG_HOME", "config".into()),
        ("XDG_RUNTIME_DIR", "run".into()),
        ("HC_SESSION", session.into()),
        ("HC_SOCKET", socket.to_string_lossy().into_owned()),
        ("HC_HEARTBEAT_MS", "200".into()),
    ];
    for pair in env.iter_mut() {
        if [
            "HOME",
            "XDG_STATE_HOME",
            "XDG_CONFIG_HOME",
            "XDG_RUNTIME_DIR",
        ]
        .contains(&pair.0)
        {
            pair.1 = root.join(&pair.1).to_string_lossy().into_owned();
        }
    }
    env
}

fn base<'a>(
    command: &'a mut Command,
    root: &Path,
    session: &str,
    socket: &Path,
) -> &'a mut Command {
    command
        .env_clear()
        .envs(fixture_env(root, session, socket))
        .current_dir(root.join("work"))
        .stdin(Stdio::null());
    command
}

/// `hc serve` in its own process group, matching the core's detached-start
/// convention — signals aimed at the test process never reach it.
fn spawn_serve(root: &Path, session: &str, socket: &Path, log: &Path) -> Child {
    let err = fs::File::create(log).unwrap();
    let mut command = Command::new(core_binary());
    base(&mut command, root, session, socket)
        .arg("serve")
        .stdout(Stdio::null())
        .stderr(Stdio::from(err))
        .process_group(0);
    command
        .spawn()
        .unwrap_or_else(|e| panic!("cannot spawn {} serve: {e}", core_binary().display()))
}

/// `hc --json server stop` under the identical fixture env; `true` only
/// when the core actually stopped. `connect_existing` never autostarts,
/// and a dead endpoint prints `{"stopped":false}` *with exit 0* — exit
/// status alone cannot distinguish a real shutdown from a no-op.
fn run_stop(root: &Path, session: &str, socket: &Path) -> bool {
    let mut command = Command::new(core_binary());
    base(&mut command, root, session, socket)
        .args(["--json", "server", "stop"])
        .stdout(Stdio::piped());
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    if !wait_for(&mut child, STOP) {
        let _ = child.kill();
        let _ = wait_for(&mut child, REAP);
        return false;
    }
    let Ok(out) = child.wait_with_output() else {
        return false;
    };
    out.status.success()
        && serde_json::from_slice::<serde_json::Value>(&out.stdout)
            .ok()
            .and_then(|v| v.get("stopped").and_then(|s| s.as_bool()))
            .unwrap_or(false)
}

/// Bounded child reap: teardown must finish even when the process won't.
fn wait_for(child: &mut Child, bound: Duration) -> bool {
    let deadline = Instant::now() + bound;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            _ => return false,
        }
    }
}

/// An owned scratch core: environment, socket endpoint and the foreground
/// `serve` child. Dropping runs `server stop`, reaps the child, then removes
/// the whole root — holders and programs go down with it.
pub struct ScratchCore {
    /// Every file the fixture owns lives under here.
    pub root: PathBuf,
    /// The session name passed as `HC_SESSION`.
    pub session: String,
    /// `Endpoint::Local` pointing at the scratch socket.
    pub endpoint: Endpoint,
    /// The controlled working directory for `terminal.run` callers.
    pub work: PathBuf,
    /// The serve child's stderr/stdout log for failure context.
    pub log: PathBuf,
    child: Option<Child>,
    /// `None` until teardown runs — latched so `finish` and `Drop` agree.
    stopped: Option<bool>,
}

impl ScratchCore {
    /// The session state directory the serve reads — `docs.json` and
    /// friends live here; tests may rewrite them while the core is down.
    pub fn state_dir(&self) -> PathBuf {
        self.root.join("state/herdr-core").join(&self.session)
    }
}

impl ScratchCore {
    /// Builds the isolated environment and spawns `serve`, waiting until a
    /// framed RPC `hello` succeeds. A construction guard removes the root
    /// on any failure before this returns.
    pub async fn spawn() -> ScratchCore {
        let root = std::env::temp_dir().join(format!("hctui-{}-{:x}", std::process::id(), nanos()));
        // Claim a new root atomically; never adopt an existing directory.
        fs::create_dir(&root).unwrap();
        let mut guard = RootGuard {
            root: root.clone(),
            armed: true,
        };
        for dir in ["state", "config", "run", "home", "work", "log"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        let session = format!("hctui-test-{:x}", nanos());
        let socket = root.join("s.sock");

        // Pre-seed a stable neutral identity so hello names `host-a`.
        let state = root.join("state/herdr-core").join(&session);
        fs::create_dir_all(&state).unwrap();
        fs::write(
            state.join("core.json"),
            r#"{"id":"scratch-core","name":"host-a"}"#,
        )
        .unwrap();

        let log = root.join("log/serve.log");
        let work = root.join("work");
        let child = spawn_serve(&root, &session, &socket, &log);
        let this = ScratchCore {
            endpoint: Endpoint::Local(Local {
                session: session.clone(),
                holder_socket: socket.with_extension("holder.sock"),
                socket,
                state,
                config: root.join("config/herdr-core").join(&session),
            }),
            root,
            session,
            work,
            log,
            child: Some(child),
            stopped: None,
        };
        // Transfer cleanup before readiness can fail or be cancelled. The
        // root guard must not remove state that ScratchCore::drop preserves
        // after a failed shutdown.
        guard.armed = false;
        if let Err(e) = wait_ready(&this.endpoint).await {
            panic!("scratch core did not become ready in {READY:?}: {e}");
        }
        this
    }

    /// Deliberate mid-test death: SIGKILL the serve child only — detached
    /// holders `setsid` out of its group and are meant to survive, so
    /// killing the group could never reach them. [`Drop`] then adopts
    /// them through a replacement serve. Distinct from the graceful
    /// `server stop` path.
    pub fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = wait_for(&mut child, REAP);
        }
    }

    /// Spawns a fresh `serve` over the same scratch state (same session,
    /// socket path and `host-a` identity) after [`kill`](Self::kill).
    pub fn respawn(&mut self) {
        assert!(self.child.is_none(), "kill the core before respawning it");
        let log = self.root.join(format!("log/serve-{}.log", nanos()));
        self.child = Some(spawn_serve(
            &self.root,
            &self.session,
            &self.root.join("s.sock"),
            &log,
        ));
    }

    /// The graceful final shutdown: `server stop` under the fixture env —
    /// the only path that reliably stops core *and* its detached
    /// holders/programs — then reaps the child, bounded.
    ///
    /// When shutdown was unclean — a mid-test [`kill`](Self::kill), a
    /// failed stop, or an unreaped child — owned state may still be
    /// served by detached holders no signal can reach. A replacement
    /// `serve` adopts them (`runtime.rs` `attach_all`/`start_holder`),
    /// then the normal stop finishes everything.
    ///
    /// Returns `true` only on a verified clean shutdown; `Drop` keeps the
    /// root for inspection when it is `false`. Latched — a later `Drop`
    /// neither re-attempts nor disagrees with `finish`'s verdict.
    fn stop(&mut self) -> bool {
        if let Some(done) = self.stopped {
            return done;
        }
        let clean = match self.child.take() {
            Some(mut child) => self.stop_running(&mut child),
            None => false,
        };
        let clean = if clean || !self.state_remains() {
            clean
        } else {
            self.respawn();
            wait_socket(&self.socket());
            match self.child.take() {
                Some(mut child) => self.stop_running(&mut child),
                None => false,
            }
        };
        self.stopped = Some(clean);
        clean
    }

    /// Explicit teardown a test can *assert*: `Ok(())` when the core and
    /// its holders stopped and the root was removed, `Err(root)` — with
    /// the evidence left in place — otherwise. Tests that finish
    /// explicitly fail on teardown failure; tests that panic midway
    /// unwind through `Drop`, which preserves evidence without
    /// panicking again.
    pub fn finish(mut self) -> Result<(), PathBuf> {
        if self.stop() {
            let _ = fs::remove_dir_all(&self.root);
            Ok(())
        } else {
            Err(self.root.clone())
        }
    }

    /// `server stop` then a bounded reap; `true` only when stop reported
    /// `stopped:true` *and* the child actually exited. An already-dead
    /// serve makes stop a `stopped:false` no-op — not a clean shutdown —
    /// so the caller falls through to holder adoption. On failure the
    /// serve child itself is killed and reaped — bounded either way.
    fn stop_running(&mut self, child: &mut Child) -> bool {
        if run_stop(&self.root, &self.session, &self.socket()) && wait_for(child, STOP) {
            return true;
        }
        let _ = child.kill();
        let _ = wait_for(child, REAP);
        false
    }

    fn socket(&self) -> PathBuf {
        self.root.join("s.sock")
    }

    /// Live client socket or holder socket leftovers (`s.holder.<id>.sock`)
    /// under this root: something here may still deserve adoption.
    fn state_remains(&self) -> bool {
        if self.socket().exists() {
            return true;
        }
        fs::read_dir(&self.root).is_ok_and(|d| {
            d.flatten().any(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name.starts_with("s.holder.") && name.ends_with(".sock")
            })
        })
    }
}

impl Drop for ScratchCore {
    fn drop(&mut self) {
        if self.stop() {
            let _ = fs::remove_dir_all(&self.root);
        } else {
            // A failed teardown must not destroy the evidence — or the
            // state a surviving holder still serves. Report, keep the
            // root, and never panic: unwind may already be in flight.
            eprintln!(
                "hc-tui scratch core at {:?} could not be fully stopped; \
                 preserving the root for inspection",
                self.root
            );
        }
    }
}

/// Removes the fixture root when construction fails partway.
struct RootGuard {
    root: PathBuf,
    armed: bool,
}
impl Drop for RootGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

/// Sync readiness for the adopt-then-stop path: `server stop` needs the
/// replacement serve's listener bound before `connect_existing` can work.
fn wait_socket(socket: &Path) {
    let deadline = Instant::now() + READY;
    while Instant::now() < deadline {
        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A framed `hello` round-trip succeeds once the socket is bound.
async fn wait_ready(endpoint: &Endpoint) -> Result<(), hc_proto::client::Error> {
    let deadline = Instant::now() + READY;
    loop {
        match Client::connect(endpoint, None::<&OsStr>).await {
            Ok(_) => return Ok(()),
            Err(e) if Instant::now() < deadline => {
                let _ = e;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(e) => return Err(e),
        }
    }
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        + u128::from(std::process::id())
}
