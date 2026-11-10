//! The per-task running budget (S9e): a running or delivered task
//! past `task_budget_s` raises one `over budget` attention on its
//! current actor key — one per task ever, a recast does not raise a
//! second — and a terminal task clears it `done`. Nothing is killed
//! or paused; the attention is all that happens.

use std::collections::BTreeSet;

use backlot_model::budget::used_ms;
use backlot_model::state::{StudioState, TaskStatus, actor_task};

use crate::reasons;

use super::Action;

/// The role key of the task's current cast: `actor-<task>-c<n>`.
fn actor_key(task: &str, casts: usize) -> String {
    format!("actor-{task}-c{casts}")
}

/// Budget actions for every task: a raise for the over-budget
/// running or delivered one (the first only — any earlier raise,
/// open or cleared, settles the task for good), a `done` clear for
/// the terminal one whose attention still stands.
pub fn actions(
    state: &StudioState,
    now_ms: u64,
    records: &mut Vec<Action>,
    cleared: &mut BTreeSet<u64>,
) {
    let Some(budget_ms) = state
        .studio
        .as_ref()
        .and_then(|s| s.task_budget_s)
        .map(u64::from)
        .map(|s| s * 1000)
    else {
        return;
    };
    for (id, t) in &state.tasks {
        let open: Vec<u64> = state
            .attention
            .iter()
            .filter(|a| {
                a.reason == reasons::OVER_BUDGET
                    && actor_task(&a.role) == Some(id.as_str())
                    && a.open
            })
            .map(|a| a.seq)
            .collect();
        if t.status.is_terminal() {
            for seq in open {
                if cleared.insert(seq) {
                    records.push(Action::ClearAttention {
                        target: seq,
                        reason: "done".into(),
                    });
                }
            }
            continue;
        }
        if !open.is_empty()
            || !matches!(t.status, TaskStatus::Running | TaskStatus::Delivered)
            || state.attention.iter().any(|a| {
                a.reason == reasons::OVER_BUDGET && actor_task(&a.role) == Some(id.as_str())
            })
        {
            continue;
        }
        let used = used_ms(t, now_ms);
        if used > budget_ms {
            records.push(Action::Attention {
                role: actor_key(id, t.casts.len()),
                reason: reasons::OVER_BUDGET.into(),
                detail: format!(
                    "task {id} is over budget: {}s used of {}s",
                    used / 1000,
                    budget_ms / 1000
                ),
            });
        }
    }
}
