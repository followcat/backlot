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
fn first_epoch_baselines_existing_completions_and_initial_idle_done_are_readiness() {
    for (word, completion) in [
        ("idle", None),
        ("done", None),
        ("idle", Some(10)),
        ("done", Some(10)),
    ] {
        let mut p = Fixture::default();
        assert_eq!(
            state(
                &update(&mut p, vec![terminal("t1", word, 10, completion)], 1),
                "t1"
            ),
            AgentStatus::Idle
        );
    }
}

#[test]
fn core_completion_survives_coalesced_work_without_a_turn_or_word_change() {
    let mut p = Fixture::default();
    let mut t = terminal("t1", "idle", 1, None);
    update(&mut p, vec![t.clone()], 1);
    t.activity = Some(activity("boot-a", 1, Some(20), Some(20)));
    let mut projection = update(&mut p, vec![t.clone()], 2);
    assert_eq!(state(&projection, "t1"), AgentStatus::Done);
    assert!(!p.acknowledge(&mut projection, "t1", Some(false)));
    assert_eq!(state(&projection, "t1"), AgentStatus::Done);
    assert!(p.acknowledge(&mut projection, "t1", Some(true)));
    assert_eq!(state(&projection, "t1"), AgentStatus::Idle);
    assert_eq!(
        state(&update(&mut p, vec![t.clone()], 3), "t1"),
        AgentStatus::Idle
    );
    t.activity = Some(activity("boot-a", 1, Some(30), Some(30)));
    assert_eq!(state(&update(&mut p, vec![t], 4), "t1"), AgentStatus::Done);
}

#[test]
fn observed_work_turns_timestamps_and_done_words_cannot_manufacture_completion() {
    for predecessor in ["working", "blocked", "error"] {
        let mut p = Fixture::default();
        let mut t = terminal("t1", predecessor, 1, None);
        update(&mut p, vec![t.clone()], 1);
        t.status.as_mut().unwrap().state = "done".into();
        t.turn = u64::MAX;
        t.status_ms = Some(u64::MAX);
        t.activity = Some(activity("boot-a", 1, Some(2), None));
        assert_eq!(state(&update(&mut p, vec![t], 2), "t1"), AgentStatus::Idle);
    }
}

#[test]
fn blocked_error_and_clear_honor_the_core_marker_instead_of_a_local_work_guard() {
    let mut p = Fixture::default();
    let mut t = terminal("t1", "working", 1, None);
    update(&mut p, vec![t.clone()], 1);
    t.status.as_mut().unwrap().state = "error".into();
    t.activity = Some(activity("boot-a", 1, Some(2), None));
    update(&mut p, vec![t.clone()], 2);
    t.status.as_mut().unwrap().state = "idle".into();
    t.activity = Some(activity("boot-a", 1, Some(3), None));
    assert_eq!(
        state(&update(&mut p, vec![t.clone()], 3), "t1"),
        AgentStatus::Idle
    );
    // A later genuine completion is authoritative even if Working was missed.
    t.activity = Some(activity("boot-a", 1, Some(5), Some(5)));
    assert_eq!(
        state(&update(&mut p, vec![t.clone()], 4), "t1"),
        AgentStatus::Done
    );
    t.activity
        .as_mut()
        .unwrap()
        .records
        .get_mut("")
        .unwrap()
        .completion_ordinal = None;
    assert_eq!(state(&update(&mut p, vec![t], 5), "t1"), AgentStatus::Idle);
}

#[test]
fn epoch_scope_origin_and_incarnation_changes_baseline_without_comparing_old_ack() {
    for change in 0..4 {
        let mut p = Fixture::default();
        let mut t = terminal("t1", "idle", 1, None);
        update(&mut p, vec![t.clone()], 1);
        t.activity = Some(activity("boot-a", 1, Some(20), Some(20)));
        assert_eq!(
            state(&update(&mut p, vec![t.clone()], 2), "t1"),
            AgentStatus::Done
        );
        match change {
            0 => t.activity.as_mut().unwrap().epoch = "boot-b".into(),
            1 => t.activity.as_mut().unwrap().scope += 1,
            2 => t.activity.as_mut().unwrap().core_id = "origin-b".into(),
            _ => t.created_ms += 1,
        }
        assert_eq!(
            state(&update(&mut p, vec![t.clone()], 3), "t1"),
            AgentStatus::Idle
        );
        let clock = t.activity.as_mut().unwrap().records.get_mut("").unwrap();
        clock.state_ordinal = Some(30);
        clock.completion_ordinal = Some(30);
        assert_eq!(state(&update(&mut p, vec![t], 4), "t1"), AgentStatus::Done);
    }
}

#[test]
fn transport_reconnect_keeps_ack_but_revision_and_metadata_do_not_advance_order() {
    let mut p = Fixture::default();
    let mut a = terminal("t1", "idle", 1, None);
    let b = terminal("t2", "working", 10, None);
    let first = update(&mut p, vec![a.clone(), b.clone()], 1);
    a.activity.as_mut().unwrap().revision = u64::MAX;
    a.status.as_mut().unwrap().title = "updated metadata".into();
    a.turn = u64::MAX;
    a.status_ms = Some(u64::MAX);
    let same = update(&mut p, vec![a.clone(), b.clone()], 99);
    assert_eq!(rank(&first, "t1"), rank(&same, "t1"));
    assert!(rank(&same, "t2") > rank(&same, "t1"));
    a.activity = Some(activity("boot-a", 1, Some(20), Some(20)));
    let mut completed = update(&mut p, vec![a.clone(), b.clone()], 100);
    p.acknowledge(&mut completed, "t1", Some(true));
    assert_eq!(
        state(&update(&mut p, vec![a, b], 101), "t1"),
        AgentStatus::Idle
    );
}

