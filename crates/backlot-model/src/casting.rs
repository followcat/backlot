//! Candidate casting history (L3, plan.md): joins the studio's
//! declared candidates against the cross-studio actor records for
//! one task's kind. Factual counts only — no score, no grade, no
//! recommendation (D8). The human or the director decides.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::event::{ActorIdentity, Candidate, UsageTotals};
use crate::records::{ActorRecord, FinalOutcome, FirstDelivery};

/// How a candidate's records were matched on `runner_version`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionMatch {
    /// Every matched record carried the candidate's own
    /// `runner_version` — an unpinned candidate matched only
    /// unversioned records.
    Exact,
    /// The identity matched ignoring `runner_version`: the same
    /// runner, model, effort and permission profile on another
    /// build, or a candidate that pins no version at all.
    AcrossVersions,
}

/// One candidate's factual record for the task being cast — the
/// ledger's own counts, in the cast's declaration order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CandidateHistory {
    /// The candidate, as `studio.created` declared it.
    pub candidate: Candidate,
    /// `None` when no record matched — the "no history yet" case.
    pub version_matched: Option<VersionMatch>,
    /// The merged record for the task's kind, when any matched.
    pub exact_kind: Option<ActorRecord>,
    /// The candidate's merged records on other task kinds —
    /// surfaced only while `exact_kind` is absent or reports fewer
    /// than `MIN_TASKS` tasks.
    pub other_kinds: Vec<ActorRecord>,
    /// `exact_kind` exists and saw at least `MIN_TASKS` tasks.
    pub enough_data: bool,
}

/// Two identities equal on everything but `runner_version`.
fn same_ignoring_version(a: &ActorIdentity, b: &ActorIdentity) -> bool {
    a.runner == b.runner
        && a.model == b.model
        && a.effort == b.effort
        && a.permission_profile == b.permission_profile
}

/// Merge records of one kind — matched across runner versions —
/// into a single record carrying the candidate's own identity.
/// Every counter is the sum of its parts; `studios` unions;
/// `enough_data` recomputes on the merged task count.
fn merge_records(identity: &ActorIdentity, task_kind: &str, recs: &[&ActorRecord]) -> ActorRecord {
    let mut out = ActorRecord {
        actor: identity.clone(),
        task_kind: task_kind.to_string(),
        studios: Vec::new(),
        tasks: 0,
        deliveries: 0,
        first_delivery: FirstDelivery::default(),
        defects: 0,
        actor_fault_defects: 0,
        defects_closed: 0,
        defects_open: 0,
        defect_repeats: 0,
        new_in_rework: 0,
        later_defects: 0,
        upstream_changes: 0,
        stage_checks_passed: 0,
        stage_checks_failed: 0,
        reported_checks_passed: 0,
        reported_checks_failed: 0,
        takeovers: 0,
        final_outcome: FinalOutcome::default(),
        wall_time_ms: 0,
        usage: UsageTotals::default(),
        enough_data: false,
    };
    let mut studios = std::collections::BTreeSet::new();
    for r in recs {
        studios.extend(r.studios.iter().cloned());
        out.tasks += r.tasks;
        out.deliveries += r.deliveries;
        let f = &mut out.first_delivery;
        let g = &r.first_delivery;
        f.accepted += g.accepted;
        f.partially_accepted += g.partially_accepted;
        f.changes_requested += g.changes_requested;
        f.blocked += g.blocked;
        f.unreviewed += g.unreviewed;
        out.defects += r.defects;
        out.actor_fault_defects += r.actor_fault_defects;
        out.defects_closed += r.defects_closed;
        out.defects_open += r.defects_open;
        out.defect_repeats += r.defect_repeats;
        out.new_in_rework += r.new_in_rework;
        out.later_defects += r.later_defects;
        out.upstream_changes += r.upstream_changes;
        out.stage_checks_passed += r.stage_checks_passed;
        out.stage_checks_failed += r.stage_checks_failed;
        out.reported_checks_passed += r.reported_checks_passed;
        out.reported_checks_failed += r.reported_checks_failed;
        out.takeovers += r.takeovers;
        let fo = &mut out.final_outcome;
        let ro = &r.final_outcome;
        fo.accepted += ro.accepted;
        fo.partially_accepted += ro.partially_accepted;
        fo.blocked += ro.blocked;
        fo.abandoned += ro.abandoned;
        fo.at_cap += ro.at_cap;
        fo.taken_over += ro.taken_over;
        fo.open += ro.open;
        out.wall_time_ms += r.wall_time_ms;
        out.usage.input_tokens += r.usage.input_tokens;
        out.usage.output_tokens += r.usage.output_tokens;
        out.usage.cache_read_tokens += r.usage.cache_read_tokens;
        out.usage.cost_micros_usd += r.usage.cost_micros_usd;
    }
    out.studios = studios.into_iter().collect();
    out.enough_data = out.tasks >= crate::records::MIN_TASKS;
    out
}

/// Each candidate's factual history for `task_kind`, in the cast's
/// declaration order. Matching is the exact actor identity first —
/// `runner_version` included — then the same `(runner, model,
/// effort, permission_profile)` across runner versions, recorded
/// as `version_matched`.
pub fn candidate_history(
    candidates: &[Candidate],
    task_kind: &str,
    all_records: &[ActorRecord],
) -> Vec<CandidateHistory> {
    candidates
        .iter()
        .map(|candidate| {
            let identity = candidate.actor();
            let exact: Vec<&ActorRecord> =
                all_records.iter().filter(|r| r.actor == identity).collect();
            let (matched, version_matched) = if !exact.is_empty() {
                (exact, Some(VersionMatch::Exact))
            } else {
                let loose: Vec<&ActorRecord> = all_records
                    .iter()
                    .filter(|r| same_ignoring_version(&r.actor, &identity))
                    .collect();
                match loose.is_empty() {
                    true => (loose, None),
                    false => (loose, Some(VersionMatch::AcrossVersions)),
                }
            };
            let mut by_kind: BTreeMap<&str, Vec<&ActorRecord>> = BTreeMap::new();
            for r in &matched {
                by_kind.entry(r.task_kind.as_str()).or_default().push(*r);
            }
            let exact_kind = by_kind
                .remove(task_kind)
                .map(|rs| merge_records(&identity, task_kind, &rs));
            let enough_data = exact_kind.as_ref().is_some_and(|r| r.enough_data);
            // Other kinds only surface while the exact-kind history
            // is missing or too thin to read — a full record speaks
            // for itself.
            let other_kinds = match exact_kind.as_ref().is_some_and(|r| r.enough_data) {
                true => Vec::new(),
                false => by_kind
                    .into_iter()
                    .map(|(kind, rs)| merge_records(&identity, kind, &rs))
                    .collect(),
            };
            CandidateHistory {
                candidate: candidate.clone(),
                version_matched,
                exact_kind,
                other_kinds,
                enough_data,
            }
        })
        .collect()
}
