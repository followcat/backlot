//! Folding events into the reduced state. `apply` folds one event;
//! `replay` folds a log. Facts are never rewritten: a correction is a
//! new event that changes the reduced view, and the log line stays as
//! it was.

use crate::correct::{self, SeqTarget};
use crate::event::*;
use crate::state::*;

/// Fold one event into the state. Total: a line that would have been
/// refused at append time is skipped rather than panicking on replay.
pub fn apply(state: &mut StudioState, event: &Event) {
    state.last_seq = event.seq;
    let seq = event.seq;
    let ts = event.ts_ms;
    let cap = state.rework_cap();
    match &event.kind {
        EventKind::StudioCreated {
            title,
            repo,
            mode,
            roles,
            candidates,
            rework_cap,
        } => {
            state.studio = Some(StudioConfig {
                title: title.clone(),
                repo: repo.clone(),
                mode: *mode,
                roles: roles.as_ref().clone(),
                candidates: candidates.clone(),
                rework_cap: *rework_cap,
            });
            state.targets.insert(seq, SeqTarget::Studio);
        }
        EventKind::RoleStarted {
            role,
            terminal,
            actor,
            session_id,
            ..
        }
        | EventKind::RoleResumed {
            role,
            terminal,
            session_id,
            actor,
            ..
        } => {
            let entry = state.roles.entry(role.clone()).or_default();
            entry.live = true;
            entry.closed = false;
            entry.since_ms = ts;
            if terminal.is_some() {
                entry.terminal = terminal.clone();
            }
            if actor.is_some() {
                entry.actor = actor.clone();
            }
            if session_id.is_some() {
                entry.session_id = session_id.clone();
            }
            state.targets.insert(seq, SeqTarget::Role(role.clone()));
        }
        EventKind::RoleLost { role, .. } => {
            if let Some(entry) = state.roles.get_mut(role) {
                entry.live = false;
                entry.since_ms = ts;
            }
        }
        EventKind::RoleClosed { role, .. } => {
            // A deliberate release keeps `actor` and `session_id`: the
            // person stays resumable while their studio is live.
            let entry = state.roles.entry(role.clone()).or_default();
            entry.live = false;
            entry.closed = true;
            entry.held = false;
            entry.since_ms = ts;
            state.targets.insert(seq, SeqTarget::Role(role.clone()));
        }
        EventKind::RoleSession { role, session_id } => {
            // A found session id lands on a role the log already
            // knows; it never creates the entry.
            if let Some(entry) = state.roles.get_mut(role) {
                entry.session_id = Some(session_id.clone());
            }
            state.targets.insert(seq, SeqTarget::Role(role.clone()));
        }
        // `held` is the panel's hold on a person: it survives a
        // resume (the hold is what keeps a reopened role desired)
        // and clears on release or on the next close.
        EventKind::RoleReopen { role } => {
            if let Some(entry) = state.roles.get_mut(role) {
                entry.held = true;
            }
            state.targets.insert(seq, SeqTarget::Role(role.clone()));
        }
        EventKind::RoleRelease { role } => {
            if let Some(entry) = state.roles.get_mut(role) {
                entry.held = false;
            }
            state.targets.insert(seq, SeqTarget::Role(role.clone()));
        }
        EventKind::ReqVersion {
            version,
            file,
            summary,
        } => {
            state.requirement_version = *version;
            state.requirements.push(ReqRecord {
                version: *version,
                file: file.clone(),
                summary: summary.clone(),
                seq,
            });
            state
                .targets
                .insert(seq, SeqTarget::ReqVersion(state.requirements.len() - 1));
        }
        EventKind::PlanPublished {
            plan_kind,
            file,
            requirement_version,
        } => {
            state.plans.push(PlanRecord {
                plan_kind: *plan_kind,
                file: file.clone(),
                requirement_version: *requirement_version,
                seq,
            });
            state
                .targets
                .insert(seq, SeqTarget::Plan(state.plans.len() - 1));
        }
        EventKind::TaskCreated {
            task,
            title,
            task_kind,
            requirement_version,
            acceptance,
            dependencies,
        } => {
            state.tasks.insert(
                task.clone(),
                Task {
                    id: task.clone(),
                    title: title.clone(),
                    task_kind: task_kind.clone(),
                    requirement_version: *requirement_version,
                    acceptance: acceptance.clone(),
                    dependencies: dependencies.clone(),
                    status: TaskStatus::Created,
                    paused_from: None,
                    rework_rounds: 0,
                    priority: Priority::default(),
                    cast: None,
                    casts: Vec::new(),
                    deliveries: Vec::new(),
                    reviews: Vec::new(),
                    finding_reports: Vec::new(),
                    state_changes: Vec::new(),
                    created_seq: seq,
                    created_ts_ms: ts,
                    last_ts_ms: ts,
                    closed_ts_ms: None,
                },
            );
            state
                .targets
                .insert(seq, SeqTarget::TaskCreated(task.clone()));
        }
        EventKind::TaskCast {
            task,
            candidate,
            actor,
            reason,
            priority,
        } => {
            if let Some(t) = state.tasks.get_mut(task) {
                t.casts.push(CastRecord {
                    candidate: candidate.clone(),
                    actor: actor.clone(),
                    reason: reason.clone(),
                    priority: *priority,
                    seq,
                    ts_ms: ts,
                });
                t.cast = t.casts.last().cloned();
                // A cast without a priority leaves the task's alone.
                if let Some(p) = priority {
                    t.priority = *p;
                }
                t.last_ts_ms = ts;
                state.targets.insert(
                    seq,
                    SeqTarget::TaskCast {
                        task: task.clone(),
                        idx: t.casts.len() - 1,
                    },
                );
                reduce_task(t, cap);
            }
        }
        EventKind::TaskPriority { task, priority } => {
            if let Some(t) = state.tasks.get_mut(task) {
                t.priority = *priority;
                t.last_ts_ms = ts;
            }
        }
        EventKind::TaskDelivered {
            task,
            delivery,
            actor,
            commits,
            report,
            requirement_version,
            checks,
        } => {
            if let Some(t) = state.tasks.get_mut(task) {
                t.deliveries.push(DeliveryRecord {
                    n: *delivery,
                    actor: actor.clone(),
                    commits: commits.clone(),
                    report: report.clone(),
                    requirement_version: *requirement_version,
                    checks: checks.clone(),
                    seq,
                    ts_ms: ts,
                });
                t.last_ts_ms = ts;
                state.targets.insert(
                    seq,
                    SeqTarget::TaskDelivered {
                        task: task.clone(),
                        idx: t.deliveries.len() - 1,
                    },
                );
                reduce_task(t, cap);
            }
        }
        EventKind::TaskReviewed {
            task,
            delivery,
            reviewer,
            verdict,
            conclusion,
            findings,
            upstream_changes,
        } => {
            if let Some(t) = state.tasks.get_mut(task) {
                t.reviews.push(ReviewRecord {
                    delivery: *delivery,
                    reviewer: reviewer.clone(),
                    verdict: *verdict,
                    conclusion: conclusion.clone(),
                    findings: findings.clone(),
                    upstream_changes: upstream_changes.clone(),
                    seq,
                    ts_ms: ts,
                });
                t.last_ts_ms = ts;
                state.targets.insert(
                    seq,
                    SeqTarget::TaskReviewed {
                        task: task.clone(),
                        idx: t.reviews.len() - 1,
                    },
                );
                reduce_task(t, cap);
            }
        }
        EventKind::TaskFinding {
            task,
            delivery,
            findings,
        } => {
            if let Some(t) = state.tasks.get_mut(task) {
                t.finding_reports.push(FindingRecord {
                    delivery: *delivery,
                    by: event.by.role.clone(),
                    findings: findings.clone(),
                    seq,
                    ts_ms: ts,
                });
                t.last_ts_ms = ts;
            }
        }
        EventKind::TaskState {
            task,
            state: s,
            reason,
        } => {
            if let Some(t) = state.tasks.get_mut(task) {
                t.state_changes.push(StateChangeRecord {
                    state: *s,
                    reason: reason.clone(),
                    seq,
                    ts_ms: ts,
                });
                t.last_ts_ms = ts;
                state.targets.insert(
                    seq,
                    SeqTarget::TaskState {
                        task: task.clone(),
                        idx: t.state_changes.len() - 1,
                    },
                );
                reduce_task(t, cap);
            }
        }
        EventKind::MessageQueued { to, file, purpose } => {
            state.messages.push(Message {
                seq,
                from: event.by.role.clone(),
                to: to.clone(),
                file: file.clone(),
                purpose: purpose.clone(),
                delivered: false,
            });
            state
                .targets
                .insert(seq, SeqTarget::Message(state.messages.len() - 1));
        }
        EventKind::MessageDelivered { target } => {
            if let Some(SeqTarget::Message(idx)) = state.targets.get(target)
                && let Some(m) = state.messages.get_mut(*idx)
            {
                m.delivered = true;
            }
        }
        EventKind::ReportSent { to, node, file } => {
            state.reports.push(ReportRecord {
                seq,
                from: event.by.role.clone(),
                to: to.clone(),
                node: node.clone(),
                file: file.clone(),
            });
            state
                .targets
                .insert(seq, SeqTarget::Report(state.reports.len() - 1));
        }
        EventKind::AttentionRaised {
            role,
            reason,
            detail,
        } => {
            state.attention.push(Attention {
                seq,
                role: role.clone(),
                reason: reason.clone(),
                detail: detail.clone(),
                open: true,
            });
            state
                .targets
                .insert(seq, SeqTarget::Attention(state.attention.len() - 1));
        }
        EventKind::AttentionCleared { target, .. } => {
            if let Some(SeqTarget::Attention(idx)) = state.targets.get(target)
                && let Some(a) = state.attention.get_mut(*idx)
            {
                a.open = false;
            }
        }
        EventKind::Correction {
            target,
            reason,
            set,
        } => {
            correct::apply_correction(state, *target, set);
            state.corrections.push(CorrectionRecord {
                seq,
                target: *target,
                reason: reason.clone(),
                set: set.clone(),
            });
        }
    }
}

/// Fold a whole log. `replay(&log[..n])` equals applying the first `n`
/// events one at a time, for every prefix `n`.
pub fn replay(events: &[Event]) -> StudioState {
    let mut state = StudioState::default();
    for event in events {
        apply(&mut state, event);
    }
    state
}
