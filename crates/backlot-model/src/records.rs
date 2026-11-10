//! Actor records (ledger.md "Record view"), computed across studios'
//! logs. Factual counts only — no numeric score or grade (D8). A group
//! with fewer than `MIN_TASKS` tasks reports "not enough data".

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::Serialize;

use crate::event::{ActorIdentity, Event, TaskStateChange, Verdict};
use crate::state::{StudioState, Task, TaskStatus, replay};

/// One studio's name and its event log.
pub struct StudioLog {
    pub studio: String,
    pub events: Vec<Event>,
}

/// Fewer than this many tasks is reported as "not enough data".
pub const MIN_TASKS: usize = 5;

/// How the review of each task's first delivery by this actor ended.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FirstDelivery {
    pub accepted: usize,
    pub partially_accepted: usize,
    pub changes_requested: usize,
    pub blocked: usize,
    pub unreviewed: usize,
}

/// The task's last status, bucketed (ledger.md "final acceptance").
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FinalOutcome {
    pub accepted: usize,
    pub partially_accepted: usize,
    pub blocked: usize,
    pub abandoned: usize,
    pub at_cap: usize,
    pub taken_over: usize,
    /// Still running, delivered, paused, blocked-but-open or cast.
    pub open: usize,
}

/// Per actor identity and task kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActorRecord {
    pub actor: ActorIdentity,
    pub task_kind: String,
    /// Studios that contributed tasks, sorted and deduplicated.
    pub studios: Vec<String>,
    /// Tasks with at least one delivery by this actor. Several
    /// deliveries of one task still count as one task.
    pub tasks: usize,
    pub deliveries: usize,
    pub first_delivery: FirstDelivery,
    /// Findings on reviews of this actor's deliveries.
    pub defects: usize,
    /// Findings whose cause is the actor's own failure; a delivery
    /// under an older requirement carries `requirement_change` instead.
    pub actor_fault_defects: usize,
    /// Defects a later review of the task no longer lists.
    pub defects_closed: usize,
    /// Defects still listed in the task's latest review.
    pub defects_open: usize,
    /// Findings on this actor's reviews whose defect id was already
    /// seen earlier in the same task — repeated rework.
    pub defect_repeats: usize,
    /// Review findings whose defect id first surfaced on a delivery
    /// after the task's first — e.g. regressions introduced by fixes.
    pub new_in_rework: usize,
    /// Findings recorded by `task.finding` events on this actor's
    /// deliveries — defects found after the review loop, e.g. during
    /// integration or real use.
    pub later_defects: usize,
    /// Fixes the director made itself on this actor's deliveries.
    pub upstream_changes: usize,
    /// Stage-run checks on this actor's deliveries that passed —
    /// the ledger's own records (S9b part 2).
    pub stage_checks_passed: usize,
    /// Stage-run checks on this actor's deliveries that failed or
    /// timed out.
    pub stage_checks_failed: usize,
    /// Check results the actor reported on `task.delivered` — its
    /// own word, never the stage's.
    pub reported_checks_passed: usize,
    /// Check results the actor reported as failing.
    pub reported_checks_failed: usize,
    /// Tasks this actor delivered on that were taken over.
    pub takeovers: usize,
    pub final_outcome: FinalOutcome,
    /// Sum over the actor's tasks of created → last task event.
    pub wall_time_ms: u64,
    /// False under `MIN_TASKS`: the CLI prints "not enough data".
    pub enough_data: bool,
}

#[derive(Default)]
struct Acc {
    studios: BTreeSet<String>,
    tasks: usize,
    deliveries: usize,
    first_delivery: FirstDelivery,
    defects: usize,
    actor_fault_defects: usize,
    defects_closed: usize,
    defects_open: usize,
    defect_repeats: usize,
    new_in_rework: usize,
    later_defects: usize,
    upstream_changes: usize,
    stage_checks_passed: usize,
    stage_checks_failed: usize,
    reported_checks_passed: usize,
    reported_checks_failed: usize,
    takeovers: usize,
    final_outcome: FinalOutcome,
    wall_time_ms: u64,
}

impl Acc {
    fn into_record(self, actor: ActorIdentity, task_kind: String) -> ActorRecord {
        ActorRecord {
            actor,
            task_kind,
            studios: self.studios.into_iter().collect(),
            tasks: self.tasks,
            deliveries: self.deliveries,
            first_delivery: self.first_delivery,
            defects: self.defects,
            actor_fault_defects: self.actor_fault_defects,
            defects_closed: self.defects_closed,
            defects_open: self.defects_open,
            defect_repeats: self.defect_repeats,
            new_in_rework: self.new_in_rework,
            later_defects: self.later_defects,
            upstream_changes: self.upstream_changes,
            stage_checks_passed: self.stage_checks_passed,
            stage_checks_failed: self.stage_checks_failed,
            reported_checks_passed: self.reported_checks_passed,
            reported_checks_failed: self.reported_checks_failed,
            takeovers: self.takeovers,
            final_outcome: self.final_outcome,
            wall_time_ms: self.wall_time_ms,
            enough_data: self.tasks >= MIN_TASKS,
        }
    }
}

fn first_delivery_bucket(acc: &mut Acc, verdict: Option<Verdict>) {
    let f = &mut acc.first_delivery;
    match verdict {
        Some(Verdict::Accepted) => f.accepted += 1,
        Some(Verdict::PartiallyAccepted) => f.partially_accepted += 1,
        Some(Verdict::ChangesRequested) => f.changes_requested += 1,
        Some(Verdict::Blocked) => f.blocked += 1,
        None => f.unreviewed += 1,
    }
}

