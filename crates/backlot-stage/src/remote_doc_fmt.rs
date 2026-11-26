//! Result formatting helpers for `plugin.backlot.res` acknowledgements.

use backlot_model::req_results::{
    AskAcknowledgeResult, AttentionClearResult, CardApplyResult, CardCancelResult, CardShowResult,
    CloseResult, DesignSignResult, DesignStartRoundResult, HollywoodSetupResult, RecastResult,
    ReopenResult, RestartResult, StudioFinishResult, StudioNewResult, StudioPlan,
    StudioReopenResult, TaskPriorityResult, TaskStateResult, TellHollywoodResult, TellResult,
};
use backlot_model::req_wire::{ReqAction, ResAck};
use serde::de::DeserializeOwned;

use crate::cli::Fail;
use crate::delete::DeleteReport;
use crate::prune::PruneReport;
use crate::restore::RestoreReport;
use crate::{verbs_card_show, verbs_design, verbs_fmt, verbs_hwood};

fn parse_res<T: DeserializeOwned>(ack: &ResAck) -> Result<T, Fail> {
    let val = ack
        .result
        .clone()
        .ok_or_else(|| Fail::core("server ack omitted result payload"))?;
    serde_json::from_value(val).map_err(|e| Fail::core(format!("malformed server result: {e}")))
}

/// Format `HollywoodSetupResult` in one user-facing line.
pub fn format_hollywood_setup(r: &HollywoodSetupResult) -> String {
    let note = if r.restarted {
        " — hollywood restarted"
    } else {
        " — stage sees it on its next tick"
    };
    format!("[hollywood] {} ({} {}){note}", r.action, r.runner, r.model)
}

/// Format `HollywoodSwitchResult` in one user-facing line.
pub fn format_hollywood_switch(r: &backlot_model::req_results::HollywoodSwitchResult) -> String {
    match &r.handoff {
        Some(h) => {
            let eff = h
                .target_effort
                .as_deref()
                .map(|e| format!("/{e}"))
                .unwrap_or_default();
            if let Some(err) = &h.error {
                format!(
                    "[hollywood] switch {} ({}/{}{eff}): {err}",
                    r.action, h.target_runner, h.target_model
                )
            } else {
                format!(
                    "[hollywood] switch {} -> {}/{}{eff} (id {}, phase {})",
                    r.action, h.target_runner, h.target_model, h.id, h.phase
                )
            }
        }
        None => format!("[hollywood] switch {}", r.action),
    }
}

/// Format `HollywoodSwitchStatusResult` in one user-facing line.
pub fn format_hollywood_switch_status(
    r: &backlot_model::req_results::HollywoodSwitchStatusResult,
) -> String {
    match &r.handoff {
        Some(h) => {
            let eff = h
                .target_effort
                .as_deref()
                .map(|e| format!("/{e}"))
                .unwrap_or_default();
            match &h.error {
                Some(err) => format!(
                    "handoff {}: {} -> {}/{}{eff}: {err}",
                    h.id, h.phase, h.target_runner, h.target_model
                ),
                None => format!(
                    "handoff {}: {} -> {}/{}{eff}",
                    h.id, h.phase, h.target_runner, h.target_model
                ),
            }
        }
        None => "no active handoff".to_string(),
    }
}

/// A destructive verb's dry-run report, unwrapped from its plan.
fn report<T: DeserializeOwned>(ack: &ResAck, dry_run: bool) -> Result<T, Fail> {
    if dry_run {
        Ok(parse_res::<StudioPlan<T>>(ack)?.report)
    } else {
        parse_res::<T>(ack)
    }
}

/// Format a positive (`ok == true`) or negative (`ok == false`) `ResAck`.
pub fn format_ack(action: &ReqAction, ack: ResAck) -> Result<String, Fail> {
    format_ack_json(action, ack, false)
}

