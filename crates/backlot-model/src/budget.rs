//! The optional per-task running budget (S9e): `task_budget_s` on
//! `studio.created` bounds the span from a task's first cast to its
//! close — or to now — leaving out every `paused` → `resumed`
//! stretch. Pure over the reduced state; the stage reads it each
//! tick.

use crate::event::TaskStateChange;
use crate::state::Task;

/// Milliseconds the task has used: its first cast to `closed_ts_ms`
/// or `now_ms`, minus each paused stretch — clamped to the counted
/// span so a pause before the first cast costs nothing.
pub fn used_ms(t: &Task, now_ms: u64) -> u64 {
    let Some(start) = t.casts.first().map(|c| c.ts_ms) else {
        return 0;
    };
    let end = t.closed_ts_ms.unwrap_or(now_ms).max(start);
    let mut paused_ms = 0u64;
    let mut since: Option<u64> = None;
    for ch in &t.state_changes {
        match ch.state {
            TaskStateChange::Paused if since.is_none() => since = Some(ch.ts_ms),
            TaskStateChange::Resumed => {
                if let Some(p) = since.take() {
                    paused_ms += ch.ts_ms.min(end).saturating_sub(p.max(start));
                }
            }
            _ => {}
        }
    }
    if let Some(p) = since {
        paused_ms += end.saturating_sub(p.max(start));
    }
    end.saturating_sub(start).saturating_sub(paused_ms)
}
