//! Who owes the next step (S9a): the obligations the ledger still
//! holds, derived from the reduced state the way the stage's notices
//! derive from its events — one per obligation, keyed on the seq of
//! the event that created it. `backlot status` prints them, the
//! summary carries the first, and the stage's nudge pass reads them.
//! Nothing is owed once the studio is finished.

use crate::event::{CheckDecl, PlanKind, TaskStateChange, Verdict};
use crate::state::{Message, StudioState, Task, TaskStatus};

/// One thing the ledger still owes: who, what, and since when.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// The seq of the event that created the obligation — a nudge's
    /// `source_seq`, stable across a stage restart.
    pub key: u64,
    /// A short label — `dispatch`, `cast t1`, `node report`.
    pub label: String,
    /// The role key that owes it — `producer`, `director`, `writer`,
    /// an `actor-<task>-c<n>` key or `user`.
    pub owner: String,
    /// The `backlot` command that discharges it; the nudge names it.
    pub command: String,
    /// When it became owed (ms).
    pub since_ms: u64,
}

/// The role key `desired` gives the task's current cast.
fn actor_key(task: &Task) -> String {
    format!("actor-{}-c{}", task.id, task.casts.len())
}

/// The latest `plan`/`dispatch` command message — the phase marker
/// the first rows key on. A `nudge` or a plain `tell` never counts.
fn last_call(state: &StudioState) -> Option<&Message> {
    state
        .messages
        .iter()
        .rev()
        .find(|m| matches!(m.purpose.as_deref(), Some("plan") | Some("dispatch")))
}

/// The earliest `plan.published` of `kind` after `seq`, as
/// `(seq, ts_ms)` — the dispatch obligation dates from when *both*
/// kinds first exist.
fn first_plan_after(state: &StudioState, kind: PlanKind, seq: u64) -> Option<(u64, u64)> {
    state
        .plans
        .iter()
        .filter(|p| p.plan_kind == kind && p.seq > seq)
        .map(|p| (p.seq, p.ts_ms))
        .min_by_key(|(s, _)| *s)
}

/// The seq of the event that closed the task — `closed_seq` on
/// current logs; on ones that predate it, the last transitioning
/// event (findings never close a task and are not in this list).
fn closed_seq(t: &Task) -> u64 {
    t.closed_seq.unwrap_or_else(|| {
        t.casts
            .iter()
            .map(|c| c.seq)
            .chain(t.deliveries.iter().map(|d| d.seq))
            .chain(t.reviews.iter().map(|r| r.seq))
            .chain(t.state_changes.iter().map(|s| s.seq))
            .max()
            .unwrap_or(t.created_seq)
    })
}

/// `(seq, ts_ms)` of the event that last put the task `running`:
/// a cast, a `changes_requested` review under the cap, or a resume
/// that restored running. Only called while the task is running.
fn running_since(t: &Task) -> (u64, u64) {
    t.casts
        .iter()
        .map(|c| (c.seq, c.ts_ms))
        .chain(
            t.reviews
                .iter()
                .filter(|r| r.verdict == Verdict::ChangesRequested)
                .map(|r| (r.seq, r.ts_ms)),
        )
        .chain(
            t.state_changes
                .iter()
                .filter(|s| s.resumed_to == Some(TaskStatus::Running))
                .map(|s| (s.seq, s.ts_ms)),
        )
        .max_by_key(|(s, _)| *s)
        .unwrap_or((t.created_seq, t.created_ts_ms))
}

/// `(seq, ts_ms)` of the event that blocked the task: a `blocked`
/// review verdict or state change, or a resume restoring `blocked`.
fn blocked_since(t: &Task) -> (u64, u64) {
    t.reviews
        .iter()
        .filter(|r| r.verdict == Verdict::Blocked)
        .map(|r| (r.seq, r.ts_ms))
        .chain(
            t.state_changes
                .iter()
                .filter(|s| {
                    s.state == TaskStateChange::Blocked || s.resumed_to == Some(TaskStatus::Blocked)
                })
                .map(|s| (s.seq, s.ts_ms)),
        )
        .max_by_key(|(s, _)| *s)
        .unwrap_or((t.created_seq, t.created_ts_ms))
}

