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

/// Wait a short window for a remote stage to prove itself live. A
/// finished terminal is removed either way, so a failed start leaves
/// no `backlot-stage` behind to block the next one.
async fn await_remote_start(
    client: &hc_proto::client::Client,
    address: &str,
    target: &str,
) -> Result<(), Fail> {
    let params = || TerminalParams {
        terminal: address.to_string(),
        caller: None,
    };
    for _ in 0..20 {
        let term = client
            .terminal_get(params())
            .await
            .map_err(|e| Fail::refused(format!("terminal.get failed: {e}")))?;
        match probe_state(&term) {
            Probe::Live => return Ok(()),
            Probe::Missing => {
                let _ = client.terminal_rm(params()).await;
                return Err(Fail::refused(not_installed(target)));
            }
            Probe::Exited(code) => {
                let _ = client.terminal_rm(params()).await;
                return Err(Fail::refused(format!(
                    "{STAGE_NAME} exited with code {code}"
                )));
            }
            Probe::Starting => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    Ok(())
}

async fn start_async(common: &Common, env: &Env) -> Result<String, Fail> {
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
        await_remote_start(&link.client, &run.terminal.address, &remote.target).await?;
    }
    Ok(format!("{STAGE_NAME} started ({})", run.terminal.address))
}

/// `stage start`: idempotent — an already-live name answers "already
/// running" and exits 0.
pub fn start(common: &Common, env: &Env) -> Result<String, Fail> {
    link::runtime()?.block_on(start_async(common, env))
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
