//! Result formatting helpers for `plugin.backlot.res` acknowledgements.

use backlot_model::req_results::{
    AttentionClearResult, CardApplyResult, CardCancelResult, CardShowResult, CloseResult,
    HollywoodSetupResult, RecastResult, ReopenResult, RestartResult, TaskPriorityResult,
    TaskStateResult, TellHollywoodResult, TellResult,
};
use backlot_model::req_wire::{ReqAction, ResAck};
use serde::de::DeserializeOwned;

use crate::cli::Fail;
use crate::{verbs_card_show, verbs_fmt, verbs_hwood};

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

/// Format a positive (`ok == true`) or negative (`ok == false`) `ResAck`.
pub fn format_ack(action: &ReqAction, ack: ResAck) -> Result<String, Fail> {
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
    };
    Ok(out)
}
