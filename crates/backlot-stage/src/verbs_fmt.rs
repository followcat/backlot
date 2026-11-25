//! CLI confirmation formatters rendered directly from typed person-verb
//! result structs so stdout matches ledger events written via docs.

use backlot_model::req_results::{
    AttentionClearResult, CloseResult, RecastResult, ReopenResult, RestartResult,
    TaskPriorityResult, TaskStateResult, TellResult,
};

pub(crate) fn format_tell(res: &TellResult) -> String {
    format!("event {}: message.queued → {}", res.seq, res.role)
}

pub(crate) fn format_attention_clear(res: &AttentionClearResult) -> String {
    format!("event {}: attention.cleared", res.seq)
}

pub(crate) fn format_recast(res: &RecastResult, note: Option<&str>) -> String {
    let mut out = format!("event {}: role.recast {}", res.seq, res.person);
    if let Some(n) = note {
        out.push_str(&format!(" — {n}"));
    }
    out
}

pub(crate) fn format_restart(res: &RestartResult) -> String {
    if res.is_hollywood {
        format!(
            "event {}: role.restart{} — the stage stops and relaunches it",
            res.seq,
            if res.fresh { " (fresh session)" } else { "" }
        )
    } else {
        format!(
            "event {}: role.restart {}{}",
            res.seq,
            res.person,
            if res.fresh { " (fresh session)" } else { "" }
        )
    }
}

pub(crate) fn format_close(res: &CloseResult) -> String {
    format!("event {}: role.release {}", res.seq, res.person)
}

pub(crate) fn format_reopen(res: &ReopenResult) -> String {
    format!("event {}: role.reopen {}", res.seq, res.person)
}

pub(crate) fn format_task_priority(res: &TaskPriorityResult) -> String {
    format!(
        "event {}: task.priority {} {}",
        res.seq, res.task, res.level
    )
}

pub(crate) fn format_task_state(res: &TaskStateResult) -> String {
    format!("event {}: task.state {} {:?}", res.seq, res.task, res.state)
}

/// `studio cast set`: on a dry run the diff the file would make, else
/// the events it landed and the diff. `--json` prints the same
/// structured.
pub(crate) fn format_cast_set(res: &crate::studio_cast::CastSetOutput) -> String {
    if res.json {
        return serde_json::to_string_pretty(res).unwrap_or_default();
    }
    let lines = res.diff.lines();
    let mut out = if res.dry_run {
        format!("dry run: studio '{}' — nothing recorded\n", res.studio)
    } else {
        let mut head = format!("event {}: studio.cast", res.seq.unwrap_or(0));
        if !res.recasts.is_empty() {
            let roles: Vec<&str> = res.recasts.iter().map(|(r, _)| r.as_str()).collect();
            head.push_str(&format!(" — recast {}", roles.join(", ")));
        }
        head.push('\n');
        head
    };
    if lines.is_empty() {
        out.push_str("  (no change)\n");
    } else {
        for line in lines {
            out.push_str(&format!("  {line}\n"));
        }
    }
    out.trim_end().to_string()
}
