//! The seam between the stage and the machine a studio's roles run on.
//!
//! Every core and OS call the stage makes for a role — its terminal,
//! git in its work area, and the files the stage writes for it or reads
//! back — goes through [`MachineDriver`]. Slice 1 has one
//! implementation, [`LocalDriver`], which makes exactly today's calls on
//! this host: the core's terminal methods, `git` through
//! [`crate::gitcmd::real_git`], and plain `std::fs`. A later slice adds
//! one that addresses a studio's machine (`host-b/t7`, `machine.exec{on}`,
//! `file.write{on}`) without touching a caller.
//!
//! The core connection is a parameter of each call, not a field: the
//! stage's `client` is reassigned when the wire heals, and a driver that
//! captured a clone would keep calling the dead one.
//!
//! What is not in the trait yet, with the reason it cannot move in slice 1:
//!
//! - the check spawn, the process-tree probe and the work-area probe:
//!   the existing tests inject them by assigning the matching
//!   [`crate::daemon::Stage`] field, so routing them through the trait
//!   would have to edit those tests — forbidden by "tests pass
//!   unchanged". The work-area probe's `git status` still runs through
//!   [`MachineDriver::git`], so its git side is routed;
//! - the signals tail: `file.read` is line-windowed where the tail is a
//!   16 KB byte window (audit §2.2.3), so a byte-windowed read has to
//!   land first;
//! - Hollywood's own terminal: the overseer is hub-only, like the daemon
//!   terminal (audit §1.1), so it stays on the core directly.

use std::cell::{RefCell, RefMut};
use std::collections::BTreeMap;
use std::future::Future;
use std::io;
use std::path::Path;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use hc_proto::client::{CallError, Client};
use hc_proto::terminal::{ReadParams, ReadResult, RunParams, RunResult, Terminal, TerminalParams};
use serde_json::{Value, json};

use crate::gitcmd::{GitRun, real_git};

/// A driver call's answer, boxed so the trait stays object-safe.
pub type Fut<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// An owned `terminal.send` future — what the stage spawns off the tick.
pub type SendFuture = Pin<Box<dyn Future<Output = Result<Value, CallError>> + Send>>;

/// A `terminal.send` sender the delivery pass can spawn: it owns its
/// core handle, so it outlives the borrow of the driver.
pub type SendHandle = Arc<dyn Fn(String, String, Duration) -> SendFuture + Send + Sync>;

/// Every call the stage makes for a studio's roles and their work.
///
/// Terminal methods take the core's terminal address as a string
/// (`t7`, or `host-b/t7` once a role lives on another machine), so a
/// remote driver fits without a caller change. `core` is the stage's
/// connection: a remote driver will use it for `on`-qualified calls and
/// address-based routes alike.
pub trait MachineDriver {
    /// `terminal.run`: start a role's program.
    fn run<'a>(&self, core: &'a Client, params: RunParams)
    -> Fut<'a, Result<RunResult, CallError>>;
    /// `terminal.get`: one terminal's facts.
    fn get<'a>(&self, core: &'a Client, terminal: &str) -> Fut<'a, Result<Terminal, CallError>>;
    /// `terminal.kill`: end a terminal's program.
    fn kill<'a>(&self, core: &'a Client, terminal: &str) -> Fut<'a, Result<Terminal, CallError>>;
    /// `terminal.rm`: remove a finished terminal's record.
    fn rm<'a>(&self, core: &'a Client, terminal: &str) -> Fut<'a, Result<Value, CallError>>;
    /// `terminal.read`: a role's screen.
    fn read<'a>(&self, core: &'a Client, terminal: &str) -> Fut<'a, Result<ReadResult, CallError>>;
    /// `terminal.send`: type a line into a role's terminal.
    fn send<'a>(
        &self,
        core: &'a Client,
        terminal: &str,
        text: &str,
    ) -> Fut<'a, Result<Value, CallError>>;
    /// [`send`](Self::send) as an owned handle for an off-loop send with
    /// its own deadline — the delivery pass holds up to three minutes.
    fn send_handle(&self, core: &Client) -> SendHandle;
    /// `terminal.keys`: press keys on a terminal.
    fn keys<'a>(
        &self,
        core: &'a Client,
        terminal: &str,
        keys: &[&str],
    ) -> Fut<'a, Result<Value, CallError>>;
    /// `terminal.label`: patch a terminal's labels.
    fn label<'a>(
        &self,
        core: &'a Client,
        terminal: &str,
        labels: &BTreeMap<String, String>,
    ) -> Fut<'a, Result<Value, CallError>>;
    /// `terminal.keep`: keep or release a finished terminal's record.
    #[allow(dead_code)] // no stage caller yet; the audit's seam shape for later slices
    fn keep<'a>(
        &self,
        core: &'a Client,
        terminal: &str,
        keep: bool,
    ) -> Fut<'a, Result<Value, CallError>>;
    /// Whether the core offers `method` — `false` disables its feature.
    fn supports(&self, core: &Client, method: &str) -> bool;
    /// The git runner the work-area, range, check, prune and liveness
    /// passes drive: argv and cwd in, the process result out.
    fn git(&self) -> RefMut<'_, Box<GitRun<'static>>>;
    /// Create a directory tree for a role's files.
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    /// Write a role's file, replacing what is there.
    fn write_file(&self, path: &Path, body: &[u8]) -> io::Result<()>;
    /// Write an executable role file — a guard shim or git hook —
    /// atomically and with the executable bit.
    fn write_executable(&self, path: &Path, body: &str) -> io::Result<()>;
    /// Read a file the stage wrote for a role back.
    fn read_file(&self, path: &Path) -> io::Result<String>;
}

