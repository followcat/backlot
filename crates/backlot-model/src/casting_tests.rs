//! Casting-history tests (L3): exact-kind match, the runner-version
//! fallback, honest "not enough data" and "no history yet" notices,
//! the other-kinds fallback, and the D8 invariant — no score field
//! anywhere in the serialized view.

use crate::casting::{VersionMatch, candidate_history};
use crate::event::*;
use crate::records::{MIN_TASKS, StudioLog, records};
use crate::testkit::*;

/// Actor `tag` with a pinned runner version.
fn versioned(tag: &str, version: &str) -> ActorIdentity {
    let mut a = actor(tag);
    a.runner_version = Some(version.into());
    a
}

/// `n` accepted `kind` tasks for `ident` on studio `studio`.
fn studio_log(_studio: &str, kind: &str, ident: &ActorIdentity, n: usize) -> Vec<Event> {
    let mut log = base_events();
    let mut seq = log.len() as u64;
    for i in 1..=n {
        let t = format!("k{i}");
        for kind_ev in [
            task_created(&t, kind, 1),
            EventKind::TaskCast {
                task: t.clone(),
                candidate: "actor-a".into(),
                actor: ident.clone(),
                reason: "picked".into(),
                priority: None,
            },
            EventKind::TaskDelivered {
                task: t.clone(),
                delivery: 1,
                actor: ident.clone(),
                commits: "a..b".into(),
                report: "r".into(),
                requirement_version: 1,
                checks: Vec::new(),
                usage: None,
            },
            reviewed(&t, 1, Verdict::Accepted, vec![]),
        ] {
            seq += 1;
            log.push(ev(seq, "director", kind_ev));
        }
    }
    log
}

fn recs_for(logs: Vec<(&str, Vec<Event>)>) -> Vec<crate::ActorRecord> {
    records(
        &logs
            .into_iter()
            .map(|(studio, events)| StudioLog {
                studio: studio.into(),
                events,
            })
            .collect::<Vec<_>>(),
    )
}

/// A candidate whose records carry its own `runner_version` matches
/// exactly — `version_matched` says `exact`.
#[test]
fn exact_kind_match_on_the_same_version() {
    let ident = versioned("a", "1.0");
    let recs = recs_for(vec![("s1", studio_log("s1", "feature", &ident, 6))]);
    let mut cand = candidate("actor-a", "a");
    cand.runner_version = Some("1.0".into());
    let hist = candidate_history(&[cand], "feature", &recs);
    assert_eq!(hist.len(), 1);
    let h = &hist[0];
    assert_eq!(h.version_matched, Some(VersionMatch::Exact));
    let exact = h.exact_kind.as_ref().expect("exact-kind record");
    assert_eq!(exact.task_kind, "feature");
    assert_eq!(exact.tasks, 6);
    assert_eq!(exact.first_delivery.accepted, 6);
    assert!(h.enough_data);
    // A full exact-kind record stands alone — no other-kinds noise.
    assert!(h.other_kinds.is_empty());
}

/// A candidate pinning no version — the common cast — matches the
/// same identity across runner versions, marked `across_versions`,
/// and the versions merge into one record per kind.
#[test]
fn runner_version_agnostic_match_merges() {
    let v1 = versioned("a", "1.0");
    let v2 = versioned("a", "2.0");
    let recs = recs_for(vec![
        ("s1", studio_log("s1", "feature", &v1, 2)),
        ("s2", studio_log("s2", "feature", &v2, 3)),
    ]);
    let cand = candidate("actor-a", "a"); // runner_version: None
    let hist = candidate_history(&[cand], "feature", &recs);
    let h = &hist[0];
    assert_eq!(h.version_matched, Some(VersionMatch::AcrossVersions));
    let exact = h.exact_kind.as_ref().expect("merged feature record");
    assert_eq!(exact.tasks, 5);
    assert_eq!(exact.deliveries, 5);
    assert_eq!(exact.studios, vec!["s1".to_string(), "s2".to_string()]);
    // 5 merged tasks is enough data even though neither version's
    // record reached MIN_TASKS alone.
    assert!(h.enough_data);
}

