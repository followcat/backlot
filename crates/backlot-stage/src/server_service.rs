//! `backlot server service [--print | --write] [--session <s>]`:
//! systemd `--user` unit `backlot-server.service` (or `backlot-server-<s>.service`).

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::server_lock::DEFAULT_SESSION;
use crate::service;

const DEFAULT_UNIT: &str = "backlot-server.service";

/// Generate or install the systemd `--user` unit for `backlot server run`.
pub(crate) fn run(common: &Common, env: &Env, write: bool) -> Result<String, Fail> {
    let session = common.session.as_deref().unwrap_or(DEFAULT_SESSION);
    let unit = unit_name(session)?;
    let exe = std::env::current_exe()
        .map_err(|e| Fail::refused(format!("current_exe: {e}")))?
        .to_string_lossy()
        .into_owned();
    let exec_start = format!(
        "{} server run --session {}",
        service::quote(&exe),
        service::quote(session)
    );
    let desc = if session == DEFAULT_SESSION {
        "backlot single server".to_string()
    } else {
        format!("backlot single server (hc session {session})")
    };
    let text = service::render_unit(
        &desc,
        env.get("BACKLOT_STATE_DIR"),
        &exec_start,
        "always",
        2,
    );
    if !write {
        return Ok(text);
    }
    service::write_unit(env, &unit, &text)
}

fn unit_name(session: &str) -> Result<String, Fail> {
    service::validate_session_name(session)?;
    if session == DEFAULT_SESSION {
        Ok(DEFAULT_UNIT.to_string())
    } else {
        Ok(format!("backlot-server-{session}.service"))
    }
}
