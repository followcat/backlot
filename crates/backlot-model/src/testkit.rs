//! Shared builders for model tests. Neutral data only.

use crate::event::*;
use crate::state::{StudioState, apply, replay};

pub fn by(role: &str) -> EventBy {
    EventBy::role(role)
}

/// Deterministic: ts is derived from seq.
pub fn ev(seq: u64, role: &str, kind: EventKind) -> Event {
    Event {
        seq,
        ts_ms: seq * 1000,
        by: by(role),
        kind,
    }
}

pub fn new_ev(role: &str, kind: EventKind) -> NewEvent {
    NewEvent::new(by(role), kind)
}

pub fn actor(tag: &str) -> ActorIdentity {
    ActorIdentity {
        runner: format!("runner-{tag}"),
        runner_version: None,
        model: format!("model-{tag}"),
        effort: "high".into(),
        permission_profile: "standard".into(),
    }
}

pub fn candidate(name: &str, tag: &str) -> Candidate {
    let a = actor(tag);
    Candidate {
        name: name.into(),
        runner: a.runner,
        runner_version: a.runner_version,
        model: a.model,
        effort: a.effort,
        permission_profile: a.permission_profile,
    }
}

pub fn studio_kind() -> EventKind {
    EventKind::StudioCreated {
        title: "acme-shop".into(),
        repo: "/home/user/acme-shop".into(),
        mode: StudioMode::Standard,
        roles: Box::new(RoleConfigs {
            producer: actor("prod"),
            director: actor("dir"),
            writer: actor("wri"),
        }),
        candidates: vec![candidate("actor-a", "a"), candidate("actor-b", "b")],
        rework_cap: 2,
        stall_limit_s: 600,
    }
}

pub fn task_created(id: &str, kind: &str, req: u32) -> EventKind {
    EventKind::TaskCreated {
        task: id.into(),
        title: format!("title {id}"),
        task_kind: kind.into(),
        requirement_version: req,
        acceptance: "plan/acceptance.md".into(),
        dependencies: Vec::new(),
    }
}

pub fn cast(task: &str, name: &str, tag: &str) -> EventKind {
    EventKind::TaskCast {
        task: task.into(),
        candidate: name.into(),
        actor: actor(tag),
        reason: "picked".into(),
        priority: None,
    }
}

pub fn delivered(task: &str, n: u32, tag: &str, req: u32) -> EventKind {
    EventKind::TaskDelivered {
        task: task.into(),
        delivery: n,
        actor: actor(tag),
        commits: "a..b".into(),
        report: format!("reports/{task}-d{n}.md"),
        requirement_version: req,
        checks: Vec::new(),
    }
}

pub fn reviewed(task: &str, n: u32, verdict: Verdict, findings: Vec<Finding>) -> EventKind {
    EventKind::TaskReviewed {
        task: task.into(),
        delivery: n,
        reviewer: "director".into(),
        verdict,
        conclusion: format!("conclusion on {task} d{n}"),
        findings,
        upstream_changes: Vec::new(),
    }
}

/// A `task.state` event — pause, resume, block, abandon, takeover.
pub fn task_state(task: &str, state: TaskStateChange, reason: &str) -> EventKind {
    EventKind::TaskState {
        task: task.into(),
        state,
        reason: reason.into(),
    }
}

/// A `task.finding` event reporting `findings` on delivery `n`.
pub fn task_finding(task: &str, n: u32, findings: Vec<Finding>) -> EventKind {
    EventKind::TaskFinding {
        task: task.into(),
        delivery: n,
        findings,
    }
}

pub fn finding(id: &str, cause: Cause) -> Finding {
    Finding {
        id: id.into(),
        summary: format!("defect {id}"),
        severity: Severity::Medium,
        cause,
    }
}

/// The record for actor `tag` and task `kind`, or panic.
pub fn rec<'a>(recs: &'a [crate::ActorRecord], tag: &str, kind: &str) -> &'a crate::ActorRecord {
    let want = actor(tag);
    recs.iter()
        .find(|r| r.actor == want && r.task_kind == kind)
        .unwrap_or_else(|| panic!("no record for {tag}/{kind}"))
}

/// Studio + requirement v1 + one created task, ready for more events.
pub fn base_events() -> Vec<Event> {
    vec![
        ev(1, "user", studio_kind()),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "first cut".into(),
            },
        ),
        ev(3, "director", task_created("t1", "feature", 1)),
    ]
}

pub fn base_state() -> StudioState {
    replay(&base_events())
}

/// Extend `base` with `kinds`, applied in order with rising seqs.
pub fn state_with(base: &[Event], kinds: Vec<(&str, EventKind)>) -> StudioState {
    let mut state = replay(base);
    let mut seq = base.len() as u64;
    for (role, kind) in kinds {
        seq += 1;
        apply(&mut state, &ev(seq, role, kind));
    }
    state
}

/// A state where t1 is running under actor-a and t2 is created.
pub fn running_state() -> StudioState {
    let mut base = base_events();
    base.push(ev(4, "director", task_created("t2", "fix", 1)));
    state_with(&base, vec![("director", cast("t1", "actor-a", "a"))])
}
