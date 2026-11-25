//! Execute B1a `ActionTracker` / `NsRun` actions over `plugin.backlot.req`/`res`
//! when connected to a server core (B2).

use std::collections::BTreeMap;
use std::time::Duration;

use backlot_stage::args;
use backlot_stage::cli::Env;
use backlot_stage::remote_doc_cli::{ERR_NOT_AVAILABLE, MappedCmd, execute_client, map_cmd};
use hc_proto::endpoint::Endpoint;
use hc_proto::watch::OpenRpc;

use crate::app::BoxFut;

/// Default bounded ack timeout in server mode.
pub(crate) const DEFAULT_ACK_TIMEOUT: Duration = Duration::from_secs(5);

/// Translate `argv` + `stdin` into a B1a request on `plugin.backlot.req`, wait for
/// the ack on `plugin.backlot.res`, and return `Ok(output)` or one single clear line
/// error (`"server busy"`, `"request expired"`, missing-ack timeout, or
/// `"not available through the server yet"`).
pub(crate) fn run(
    open: Option<OpenRpc>,
    endpoint: Option<Endpoint>,
    argv: Vec<String>,
    stdin: Option<String>,
    fallback_studio: Option<String>,
    timeout: Duration,
) -> BoxFut<Result<String, String>> {
    Box::pin(async move {
        let (common, cmd) = args::parse(&argv).map_err(|e| first_line(e.message()))?;
        let env = Env {
            vars: BTreeMap::from([("BACKLOT_ROLE".into(), "user".into())]),
        };
        let mapped =
            map_cmd(&common, &cmd, &env, stdin.as_deref()).map_err(|e| first_line(e.message()))?;
        let action = match mapped {
            MappedCmd::Action(a) => a,
            MappedCmd::NeedsStudio(make) => {
                let studio = fallback_studio.ok_or_else(|| "--studio is required".to_owned())?;
                make(studio)
            }
            MappedCmd::Unsupported => return Err(ERR_NOT_AVAILABLE.to_owned()),
        };
        let open = open.ok_or_else(|| "not connected to server".to_owned())?;
        let client = open().await.map_err(|e| first_line(&e.to_string()))?;
        execute_client(&client, endpoint.as_ref(), &action, timeout)
            .await
            .map_err(|e| first_line(e.message()))
    })
}

fn first_line(msg: &str) -> String {
    msg.lines()
        .find(|l| !l.trim().is_empty())
        .map(str::trim)
        .unwrap_or("request failed")
        .to_owned()
}
