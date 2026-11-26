//! User-initiated Hollywood switch CLI and server-RPC verb handlers
//! (`backlot hollywood switch`, `--cancel`, `--status`, and `--wait`).

use std::path::Path;
use std::time::Duration;

use backlot_model::req_results::{HollywoodSwitchResult, HollywoodSwitchStatusResult};
use backlot_model::summary::HandoffSummary;

use crate::args::Common;
use crate::cli::{Env, Fail};
use crate::config;
use crate::handoff::{
    DEFAULT_TIMEOUT_MS, HandoffPhase, HandoffRecord, load_handoff, request_cancel,
    stage_handoff_request,
};
use crate::hsetup::validate_hollywood_target;
use crate::hstage;
use crate::hwood::base_of;
use crate::live::{LiveView, now_ms};
use crate::proc_guard::{CliVerb, check_cli_gate};
use crate::remote_doc_cli::{format_hollywood_switch, format_hollywood_switch_status};
use crate::store;

const WAIT_POLL_MS: u64 = 50;
const WAIT_SLACK_MS: u64 = 30_000;
const OFFLINE_WAIT_GRACE_MS: u64 = 5_000;

/// Enforce that only the human user (`--as user` or unset, never inside
/// a role terminal or with `BACKLOT_ROLE` set) can run `hollywood switch`
/// or `hollywood setup`.
pub(crate) fn check_user_control(common: &Common, env: &Env) -> Result<(), Fail> {
    check_cli_gate(CliVerb::HollywoodControl, common, env)?;
    if let Some(r) = common.role.as_deref()
        && r != "user"
    {
        return Err(Fail::refused(format!(
            "role '{r}' cannot switch hollywood; the user can"
        )));
    }
    Ok(())
}

fn check_target_differs(
    hdir: &Path,
    env: &Env,
    runner: &str,
    model: &str,
    effort: Option<&str>,
    force: bool,
) -> Result<(), Fail> {
    if force {
        return Ok(());
    }
    let hstate = store::load_hstate(hdir).unwrap_or_default();
    let cfg = config::hollywood(&env.vars).ok().flatten();
    let current = if hstate.role.live {
        hstate
            .role
            .actor
            .or_else(|| cfg.as_ref().map(hstage::identity))
    } else {
        cfg.as_ref().map(hstage::identity)
    };
    if let Some(cur) = current
        && cur.runner == runner
        && cur.model == model
        && cur.effort.as_deref() == effort
    {
        return Err(Fail::refused(format!(
            "hollywood is already running {} (pass --force to hand off to a fresh session anyway)",
            backlot_model::recast::identity_text(&cur)
        )));
    }
    Ok(())
}

fn stage_handoff_validated(
    base: &Path,
    env: &Env,
    runner: &str,
    model: &str,
    effort: Option<&str>,
    timeout_s: Option<u64>,
    force: bool,
) -> Result<HandoffRecord, Fail> {
    validate_hollywood_target(env, runner, model, effort)?;
    let hdir = store::hollywood_dir(base);
    check_target_differs(&hdir, env, runner, model, effort, force)?;
    let timeout_ms = timeout_s.map_or(DEFAULT_TIMEOUT_MS, |s| s.saturating_mul(1_000));
    stage_handoff_request(
        &hdir,
        runner,
        model,
        effort,
        timeout_ms,
        force,
        "user switch",
        None,
        now_ms(),
    )
    .map_err(Fail::refused)
}

fn render_switch_result(
    action: &str,
    handoff: Option<HandoffSummary>,
    json: bool,
) -> Result<String, Fail> {
    let res = HollywoodSwitchResult {
        action: action.to_string(),
        handoff,
    };
    if json {
        serde_json::to_string_pretty(&res).map_err(|e| Fail::refused(e.to_string()))
    } else {
        Ok(format_hollywood_switch(&res))
    }
}

fn render_status_result(rec: Option<HandoffRecord>, json: bool) -> Result<String, Fail> {
    let active = rec.as_ref().is_some_and(|r| r.phase.is_active());
    let res = HollywoodSwitchStatusResult {
        active,
        handoff: rec.map(|r| r.to_summary()),
    };
    if json {
        serde_json::to_string_pretty(&res).map_err(|e| Fail::refused(e.to_string()))
    } else if res.handoff.is_none() {
        Ok("no hollywood handoff recorded".to_string())
    } else {
        Ok(format_hollywood_switch_status(&res))
    }
}

fn format_rollback_err(rec: &HandoffRecord, json: bool) -> Fail {
    if json {
        let res = HollywoodSwitchResult {
            action: "rolled_back".to_string(),
            handoff: Some(rec.to_summary()),
        };
        if let Ok(s) = serde_json::to_string_pretty(&res) {
            return Fail::refused(s);
        }
    }
    let msg = rec
        .error
        .clone()
        .unwrap_or_else(|| "hollywood handoff rolled back".to_string());
    Fail::refused(msg)
}

