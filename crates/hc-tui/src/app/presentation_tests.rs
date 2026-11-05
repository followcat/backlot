use super::*;
use crate::sidebar::fixture::term;
use hc_proto::terminal::{Status, Terminal};

fn terminal(address: &str, state: &str) -> Terminal {
    let mut t = term(address);
    t.reported = true;
    t.status = Some(Status {
        state: state.into(),
        ..Status::default()
    });
    t.labels.insert("herdr.seen".into(), "0".into());
    t
}

fn update(p: &mut Presentation, terminals: Vec<Terminal>, generation: u64) -> Projection {
    let mut view = View {
        generation,
        version: generation,
        ..View::default()
    };
    view.inventory.replace(generation, terminals);
    let mut projection = crate::project::project(&view, Some("t1"));
    p.project(&view, &mut projection);
    projection
}

fn state(projection: &Projection, id: &str) -> AgentStatus {
    projection
        .snapshot
        .agents
        .iter()
        .find(|agent| agent.pane_id == id)
        .unwrap()
        .agent_status
}

#[test]
fn initial_done_is_source_idle_baseline_and_next_completion_requires_acknowledgement() {
    let mut p = Presentation::default();
    let mut t = terminal("t1", "done");
    assert_eq!(
        state(&update(&mut p, vec![t.clone()], 1), "t1"),
        AgentStatus::Idle
    );
    t.status.as_mut().unwrap().state = "working".into();
    assert_eq!(
        state(&update(&mut p, vec![t.clone()], 2), "t1"),
        AgentStatus::Working
    );
    t.status.as_mut().unwrap().state = "idle".into();
    t.turn += 1;
    let mut projection = update(&mut p, vec![t.clone()], 3);
    assert_eq!(state(&projection, "t1"), AgentStatus::Done);
    assert!(!p.acknowledge(&mut projection, "t1", Some(false)));
    assert_eq!(state(&projection, "t1"), AgentStatus::Done);
    assert!(p.acknowledge(&mut projection, "t1", Some(true)));
    assert_eq!(state(&projection, "t1"), AgentStatus::Idle);
    // An unchanged server label cannot undo local Source acknowledgement.
    assert_eq!(
        state(&update(&mut p, vec![t.clone()], 4), "t1"),
        AgentStatus::Idle
    );
    t.status.as_mut().unwrap().state = "working".into();
    update(&mut p, vec![t.clone()], 5);
    t.status.as_mut().unwrap().state = "idle".into();
    t.turn += 1;
    assert_eq!(state(&update(&mut p, vec![t], 6), "t1"), AgentStatus::Done);
}

#[test]
fn blocked_after_work_retains_completion_history_and_incarnation_does_not() {
    let mut p = Presentation::default();
    let mut t = terminal("t1", "working");
    update(&mut p, vec![t.clone()], 1);
    t.status.as_mut().unwrap().state = "blocked".into();
    update(&mut p, vec![t.clone()], 2);
    t.status.as_mut().unwrap().state = "idle".into();
    assert_eq!(
        state(&update(&mut p, vec![t.clone()], 3), "t1"),
        AgentStatus::Done
    );
    t.created_ms += 1;
    assert_eq!(state(&update(&mut p, vec![t], 4), "t1"), AgentStatus::Idle);
}

#[test]
fn authoritative_turn_change_recovers_a_missed_completion_without_sorting_by_turn_count() {
    let mut p = Presentation::default();
    let mut a = terminal("t1", "idle");
    let mut b = terminal("t2", "working");
    a.turn = 500;
    let first = update(&mut p, vec![a.clone(), b.clone()], 1);
    let seq = first
        .snapshot
        .agents
        .iter()
        .find(|agent| agent.pane_id == "t2")
        .unwrap()
        .state_change_seq;
    a.turn += 1; // A working-to-idle transition occurred between snapshots.
    let next = update(&mut p, vec![a.clone(), b.clone()], 2);
    assert_eq!(state(&next, "t1"), AgentStatus::Done);
    assert!(
        next.snapshot
            .agents
            .iter()
            .find(|agent| agent.pane_id == "t1")
            .unwrap()
            .state_change_seq
            > seq
    );
    // Merely editing agent metadata is not a fresh state transition.
    b.status.as_mut().unwrap().title = "another task".into();
    let next = update(&mut p, vec![a, b], 3);
    assert_eq!(
        next.snapshot
            .agents
            .iter()
            .find(|agent| agent.pane_id == "t2")
            .unwrap()
            .state_change_seq,
        seq
    );
}

#[test]
fn acknowledgement_only_covers_the_displayed_pane() {
    let mut p = Presentation::default();
    let mut a = terminal("t1", "working");
    let mut b = terminal("t2", "working");
    update(&mut p, vec![a.clone(), b.clone()], 1);
    a.status.as_mut().unwrap().state = "idle".into();
    b.status.as_mut().unwrap().state = "idle".into();
    let mut projection = update(&mut p, vec![a, b], 2);
    p.acknowledge(&mut projection, "t1", None);
    assert_eq!(state(&projection, "t1"), AgentStatus::Idle);
    assert_eq!(state(&projection, "t2"), AgentStatus::Done);
}
