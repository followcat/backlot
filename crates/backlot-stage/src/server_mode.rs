//! `backlot server run|start|stop` and shared daemon-terminal control helpers.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::time::Duration;

use hc_proto::endpoint::Endpoint;
use hc_proto::terminal::{ListParams, RunParams, Terminal};

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::server_lock::{self, DEFAULT_SESSION, ServerLockGuard};
use crate::{daemon, link, stage};

/// Terminal name registered in `hc` when started via `backlot server start`.
pub(crate) const SERVER_NAME: &str = "backlot-server";
/// `herdr.tab` value differentiating `backlot-server` from legacy `backlot-stage`.
pub(crate) const SERVER_TAB: &str = "server";

/// Parameters customizing daemon terminal spawning for `backlot-stage` vs `backlot-server`.
pub(crate) struct DaemonSpawnSpec<'a> {
    pub(crate) name: &'a str,
    pub(crate) tab: &'a str,
    pub(crate) subcmd: &'a [&'a str],
    pub(crate) set_stage_env: bool,
    pub(crate) session_override: Option<&'a str>,
}

pub(crate) fn effective_session(common: &Common) -> &str {
    common.session.as_deref().unwrap_or(DEFAULT_SESSION)
}

/// Whether `t` is this core's own live/finished `backlot-server` terminal.
pub(crate) fn is_local_server_terminal(t: &Terminal) -> bool {
    !t.address.contains('/')
        && (t.name.as_deref() == Some(SERVER_NAME)
            || (t.labels.get("backlot.role").map(String::as_str) == Some("stage")
                && t.labels.get("herdr.tab").map(String::as_str) == Some(SERVER_TAB)))
}

/// Whether `t` is a legacy `backlot-stage` terminal (never matching `backlot-server`).
pub(crate) fn is_legacy_stage_terminal(t: &Terminal) -> bool {
    !t.address.contains('/') && stage::is_local_stage(t) && !is_local_server_terminal(t)
}

/// Build `terminal.run` params for `backlot-stage` or `backlot-server`.
pub(crate) fn daemon_params(
    exe: &str,
    endpoint: &Endpoint,
    env: &Env,
    spec: DaemonSpawnSpec<'_>,
) -> RunParams {
    let mut tenv = BTreeMap::new();
    if spec.set_stage_env {
        tenv.insert("BACKLOT_STAGE".into(), "1".into());
        if let Some(ms) = env.get("BACKLOT_HOLLYWOOD_PREVIOUS_MS") {
            tenv.insert("BACKLOT_HOLLYWOOD_PREVIOUS_MS".into(), ms.into());
        }
    }
    let (argv, line) = match endpoint {
        Endpoint::Local(local) => {
            if let Some(d) = env.get("BACKLOT_STATE_DIR") {
                tenv.insert("BACKLOT_STATE_DIR".into(), d.into());
            }
            let session = spec.session_override.unwrap_or(&local.session).to_string();
            let mut v = vec![exe.to_string()];
            v.extend(spec.subcmd.iter().map(|s| (*s).to_string()));
            v.push("--session".into());
            v.push(session);
            (v, None)
        }
        Endpoint::Remote(remote) => {
            let session = spec.session_override.unwrap_or(&remote.session);
            let cmd = spec.subcmd.join(" ");
            let line = format!(
                "command -v backlot >/dev/null 2>&1 || exit 127; exec backlot {cmd} --session {}",
                sh_quote(session)
            );
            (Vec::new(), Some(line))
        }
    };
    RunParams {
        argv,
        line,
        name: Some(spec.name.into()),
        labels: BTreeMap::from([
            ("backlot.role".into(), "stage".into()),
            ("herdr.space".into(), "backlot".into()),
            ("herdr.tab".into(), spec.tab.into()),
            ("herdr.seen".into(), "0".into()),
        ]),
        env: tenv,
        no_parent: true,
        no_notify: true,
        ..RunParams::default()
    }
}

fn sh_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// Best-effort removal of finished local daemon tombstones matching `pred`.
pub(crate) async fn reap_finished_matching(
    driver: &dyn crate::driver::MachineDriver,
    client: &hc_proto::client::Client,
    terminals: &[Terminal],
    pred: impl Fn(&Terminal) -> bool,
) {
    for t in terminals
        .iter()
        .filter(|t| t.state == "finished" && pred(t))
    {
        let _ = driver.rm(client, &t.address).await;
    }
}