fn final_bucket(acc: &mut Acc, status: TaskStatus) {
    let f = &mut acc.final_outcome;
    match status {
        TaskStatus::Accepted => f.accepted += 1,
        TaskStatus::PartiallyAccepted => f.partially_accepted += 1,
        TaskStatus::Blocked => f.blocked += 1,
        TaskStatus::Abandoned => f.abandoned += 1,
        TaskStatus::AtCap => f.at_cap += 1,
        TaskStatus::TakenOver => f.taken_over += 1,
        _ => f.open += 1,
    }
}

/// Fold one task into the record of every actor that delivered on it.
fn fold_task(accs: &mut BTreeMap<(ActorIdentity, String), Acc>, task: &Task, studio: &str) {
    let actors: BTreeSet<&ActorIdentity> = task.deliveries.iter().map(|d| &d.actor).collect();
    if actors.is_empty() {
        return;
    }

    // Task-wide defect timeline, in review order.
    let mut reviews: Vec<&crate::state::ReviewRecord> = task.reviews.iter().collect();
    reviews.sort_by_key(|r| (r.delivery, r.seq));
    let mut first_seen: HashMap<&str, u32> = HashMap::new();
    let mut latest_review_seq = 0u64;
    for r in &reviews {
        latest_review_seq = latest_review_seq.max(r.seq);
        for f in &r.findings {
            first_seen.entry(f.id.as_str()).or_insert(r.delivery);
        }
    }
    // Defect ids still listed in the task's latest review stay open.
    let open_defects: BTreeSet<&str> = task
        .reviews
        .iter()
        .filter(|r| r.seq == latest_review_seq)
        .flat_map(|r| r.findings.iter().map(|f| f.id.as_str()))
        .collect();
    let taken_over = task
        .state_changes
        .iter()
        .any(|s| s.state == TaskStateChange::TakenOver);

    for actor in actors {
        let acc = accs
            .entry((actor.clone(), task.task_kind.clone()))
            .or_default();
        acc.studios.insert(studio.to_string());
        acc.tasks += 1;
        // Wall time is the actor's own cast intervals: each of their
        // casts runs to the next cast on the task, or to the task's
        // last event if theirs is the latest. A recast splits the
        // task's span rather than double-counting it.
        for (i, c) in task.casts.iter().enumerate() {
            if &c.actor == actor {
                let end = task
                    .casts
                    .get(i + 1)
                    .map(|n| n.ts_ms)
                    .unwrap_or(task.last_ts_ms);
                acc.wall_time_ms += end.saturating_sub(c.ts_ms);
            }
        }
        if taken_over {
            acc.takeovers += 1;
        }
        final_bucket(acc, task.status);

        let mine: BTreeSet<u32> = task
            .deliveries
            .iter()
            .filter(|d| &d.actor == actor)
            .map(|d| d.n)
            .collect();
        acc.deliveries += mine.len();
        let first_n = mine.iter().next().copied().unwrap_or(0);
        first_delivery_bucket(acc, task.review_of(first_n).map(|r| r.verdict));

        // The stage's own check records on this actor's deliveries
        // count apart from the results the delivery reported.
        for n in &mine {
            for r in task.checked_of(*n) {
                if r.passed() {
                    acc.stage_checks_passed += 1;
                } else {
                    acc.stage_checks_failed += 1;
                }
            }
            if let Some(d) = task.deliveries.iter().find(|d| d.n == *n) {
                for c in &d.checks {
                    if c.passed {
                        acc.reported_checks_passed += 1;
                    } else {
                        acc.reported_checks_failed += 1;
                    }
                }
            }
        }

        let mut counted: BTreeSet<&str> = BTreeSet::new();
        for r in &reviews {
            if !mine.contains(&r.delivery) {
                continue;
            }
            acc.upstream_changes += r.upstream_changes.len();
            for f in &r.findings {
                acc.defects += 1;
                if f.cause.is_actor_fault() {
                    acc.actor_fault_defects += 1;
                }
                if first_seen
                    .get(f.id.as_str())
                    .is_some_and(|n| *n < r.delivery)
                {
                    acc.defect_repeats += 1;
                }
                if r.delivery > 1 && first_seen.get(f.id.as_str()) == Some(&r.delivery) {
                    acc.new_in_rework += 1;
                }
                if counted.insert(f.id.as_str()) {
                    if open_defects.contains(f.id.as_str()) {
                        acc.defects_open += 1;
                    } else {
                        acc.defects_closed += 1;
                    }
                }
            }
        }
        // `task.finding` findings land on the delivery they name:
        // the actor who made it carries them as later defects.
        for fr in &task.finding_reports {
            if mine.contains(&fr.delivery) {
                acc.later_defects += fr.findings.len();
            }
        }
    }
}

/// Actor records across all studios' logs, per actor identity and
/// task kind, sorted for stable output.
pub fn records(logs: &[StudioLog]) -> Vec<ActorRecord> {
    let mut accs: BTreeMap<(ActorIdentity, String), Acc> = BTreeMap::new();
    for log in logs {
        let state: StudioState = replay(&log.events);
        for task in state.tasks.values() {
            fold_task(&mut accs, task, &log.studio);
        }
    }
    accs.into_iter()
        .map(|((actor, kind), acc)| acc.into_record(actor, kind))
        .collect()
}
