//! Recompute a task's status and counters from its recorded facts.
//! `apply` calls this after each task event; corrections call it after
//! rewriting a fact.

use crate::event::*;
use crate::state::*;

/// Recompute `status`, `rework_rounds` and `paused_from` from the
/// task's own event lists. Runs after every task event and after a
/// correction, so both paths share one definition of the machine.
pub fn reduce_task(task: &mut Task, cap: u32) {
    enum Step {
        Cast,
        Delivered,
        Reviewed(Verdict),
        State(TaskStateChange),
    }
    let mut steps: Vec<(u64, Step)> = Vec::new();
    steps.extend(task.casts.iter().map(|c| (c.seq, Step::Cast)));
    steps.extend(task.deliveries.iter().map(|d| (d.seq, Step::Delivered)));
    steps.extend(
        task.reviews
            .iter()
            .map(|r| (r.seq, Step::Reviewed(r.verdict))),
    );
    steps.extend(
        task.state_changes
            .iter()
            .map(|s| (s.seq, Step::State(s.state))),
    );
    steps.sort_by_key(|(seq, _)| *seq);

    let mut status = TaskStatus::Created;
    let mut paused_from = None;
    // `rework_rounds` is the task's total; the cap counts the
    // changes-requested rounds since the latest cast — a recast
    // starts the producer's budget over.
    let mut rounds = 0u32;
    let mut cap_rounds = 0u32;
    let mut closed = None;
    // Each `resumed`'s outcome, for the record annotations below:
    // `running` means the actor should carry on, anything else is
    // only bookkeeping.
    let mut resumed_to: Vec<(u64, TaskStatus)> = Vec::new();
    for (seq, step) in steps {
        let next = match step {
            Step::Cast => {
                cap_rounds = 0;
                TaskStatus::Running
            }
            Step::Delivered => TaskStatus::Delivered,
            Step::Reviewed(verdict) => match verdict {
                Verdict::Accepted => TaskStatus::Accepted,
                Verdict::PartiallyAccepted => TaskStatus::PartiallyAccepted,
                Verdict::Blocked => TaskStatus::Blocked,
                Verdict::ChangesRequested => {
                    rounds += 1;
                    cap_rounds += 1;
                    if cap_rounds > cap {
                        TaskStatus::AtCap
                    } else {
                        TaskStatus::Running
                    }
                }
            },
            Step::State(change) => match change {
                TaskStateChange::Paused => {
                    paused_from = Some(status);
                    TaskStatus::Paused
                }
                TaskStateChange::Resumed => {
                    let next = paused_from.take().unwrap_or(TaskStatus::Running);
                    resumed_to.push((seq, next));
                    next
                }
                TaskStateChange::Abandoned => TaskStatus::Abandoned,
                TaskStateChange::TakenOver => TaskStatus::TakenOver,
                TaskStateChange::Blocked => TaskStatus::Blocked,
            },
        };
        if next.is_terminal() && closed.is_none() {
            closed = task
                .deliveries
                .iter()
                .map(|d| (d.seq, d.ts_ms))
                .chain(task.reviews.iter().map(|r| (r.seq, r.ts_ms)))
                .chain(task.state_changes.iter().map(|s| (s.seq, s.ts_ms)))
                .chain(task.casts.iter().map(|c| (c.seq, c.ts_ms)))
                .find(|(s, _)| *s == seq)
                .map(|(_, ts)| ts);
        }
        status = next;
    }
    task.status = status;
    task.paused_from = paused_from;
    task.rework_rounds = rounds;
    task.closed_ts_ms = if status.is_terminal() { closed } else { None };
    for change in &mut task.state_changes {
        if change.state == TaskStateChange::Resumed {
            change.resumed_to = resumed_to
                .iter()
                .find(|(s, _)| *s == change.seq)
                .map(|(_, st)| *st);
        }
    }
}