/// Kill one terminal cleanly, wait briefly for exit, and remove its tombstone.
pub(crate) async fn stop_terminal(
    driver: &dyn crate::driver::MachineDriver,
    client: &hc_proto::client::Client,
    terminal: &str,
) -> Result<(), Fail> {
    driver
        .kill(client, terminal)
        .await
        .map_err(|e| Fail::refused(format!("terminal.kill failed: {e}")))?;
    for _ in 0..10 {
        match driver.get(client, terminal).await {
            Ok(t) if t.state == "finished" => {
                let _ = driver.rm(client, terminal).await;
                break;
            }
            Ok(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            Err(_) => break,
        }
    }
    Ok(())
}

/// `backlot server run [--session <s>]`: acquire `server.lock` and run the shared daemon loop directly.
pub(crate) fn run(common: &Common, env: &Env) -> Result<String, Fail> {
    if common.remote.is_some() {
        return Err(Fail::refused(
            "server run runs directly on the server host, not over --remote",
        ));
    }
    let session = effective_session(common);
    let base = server_lock::server_dir(&env.vars);
    let mut guard = ServerLockGuard::acquire(&base, session)?;
    ensure_layout(&base)?;

    let mut run_common = common.clone();
    run_common.session = Some(session.to_string());
    link::runtime()?.block_on(daemon::run_with(&run_common, base, env, Some(&mut guard)))?;
    Ok(String::new())
}

/// `backlot server start [--session <s>] [--remote <t>]`.
pub(crate) fn start(common: &Common, env: &Env) -> Result<String, Fail> {
    let session = effective_session(common);
    link::runtime()?.block_on(start_async(common, session, env))
}

/// `backlot server stop [--session <s>] [--remote <t>]`.
pub(crate) fn stop(common: &Common, env: &Env) -> Result<String, Fail> {
    let session = effective_session(common);
    link::runtime()?.block_on(stop_async(common, session, env))
}

async fn start_async(common: &Common, session: &str, env: &Env) -> Result<String, Fail> {
    let lnk = link::connect(env, Some(OsStr::new(session)), common.remote.as_deref()).await?;
    let driver = crate::driver::LocalDriver::new();
    let list = lnk
        .client
        .terminal_list(ListParams {
            all: true,
            ..ListParams::default()
        })
        .await
        .map_err(|e| Fail::refused(format!("terminal.list failed: {e}")))?;

    reap_finished_matching(
        &driver,
        &lnk.client,
        &list.terminals,
        is_local_server_terminal,
    )
    .await;
    if let Some(t) = list
        .terminals
        .iter()
        .find(|t| t.state != "finished" && is_local_server_terminal(t))
    {
        return Ok(format!("{SERVER_NAME} is already running ({})", t.address));
    }
    if common.remote.is_none()
        && let Some(srv) = server_lock::running_server(&env.vars)
    {
        return Err(server_lock::running_server_refusal(&srv));
    }

    stage::spawn_daemon_terminal(
        &lnk,
        env,
        DaemonSpawnSpec {
            name: SERVER_NAME,
            tab: SERVER_TAB,
            subcmd: &["server", "run"],
            set_stage_env: false,
            session_override: Some(session),
        },
    )
    .await
}

async fn stop_async(common: &Common, session: &str, env: &Env) -> Result<String, Fail> {
    let lnk = link::connect(env, Some(OsStr::new(session)), common.remote.as_deref()).await?;
    let driver = crate::driver::LocalDriver::new();
    let list = lnk
        .client
        .terminal_list(ListParams {
            all: true,
            ..ListParams::default()
        })
        .await
        .map_err(|e| Fail::refused(format!("terminal.list failed: {e}")))?;

    reap_finished_matching(
        &driver,
        &lnk.client,
        &list.terminals,
        is_local_server_terminal,
    )
    .await;
    let target = list
        .terminals
        .iter()
        .find(|t| t.state == "live" && is_local_server_terminal(t))
        .map(|t| t.address.clone());
    match target {
        Some(addr) => {
            stop_terminal(&driver, &lnk.client, &addr).await?;
            Ok("server stopped".to_string())
        }
        None => Ok("server already stopped".to_string()),
    }
}

fn ensure_layout(base: &Path) -> Result<(), Fail> {
    for sub in ["studios", "hollywood", "archive"] {
        fs::create_dir_all(base.join(sub))
            .map_err(|e| Fail::refused(format!("create {}/{sub}: {e}", base.display())))?;
    }
    Ok(())
}
