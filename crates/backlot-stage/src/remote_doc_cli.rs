//! Person CLI routing through `plugin.backlot.req` -> `plugin.backlot.res`
//! when targeting a remote server (B2).

use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use backlot_model::req_wire::ReqAction;
use backlot_model::summary::Summary;
use hc_proto::client::Client;
use hc_proto::doc::DocGetParams;
use hc_proto::endpoint::{Endpoint, Sys};

use crate::args::{Cmd, Common, TextSource};
use crate::cli::{Env, Fail};
use crate::server_cfg::{self, SERVER_ENV, ServerTarget};

#[path = "remote_doc_fmt.rs"]
mod remote_doc_fmt;

pub(crate) use remote_doc_fmt::format_ack_json;
pub use remote_doc_fmt::{format_ack, format_hollywood_setup};

/// Exact user-facing refusal for commands not yet supported in B1a.
pub const ERR_NOT_AVAILABLE: &str = "not available through the server yet";

/// Bounded wait for a matching ack in `plugin.backlot.res`.
pub const DEFAULT_ACK_TIMEOUT: Duration = Duration::from_secs(5);

/// Result of translating a parsed CLI command for remote server execution.
pub enum MappedCmd {
    /// Action supported by B1a's `plugin.backlot.req` handlers.
    Action(ReqAction),
    /// B1a verb requiring `studio` (resolved from `backlot` doc if unique).
    NeedsStudio(Box<dyn FnOnce(String) -> ReqAction + Send>),
    /// Verb not yet covered by B1a.
    Unsupported,
}

fn read_text_source(source: &TextSource, stdin: Option<&str>) -> Result<String, Fail> {
    let body = match source {
        TextSource::Text(t) => t.clone(),
        TextSource::File(p) => fs::read_to_string(p)
            .map_err(|e| Fail::usage(format!("file not found: {}: {e}", p.display())))?,
        TextSource::Stdin => match stdin {
            Some(s) => s.to_owned(),
            None => {
                let mut buf = String::new();
                std::io::stdin()
                    .read_to_string(&mut buf)
                    .map_err(|e| Fail::refused(format!("stdin: {e}")))?;
                buf
            }
        },
    };
    if body.trim().is_empty() {
        return Err(Fail::refused("tell text cannot be empty"));
    }
    Ok(body)
}

/// A file or `-` for stdin, read on the client — the body travels as
/// content, never as a path the hub would resolve.
fn read_body(path: &Path, stdin: Option<&str>, flag: &str) -> Result<String, Fail> {
    if path == Path::new("-") {
        return match stdin {
            Some(s) => Ok(s.to_owned()),
            None => {
                let mut buf = String::new();
                std::io::stdin()
                    .read_to_string(&mut buf)
                    .map_err(|e| Fail::refused(format!("{flag} -: {e}")))?;
                Ok(buf)
            }
        };
    }
    if !path.is_file() {
        return Err(Fail::usage(format!(
            "{flag}: file not found: {}",
            path.display()
        )));
    }
    fs::read_to_string(path).map_err(|e| Fail::usage(format!("{flag}: {e}")))
}

fn check_user_caller(common: &Common, env: &Env) -> Result<(), Fail> {
    let role = common
        .role
        .as_deref()
        .or_else(|| env.get("BACKLOT_ROLE"))
        .unwrap_or("user");
    if role != "user" {
        return Err(Fail::refused(format!(
            "role '{role}' cannot run remote server commands (expected user)"
        )));
    }
    Ok(())
}

fn task_pin(common: &Common, env: &Env) -> Result<String, Fail> {
    common
        .task
        .clone()
        .or_else(|| env.get("BACKLOT_TASK").map(str::to_owned))
        .ok_or_else(|| Fail::usage("--task is required"))
}

fn with_studio(
    common: &Common,
    env: &Env,
    make: impl FnOnce(String) -> ReqAction + Send + 'static,
) -> MappedCmd {
    match common
        .studio
        .as_deref()
        .or_else(|| env.get("BACKLOT_STUDIO"))
    {
        Some(studio) => MappedCmd::Action(make(studio.to_owned())),
        None => MappedCmd::NeedsStudio(Box::new(make)),
    }
}

#[path = "remote_doc_exec.rs"]
mod remote_doc_exec;
#[path = "remote_doc_map.rs"]
mod remote_doc_map;

pub(crate) use remote_doc_exec::execute_client_with;
pub use remote_doc_exec::{execute_client_confirmed, execute_plan};

