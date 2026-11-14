//! Corrections: which recorded fields a `correction` event may change.
//! Facts stay in the log; the correction changes the reduced view.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::event::*;
use crate::state::{StudioState, reduce_task};

/// What an event's seq resolves to, so a correction knows which
/// reduced record to patch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeqTarget {
    Studio,
    ReqVersion(usize),
    Plan(usize),
    TaskCreated(String),
    TaskCast { task: String, idx: usize },
    TaskDelivered { task: String, idx: usize },
    TaskReviewed { task: String, idx: usize },
    TaskState { task: String, idx: usize },
    Message(usize),
    Report(usize),
    Attention(usize),
    Role(String),
}

/// Keys a correction may set on each target. Anything else is refused.
pub fn correction_keys(target: &SeqTarget) -> &'static [&'static str] {
    match target {
        SeqTarget::Studio => &["title", "repo", "mode", "rework_cap"],
        SeqTarget::ReqVersion(_) => &["summary", "file"],
        SeqTarget::Plan(_) => &["file", "plan_kind", "requirement_version"],
        SeqTarget::TaskCreated(_) => &[
            "title",
            "task_kind",
            "requirement_version",
            "acceptance",
            "dependencies",
        ],
        SeqTarget::TaskCast { .. } => &[
            "candidate",
            "reason",
            "runner",
            "runner_version",
            "model",
            "effort",
            "permission_profile",
        ],
        SeqTarget::TaskDelivered { .. } => &[
            "commits",
            "report",
            "requirement_version",
            "candidate",
            "runner",
            "runner_version",
            "model",
            "effort",
            "permission_profile",
        ],
        SeqTarget::TaskReviewed { .. } => &["verdict", "conclusion"],
        SeqTarget::TaskState { .. } => &["state", "reason"],
        SeqTarget::Message(_) => &["to", "file"],
        SeqTarget::Report(_) => &["to", "node", "file"],
        SeqTarget::Attention(_) => &["role", "reason", "detail"],
        SeqTarget::Role(_) => &["terminal", "session_id"],
    }
}

/// Whether `value` parses for `key`. Text fields accept anything.
pub fn correction_value_ok(key: &str, value: &str) -> bool {
    match key {
        "rework_cap" | "requirement_version" => value.parse::<u32>().is_ok(),
        "mode" => parse_mode(value).is_some(),
        "plan_kind" => parse_plan_kind(value).is_some(),
        "verdict" => parse_verdict(value).is_some(),
        "state" => parse_task_state(value).is_some(),
        "candidate" => !value.is_empty(),
        _ => true,
    }
}

fn norm(s: &str) -> String {
    s.replace('-', "_")
}

pub fn parse_mode(s: &str) -> Option<StudioMode> {
    match norm(s).as_str() {
        "standard" => Some(StudioMode::Standard),
        "experimental" => Some(StudioMode::Experimental),
        _ => None,
    }
}

pub fn parse_plan_kind(s: &str) -> Option<PlanKind> {
    match norm(s).as_str() {
        "plan" => Some(PlanKind::Plan),
        "acceptance" => Some(PlanKind::Acceptance),
        _ => None,
    }
}

pub fn parse_verdict(s: &str) -> Option<Verdict> {
    match norm(s).as_str() {
        "accepted" | "accept" => Some(Verdict::Accepted),
        "partially_accepted" | "partial" => Some(Verdict::PartiallyAccepted),
        "changes_requested" | "changes" => Some(Verdict::ChangesRequested),
        "blocked" => Some(Verdict::Blocked),
        _ => None,
    }
}

pub fn parse_task_state(s: &str) -> Option<TaskStateChange> {
    match norm(s).as_str() {
        "paused" | "pause" => Some(TaskStateChange::Paused),
        "resumed" | "resume" => Some(TaskStateChange::Resumed),
        "abandoned" | "abandon" => Some(TaskStateChange::Abandoned),
        "taken_over" | "takeover" => Some(TaskStateChange::TakenOver),
        "blocked" | "block" => Some(TaskStateChange::Blocked),
        _ => None,
    }
}

fn find_candidate<'a>(state: &'a StudioState, name: &str) -> Option<&'a Candidate> {
    state
        .studio
        .as_ref()?
        .candidates
        .iter()
        .find(|c| c.name == name)
}

fn patch_actor(actor: &mut ActorIdentity, key: &str, value: &str) {
    match key {
        "runner" => actor.runner = value.to_string(),
        "runner_version" => {
            actor.runner_version = if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        }
        "model" => actor.model = value.to_string(),
        "effort" => {
            actor.effort = if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        }
        "permission_profile" => actor.permission_profile = value.to_string(),
        _ => {}
    }
}