/// The same, printing a command's `--json` document when it asked for
/// one.
pub fn format_ack_json(action: &ReqAction, ack: ResAck, json: bool) -> Result<String, Fail> {
    if !ack.ok {
        let msg = ack
            .error
            .as_deref()
            .and_then(|e| e.lines().find(|l| !l.trim().is_empty()))
            .map(str::trim)
            .unwrap_or("request refused");
        return Err(Fail::refused(msg));
    }
    let out = match action {
        ReqAction::Tell { .. } => verbs_fmt::format_tell(&parse_res::<TellResult>(&ack)?),
        ReqAction::TellHollywood { .. } => {
            verbs_hwood::format_tell_hollywood(&parse_res::<TellHollywoodResult>(&ack)?)
        }
        ReqAction::CardApply { .. } => parse_res::<CardApplyResult>(&ack)?.summary,
        ReqAction::CardCancel { .. } => {
            verbs_hwood::format_card_cancel(&parse_res::<CardCancelResult>(&ack)?)
        }
        ReqAction::CardShow { .. } => {
            verbs_card_show::format_card_show(&parse_res::<CardShowResult>(&ack)?)
        }
        ReqAction::AskAcknowledge { .. } => {
            let r: AskAcknowledgeResult = parse_res(&ack)?;
            format!("question #{} in {} marked seen", r.target, r.studio)
        }
        ReqAction::AttentionClear { .. } => {
            verbs_fmt::format_attention_clear(&parse_res::<AttentionClearResult>(&ack)?)
        }
        ReqAction::Recast { .. } => {
            verbs_fmt::format_recast(&parse_res::<RecastResult>(&ack)?, None)
        }
        ReqAction::Restart { .. } => verbs_fmt::format_restart(&parse_res::<RestartResult>(&ack)?),
        ReqAction::Close { .. } => verbs_fmt::format_close(&parse_res::<CloseResult>(&ack)?),
        ReqAction::Reopen { .. } => verbs_fmt::format_reopen(&parse_res::<ReopenResult>(&ack)?),
        ReqAction::TaskPriority { .. } => {
            verbs_fmt::format_task_priority(&parse_res::<TaskPriorityResult>(&ack)?)
        }
        ReqAction::TaskPause { .. }
        | ReqAction::TaskResume { .. }
        | ReqAction::TaskAbandon { .. } => {
            verbs_fmt::format_task_state(&parse_res::<TaskStateResult>(&ack)?)
        }
        ReqAction::HollywoodSetup { .. } => {
            format_hollywood_setup(&parse_res::<HollywoodSetupResult>(&ack)?)
        }
        ReqAction::StudioNew { .. } => {
            crate::studio_fmt::studio_new(&parse_res::<StudioNewResult>(&ack)?)
        }
        ReqAction::StudioFinish { .. } => {
            crate::studio_fmt::studio_finish(&parse_res::<StudioFinishResult>(&ack)?)
        }
        ReqAction::StudioReopen { .. } => {
            crate::studio_fmt::studio_reopen(&parse_res::<StudioReopenResult>(&ack)?)
        }
        ReqAction::StudioCastShow { json: want, .. } => {
            let res: backlot_model::req_results::StudioCastShowResult = parse_res(&ack)?;
            if *want || json {
                serde_json::to_string_pretty(&res.settings)
                    .map_err(|e| Fail::refused(format!("cast show: {e}")))?
            } else {
                res.cast
            }
        }
        ReqAction::StudioCastSet { json: want, .. } => {
            let res: backlot_model::req_results::StudioCastSetResult = parse_res(&ack)?;
            verbs_fmt::format_cast_set_result(&res, *want || json)
        }
        ReqAction::StudioPrune { dry_run, .. } => {
            let r: PruneReport = report(&ack, *dry_run)?;
            crate::prune_render::format_report(&r, json)?
        }
        ReqAction::StudioDelete { dry_run, .. } => {
            let r: DeleteReport = report(&ack, *dry_run)?;
            if r.blocked {
                return Err(Fail::refused(crate::delete_render::format_report(
                    &r, json,
                )?));
            }
            crate::delete_render::format_report(&r, json)?
        }
        ReqAction::StudioRestore { dry_run, .. } => {
            let r: RestoreReport = report(&ack, *dry_run)?;
            crate::restore::format_report(&r, json)?
        }
        ReqAction::Repos { .. } => {
            let listing: crate::repos::Listing = parse_res(&ack)?;
            crate::repos::format_listing(&listing, json)?
        }
        ReqAction::Runners => {
            let rows: Vec<backlot_model::req_wire::RunnerCatalogEntry> = parse_res(&ack)?;
            if json {
                serde_json::to_string_pretty(&rows)
                    .map_err(|e| Fail::refused(format!("runners: {e}")))?
            } else {
                // The hub's HOME is not on the wire; paths stay absolute.
                crate::runners::detect_report::format_rows(&rows, &crate::cli::Env::default())
            }
        }
        ReqAction::Version => parse_res::<String>(&ack)?,
        ReqAction::HollywoodSwitch { .. } | ReqAction::HollywoodSwitchCancel => {
            let res: backlot_model::req_results::HollywoodSwitchResult = parse_res(&ack)?;
            if json {
                serde_json::to_string_pretty(&res)
                    .map_err(|e| Fail::refused(format!("switch: {e}")))?
            } else {
                format_hollywood_switch(&res)
            }
        }
        ReqAction::HollywoodSwitchStatus => {
            let res: backlot_model::req_results::HollywoodSwitchStatusResult = parse_res(&ack)?;
            if json {
                serde_json::to_string_pretty(&res)
                    .map_err(|e| Fail::refused(format!("switch status: {e}")))?
            } else {
                format_hollywood_switch_status(&res)
            }
        }
        ReqAction::DesignSign { .. } => {
            let res: DesignSignResult = parse_res(&ack)?;
            if json {
                serde_json::to_string_pretty(&res)
                    .map_err(|e| Fail::refused(format!("design sign: {e}")))?
            } else {
                verbs_design::format_design_sign(&res)
            }
        }
        ReqAction::DesignStartRound { .. } => {
            let res: DesignStartRoundResult = parse_res(&ack)?;
            if json {
                serde_json::to_string_pretty(&res)
                    .map_err(|e| Fail::refused(format!("design start-round: {e}")))?
            } else {
                verbs_design::format_design_open(&res)
            }
        }
    };
    Ok(out)
}
