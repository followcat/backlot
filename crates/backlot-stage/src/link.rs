//! Reaching the hc core: endpoint resolution, one framed connection
//! and the core id the state directory is keyed by. Everything async
//! lives here so `cli` and the stage loop share one path.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use hc_proto::client::Client;
use hc_proto::endpoint::{self, Endpoint};

use crate::cli::{Env, Fail};
use crate::store;

/// A negotiated connection and who it is talking to.
pub struct Link {
    pub client: Client,
    /// The remote endpoint, when the core is on another machine.
    pub endpoint: Endpoint,
    /// `hello`'s permanent core id.
    pub core_id: String,
}

/// `cli::Env` (string map) as the endpoint layer's os-string map.
fn endpoint_env(env: &Env) -> endpoint::Env {
    env.vars
        .iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect()
}

/// Resolve `--session`/`--remote` over the given environment and open
/// one framed client. Errors tell the user the core could not be
/// reached and that `BACKLOT_STATE_DIR` skips this step.
pub async fn connect(
    env: &Env,
    session: Option<&OsStr>,
    remote: Option<&str>,
) -> Result<Link, Fail> {
    let endpoint = endpoint::resolve(&endpoint_env(env), session, remote, &endpoint::Sys)
        .map_err(|e| Fail::refused(format!("cannot resolve the hc endpoint: {e}")))?;
    let client = Client::connect(&endpoint, None).await.map_err(|e| {
        Fail::refused(format!(
            "cannot reach the hc core: {e}\nset BACKLOT_STATE_DIR to use a state directory without a core"
        ))
    })?;
    Ok(Link {
        core_id: client.hello().core.id.clone(),
        client,
        endpoint,
    })
}

/// The state base directory: `BACKLOT_STATE_DIR` wins and never needs
/// a core; otherwise one connection supplies the core id. Role
/// terminals always carry `BACKLOT_STATE_DIR`, so they never connect.
pub async fn state_dir(
    env: &Env,
    session: Option<&OsStr>,
    remote: Option<&str>,
) -> Result<PathBuf, Fail> {
    if let Some(d) = env.get("BACKLOT_STATE_DIR") {
        return Ok(PathBuf::from(d));
    }
    let link = connect(env, session, remote).await?;
    Ok(store::base_dir(&env.vars, &link.core_id))
}

/// Build a current-thread runtime; a failure is a refusal, not a panic.
pub fn runtime() -> Result<tokio::runtime::Runtime, Fail> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| Fail::refused(format!("cannot start a tokio runtime: {e}")))
}

/// `state_dir` for synchronous callers.
pub fn state_dir_sync(
    env: &Env,
    session: Option<&OsStr>,
    remote: Option<&str>,
) -> Result<PathBuf, Fail> {
    runtime()?.block_on(state_dir(env, session, remote))
}
