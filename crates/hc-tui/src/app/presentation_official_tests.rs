//! Cores without `record_activity` (official hc): `turn` is the only
//! completion marker and `herdr.seen` the only seen authority.
use super::*;
use crate::chrome::status::AgentStatus;
use crate::sidebar::fixture::{agent, status, term};
use hc_proto::terminal::Terminal;

/// An official-core agent: a reported word, a `turn` counter, an optional
/// `herdr.seen` label — and no `activity`/`repository` facts at all.
fn official(id: &str, word: &str, turn: u64, seen: Option<u64>) -> Terminal {
    let mut t = agent(id, word);
    t.turn = turn;
    if let Some(seen) = seen {
        t.labels.insert("herdr.seen".into(), seen.to_string());
    }
    t
}

#[derive(Default)]
struct Fixture {
    presentation: Presentation,
    receipts: hc_proto::watch::ActivityReceipts,
}
impl std::ops::Deref for Fixture {
    type Target = Presentation;
    fn deref(&self) -> &Self::Target {
        &self.presentation
    }
}
impl std::ops::DerefMut for Fixture {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.presentation
    }
}
fn update(p: &mut Fixture, terminals: Vec<Terminal>, generation: u64) -> Projection {
    let mut view = View {
        generation,
        version: generation,
        ..View::default()
    };
    view.inventory.replace(generation, terminals);
    p.receipts.ingest(&view.inventory, &[]);
    view.activity_receipts = p.receipts.clone();
    let mut projection = crate::project::project(&view, Some("t1"));
    p.project(&view, &mut projection);
    projection
}
fn state(projection: &Projection, id: &str) -> AgentStatus {
    projection
        .snapshot
        .agents
        .iter()
        .find(|a| a.pane_id == id)
        .unwrap()
        .agent_status
}
fn rank(projection: &Projection, id: &str) -> u64 {
    projection
        .snapshot
        .agents
        .iter()
        .find(|a| a.pane_id == id)
        .unwrap()
        .state_change_seq
}

#[test]
fn unseen_turn_presents_done_and_acknowledge_reads_it() {
    let mut p = Fixture::default();
    let mut projection = update(&mut p, vec![official("t1", "idle", 2, None)], 1);
    assert_eq!(state(&projection, "t1"), AgentStatus::Done);
    assert!(!p.acknowledge(&mut projection, "t1", Some(false)));
    assert_eq!(state(&projection, "t1"), AgentStatus::Done);
    assert!(p.acknowledge(&mut projection, "t1", Some(true)));
    assert_eq!(state(&projection, "t1"), AgentStatus::Idle);
}

#[test]
fn the_seen_label_is_the_shared_acknowledgement() {
    let mut p = Fixture::default();
    let projection = update(&mut p, vec![official("t1", "idle", 2, Some(2))], 1);
    assert_eq!(state(&projection, "t1"), AgentStatus::Idle);
    let projection = update(&mut p, vec![official("t1", "idle", 2, Some(1))], 2);
    assert_eq!(state(&projection, "t1"), AgentStatus::Done);
}

#[test]
fn mark_unread_lowers_the_marker_and_re_arms_the_badge() {
    let mut p = Fixture::default();
    let projection = update(&mut p, vec![official("t1", "idle", 2, Some(2))], 1);
    assert_eq!(state(&projection, "t1"), AgentStatus::Idle);
    // `terminal.label {herdr.seen: turn-1}` as mark_unseen_for writes it.
    let projection = update(&mut p, vec![official("t1", "idle", 2, Some(1))], 2);
    assert_eq!(state(&projection, "t1"), AgentStatus::Done);
}

#[test]
fn a_new_turn_re_arms_and_repaints_the_badge() {
    let mut p = Fixture::default();
    let projection = update(&mut p, vec![official("t1", "idle", 2, Some(2))], 1);
    assert_eq!(state(&projection, "t1"), AgentStatus::Idle);
    // Turn 3 arrives while the label still reads 2: genuinely unseen.
    let mut projection = update(&mut p, vec![official("t1", "idle", 3, Some(2))], 2);
    assert_eq!(state(&projection, "t1"), AgentStatus::Done);
    assert!(p.acknowledge(&mut projection, "t1", Some(true)));
    assert_eq!(state(&projection, "t1"), AgentStatus::Idle);
}

#[test]
fn working_and_rootless_never_fake_a_completion() {
    let mut p = Fixture::default();
    let projection = update(&mut p, vec![official("t1", "working", 3, Some(1))], 1);
    assert_eq!(state(&projection, "t1"), AgentStatus::Working);
    // A named record but no root status: `turn` marks only root exits, so
    // there is no root completion to present.
    let mut rootless = term("t2");
    rootless.reported = true;
    rootless.records.insert("w".into(), status("idle"));
    rootless.turn = 9;
    let projection = update(&mut p, vec![rootless], 2);
    assert_eq!(state(&projection, "t2"), AgentStatus::Idle);
}

#[test]
fn turn_zero_reports_nothing_finished() {
    let mut p = Fixture::default();
    let projection = update(&mut p, vec![official("t1", "idle", 0, None)], 1);
    assert_eq!(state(&projection, "t1"), AgentStatus::Idle);
}

#[test]
fn turn_bumps_reallocate_the_recency_receipt() {
    let mut p = Fixture::default();
    let projection = update(
        &mut p,
        vec![
            official("t1", "idle", 1, Some(1)),
            official("t2", "idle", 1, Some(1)),
        ],
        1,
    );
    let before = (rank(&projection, "t1"), rank(&projection, "t2"));
    // A word change alone is not a completion: the receipt stays.
    let projection = update(
        &mut p,
        vec![
            official("t1", "idle", 1, Some(1)),
            official("t2", "working", 1, Some(1)),
        ],
        2,
    );
    assert_eq!(rank(&projection, "t2"), before.1);
    // A turn bump takes a fresh receipt — the recency fallback moves.
    let projection = update(
        &mut p,
        vec![
            official("t1", "idle", 1, Some(1)),
            official("t2", "idle", 2, Some(1)),
        ],
        3,
    );
    assert_eq!(rank(&projection, "t1"), before.0);
    assert!(rank(&projection, "t2") > before.1);
}

#[test]
fn mixed_forks_and_official_terminals_present_together() {
    use hc_proto::terminal::Activity;
    // A fork record: completion_ordinal already seen at first sight — the
    // epoch baseline marks it read. The official record beside it has an
    // unseen `turn` — the marker fallback reports it Done.
    let mut fork = agent("t1", "done");
    fork.activity = Some(
        serde_json::from_value::<Activity>(serde_json::json!({
            "core_id": "origin-a", "epoch": "boot", "scope": 1, "revision": 7,
            "records": {"": {"state_ordinal": 7, "completion_ordinal": 7}}
        }))
        .unwrap(),
    );
    let mut p = Fixture::default();
    let projection = update(&mut p, vec![fork, official("t2", "idle", 1, Some(0))], 1);
    assert_eq!(state(&projection, "t1"), AgentStatus::Idle);
    assert_eq!(state(&projection, "t2"), AgentStatus::Done);
}
