use super::*;
use crate::chrome::status::AgentStatus;
use crate::sidebar::fixture::term;
use hc_proto::terminal::{Activity, Status, Terminal};

fn activity(epoch: &str, scope: u64, state: Option<u64>, completion: Option<u64>) -> Activity {
    serde_json::from_value(serde_json::json!({
        "core_id":"origin-a", "epoch":epoch, "scope":scope, "revision":state.unwrap_or(0),
        "records":{"":{"state_ordinal":state,"completion_ordinal":completion}}
    }))
    .unwrap()
}
fn terminal(id: &str, word: &str, ordinal: u64, completion: Option<u64>) -> Terminal {
    let mut t = term(id);
    t.reported = true;
    t.status = Some(Status {
        state: word.into(),
        ..Status::default()
    });
    t.activity = Some(activity("boot-a", 1, Some(ordinal), completion));
    t
}
#[derive(Default)]
struct Fixture {
    presentation: Presentation,
    receipts: hc_proto::watch::ActivityReceipts,
}
/// One watcher publish the UI never consumed: ingest only, no project.
fn ingest_only(p: &mut Fixture, terminals: Vec<Terminal>, generation: u64) {
    let mut inventory = hc_proto::inventory::Inventory::default();
    inventory.replace(generation, terminals);
    p.receipts.ingest(&inventory, &[]);
}
/// One consumed publish: ingest, then project like the draw path does.
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
    p.presentation.project(&view, &mut projection);
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
fn post_epoch_completion_ingested_before_the_first_draw_stays_visible() {
    let mut p = Fixture::default();
    let mut t = terminal("t1", "idle", 1, None);
    update(&mut p, vec![t.clone()], 1);
    // A genuine epoch boundary is ingested, baselining its own completion.
    // The UI consumes nothing yet.
    t.activity = Some(activity("boot-b", 1, Some(5), Some(5)));
    ingest_only(&mut p, vec![t.clone()], 2);
    // Real post-boundary work completes before the first draw of the new
    // epoch: both updates coalesce into a single consumed view.
    t.activity = Some(activity("boot-b", 1, Some(9), Some(9)));
    let projection = update(&mut p, vec![t], 3);
    assert_eq!(state(&projection, "t1"), AgentStatus::Done);
}

#[test]
fn every_reallocation_kind_keeps_coalesced_post_boundary_work_visible() {
    for change in 0..4 {
        let mut p = Fixture::default();
        let mut t = terminal("t1", "idle", 1, None);
        update(&mut p, vec![t.clone()], 1);
        t.activity = Some(activity("boot-a", 1, Some(5), Some(5)));
        match change {
            0 => t.activity.as_mut().unwrap().epoch = "boot-b".into(),
            1 => t.activity.as_mut().unwrap().scope += 1,
            2 => t.activity.as_mut().unwrap().core_id = "origin-b".into(),
            _ => t.created_ms += 1,
        }
        ingest_only(&mut p, vec![t.clone()], 2);
        t.activity
            .as_mut()
            .unwrap()
            .records
            .get_mut("")
            .unwrap()
            .state_ordinal = Some(9);
        t.activity
            .as_mut()
            .unwrap()
            .records
            .get_mut("")
            .unwrap()
            .completion_ordinal = Some(9);
        let projection = update(&mut p, vec![t], 3);
        assert_eq!(state(&projection, "t1"), AgentStatus::Done, "{change}");
    }
}

#[test]
fn a_coalesced_boundary_without_later_work_still_baselines_its_completion() {
    let mut p = Fixture::default();
    let mut t = terminal("t1", "idle", 1, None);
    update(&mut p, vec![t.clone()], 1);
    t.activity = Some(activity("boot-b", 1, Some(5), Some(5)));
    ingest_only(&mut p, vec![t.clone()], 2);
    let projection = update(&mut p, vec![t], 3);
    assert_eq!(state(&projection, "t1"), AgentStatus::Idle);
}

#[test]
fn a_consumed_boundary_keeps_later_same_epoch_completions_visible() {
    let mut p = Fixture::default();
    let mut t = terminal("t1", "idle", 1, None);
    update(&mut p, vec![t.clone()], 1);
    t.activity = Some(activity("boot-b", 1, Some(5), Some(5)));
    // The boundary itself is drawn before any post-boundary work exists.
    assert_eq!(
        state(&update(&mut p, vec![t.clone()], 2), "t1"),
        AgentStatus::Idle
    );
    t.activity = Some(activity("boot-b", 1, Some(9), Some(9)));
    assert_eq!(state(&update(&mut p, vec![t], 3), "t1"), AgentStatus::Done);
}

#[test]
fn ordinary_same_epoch_completions_and_acknowledgement_are_unchanged() {
    let mut p = Fixture::default();
    let mut t = terminal("t1", "idle", 1, None);
    update(&mut p, vec![t.clone()], 1);
    t.activity = Some(activity("boot-a", 1, Some(9), Some(9)));
    let mut projection = update(&mut p, vec![t.clone()], 2);
    assert_eq!(state(&projection, "t1"), AgentStatus::Done);
    assert!(
        p.presentation
            .acknowledge(&mut projection, "t1", Some(true))
    );
    assert_eq!(state(&projection, "t1"), AgentStatus::Idle);
    assert_eq!(state(&update(&mut p, vec![t], 3), "t1"), AgentStatus::Idle);
}