#[test]
fn named_record_completions_are_independent_and_removal_clears_their_markers() {
    let mut p = Fixture::default();
    let mut t = terminal("t1", "idle", 1, None);
    update(&mut p, vec![t.clone()], 1);
    t.records.insert(
        "build/test".into(),
        Status {
            state: "idle".into(),
            ..Status::default()
        },
    );
    t.activity.as_mut().unwrap().records.insert(
        "build/test".into(),
        hc_proto::terminal::RecordClock {
            state_ordinal: Some(20),
            completion_ordinal: Some(20),
        },
    );
    let mut completed = update(&mut p, vec![t.clone()], 2);
    assert_eq!(state(&completed, "t1"), AgentStatus::Done);
    p.acknowledge(&mut completed, "t1", Some(true));
    t.records.remove("build/test");
    t.activity.as_mut().unwrap().records.remove("build/test");
    assert_eq!(
        state(&update(&mut p, vec![t.clone()], 3), "t1"),
        AgentStatus::Idle
    );
    t.records.insert(
        "build/test".into(),
        Status {
            state: "idle".into(),
            ..Status::default()
        },
    );
    t.activity.as_mut().unwrap().records.insert(
        "build/test".into(),
        hc_proto::terminal::RecordClock {
            state_ordinal: Some(30),
            completion_ordinal: Some(30),
        },
    );
    assert_eq!(state(&update(&mut p, vec![t], 4), "t1"), AgentStatus::Done);
}

#[test]
fn unseen_completion_does_not_override_a_current_blocker_or_working_record() {
    for word in ["working", "blocked"] {
        let mut p = Fixture::default();
        let mut t = terminal("t1", "idle", 1, None);
        update(&mut p, vec![t.clone()], 1);
        t.activity = Some(activity("boot-a", 1, Some(2), Some(2)));
        t.records.insert(
            "child".into(),
            Status {
                state: word.into(),
                ..Status::default()
            },
        );
        t.activity.as_mut().unwrap().records.insert(
            "child".into(),
            hc_proto::terminal::RecordClock {
                state_ordinal: Some(3),
                completion_ordinal: None,
            },
        );
        let expected = if word == "working" {
            AgentStatus::Working
        } else {
            AgentStatus::Blocked
        };
        assert_eq!(state(&update(&mut p, vec![t], 2), "t1"), expected);
    }
}

#[test]
fn new_agent_completion_in_an_already_observed_epoch_is_not_lost() {
    let mut p = Fixture::default();
    let a = terminal("t1", "idle", 1, None);
    update(&mut p, vec![a.clone()], 1);
    update(&mut p, vec![], 2);
    let b = terminal("t2", "idle", 20, Some(20));
    assert_eq!(
        state(&update(&mut p, vec![a, b], 3), "t2"),
        AgentStatus::Done
    );
}

#[test]
fn acknowledgement_is_local_and_only_covers_the_rendered_pane() {
    let mut p = Fixture::default();
    let mut other = Fixture::default();
    let a = terminal("t1", "working", 1, None);
    let b = terminal("t2", "working", 2, None);
    update(&mut p, vec![a.clone(), b.clone()], 1);
    update(&mut other, vec![a, b], 1);
    let a = terminal("t1", "idle", 10, Some(10));
    let b = terminal("t2", "idle", 20, Some(20));
    let mut viewed = update(&mut p, vec![a.clone(), b.clone()], 2);
    let background = update(&mut other, vec![a, b], 2);
    p.acknowledge(&mut viewed, "t1", Some(true));
    assert_eq!(state(&viewed, "t1"), AgentStatus::Idle);
    assert_eq!(state(&viewed, "t2"), AgentStatus::Done);
    assert_eq!(state(&background, "t1"), AgentStatus::Done);
}

#[test]
fn absent_activity_never_uses_legacy_transition_inference() {
    let mut p = Fixture::default();
    let mut t = terminal("t1", "working", 1, None);
    t.activity = None;
    update(&mut p, vec![t.clone()], 1);
    t.status.as_mut().unwrap().state = "done".into();
    t.turn += 1;
    assert_eq!(state(&update(&mut p, vec![t], 2), "t1"), AgentStatus::Idle);
}

#[test]
fn presentation_consumes_precoalesced_receipts_without_reallocating_at_draw() {
    let mut p = Fixture::default();
    let mut a20 = terminal("a20", "idle", 20, None);
    let mut b = terminal("b", "idle", 1, None);
    b.activity.as_mut().unwrap().core_id = "origin-b".into();
    update(&mut p, vec![a20.clone()], 1);
    let mut view = View {
        generation: 2,
        version: 2,
        ..View::default()
    };
    view.inventory.replace(2, vec![a20.clone(), b.clone()]);
    p.receipts.ingest(&view.inventory, &[]);
    let a10 = terminal("a10", "idle", 10, None);
    view.inventory.replace(3, vec![a20.clone(), b, a10]);
    p.receipts.ingest(&view.inventory, &[]);
    view.activity_receipts = p.receipts.clone();
    let mut projection = crate::project::project(&view, Some("a20"));
    p.project(&view, &mut projection);
    assert_eq!(
        (
            rank(&projection, "a20"),
            rank(&projection, "b"),
            rank(&projection, "a10")
        ),
        (1, 2, 3)
    );
    a20.created_ms += 1;
    let next = update(&mut p, vec![a20], 4);
    assert_eq!(state(&next, "a20"), AgentStatus::Idle);
}
