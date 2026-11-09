//! `backlot stage start` and `backlot stage run`.
//!
//! `start` asks the core for a terminal named `backlot-stage` running
//! `<this exe> stage run` — the unique-among-live name makes it one
//! stage per core. `run` executes the loop inside that terminal and
//! refuses anywhere else (`BACKLOT_STAGE=1` is the proof).

use std::collections::BTreeMap;

use hc_proto::endpoint::Endpoint;
use hc_proto::terminal::{ListParams, RunParams};

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::{daemon, link};

/// The stage terminal's name — unique among live terminals.
pub const STAGE_NAME: &str = "backlot-stage";

/// The exact `terminal.run` for the stage process.
fn stage_params(exe: &str, endpoint: &Endpoint, env: &Env) -> RunParams {
    let mut argv = vec![exe.to_string(), "stage".into(), "run".into()];
    let remote = match endpoint {
        Endpoint::Local(l) => {
            argv.extend(["--session".into(), l.session.clone()]);
            None
        }
        Endpoint::Remote(r) => {
            argv.extend(["--session".into(), r.session.clone()]);
            Some(r.target.clone())
        }
    };
    if let Some(target) = remote {
        argv.extend(["--remote".into(), target]);
    }
    let mut tenv = BTreeMap::from([("BACKLOT_STAGE".into(), "1".into())]);
    if let Some(d) = env.get("BACKLOT_STATE_DIR") {
        tenv.insert("BACKLOT_STATE_DIR".into(), d.into());
    }
    RunParams {
        argv,
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
