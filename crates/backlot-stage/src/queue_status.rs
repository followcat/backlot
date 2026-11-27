//! Per-role delivery gate blocking and `RoleSummary.queue` status derivation (§1.3.2).

use backlot_model::state::StudioState;
use backlot_model::summary::{QueueReason, QueueStatus, RoleSummary, Work};

fn clean_detail(raw: Option<&str>) -> Option<String> {
    raw.and_then(|s| s.split(';').next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn is_quota_exhausted_text(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("weekly")
        || lower.contains("monthly")
        || lower.contains("quota")
        || lower.contains("exhausted")
        || lower.contains("out of quota")
        || lower.contains("needs the user")
}

/// True while a role's active rate-limit / quota / connection-error episode
/// should keep queued messages held safely in the ledger rather than typing into
/// an error banner (§1.3.2).
pub(crate) fn role_delivery_blocked(
    state: &StudioState,
    role: &str,
    rate_pace_active: bool,
) -> bool {
    if rate_pace_active {
        return true;
    }
    state.open_attention().any(|a| {
        a.role == role
            && (a.reason == crate::ratelimit::RATE_LIMITED
                || a.reason == crate::connerr::CONNECTION_ERROR)
    })
}

/// Derive `RoleSummary.queue` when undelivered messages are queued for `rs.role`.
pub(crate) fn role_queue_status(
    state: &StudioState,
    rs: &RoleSummary,
    work: Option<Work>,
    rate_pace_active: bool,
    terminal_live: bool,
    now_ms: u64,
) -> Option<QueueStatus> {
    let count = state
        .messages
        .iter()
        .filter(|m| m.to == rs.role && !m.delivered && !m.dropped)
        .count() as u32;
    if count == 0 {
        return None;
    }

    let open: Vec<_> = state
        .open_attention()
        .filter(|a| a.role == rs.role)
        .collect();

    let (reason, detail) = if let Some(rl) = open
        .iter()
        .find(|a| a.reason == crate::ratelimit::RATE_LIMITED)
    {
        let detail = clean_detail(Some(&rl.detail));
        if rl.needs_user && is_quota_exhausted_text(&rl.detail) {
            (QueueReason::OutOfQuota, detail)
        } else {
            (QueueReason::RateLimited, detail)
        }
    } else if rate_pace_active {
        (QueueReason::RateLimited, None)
    } else if let Some(auth) = open.iter().find(|a| {
        a.reason == "agent error"
            || a.reason == "auth error"
            || a.reason.to_lowercase().contains("auth")
    }) {
        (QueueReason::AuthError, clean_detail(Some(&auth.detail)))
    } else if let Some(conn) = open
        .iter()
        .find(|a| a.reason == crate::connerr::CONNECTION_ERROR)
    {
        (QueueReason::RateLimited, clean_detail(Some(&conn.detail)))
    } else if rs.state != "live" || !terminal_live {
        (
            QueueReason::Down,
            Some(format!("{} is {}", rs.role, rs.state)),
        )
    } else if work == Some(Work::Blocked)
        || rs.attention_user
        || open
            .iter()
            .any(|a| a.reason == "permission prompt" || a.reason == "ask")
    {
        let detail = clean_detail(rs.attention_detail.as_deref())
            .or_else(|| clean_detail(open.first().map(|a| a.detail.as_str())));
        (QueueReason::Prompt, detail)
    } else if work == Some(Work::Working) {
        let detail = rs
            .doing
            .clone()
            .or_else(|| rs.running.as_ref().map(|r| r.cmd.clone()));
        (QueueReason::Busy, detail)
    } else if rs.since_ms > 0 && now_ms.saturating_sub(rs.since_ms) < crate::deliver::MIN_UP_MS {
        (QueueReason::Starting, None)
    } else {
        (QueueReason::WaitingQuiet, None)
    };

    Some(QueueStatus {
        count,
        reason,
        detail,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use backlot_model::state::{Attention, Message};
    use backlot_model::summary::RoleRunning;

    fn msg(seq: u64, to: &str) -> Message {
        Message {
            seq,
            from: "user".into(),
            to: to.into(),
            file: Some(format!("messages/{seq}-tell.md")),
            purpose: None,
            source_seq: None,
            delivered: false,
            dropped: false,
            dropped_reason: None,
            ts_ms: 1_000,
            delivered_ms: None,
        }
    }

    fn base_role() -> RoleSummary {
        RoleSummary {
            role: "producer".into(),
            name: "producer".into(),
            terminal: Some("t1".into()),
            runner: Some("claude".into()),
            model: Some("sonnet".into()),
            effort: None,
            state: "live".into(),
            doing: Some("working on t1 review".into()),
            progress: None,
            running: Some(RoleRunning {
                cmd: "cargo test".into(),
                since_ms: 1_000,
            }),
            work: Some(Work::Waiting),
            failed: None,
            workdir: None,
            task: None,
            priority: None,
            resumable: false,
            since_ms: 1_000,
            attention: None,
            attention_class: None,
            asks: None,
            attention_user: false,
            attention_seq: None,
            attention_detail: None,
            stage_answer_ms: None,
            can_answer_prompt: true,
            restarted_ms: None,
            idle_since_ms: None,
            queue: None,
            account: None,
        }
    }

    #[test]
    fn role_queue_status_classifies_busy_prompt_quota_down_starting_and_quiet() {
        let mut st = StudioState::default();
        let rs = base_role();
        // Empty outbox -> None.
        assert_eq!(
            role_queue_status(&st, &rs, Some(Work::Working), false, true, 20_000),
            None
        );

        st.messages.push(msg(1, "producer"));
        // Busy when Working.
        let busy = role_queue_status(&st, &rs, Some(Work::Working), false, true, 20_000).unwrap();
        assert_eq!(busy.reason, QueueReason::Busy);
        assert_eq!(busy.detail.as_deref(), Some("working on t1 review"));

        // Starting within MIN_UP_MS.
        let starting =
            role_queue_status(&st, &rs, Some(Work::Waiting), false, true, 3_000).unwrap();
        assert_eq!(starting.reason, QueueReason::Starting);

        // WaitingQuiet after MIN_UP_MS.
        let quiet = role_queue_status(&st, &rs, Some(Work::Waiting), false, true, 20_000).unwrap();
        assert_eq!(quiet.reason, QueueReason::WaitingQuiet);

        // Down when terminal not live.
        let down = role_queue_status(&st, &rs, Some(Work::Waiting), false, false, 20_000).unwrap();
        assert_eq!(down.reason, QueueReason::Down);

        // Prompt when Blocked or attention_user.
        let prompt = role_queue_status(&st, &rs, Some(Work::Blocked), false, true, 20_000).unwrap();
        assert_eq!(prompt.reason, QueueReason::Prompt);

        // Escalated quota -> OutOfQuota and delivery blocked.
        st.attention.push(Attention {
            seq: 2,
            role: "producer".into(),
            reason: crate::ratelimit::RATE_LIMITED.into(),
            detail: "weekly limit reached; needs the user".into(),
            open: true,
            ts_ms: 2_000,
            needs_user: true,
            cleared_reason: None,
            account: None,
            reset_at_ms: None,
        });
        assert!(role_delivery_blocked(&st, "producer", false));
        let quota = role_queue_status(&st, &rs, Some(Work::Waiting), false, true, 20_000).unwrap();
        assert_eq!(quota.reason, QueueReason::OutOfQuota);
        assert_eq!(quota.detail.as_deref(), Some("weekly limit reached"));
    }
}