/// Fewer than `MIN_TASKS` tasks on the kind reports `enough_data`
/// false — the honest "not enough data" state.
#[test]
fn fewer_than_min_tasks_is_not_enough_data() {
    let ident = actor("a");
    let recs = recs_for(vec![("s1", studio_log("s1", "feature", &ident, 2))]);
    let hist = candidate_history(&[candidate("actor-a", "a")], "feature", &recs);
    let h = &hist[0];
    assert_eq!(h.exact_kind.as_ref().map(|r| r.tasks), Some(2));
    assert!(!h.enough_data);
    assert!(!h.exact_kind.as_ref().unwrap().enough_data);
}

/// A candidate with no record anywhere gets the empty view: no
/// `exact_kind`, no `version_matched`, `enough_data` false.
#[test]
fn a_candidate_with_no_history() {
    let recs = recs_for(vec![("s1", studio_log("s1", "feature", &actor("b"), 6))]);
    let hist = candidate_history(&[candidate("actor-a", "a")], "feature", &recs);
    let h = &hist[0];
    assert!(h.exact_kind.is_none());
    assert_eq!(h.version_matched, None);
    assert!(h.other_kinds.is_empty());
    assert!(!h.enough_data);
}

/// Thin or missing exact-kind history surfaces the candidate's
/// other kinds as compact facts — and a full exact-kind record
/// hides them again.
#[test]
fn other_kinds_surface_only_when_exact_is_thin() {
    let ident = actor("a");
    let recs = recs_for(vec![
        ("s1", studio_log("s1", "fix", &ident, 7)),
        ("s2", studio_log("s2", "research", &ident, 3)),
    ]);
    // No `feature` history at all — other kinds stand in.
    let hist = candidate_history(&[candidate("actor-a", "a")], "feature", &recs);
    let h = &hist[0];
    assert!(h.exact_kind.is_none());
    let kinds: Vec<&str> = h.other_kinds.iter().map(|r| r.task_kind.as_str()).collect();
    assert_eq!(kinds, vec!["fix", "research"]);
    assert_eq!(h.other_kinds[0].tasks, 7);

    // A thin exact-kind record still shows them.
    let mut recs = recs;
    recs.extend(recs_for(vec![(
        "s3",
        studio_log("s3", "feature", &ident, 1),
    )]));
    let hist = candidate_history(&[candidate("actor-a", "a")], "feature", &recs);
    let h = &hist[0];
    assert_eq!(h.exact_kind.as_ref().map(|r| r.tasks), Some(1));
    assert_eq!(h.other_kinds.len(), 2);

    // A full exact-kind record speaks for itself.
    recs.extend(recs_for(vec![(
        "s4",
        studio_log("s4", "feature", &ident, MIN_TASKS - 1),
    )]));
    let hist = candidate_history(&[candidate("actor-a", "a")], "feature", &recs);
    let h = &hist[0];
    assert_eq!(h.exact_kind.as_ref().map(|r| r.tasks), Some(MIN_TASKS));
    assert!(h.enough_data);
    assert!(h.other_kinds.is_empty());
}

/// Candidate order is declaration order — the view never ranks.
#[test]
fn declaration_order_is_kept() {
    let recs = recs_for(vec![("s1", studio_log("s1", "feature", &actor("a"), 6))]);
    let hist = candidate_history(
        &[candidate("actor-b", "b"), candidate("actor-a", "a")],
        "feature",
        &recs,
    );
    assert_eq!(hist[0].candidate.name, "actor-b");
    assert_eq!(hist[1].candidate.name, "actor-a");
    assert!(hist[0].exact_kind.is_none());
    assert!(hist[1].exact_kind.is_some());
}

/// D8: the serialized view carries no score, rank, grade or
/// recommendation — only the ledger's own counts.
#[test]
fn no_score_fields_in_the_json() {
    let recs = recs_for(vec![("s1", studio_log("s1", "feature", &actor("a"), 6))]);
    let hist = candidate_history(&[candidate("actor-a", "a")], "feature", &recs);
    let json = serde_json::to_string(&hist).unwrap();
    for bad in ["score", "rank", "grade", "recommend", "winner", "percent"] {
        assert!(
            !json.to_lowercase().contains(bad),
            "serialized CandidateHistory contains '{bad}': {json}"
        );
    }
    // It is still structured data: the pieces the CLI prints.
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v[0]["candidate"]["name"], "actor-a");
    assert_eq!(v[0]["version_matched"], "exact");
    assert_eq!(v[0]["exact_kind"]["tasks"], 6);
    assert_eq!(v[0]["enough_data"], true);
}