/// Every step the ledger still owes, in key order. Empty while the
/// studio is finished — or before anything is owed at all.
pub fn next_steps(state: &StudioState) -> Vec<Step> {
    if state.finished.is_some() || state.studio.is_none() {
        return Vec::new();
    }
    let mut out = Vec::new();

    // The plan/dispatch phase, decided by the latest `plan` or
    // `dispatch` command message — the same rule the writer's
    // wanted set uses.
    match last_call(state) {
        Some(m) if m.purpose.as_deref() == Some("plan") => {
            let plan = first_plan_after(state, PlanKind::Plan, m.seq);
            let acceptance = first_plan_after(state, PlanKind::Acceptance, m.seq);
            match (plan, acceptance) {
                // Both published after the request: the producer
                // owes the dispatch — owed from the second publish.
                (Some((s1, t1)), Some((s2, t2))) => out.push(Step {
                    key: s1.max(s2),
                    label: "dispatch".into(),
                    owner: "producer".into(),
                    command: "backlot dispatch --file <file>".into(),
                    since_ms: t1.max(t2),
                }),
                _ => out.push(Step {
                    key: m.seq,
                    label: "plan and acceptance list".into(),
                    owner: "writer".into(),
                    command:
                        "backlot plan publish --kind plan --file <file> (and --kind acceptance)"
                            .into(),
                    since_ms: m.ts_ms,
                }),
            }
        }
        // A dispatch handed to the director, not yet answered by a
        // task: the director owes the split.
        Some(m) if m.delivered && !state.tasks.values().any(|t| t.created_seq > m.seq) => {
            out.push(Step {
                key: m.seq,
                label: "tasks".into(),
                owner: "director".into(),
                command: "backlot task new --title <title> --kind <kind> --acceptance-file <file>"
                    .into(),
                since_ms: m.delivered_ms.unwrap_or(m.ts_ms),
            });
        }
        _ => {}
    }

    for t in state.tasks.values() {
        if t.status.is_terminal() || t.status == TaskStatus::Paused {
            continue;
        }
        // Created and never cast: whoever created it casts it — the
        // director on the split path, the producer when it made the
        // task itself. Old logs carry no `by`: the director is the
        // usual hand. A task whose dependencies are still open owes
        // no cast step — nobody is nudged for a cast they may not do.
        if t.casts.is_empty() {
            if !t.open_dependencies(state).is_empty() {
                continue;
            }
            out.push(Step {
                key: t.created_seq,
                label: format!("cast {}", t.id),
                owner: if t.created_by.is_empty() {
                    "director".into()
                } else {
                    t.created_by.clone()
                },
                command: format!("backlot cast --task {} --actor <name> --reason <r>", t.id),
                since_ms: t.created_ts_ms,
            });
            continue;
        }
        match t.status {
            TaskStatus::Running => {
                let (seq, ts) = running_since(t);
                out.push(Step {
                    key: seq,
                    label: format!("deliver {}", t.id),
                    owner: actor_key(t),
                    command: format!(
                        "backlot deliver --task {} --commits <A..B> --report <file>",
                        t.id
                    ),
                    since_ms: ts,
                });
            }
            // The latest delivery still awaits its review. While the
            // cast declares checks the step waits for the stage's
            // records and is then owed from the last `task.checked`
            // — the nudge cannot overtake the delivery notice, which
            // waits on the same records.
            TaskStatus::Delivered => {
                if let Some(d) = t.deliveries.last()
                    && t.review_of(d.n).is_none()
                {
                    let declared: &[CheckDecl] = state
                        .studio
                        .as_ref()
                        .map(|s| s.checks.as_slice())
                        .unwrap_or(&[]);
                    let key_ts = if declared.is_empty() {
                        Some((d.seq, d.ts_ms))
                    } else if t.checks_missing(d.n, declared).is_empty() {
                        t.checked_of(d.n)
                            .max_by_key(|c| c.seq)
                            .map(|c| (c.seq, c.ts_ms))
                    } else {
                        None
                    };
                    if let Some((seq, ts)) = key_ts {
                        out.push(Step {
                            key: seq,
                            label: format!("review {}", t.id),
                            owner: t.reviewer().into(),
                            command: format!(
                                "backlot review --task {} --delivery {} --verdict <verdict> --conclusion <text>",
                                t.id, d.n
                            ),
                            since_ms: ts,
                        });
                    }
                }
            }
            // Past the cap the call is the producer's: accept with
            // the gaps named, rescope or recast.
            TaskStatus::AtCap => {
                let r = t
                    .reviews
                    .iter()
                    .filter(|r| r.verdict == Verdict::ChangesRequested)
                    .max_by_key(|r| r.seq);
                if let Some(r) = r {
                    out.push(Step {
                        key: r.seq,
                        label: format!("{} at cap", t.id),
                        owner: "producer".into(),
                        command: format!(
                            "backlot review --task {} --delivery {} --verdict partially-accepted --conclusion <text> (or rescope/recast)",
                            t.id, r.delivery
                        ),
                        since_ms: r.ts_ms,
                    });
                }
            }
            TaskStatus::Blocked => {
                let (seq, ts) = blocked_since(t);
                out.push(Step {
                    key: seq,
                    label: format!("unblock {}", t.id),
                    owner: t.reviewer().into(),
                    command: format!("backlot task resume --task {} --reason <r>", t.id),
                    since_ms: ts,
                });
            }
            _ => {}
        }
    }

    // The report chain once every task is terminal: the director's
    // node report — the producer's own when it cast every task
    // itself — then the producer's relay to the user, then the
    // user's `studio finish`.
    let last_close = (!state.tasks.is_empty()
        && state.tasks.values().all(|t| t.status.is_terminal()))
    .then(|| {
        state
            .tasks
            .values()
            .map(|t| (closed_seq(t), t.closed_ts_ms.unwrap_or(t.last_ts_ms)))
            .max_by_key(|(s, _)| *s)
    })
    .flatten();
    if let Some((close_seq, close_ts)) = last_close {
        let producer_ran = state.tasks.values().all(|t| {
            if t.casts.is_empty() {
                t.created_by == "producer"
            } else {
                t.casts.iter().all(|c| c.by == "producer")
            }
        });
        let owner = if producer_ran { "producer" } else { "director" };
        if !state
            .reports
            .iter()
            .any(|r| r.from == owner && r.seq > close_seq)
        {
            out.push(Step {
                key: close_seq,
                label: "node report".into(),
                owner: owner.into(),
                command: "backlot report --node <node> --file <file>".into(),
                since_ms: close_ts,
            });
        }
        if let Some(r) = state
            .reports
            .iter()
            .find(|r| r.to == "user" && r.seq > close_seq)
        {
            out.push(Step {
                key: r.seq,
                label: "finish".into(),
                owner: "user".into(),
                command: "backlot studio finish --reason <reason>".into(),
                since_ms: r.ts_ms,
            });
        }
    }

    // A report handed to the producer and never relayed on: the
    // producer owes the user a report after the latest one
    // delivered to it.
    if let Some(r) = state
        .reports
        .iter()
        .rev()
        .find(|r| r.to == "producer" && r.delivered)
        && !state
            .reports
            .iter()
            .any(|a| a.to == "user" && a.seq > r.seq)
    {
        out.push(Step {
            key: r.seq,
            label: "report to user".into(),
            owner: "producer".into(),
            command: "backlot report --node <node> --file <file>".into(),
            since_ms: r.delivered_ms.unwrap_or(r.ts_ms),
        });
    }

    out.sort_by_key(|s| s.key);
    out
}
