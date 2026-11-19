//! `backlot stage start` and `backlot stage run`.
//!
//! `start` asks the core for a terminal named `backlot-stage` running
//! `<this exe> stage run` — the unique-among-live name makes it one
//! stage per core. `run` executes the loop inside that terminal and
//! refuses anywhere else (`BACKLOT_STAGE=1` is the proof).

use std::collections::BTreeMap;
use std::time::Duration;

use hc_proto::endpoint::Endpoint;
use hc_proto::terminal::{ListParams, RunParams, Terminal, TerminalParams};

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::{daemon, link};

/// The stage terminal's name — unique among live terminals.
pub const STAGE_NAME: &str = "backlot-stage";

/// POSIX single-quote escaping for one shell word.
fn sh_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// The message for a remote host without `backlot` — names the target
/// and the fix.
fn not_installed(target: &str) -> String {
    format!(
        "backlot is not installed on {target}: install it there (curl -fsSL \
         https://github.com/followcat/backlot/releases/latest/download/install.sh | \
         BACKLOT_SKIP_CORE=1 sh) or run backlot on that host"
    )
}

/// The remote stage's shell line: the host's own `backlot`, and exit
/// 127 when it is missing so `start` can say why.
fn remote_stage_line(session: &str) -> String {
    format!(
        "command -v backlot >/dev/null 2>&1 || exit 127; exec backlot stage run --session {}",
        sh_quote(session)
    )
}

/// The exact `terminal.run` for the stage process. Local: this exe,
/// this state dir. Remote: the core's host is the one that runs the
/// terminal, so its `backlot` from `PATH`, `--session` alone, and no
/// `BACKLOT_STATE_DIR` — this machine's path means nothing there.
fn stage_params(exe: &str, endpoint: &Endpoint, env: &Env) -> RunParams {
    let mut tenv = BTreeMap::from([("BACKLOT_STAGE".into(), "1".into())]);
    let (argv, line) = match endpoint {
        Endpoint::Local(local) => {
            if let Some(d) = env.get("BACKLOT_STATE_DIR") {
                tenv.insert("BACKLOT_STATE_DIR".into(), d.into());
            }
            (
                vec![
                    exe.to_string(),
                    "stage".into(),
                    "run".into(),
                    "--session".into(),
                    local.session.clone(),
                ],
                None,
            )
        }
        Endpoint::Remote(remote) => (Vec::new(), Some(remote_stage_line(&remote.session))),
    };
    RunParams {
        argv,
        line,
        name: Some(STAGE_NAME.into()),
        labels: BTreeMap::from([
            ("backlot.role".into(), "stage".into()),
            ("herdr.space".into(), "backlot".into()),
            ("herdr.tab".into(), "stage".into()),
            ("herdr.seen".into(), "0".into()),
        ]),
        env: tenv,
        no_parent: true,
        no_notify: true,
        ..RunParams::default()
    }
}

/// What a remote stage start's polls learned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Probe {
    /// The stage program is up.
    Live,
    /// Still spawning; look again.
    Starting,
    /// The host has no `backlot`: the shell exited 127.
    Missing,
    /// The stage program exited with this code.
    Exited(i32),
}

fn probe_state(terminal: &Terminal) -> Probe {
    if terminal.state == "live" {
        return Probe::Live;
    }
    if terminal.state != "finished" {
        return Probe::Starting;
    }
    match terminal.exit_code {
        Some(127) => Probe::Missing,
        Some(code) => Probe::Exited(code),
        None => Probe::Exited(-1),
    }
}

/// One poll interval of the remote start's settle window.
const POLL: Duration = Duration::from_millis(100);
/// A remote stage must stay live this long before it counts as
/// started: the core runs the line as `sh -c`, so the wrapper shell is
/// live while it is still checking `command -v backlot`, and a very
/// early stage failure must not read as a start.
const SETTLE: Duration = Duration::from_millis(600);
/// The overall bound while a terminal is neither live nor finished.
const BOUND: Duration = Duration::from_millis(2000);

/// The polling seam: a remote stage terminal and the clock. Tests
/// script the probes and advance time without a core or a sleep.
trait StageHost {
    async fn probe(&self) -> Result<Probe, Fail>;
    async fn remove(&self);
    fn elapsed(&self) -> Duration;
    async fn tick(&self);
}

/// The real core's stage terminal, measured from the `terminal.run`
/// result.
struct RemoteStage<'a> {
    client: &'a hc_proto::client::Client,
    address: &'a str,
    started: std::time::Instant,
}

impl RemoteStage<'_> {
    fn params(&self) -> TerminalParams {
        TerminalParams {
            terminal: self.address.to_string(),
            caller: None,
        }
    }
}

impl StageHost for RemoteStage<'_> {
    async fn probe(&self) -> Result<Probe, Fail> {
        let term = self
            .client
            .terminal_get(self.params())
            .await
            .map_err(|e| Fail::refused(format!("terminal.get failed: {e}")))?;
        Ok(probe_state(&term))
    }
    async fn remove(&self) {
        let _ = self.client.terminal_rm(self.params()).await;
    }
    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
    async fn tick(&self) {
        tokio::time::sleep(POLL).await;
    }
}

