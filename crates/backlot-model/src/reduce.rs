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
            members,
            no_git,
            mode,
            roles,
            candidates,
            rework_cap,
            stall_limit_s,
            nudge_after_s,
            escalate_after_s,
            check_env,
            checks,
            task_budget_s,
            limits,
        } => {
            state.studio = Some(StudioConfig {
                title: title.clone(),
                repo: repo.clone(),
                members: members.clone(),
                no_git: *no_git,
                mode: *mode,
                roles: roles.as_ref().clone(),
                candidates: candidates.clone(),
                rework_cap: *rework_cap,
                stall_limit_s: *stall_limit_s,
                nudge_after_s: *nudge_after_s,
                escalate_after_s: *escalate_after_s,
                check_env: check_env.clone(),
                checks: checks.clone(),
                task_budget_s: *task_budget_s,
                limits: limits.clone(),
            });
            state.targets.insert(seq, SeqTarget::Studio);
        }
        EventKind::RoleStarted {
            role,
            terminal,
            actor,
            session_id,
            reason,
        }
        | EventKind::RoleResumed {
            role,
            terminal,
            session_id,
            actor,
            reason,
        } => {
            let entry = state.roles.entry(role.clone()).or_default();
            entry.live = true;
            entry.closed = false;
            entry.since_ms = ts;
            // The start a restart asked for is done; the role's newest
            // move is the `restarted` word the summary shows. Any
            // other start leaves the word behind.
            let restarted = reason.as_deref() == Some(RESTARTED);
            entry.restarted_ms = restarted.then_some(ts);
            if restarted {
                entry.restart = None;
                // A fresh restart begins a new conversation: the id
                // the launch minted (or none, for a post-launch
                // runner) is the whole truth. A resumed one carries
                // the id it resumed, set below.
                if matches!(event.kind, EventKind::RoleStarted { .. }) {
                    entry.session_id = None;
                }
            }
            if terminal.is_some() {
                entry.terminal = terminal.clone();
            }
            if let Some(actor) = actor {
                // A new identity starts a new conversation: the old
                // session id belongs to the runner that left, so a
                // recast's start never resumes it later. A restarted
                // binary's loader-filled `runner_version` is not a new
                // identity.
                if entry
                    .actor
                    .as_ref()
                    .is_some_and(|old| !crate::recast::same_identity(old, actor))
                {
                    entry.session_id = None;
                }
                entry.actor = Some(actor.clone());
            }
            if session_id.is_some() {
                entry.session_id = session_id.clone();
            }
            // A `recovered` resume owes the role a notice; any other
            // start or resume clears the owed seq.
            entry.recovered_seq = match &event.kind {
                EventKind::RoleResumed {
                    reason: Some(reason),
                    ..
                } if reason == RECOVERED => Some(seq),
                _ => None,
            };
            // An actor's start opens its task's budget clock (S9e):
            // the first start ever is the origin — a recast does
            // not rewind it.
            if let Some(t) = actor_task(role).and_then(|id| state.tasks.get_mut(id)) {
                let origin = t.actor_started_ms.get_or_insert(ts);
                *origin = (*origin).min(ts);
            }
            state.targets.insert(seq, SeqTarget::Role(role.clone()));
        }
        EventKind::RoleLost { role, .. } => {
            if let Some(entry) = state.roles.get_mut(role) {
                entry.live = false;
                entry.since_ms = ts;
                entry.losses.push(ts);
            }
        }
        EventKind::RoleClosed { role, reason } => {
            // A deliberate release keeps `actor` and `session_id`: the
            // person stays resumable while their studio is live.
            let entry = state.roles.entry(role.clone()).or_default();
            entry.live = false;
            entry.closed = true;
            entry.held = false;
            entry.since_ms = ts;
            // The stop a restart asked for carries the request on: the
            // start that follows it clears it. Any other close ends it.
            if reason.as_deref() != Some(RESTARTED) {
                entry.restart = None;
            }
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
        // The newest recast is the fixed role's identity from here on;
        // the role's recorded lifecycle — and every earlier delivery
        // and review — keeps the identity it was started with.
        EventKind::RoleRecast {
            role,
            actor,
            reason,
            card,
        } => {
            state.recasts.push(RecastRecord {
                role: role.clone(),
                actor: actor.clone(),
                reason: reason.clone(),
                card: card.clone(),
                seq,
                ts_ms: ts,
            });
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
        // The request the stage carries out next tick: stop the
        // terminal, then start the role again.
        EventKind::RoleRestart {
            role,
            fresh,
            reason,
        } => {
            let entry = state.roles.entry(role.clone()).or_default();
            entry.restart = Some(RestartRequest {
                seq,
                fresh: *fresh,
                reason: reason.clone(),
            });
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
                ts_ms: ts,
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
                    checked: Vec::new(),
                    reviews: Vec::new(),
                    finding_reports: Vec::new(),
                    state_changes: Vec::new(),
                    created_seq: seq,
                    created_ts_ms: ts,
                    created_by: event.by.role.clone(),
                    last_ts_ms: ts,
                    closed_ts_ms: None,
                    closed_seq: None,
                    actor_started_ms: None,
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
                    by: event.by.role.clone(),
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
            usage,
        } => {
            if let Some(u) = usage {
                state.usage.add(u);
            }
            if let Some(t) = state.tasks.get_mut(task) {
                t.deliveries.push(DeliveryRecord {
                    n: *delivery,
                    actor: actor.clone(),
                    commits: commits.clone(),
                    report: report.clone(),
                    requirement_version: *requirement_version,
                    checks: checks.clone(),
                    usage: usage.clone(),
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
        EventKind::TaskChecked {
            task,
            delivery,
            name,
            exit_code,
            timed_out,
            output,
            duration_ms,
        } => {
            if let Some(t) = state.tasks.get_mut(task) {
                t.checked.push(CheckedRecord {
                    delivery: *delivery,
                    name: name.clone(),
                    exit_code: *exit_code,
                    timed_out: *timed_out,
                    output: output.clone(),
                    duration_ms: *duration_ms,
                    seq,
                    ts_ms: ts,
                });
                t.last_ts_ms = ts;
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
                    by: event.by.role.clone(),
                    resumed_to: None,
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
        EventKind::MessageQueued {
            to,
            file,
            purpose,
            source_seq,
        } => {
            state.messages.push(Message {
                seq,
                from: event.by.role.clone(),
                to: to.clone(),
                file: file.clone(),
                purpose: purpose.clone(),
                source_seq: *source_seq,
                delivered: false,
                dropped: false,
                ts_ms: ts,
                delivered_ms: None,
            });
            state
                .targets
                .insert(seq, SeqTarget::Message(state.messages.len() - 1));
        }
        EventKind::MessageDelivered { target } => match state.targets.get(target) {
            Some(SeqTarget::Message(idx)) => {
                if let Some(m) = state.messages.get_mut(*idx) {
                    m.delivered = true;
                    m.delivered_ms = Some(ts);
                }
            }
            Some(SeqTarget::Report(idx)) => {
                if let Some(r) = state.reports.get_mut(*idx) {
                    r.delivered = true;
                    r.delivered_ms = Some(ts);
                }
            }
            _ => {}
        },
        EventKind::MessageDropped { target, .. } => {
            if let Some(SeqTarget::Message(idx)) = state.targets.get(target)
                && let Some(m) = state.messages.get_mut(*idx)
            {
                m.dropped = true;
            }
        }
        EventKind::ReportSent { to, node, file } => {
            state.reports.push(ReportRecord {
                seq,
                from: event.by.role.clone(),
                to: to.clone(),
                node: node.clone(),
                file: file.clone(),
                delivered: false,
                ts_ms: ts,
                delivered_ms: None,
            });
            state
                .targets
                .insert(seq, SeqTarget::Report(state.reports.len() - 1));
        }
        EventKind::AttentionRaised {
            role,
            reason,
            detail,
            needs_user,
        } => {
            state.attention.push(Attention {
                seq,
                role: role.clone(),
                reason: reason.clone(),
                detail: detail.clone(),
                open: true,
                ts_ms: ts,
                needs_user: *needs_user,
                cleared_reason: None,
            });
            state
                .targets
                .insert(seq, SeqTarget::Attention(state.attention.len() - 1));
        }
        EventKind::AttentionCleared { target, reason } => {
            if let Some(SeqTarget::Attention(idx)) = state.targets.get(target)
                && let Some(a) = state.attention.get_mut(*idx)
            {
                a.open = false;
                a.cleared_reason = reason.clone();
            }
        }
        // A stage answer lands on a role the log already knows —
        // the stage only answers dialogs of roles it started — and
        // stamps the role's bookkeeping only: no attention record,
        // nothing to clear.
        EventKind::StageAnswered { role, .. } => {
            if let Some(entry) = state.roles.get_mut(role) {
                entry.stage_answer_ms = Some(ts);
            }
        }
        EventKind::RoleRefused { .. } => {}
        EventKind::RoleApproval { role, decision } => {
            state.approvals.push(ApprovalRecord {
                seq,
                role: role.clone(),
                by: event.by.role.clone(),
                decision: *decision,
                ts_ms: ts,
            });
        }
        EventKind::StudioFinished { reason } => {
            state.finished = Some(FinishRecord {
                seq,
                reason: reason.clone(),
                ts_ms: ts,
            });
        }
        EventKind::StudioDeleted { .. } => {
            for entry in state.roles.values_mut() {
                entry.live = false;
                entry.closed = true;
                entry.held = false;
                entry.since_ms = ts;
            }
            state.targets.insert(seq, SeqTarget::Studio);
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