/// Convert a CLI `Cmd` into a [`MappedCmd`] without touching network I/O.
pub fn map_cmd(
    common: &Common,
    cmd: &Cmd,
    env: &Env,
    stdin: Option<&str>,
) -> Result<MappedCmd, Fail> {
    remote_doc_map::map(common, cmd, env, stdin)
}

/// Whether the command's output is its `--json` document.
pub fn wants_json(cmd: &Cmd) -> bool {
    match cmd {
        Cmd::StudioPrune { json, .. }
        | Cmd::StudioDelete { json, .. }
        | Cmd::StudioRestore { json, .. }
        | Cmd::Repos { json, .. }
        | Cmd::Runners { json } => *json,
        Cmd::StudioCast(crate::args::CastCmd::Show { json })
        | Cmd::StudioCast(crate::args::CastCmd::Set { json, .. }) => *json,
        _ => false,
    }
}

/// Send `action` on an already-connected `Client` and format the reply.
pub async fn execute_client(
    client: &Client,
    endpoint: Option<&Endpoint>,
    action: &ReqAction,
    timeout: Duration,
) -> Result<String, Fail> {
    execute_client_with(client, endpoint, action, timeout, false).await
}
async fn resolve_remote_studio(client: &Client) -> Result<String, Fail> {
    let doc = client
        .doc_get(DocGetParams {
            name: crate::doc::DOC_NAME.into(),
            on: None,
        })
        .await
        .map_err(|e| Fail::core(format!("reading server summary: {e}")))?;
    let summary: Summary =
        serde_json::from_value(doc.value).map_err(|_| Fail::refused("--studio is required"))?;
    match summary.studios.as_slice() {
        [only] => Ok(only.id.clone()),
        [] => Err(Fail::refused("no active studios on server")),
        _ => Err(Fail::usage(
            "--studio is required (multiple studios on server)",
        )),
    }
}

/// Execute a remote CLI command against `target` over `Endpoint::Remote`.
pub(crate) fn run(
    common: &Common,
    cmd: &Cmd,
    target: &ServerTarget,
    env: &Env,
    stdin: Option<&str>,
) -> Result<String, Fail> {
    let mapped = map_cmd(common, cmd, env, stdin)?;
    if matches!(mapped, MappedCmd::Unsupported) {
        return Err(Fail::refused(ERR_NOT_AVAILABLE));
    }
    let json = wants_json(cmd);
    let proto_env = crate::link::endpoint_env(env);
    let endpoint = server_cfg::resolve_endpoint_with(target, &proto_env, &Sys, None, None)
        .map_err(|e| Fail::refused(format!("cannot resolve server endpoint: {e}")))?;
    let rt = crate::link::runtime()?;
    rt.block_on(async move {
        let client = Client::connect(&endpoint, None)
            .await
            .map_err(|e| Fail::refused(format!("cannot reach server '{}': {e}", target.host)))?;
        let action = match mapped {
            MappedCmd::Action(a) => a,
            MappedCmd::NeedsStudio(make) => make(resolve_remote_studio(&client).await?),
            MappedCmd::Unsupported => unreachable!(),
        };
        execute_client_with(&client, Some(&endpoint), &action, DEFAULT_ACK_TIMEOUT, json).await
    })
}

/// Intercept CLI invocation when `--server`, `BACKLOT_SERVER`, or `[client]`
/// targets a server.
pub(crate) fn route(
    common: &mut Common,
    cmd: &Cmd,
    env: &Env,
    stdin: Option<&str>,
    local_hostname: Option<&str>,
    current_user: Option<&str>,
) -> Option<Result<String, Fail>> {
    if common.server.is_some() && (common.remote.is_some() || common.session.is_some()) {
        return Some(Err(Fail::usage(
            "--server cannot be combined with --remote or --session",
        )));
    }
    let explicit_env = env
        .get(SERVER_ENV)
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    let use_resolver = common.server.is_some()
        || (common.session.is_none()
            && common.remote.is_none()
            && (explicit_env || env.get("BACKLOT_STATE_DIR").is_none()));
    if !use_resolver {
        return None;
    }
    let target = match server_cfg::resolve(common.server.as_deref(), &env.vars) {
        Ok(Some(t)) => t,
        Ok(None) => return None,
        Err(e) => return Some(Err(e)),
    };
    if server_cfg::is_local_host_with(&target.host, local_hostname, current_user) {
        if common.session.is_none() {
            common.session = Some(target.session);
        }
        return None;
    }
    Some(run(common, cmd, &target, env, stdin))
}