#[test]
fn a_coalesced_post_epoch_completion_survives_the_acknowledgement_cycle() {
    let mut p = Fixture::default();
    let mut t = terminal("t1", "idle", 1, None);
    update(&mut p, vec![t.clone()], 1);
    t.activity = Some(activity("boot-b", 1, Some(5), Some(5)));
    ingest_only(&mut p, vec![t.clone()], 2);
    t.activity = Some(activity("boot-b", 1, Some(9), Some(9)));
    let mut projection = update(&mut p, vec![t.clone()], 3);
    assert_eq!(state(&projection, "t1"), AgentStatus::Done);
    assert!(
        p.presentation
            .acknowledge(&mut projection, "t1", Some(true))
    );
    assert_eq!(state(&projection, "t1"), AgentStatus::Idle);
    assert_eq!(state(&update(&mut p, vec![t], 4), "t1"), AgentStatus::Idle);
}

#[test]
fn a_new_agent_coalesced_into_a_new_epoch_is_not_baselines() {
    // The epoch-eligibility decision is ingestion-time, not draw-time: a
    // newcomer ingested inside an already-ingested epoch is never boundary
    // content, even when the epoch's first view coalesces away.
    let mut p = Fixture::default();
    let mut t1 = terminal("t1", "idle", 1, None);
    update(&mut p, vec![t1.clone()], 1);
    t1.activity = Some(activity("boot-b", 1, Some(5), Some(5)));
    ingest_only(&mut p, vec![t1.clone()], 2);
    // A genuinely completed newcomer joins boot-b before any draw of it.
    let mut t2 = terminal("t2", "idle", 20, Some(20));
    t2.activity = Some(activity("boot-b", 1, Some(20), Some(20)));
    ingest_only(&mut p, vec![t1.clone(), t2.clone()], 3);
    let projection = update(&mut p, vec![t1, t2], 4);
    assert_eq!(state(&projection, "t2"), AgentStatus::Done);
    assert_eq!(state(&projection, "t1"), AgentStatus::Idle);
}

#[test]
fn an_activity_loss_and_regain_rebaselines_the_regained_completions() {
    // Some(A) -> None -> Some(A) is a genuine boundary even when the UI
    // only consumes the final Some(A): the regain's own completions are
    // the boundary baseline.
    let mut p = Fixture::default();
    let mut t = terminal("t1", "idle", 1, None);
    update(&mut p, vec![t.clone()], 1);
    t.activity = Some(activity("boot-a", 1, Some(20), Some(20)));
    let mut projection = update(&mut p, vec![t.clone()], 2);
    assert!(
        p.presentation
            .acknowledge(&mut projection, "t1", Some(true))
    );
    t.activity = None;
    ingest_only(&mut p, vec![t.clone()], 3);
    t.activity = Some(activity("boot-a", 1, Some(30), Some(30)));
    ingest_only(&mut p, vec![t.clone()], 4);
    let projection = update(&mut p, vec![t], 5);
    assert_eq!(state(&projection, "t1"), AgentStatus::Idle);
}

#[test]
fn a_genuinely_later_completion_after_a_regain_stays_visible() {
    // Companion control: work ingested AFTER the regain boundary is not
    // part of the boundary baseline and must stay visible.
    let mut p = Fixture::default();
    let mut t = terminal("t1", "idle", 1, None);
    update(&mut p, vec![t.clone()], 1);
    t.activity = Some(activity("boot-a", 1, Some(20), Some(20)));
    let mut projection = update(&mut p, vec![t.clone()], 2);
    assert!(
        p.presentation
            .acknowledge(&mut projection, "t1", Some(true))
    );
    t.activity = None;
    ingest_only(&mut p, vec![t.clone()], 3);
    t.activity = Some(activity("boot-a", 1, Some(30), Some(30)));
    ingest_only(&mut p, vec![t.clone()], 4);
    t.activity = Some(activity("boot-a", 1, Some(40), Some(40)));
    let projection = update(&mut p, vec![t], 5);
    assert_eq!(state(&projection, "t1"), AgentStatus::Done);
}

#[test]
fn the_boundary_and_post_boundary_work_each_allocate_a_fresh_receipt() {
    let mut p = Fixture::default();
    let mut t = terminal("t1", "idle", 1, None);
    let first = update(&mut p, vec![t.clone()], 1);
    t.activity = Some(activity("boot-b", 1, Some(5), Some(5)));
    ingest_only(&mut p, vec![t.clone()], 2);
    t.activity = Some(activity("boot-b", 1, Some(9), Some(9)));
    let projection = update(&mut p, vec![t], 3);
    // The epoch boundary itself reallocates the display receipt; genuine
    // post-boundary work allocates once more.
    assert_eq!(rank(&projection, "t1"), rank(&first, "t1") + 2);
}