/// Apply the corrected fields to the reduced view. Unknown keys are
/// skipped: append-time validation is what refuses them, and replay
/// must stay a total function.
pub fn apply_correction(state: &mut StudioState, target: u64, set: &BTreeMap<String, String>) {
    let Some(seq_target) = state.targets.get(&target).cloned() else {
        return;
    };
    let cap = state.rework_cap();
    for (key, value) in set {
        if !correction_keys(&seq_target).contains(&key.as_str()) {
            continue;
        }
        if !correction_value_ok(key, value) {
            continue;
        }
        match &seq_target {
            SeqTarget::Studio => {
                if let Some(s) = state.studio.as_mut() {
                    match key.as_str() {
                        "title" => s.title = value.clone(),
                        "repo" => s.repo = value.clone(),
                        "mode" => s.mode = parse_mode(value).unwrap_or(s.mode),
                        "rework_cap" => {
                            if let Ok(cap) = value.parse::<u32>() {
                                s.rework_cap = cap;
                                for task in state.tasks.values_mut() {
                                    reduce_task(task, cap);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            SeqTarget::ReqVersion(idx) => {
                if let Some(r) = state.requirements.get_mut(*idx) {
                    match key.as_str() {
                        "summary" => r.summary = value.clone(),
                        "file" => r.file = value.clone(),
                        _ => {}
                    }
                }
            }
            SeqTarget::Plan(idx) => {
                if let Some(p) = state.plans.get_mut(*idx) {
                    match key.as_str() {
                        "file" => p.file = value.clone(),
                        "plan_kind" => p.plan_kind = parse_plan_kind(value).unwrap_or(p.plan_kind),
                        "requirement_version" => {
                            if let Ok(v) = value.parse() {
                                p.requirement_version = v;
                            }
                        }
                        _ => {}
                    }
                }
            }
            SeqTarget::TaskCreated(id) => {
                if let Some(t) = state.tasks.get_mut(id) {
                    match key.as_str() {
                        "title" => t.title = value.clone(),
                        "task_kind" => t.task_kind = value.clone(),
                        "requirement_version" => {
                            if let Ok(v) = value.parse() {
                                t.requirement_version = v;
                            }
                        }
                        "acceptance" => t.acceptance = value.clone(),
                        "dependencies" => {
                            t.dependencies = value
                                .split(',')
                                .map(|d| d.trim().to_string())
                                .filter(|d| !d.is_empty())
                                .collect();
                        }
                        _ => {}
                    }
                }
            }
            SeqTarget::TaskCast { task, idx } => {
                let resolved = if key == "candidate" {
                    find_candidate(state, value).map(|c| (c.name.clone(), c.actor()))
                } else {
                    None
                };
                if key == "candidate" && resolved.is_none() {
                    continue;
                }
                if let Some(t) = state.tasks.get_mut(task)
                    && let Some(cast) = t.casts.get_mut(*idx)
                {
                    if let Some((name, actor)) = resolved {
                        cast.candidate = name;
                        cast.actor = actor;
                    } else if key == "reason" {
                        cast.reason = value.clone();
                    } else {
                        patch_actor(&mut cast.actor, key, value);
                    }
                    t.cast = t.casts.last().cloned();
                }
            }
            SeqTarget::TaskDelivered { task, idx } => {
                let resolved = if key == "candidate" {
                    find_candidate(state, value).map(|c| c.actor())
                } else {
                    None
                };
                if key == "candidate" && resolved.is_none() {
                    continue;
                }
                if let Some(t) = state.tasks.get_mut(task)
                    && let Some(d) = t.deliveries.get_mut(*idx)
                {
                    match key.as_str() {
                        "commits" => d.commits = value.clone(),
                        "report" => d.report = value.clone(),
                        "requirement_version" => {
                            if let Ok(v) = value.parse() {
                                d.requirement_version = v;
                            }
                        }
                        "candidate" => {
                            if let Some(actor) = resolved {
                                d.actor = actor;
                            }
                        }
                        _ => patch_actor(&mut d.actor, key, value),
                    }
                }
            }
            SeqTarget::TaskReviewed { task, idx } => {
                if let Some(t) = state.tasks.get_mut(task) {
                    if let Some(r) = t.reviews.get_mut(*idx) {
                        match key.as_str() {
                            "verdict" => {
                                if let Some(v) = parse_verdict(value) {
                                    r.verdict = v;
                                }
                            }
                            "conclusion" => r.conclusion = value.clone(),
                            _ => {}
                        }
                    }
                    reduce_task(t, cap);
                }
            }
            SeqTarget::TaskState { task, idx } => {
                if let Some(t) = state.tasks.get_mut(task) {
                    if let Some(s) = t.state_changes.get_mut(*idx) {
                        match key.as_str() {
                            "state" => {
                                if let Some(v) = parse_task_state(value) {
                                    s.state = v;
                                }
                            }
                            "reason" => s.reason = value.clone(),
                            _ => {}
                        }
                    }
                    reduce_task(t, cap);
                }
            }
            SeqTarget::Message(idx) => {
                if let Some(m) = state.messages.get_mut(*idx) {
                    match key.as_str() {
                        "to" => m.to = value.clone(),
                        "file" => m.file = Some(value.clone()),
                        _ => {}
                    }
                }
            }
            SeqTarget::Report(idx) => {
                if let Some(r) = state.reports.get_mut(*idx) {
                    match key.as_str() {
                        "to" => r.to = value.clone(),
                        "node" => r.node = value.clone(),
                        "file" => r.file = value.clone(),
                        _ => {}
                    }
                }
            }
            SeqTarget::Attention(idx) => {
                if let Some(a) = state.attention.get_mut(*idx) {
                    match key.as_str() {
                        "role" => a.role = value.clone(),
                        "reason" => a.reason = value.clone(),
                        "detail" => a.detail = value.clone(),
                        _ => {}
                    }
                }
            }
            SeqTarget::Role(role) => {
                if let Some(r) = state.roles.get_mut(role) {
                    match key.as_str() {
                        "terminal" => r.terminal = Some(value.clone()),
                        "session_id" => r.session_id = Some(value.clone()),
                        _ => {}
                    }
                }
            }
        }
    }
}
