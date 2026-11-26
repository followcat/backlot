//! Switch handoff and quota-out state presentation for the Call Sheet
//! and Lot Pill (§2.4.2). Pure helpers reading `HollywoodSummary`.

use backlot_model::summary::{HandoffSummary, HollywoodSummary, QueueReason, Work};

use super::items::Chip;
use super::rows::HollywoodActivity;

/// High-level state override for the collapsed Lot Pill during quota
/// exhaustion or an active make-before-break handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PillSwitchState {
    QuotaOut { urgent: usize },
    AuthError { urgent: usize },
    Switching,
    NeedsApproval,
}

pub(crate) fn handoff_phase(hw: &HollywoodSummary) -> Option<&str> {
    hw.handoff.as_ref().map(|h| h.phase.as_str())
}

pub(crate) fn is_handoff_active(hw: &HollywoodSummary) -> bool {
    matches!(
        handoff_phase(hw),
        Some("preparing" | "starting" | "verifying" | "needs_approval" | "committing")
    )
}

pub(crate) fn is_handoff_waiting_visible(hw: &HollywoodSummary) -> bool {
    matches!(
        handoff_phase(hw),
        Some("preparing" | "starting" | "verifying" | "needs_approval" | "rolled_back")
    )
}

pub(crate) fn is_quota_or_error(hw: &HollywoodSummary) -> bool {
    hw.queue.as_ref().is_some_and(|q| {
        matches!(
            q.reason,
            QueueReason::OutOfQuota | QueueReason::AuthError | QueueReason::RateLimited
        )
    }) || hw.attention.as_deref().is_some_and(|a| {
        matches!(
            a,
            "out of quota" | "auth error" | "rate limited" | "agent error" | "connection error"
        )
    })
}