fn wait_for_handoff(
    base: &Path,
    hdir: &Path,
    initial: &HandoffRecord,
    json: bool,
) -> Result<String, Fail> {
    let start = now_ms();
    let deadline_ms = initial.timeout_ms.saturating_add(WAIT_SLACK_MS);
    let mut last_phase = initial.phase;
    let mut offline_since: Option<u64> = None;
    if !json {
        eprintln!(
            "[hollywood] switch {} -> {}/{} ({})",
            initial.id,
            initial.target_runner,
            initial.target_model,
            initial.phase.as_str()
        );
    }
    loop {
        let cur_ms = now_ms();
        let rec = load_handoff(hdir).map_err(Fail::refused)?;
        let Some(rec) = rec else {
            return Err(Fail::refused("handoff state disappeared while waiting"));
        };
        if rec.id != initial.id {
            return Err(Fail::refused(format!(
                "handoff '{}' replaced by '{}' while waiting",
                initial.id, rec.id
            )));
        }
        if rec.phase != last_phase {
            last_phase = rec.phase;
            offline_since = None;
            if !json {
                eprintln!("[hollywood] switch: {}", rec.phase.as_str());
            }
        }
        match rec.phase {
            HandoffPhase::Committed => {
                return render_switch_result("committed", Some(rec.to_summary()), json);
            }
            HandoffPhase::RolledBack => return Err(format_rollback_err(&rec, json)),
            _ => {}
        }
        if cur_ms.saturating_sub(start) >= deadline_ms {
            return Err(Fail::refused(
                "timed out waiting for stage to finish handoff",
            ));
        }
        if LiveView::read(base, cur_ms).running {
            offline_since = None;
        } else {
            let since = *offline_since.get_or_insert(cur_ms);
            if cur_ms.saturating_sub(since) >= OFFLINE_WAIT_GRACE_MS {
                return Err(Fail::refused(
                    "stage stopped running while waiting for handoff",
                ));
            }
        }
        std::thread::sleep(Duration::from_millis(WAIT_POLL_MS));
    }
}

/// Local CLI handler for `backlot hollywood switch ...`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn switch_local(
    common: &Common,
    env: &Env,
    runner: &str,
    model: &str,
    effort: Option<&str>,
    timeout_s: Option<u64>,
    force: bool,
    no_wait: bool,
    json: bool,
) -> Result<String, Fail> {
    check_user_control(common, env)?;
    let base = base_of(common, env)?;
    if !LiveView::read(&base, now_ms()).running {
        return Err(Fail::refused(
            "stage is not running — start the stage for a live handoff, or use 'backlot hollywood setup' to edit config.toml offline",
        ));
    }
    let rec = stage_handoff_validated(&base, env, runner, model, effort, timeout_s, force)?;
    if no_wait {
        return render_switch_result("queued", Some(rec.to_summary()), json);
    }
    let hdir = store::hollywood_dir(&base);
    wait_for_handoff(&base, &hdir, &rec, json)
}

/// Local CLI handler for `backlot hollywood switch --cancel`.
pub(crate) fn cancel_local(common: &Common, env: &Env, json: bool) -> Result<String, Fail> {
    check_user_control(common, env)?;
    let base = base_of(common, env)?;
    let res = rpc_cancel(&base, env)?;
    if json {
        serde_json::to_string_pretty(&res).map_err(|e| Fail::refused(e.to_string()))
    } else {
        Ok(format_hollywood_switch(&res))
    }
}

/// Local CLI handler for `backlot hollywood switch --status`.
pub(crate) fn status_local(common: &Common, env: &Env, json: bool) -> Result<String, Fail> {
    check_user_control(common, env)?;
    let base = base_of(common, env)?;
    let hdir = store::hollywood_dir(&base);
    let rec = load_handoff(&hdir).map_err(Fail::refused)?;
    render_status_result(rec, json)
}

/// Server-side `plugin.backlot.req` handler for `hollywood-switch`.
pub(crate) fn rpc_switch(
    base: &Path,
    env: &Env,
    runner: &str,
    model: &str,
    effort: Option<&str>,
    timeout_s: Option<u64>,
    force: bool,
) -> Result<HollywoodSwitchResult, Fail> {
    if env.get("BACKLOT_ROLE").is_some_and(|r| !r.is_empty()) {
        return Err(Fail::refused(
            "switching hollywood is the person's call — BACKLOT_ROLE is set",
        ));
    }
    let rec = stage_handoff_validated(base, env, runner, model, effort, timeout_s, force)?;
    Ok(HollywoodSwitchResult {
        action: "queued".to_string(),
        handoff: Some(rec.to_summary()),
    })
}

/// Server-side `plugin.backlot.req` handler for `hollywood-switch-cancel`.
pub(crate) fn rpc_cancel(base: &Path, env: &Env) -> Result<HollywoodSwitchResult, Fail> {
    if env.get("BACKLOT_ROLE").is_some_and(|r| !r.is_empty()) {
        return Err(Fail::refused(
            "cancelling hollywood switch is the person's call — BACKLOT_ROLE is set",
        ));
    }
    let hdir = store::hollywood_dir(base);
    let rec = request_cancel(&hdir).map_err(Fail::refused)?;
    Ok(HollywoodSwitchResult {
        action: "cancel-requested".to_string(),
        handoff: Some(rec.to_summary()),
    })
}

/// Server-side `plugin.backlot.req` handler for `hollywood-switch-status`.
pub(crate) fn rpc_status(base: &Path, _env: &Env) -> Result<HollywoodSwitchStatusResult, Fail> {
    let hdir = store::hollywood_dir(base);
    let rec = load_handoff(&hdir).map_err(Fail::refused)?;
    let active = rec.as_ref().is_some_and(|r| r.phase.is_active());
    Ok(HollywoodSwitchStatusResult {
        active,
        handoff: rec.map(|r| r.to_summary()),
    })
}