/// The git runner shared by a [`LocalDriver`] and the
/// [`crate::daemon::Stage`] whose field the check and liveness tests
/// replace.
pub type SharedGit = Rc<RefCell<Box<GitRun<'static>>>>;

/// A fresh git runner, `real_git` live.
pub fn shared_git() -> SharedGit {
    Rc::new(RefCell::new(Box::new(real_git)))
}

/// Today's driver: every call on this host, exactly as before the seam.
pub struct LocalDriver {
    git: SharedGit,
}

impl LocalDriver {
    /// A driver with a fresh `real_git` runner.
    pub fn new() -> LocalDriver {
        LocalDriver { git: shared_git() }
    }

    /// A driver whose git runner is shared with a [`crate::daemon::Stage`].
    pub fn with_git(git: SharedGit) -> LocalDriver {
        LocalDriver { git }
    }
}

impl Default for LocalDriver {
    fn default() -> LocalDriver {
        LocalDriver::new()
    }
}

fn terminal_params(terminal: &str) -> TerminalParams {
    TerminalParams {
        terminal: terminal.to_string(),
        caller: None,
    }
}

impl MachineDriver for LocalDriver {
    fn run<'a>(
        &self,
        core: &'a Client,
        params: RunParams,
    ) -> Fut<'a, Result<RunResult, CallError>> {
        Box::pin(core.terminal_run(params))
    }

    fn get<'a>(&self, core: &'a Client, terminal: &str) -> Fut<'a, Result<Terminal, CallError>> {
        Box::pin(core.terminal_get(terminal_params(terminal)))
    }

    fn kill<'a>(&self, core: &'a Client, terminal: &str) -> Fut<'a, Result<Terminal, CallError>> {
        Box::pin(core.terminal_kill(terminal_params(terminal)))
    }

    fn rm<'a>(&self, core: &'a Client, terminal: &str) -> Fut<'a, Result<Value, CallError>> {
        let params = json!({ "terminal": terminal });
        Box::pin(core.call("terminal.rm", Some(params)))
    }

    fn read<'a>(&self, core: &'a Client, terminal: &str) -> Fut<'a, Result<ReadResult, CallError>> {
        let params = ReadParams {
            terminal: terminal.to_string(),
            lines: 0,
            caller: None,
        };
        Box::pin(core.terminal_read(params))
    }

    fn send<'a>(
        &self,
        core: &'a Client,
        terminal: &str,
        text: &str,
    ) -> Fut<'a, Result<Value, CallError>> {
        let params = json!({ "terminal": terminal, "text": text });
        Box::pin(core.call("terminal.send", Some(params)))
    }

    fn send_handle(&self, core: &Client) -> SendHandle {
        let core = core.clone();
        Arc::new(move |terminal: String, text: String, deadline: Duration| {
            let core = core.clone();
            Box::pin(async move {
                let params = json!({ "terminal": terminal, "text": text });
                core.call_timeout("terminal.send", Some(params), deadline)
                    .await
            })
        })
    }

    fn keys<'a>(
        &self,
        core: &'a Client,
        terminal: &str,
        keys: &[&str],
    ) -> Fut<'a, Result<Value, CallError>> {
        let params = json!({ "terminal": terminal, "keys": keys });
        Box::pin(core.call("terminal.keys", Some(params)))
    }

    fn label<'a>(
        &self,
        core: &'a Client,
        terminal: &str,
        labels: &BTreeMap<String, String>,
    ) -> Fut<'a, Result<Value, CallError>> {
        let params = json!({ "terminal": terminal, "labels": labels });
        Box::pin(core.call("terminal.label", Some(params)))
    }

    fn keep<'a>(
        &self,
        core: &'a Client,
        terminal: &str,
        keep: bool,
    ) -> Fut<'a, Result<Value, CallError>> {
        let params = json!({ "terminal": terminal, "keep": keep });
        Box::pin(core.call("terminal.keep", Some(params)))
    }

    fn supports(&self, core: &Client, method: &str) -> bool {
        core.explain_unavailable(method).is_none()
    }

    fn git(&self) -> RefMut<'_, Box<GitRun<'static>>> {
        self.git.borrow_mut()
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn write_file(&self, path: &Path, body: &[u8]) -> io::Result<()> {
        std::fs::write(path, body)
    }

    fn write_executable(&self, path: &Path, body: &str) -> io::Result<()> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, body)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::rename(&tmp, path)
    }

    fn read_file(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }
}