pub(crate) fn handoff_target_short(h: &HandoffSummary) -> String {
    [
        Some(h.target_runner.as_str()),
        Some(h.target_model.as_str()),
        h.target_effort.as_deref().filter(|e| *e != "auto"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("/")
}

pub(crate) fn current_actor_short(hw: &HollywoodSummary) -> String {
    [
        hw.runner.as_deref(),
        hw.model.as_deref(),
        hw.effort.as_deref().filter(|e| *e != "auto"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("/")
}

fn quota_word(hw: &HollywoodSummary) -> &'static str {
    match hw.queue.as_ref().map(|q| q.reason) {
        Some(QueueReason::OutOfQuota) => "out of quota",
        Some(QueueReason::AuthError) => "auth error",
        Some(QueueReason::RateLimited) => "rate limited",
        _ => match hw.attention.as_deref() {
            Some("out of quota") => "out of quota",
            Some("auth error") => "auth error",
            Some("rate limited") => "rate limited",
            Some("connection error") => "connection error",
            Some("agent error") => "agent error",
            _ => "blocked",
        },
    }
}

pub(crate) fn elapsed_secs(h: &HandoffSummary, now_ms: u64) -> u64 {
    now_ms.saturating_sub(h.started_ms) / 1000
}

pub(crate) fn handoff_chips(hw: &HollywoodSummary, can_answer: bool) -> Option<Vec<Chip>> {
    if let Some(hf) = &hw.handoff {
        return match hf.phase.as_str() {
            "preparing" | "starting" => Some(vec![Chip {
                label: "cancel",
                key: "x",
                primary: false,
            }]),
            "verifying" => Some(vec![
                Chip {
                    label: "peek next",
                    key: "p",
                    primary: true,
                },
                Chip {
                    label: "cancel",
                    key: "x",
                    primary: false,
                },
            ]),
            "needs_approval" => {
                let mut out = if can_answer {
                    vec![Chip {
                        label: "allow",
                        key: "y",
                        primary: true,
                    }]
                } else {
                    Vec::new()
                };
                out.push(Chip {
                    label: "peek",
                    key: "p",
                    primary: !can_answer,
                });
                out.push(Chip {
                    label: "cancel",
                    key: "x",
                    primary: false,
                });
                Some(out)
            }
            "rolled_back" => Some(vec![
                Chip {
                    label: "switch",
                    key: "w",
                    primary: true,
                },
                Chip {
                    label: "peek",
                    key: "p",
                    primary: false,
                },
                Chip {
                    label: "open",
                    key: "↵",
                    primary: false,
                },
            ]),
            _ => None,
        };
    }
    if is_quota_or_error(hw) || (!hw.attention_user && hw.work == Some(Work::Blocked)) {
        return Some(vec![
            Chip {
                label: "switch",
                key: "w",
                primary: true,
            },
            Chip {
                label: "peek",
                key: "p",
                primary: false,
            },
            Chip {
                label: "open",
                key: "↵",
                primary: false,
            },
        ]);
    }
    None
}

pub(crate) fn handoff_wait_head(
    hw: &HollywoodSummary,
    now_ms: u64,
) -> Option<(String, &'static str)> {
    if let Some(hf) = &hw.handoff {
        let target = handoff_target_short(hf);
        return match hf.phase.as_str() {
            "preparing" => Some((
                format!("HOLLYWOOD  switching → {target} · preparing brief…"),
                "⠶",
            )),
            "starting" => Some((format!("HOLLYWOOD  switching → {target} · starting…"), "⠶")),
            "verifying" => {
                let secs = elapsed_secs(hf, now_ms);
                let cur = current_actor_short(hw);
                if is_quota_or_error(hw) && !cur.is_empty() {
                    let why = quota_word(hw);
                    Some((
                        format!("HOLLYWOOD  {cur} {why} · switching → {target} ({secs}s)"),
                        "!",
                    ))
                } else {
                    Some((format!("HOLLYWOOD  switching → {target} ({secs}s)"), "⠶"))
                }
            }
            "needs_approval" => Some((
                format!("HOLLYWOOD  switching → {target} · needs approval"),
                "⚠",
            )),
            "rolled_back" => Some((format!("HOLLYWOOD  switch to {target} failed"), "!")),
            _ => None,
        };
    }
    if is_quota_or_error(hw) {
        let cur = current_actor_short(hw);
        let why = quota_word(hw);
        let q_part = hw
            .queue
            .as_ref()
            .map(|q| {
                format!(
                    " · {} tell{} queued",
                    q.count,
                    if q.count == 1 { "" } else { "s" }
                )
            })
            .unwrap_or_default();
        let line = if cur.is_empty() {
            format!("HOLLYWOOD  {why}{q_part}")
        } else {
            format!("HOLLYWOOD  {cur} {why}{q_part}")
        };
        return Some((line, "!"));
    }
    None
}

pub(crate) fn handoff_wait_detail(hw: &HollywoodSummary) -> Option<String> {
    if let Some(hf) = &hw.handoff {
        return match hf.phase.as_str() {
            "preparing" => Some("old session kept live · building handoff brief".into()),
            "starting" => Some("old session kept live · launching candidate".into()),
            "verifying" => {
                if let Some(q) = &hw.queue {
                    Some(format!(
                        "old session kept live until ready · {} user tell{} queued",
                        q.count,
                        if q.count == 1 { "" } else { "s" }
                    ))
                } else {
                    Some("old session kept live until ready".into())
                }
            }
            "needs_approval" => {
                let ask = hw
                    .attention_detail
                    .as_deref()
                    .or(hw.attention.as_deref())
                    .unwrap_or("permission prompt");
                Some(format!(
                    "new hollywood asks: {ask} — allow to finish switch"
                ))
            }
            "rolled_back" => Some(rollback_banner(hw, hf)),
            _ => None,
        };
    }
    if is_quota_or_error(hw) {
        return match hw
            .queue
            .as_ref()
            .and_then(|q| q.detail.as_deref())
            .or(hw.attention_detail.as_deref())
        {
            Some(detail) => Some(format!("{detail} — press w to switch agent")),
            None => Some("press w to switch agent".into()),
        };
    }
    None
}

pub(crate) fn rollback_banner(hw: &HollywoodSummary, hf: &HandoffSummary) -> String {
    let target = handoff_target_short(hf);
    let err = hf.error.as_deref().unwrap_or("handoff failed");
    let cur = current_actor_short(hw);
    if cur.is_empty() {
        format!("switch to {target} failed: {err}")
    } else {
        format!("switch to {target} failed: {err} — kept {cur}")
    }
}

/// Title-bar status override: returns `(full_line, short_line)` when a
/// switch or quota/error condition should replace the default title suffix.
pub(crate) fn title_override(
    hw: &HollywoodSummary,
    _activity: HollywoodActivity,
    now_ms: u64,
) -> Option<(String, String)> {
    if let Some(hf) = &hw.handoff {
        let target = handoff_target_short(hf);
        match hf.phase.as_str() {
            "preparing" => {
                return Some((
                    format!("switching → {target} · preparing brief…"),
                    "switching…".into(),
                ));
            }
            "starting" => {
                return Some((
                    format!("switching → {target} · starting…"),
                    "switching…".into(),
                ));
            }
            "verifying" => {
                let secs = elapsed_secs(hf, now_ms);
                return Some((
                    format!("switching → {target} · verifying ({secs}s)"),
                    format!("verifying ({secs}s)"),
                ));
            }
            "needs_approval" => {
                return Some((
                    format!("switching → {target} · needs approval"),
                    "needs approval".into(),
                ));
            }
            _ => {}
        }
    }
    if is_quota_or_error(hw) {
        let state = hw.state.as_str();
        let cur = current_actor_short(hw);
        let why = quota_word(hw);
        let full = if cur.is_empty() {
            format!("{state} · {why}")
        } else {
            format!("{state} · {cur} · {why}")
        };
        return Some((full, why.into()));
    }
    None
}

pub(crate) fn pill_switch_state(
    hw: Option<&HollywoodSummary>,
    urgent: usize,
) -> Option<PillSwitchState> {
    let hw = hw?;
    if let Some(hf) = &hw.handoff {
        match hf.phase.as_str() {
            "preparing" | "starting" | "verifying" => return Some(PillSwitchState::Switching),
            "needs_approval" => return Some(PillSwitchState::NeedsApproval),
            _ => {}
        }
    }
    if is_quota_or_error(hw) {
        let auth = hw
            .queue
            .as_ref()
            .is_some_and(|q| q.reason == QueueReason::AuthError)
            || hw.attention.as_deref() == Some("auth error");
        return Some(if auth {
            PillSwitchState::AuthError {
                urgent: urgent.max(1),
            }
        } else {
            PillSwitchState::QuotaOut {
                urgent: urgent.max(1),
            }
        });
    }
    None
}