/// What one remote-start observation means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// Keep polling.
    Wait,
    /// The stage stayed live through the settle window.
    Started,
    /// The host has no `backlot`; the terminal is removed.
    Missing,
    /// The stage exited with this code; the terminal is removed.
    Exited(i32),
}

fn start_verdict(probe: Probe, elapsed: Duration) -> Verdict {
    match probe {
        Probe::Missing => Verdict::Missing,
        Probe::Exited(code) => Verdict::Exited(code),
        Probe::Live if elapsed >= SETTLE => Verdict::Started,
        Probe::Live | Probe::Starting if elapsed >= BOUND => Verdict::Started,
        _ => Verdict::Wait,
    }
}

/// Wait for a remote stage to prove itself live. A `live` sighting
/// only counts after the settle window — the core runs the line as
/// `sh -c`, so the wrapper shell is live while it checks for
/// `backlot`. A finished terminal is removed either way, so a failed
/// start leaves no `backlot-stage` behind to block the next one.
async fn await_remote_start<S: StageHost>(stage: &S, target: &str) -> Result<(), Fail> {
    loop {
        let verdict = start_verdict(stage.probe().await?, stage.elapsed());
        match verdict {
            Verdict::Started => return Ok(()),
            Verdict::Missing => {
                stage.remove().await;
                return Err(Fail::refused(not_installed(target)));
            }
            Verdict::Exited(code) => {
                stage.remove().await;
                return Err(Fail::refused(format!(
                    "{STAGE_NAME} exited with code {code}"
                )));
            }
            Verdict::Wait => stage.tick().await,
        }
    }
}

async fn start_async(common: &Common, env: &Env) -> Result<String, Fail> {
    let link = link::connect(
        env,
        common.session.as_deref().map(std::ffi::OsStr::new),
        common.remote.as_deref(),
    )
    .await?;
    start_on(&link, env).await
}

/// The idempotent start on an already-connected core — `stage start`
/// and `stage watch --restart` share this one path.
pub(crate) async fn start_on(link: &link::Link, env: &Env) -> Result<String, Fail> {
    let list = link
        .client
        .terminal_list(ListParams::default())
        .await
        .map_err(|e| Fail::refused(format!("terminal.list failed: {e}")))?;
    if let Some(t) = list
        .terminals
        .iter()
        .find(|t| t.state == "live" && t.name.as_deref() == Some(STAGE_NAME))
    {
        return Ok(format!("{STAGE_NAME} is already running ({})", t.address));
    }
    let exe = std::env::current_exe()
        .map_err(|e| Fail::refused(format!("cannot find this executable: {e}")))?
        .to_string_lossy()
        .into_owned();
    let run = link
        .client
        .terminal_run(stage_params(&exe, &link.endpoint, env))
        .await
        .map_err(|e| Fail::refused(format!("terminal.run failed: {e}")))?;
    if let Endpoint::Remote(remote) = &link.endpoint {
        let stage = RemoteStage {
            client: &link.client,
            address: &run.terminal.address,
            started: std::time::Instant::now(),
        };
        await_remote_start(&stage, &remote.target).await?;
    }
    Ok(format!("{STAGE_NAME} started ({})", run.terminal.address))
}

/// `stage start`: idempotent — an already-live name answers "already
/// running" and exits 0.
pub fn start(common: &Common, env: &Env) -> Result<String, Fail> {
    link::runtime()?.block_on(start_async(common, env))
}

/// The stop verdict (G1): the live stage terminal's address to kill
/// plus the report — `None` is "already stopped". Both exit 0.
fn stop_verdict(terminals: &[Terminal]) -> (Option<String>, &'static str) {
    match terminals
        .iter()
        .find(|t| t.state == "live" && t.name.as_deref() == Some(STAGE_NAME))
    {
        Some(t) => (Some(t.address.clone()), "stage stopped"),
        None => (None, "stage already stopped"),
    }
}

/// `stage stop` (G1): `terminal.kill` on the core's live
/// `backlot-stage` — the core's own graceful end. No live stage is
/// still an exit-0 answer.
async fn stop_async(common: &Common, env: &Env) -> Result<String, Fail> {
    let link = link::connect(
        env,
        common.session.as_deref().map(std::ffi::OsStr::new),
        common.remote.as_deref(),
    )
    .await?;
    let list = link
        .client
        .terminal_list(ListParams::default())
        .await
        .map_err(|e| Fail::refused(format!("terminal.list failed: {e}")))?;
    let (address, report) = stop_verdict(&list.terminals);
    if let Some(terminal) = address {
        link.client
            .terminal_kill(TerminalParams {
                terminal,
                caller: None,
            })
            .await
            .map_err(|e| Fail::refused(format!("terminal.kill failed: {e}")))?;
    }
    Ok(report.into())
}

pub fn stop(common: &Common, env: &Env) -> Result<String, Fail> {
    link::runtime()?.block_on(stop_async(common, env))
}

/// `stage run`: only inside the stage terminal.
pub fn run(common: &Common, env: &Env) -> Result<String, Fail> {
    if env.get("BACKLOT_STAGE") != Some("1") {
        return Err(Fail::refused(
            "stage run belongs inside the stage terminal; start it with 'backlot stage start'",
        ));
    }
    link::runtime()?.block_on(daemon::run(common, env))?;
    Ok(String::new())
}

#[cfg(test)]
#[path = "stage_tests.rs"]
mod tests;
