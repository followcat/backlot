//! Execute `ActionTracker` / `NsRun` actions over `plugin.backlot.req`/`res`
//! when connected to a server core (B2/B1b).
//!
//! The tracker speaks argv, so the TUI's own extras ride as hidden
//! flags beside a command: `--plan` asks for a dry run's
//! `{ text, confirm_token }`, `--confirm-token` carries the token a
//! dialog reviewed, and `--ask-content` carries a multi-line first
//! message as base64 content (nothing is written on the client). They
//! are stripped here and never reach the CLI parser.

use std::time::Duration;

use backlot_stage::args;
use backlot_stage::cli::Env;
use backlot_stage::remote_doc_cli::{
    ERR_NOT_AVAILABLE, MappedCmd, execute_client_confirmed, execute_plan, map_cmd, wants_json,
};
use hc_proto::endpoint::Endpoint;
use hc_proto::watch::OpenRpc;
use serde_json::json;

use crate::app::BoxFut;

/// Default bounded ack timeout in server mode.
pub(crate) const DEFAULT_ACK_TIMEOUT: Duration = Duration::from_secs(5);

/// A plan-requesting dry run's own flag (TUI only).
pub(crate) const PLAN_FLAG: &str = "--plan";
/// A reviewed confirm token (TUI only).
pub(crate) const CONFIRM_FLAG: &str = "--confirm-token";
/// A first message sent as base64 content (TUI only).
pub(crate) const ASK_FLAG: &str = "--ask-content";

/// Translate `argv` + `stdin` into a B1a/B1b request on `plugin.backlot.req`,
/// wait for the ack on `plugin.backlot.res`, and return `Ok(output)` or one
/// single clear line error (`"server busy"`, `"request expired"`, missing-ack
/// timeout, or `"not available through the server yet"`).
pub(crate) fn run(
    open: Option<OpenRpc>,
    endpoint: Option<Endpoint>,
    argv: Vec<String>,
    stdin: Option<String>,
    fallback_studio: Option<String>,
    timeout: Duration,
) -> BoxFut<Result<String, String>> {
    Box::pin(async move {
        let mut argv = argv;
        let token = take_flag(&mut argv, CONFIRM_FLAG);
        let plan = take_switch(&mut argv, PLAN_FLAG);
        let ask = match take_flag(&mut argv, ASK_FLAG) {
            Some(text) => Some(
                crate::selection::base64_decode(&text)
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                    .ok_or_else(|| "bad ask content".to_owned())?,
            ),
            None => None,
        };
        let (mapped, json) = if argv.len() == 1 && matches!(argv[0].as_str(), "--version" | "-V") {
            (
                MappedCmd::Action(backlot_model::req_wire::ReqAction::Version),
                false,
            )
        } else {
            let (common, cmd) = args::parse(&argv).map_err(|e| first_line(e.message()))?;
            let json = wants_json(&cmd);
            let env = Env {
                vars: std::collections::BTreeMap::from([("BACKLOT_ROLE".into(), "user".into())]),
            };
            (
                map_cmd(&common, &cmd, &env, stdin.as_deref())
                    .map_err(|e| first_line(e.message()))?,
                json,
            )
        };
        let mut action = match mapped {
            MappedCmd::Action(a) => a,
            MappedCmd::NeedsStudio(make) => {
                let studio = fallback_studio.ok_or_else(|| "--studio is required".to_owned())?;
                make(studio)
            }
            MappedCmd::Unsupported => return Err(ERR_NOT_AVAILABLE.to_owned()),
        };
        if let (Some(ask), backlot_model::req_wire::ReqAction::StudioNew { ask: slot, .. }) =
            (ask, &mut action)
        {
            *slot = Some(ask);
        }
        let open = open.ok_or_else(|| "not connected to server".to_owned())?;
        let client = open().await.map_err(|e| first_line(&e.to_string()))?;
        if plan {
            let (text, token) = execute_plan(&client, endpoint.as_ref(), &action, timeout, json)
                .await
                .map_err(|e| first_line(e.message()))?;
            let wrapper = json!({ "text": text, "confirm_token": token });
            return Ok(wrapper.to_string());
        }
        execute_client_confirmed(&client, endpoint.as_ref(), &action, timeout, json, token)
            .await
            .map_err(|e| first_line(e.message()))
    })
}

/// Remove `flag <value>` (or a dangling `flag`) and answer the value.
fn take_flag(argv: &mut Vec<String>, flag: &str) -> Option<String> {
    let at = argv.iter().position(|a| a == flag)?;
    argv.remove(at);
    if at < argv.len() {
        Some(argv.remove(at))
    } else {
        None
    }
}

/// Remove a bare switch.
fn take_switch(argv: &mut Vec<String>, flag: &str) -> bool {
    let at = argv.iter().position(|a| a == flag);
    match at {
        Some(at) => {
            argv.remove(at);
            true
        }
        None => false,
    }
}

/// A `--plan` reply is `{ text, confirm_token }`; anything else is the
/// command's own output and carries no token.
pub(crate) fn split_plan_reply(text: &str) -> (String, Option<String>) {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim())
        && let (Some(body), Some(token)) = (
            value.get("text").and_then(|v| v.as_str()),
            value.get("confirm_token").and_then(|v| v.as_str()),
        )
    {
        return (body.to_owned(), Some(token.to_owned()));
    }
    (text.to_owned(), None)
}

/// Whether a refusal is the server's stale-plan line.
pub(crate) fn is_stale_plan(error: &str) -> bool {
    first_line(error) == backlot_model::req_results::ERR_STALE_PLAN
}

fn first_line(msg: &str) -> String {
    msg.lines()
        .find(|l| !l.trim().is_empty())
        .map(str::trim)
        .unwrap_or("request failed")
        .to_owned()
}
