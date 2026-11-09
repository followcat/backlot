//! Pure reconcile tests: desired roles against live terminal names.

use std::collections::BTreeMap;

use backlot_model::event::*;
use backlot_model::state::*;
use backlot_model::testkit::*;

use super::*;

const STUDIO: &str = "acme-shop";

fn log(events: Vec<Event>) -> StudioState {
    replay(&events)
}

fn studio() -> Vec<Event> {
    vec![
        ev(1, "user", studio_kind()),
        ev(
            2,
            "producer",
            EventKind::ReqVersion {
                version: 1,
                file: "req/v1.md".into(),
                summary: "s".into(),
            },
        ),
    ]
}

fn plan_request(seq: u64) -> Event {
    ev(
        seq,
        "producer",
        EventKind::MessageQueued {
            to: "writer".into(),
            file: None,
            purpose: Some("plan".into()),
        },
    )
}

fn dispatch(seq: u64) -> Event {
    ev(
        seq,
        "producer",
        EventKind::MessageQueued {
            to: "director".into(),
            file: Some("messages/d.md".into()),
            purpose: Some("dispatch".into()),
        },
    )
}

fn started(seq: u64, role: &str, term: &str, sess: &str) -> Event {
    ev(
        seq,
        "stage",
        EventKind::RoleStarted {
            role: role.into(),
            terminal: Some(term.into()),
            actor: Some(actor("a")),
            session_id: Some(sess.into()),
            reason: None,
        },
    )
}

fn closed(seq: u64, role: &str) -> Event {
    ev(
        seq,
        "stage",
        EventKind::RoleClosed {
            role: role.into(),
            reason: Some("no longer desired".into()),
        },
    )
}

fn lost(seq: u64, role: &str) -> Event {
    ev(
        seq,
        "stage",
        EventKind::RoleLost {
            role: role.into(),
            reason: None,
        },
    )
}

fn live(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(n, a)| (n.to_string(), a.to_string()))
        .collect()
}

fn starts(actions: &[Action]) -> Vec<&Start> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::Start(s) => Some(s),
            _ => None,
        })
        .collect()
}

fn stops(actions: &[Action]) -> Vec<(&str, &str, bool)> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::Stop {
                role,
                terminal,
                close,
                ..
            } => Some((role.as_str(), terminal.as_str(), *close)),
            _ => None,
        })
        .collect()
}

/// A fresh studio wants producer and director, nothing else.
#[test]
fn a_new_studio_wants_producer_and_director() {
    let actions = reconcile(STUDIO, &log(studio()), &live(&[]));
    let starts = starts(&actions);
    let names: Vec<&str> = starts.iter().map(|s| s.terminal.as_str()).collect();
    assert_eq!(starts.len(), 2, "{names:?}");
    assert!(names.contains(&"bl-acme-shop-producer"), "{names:?}");
    assert!(names.contains(&"bl-acme-shop-director"), "{names:?}");
    assert_eq!(actions.len(), 2);
}

/// `plan request` wants the writer; `dispatch` releases it; a later
/// request brings it back as a new start, even after a close.
#[test]
fn the_writer_is_wanted_between_plan_and_dispatch() {
    let mut events = studio();
    events.push(plan_request(3));
    let actions = reconcile(STUDIO, &log(events.clone()), &live(&[]));
    assert!(
        starts(&actions).iter().any(|s| s.role == "writer"),
        "{actions:?}"
    );

    // The writer ran; the window is still open — nothing for it.
    events.push(started(4, "writer", "t3", "fake-bl-acme-shop-writer"));
    let actions = reconcile(
        STUDIO,
        &log(events.clone()),
        &live(&[("bl-acme-shop-writer", "t3")]),
    );
    assert!(
        !actions.iter().any(|a| matches!(a,
            Action::Start(Start { role, .. }) | Action::Stop { role, .. } if role == "writer")),
        "{actions:?}"
    );

    // Dispatch closes the window: the live writer is stopped, and the
    // stop owes a role.closed first.
    events.push(dispatch(5));
    let actions = reconcile(
        STUDIO,
        &log(events.clone()),
        &live(&[("bl-acme-shop-writer", "t3")]),
    );
    assert_eq!(stops(&actions), [("writer", "bl-acme-shop-writer", true)]);

    // Closed and gone: the writer is not relaunched while undesired.
    events.push(closed(6, "writer"));
    let state = log(events.clone());
    let actions = reconcile(STUDIO, &state, &live(&[]));
    assert!(
        !actions.iter().any(|a| matches!(a,
            Action::Start(Start { role, .. })
            | Action::Stop { role, .. }
            | Action::Lost { role, .. }
            | Action::Adopt { role, .. } if role == "writer")),
        "closed writer must not restart while undesired: {actions:?}"
    );

    // A new plan request starts it fresh.
    events.push(plan_request(7));
    let actions = reconcile(STUDIO, &log(events), &live(&[]));
    assert!(
        starts(&actions).iter().any(|s| s.role == "writer"),
        "a closed writer comes back on the next plan request: {actions:?}"
    );
}

/// A live role whose terminal vanished is `role.lost`, never a stop —
/// and a lost role is not relaunched.
#[test]
fn a_gone_terminal_records_lost_once_and_never_relaunches() {
    let mut events = studio();
    events.push(started(2, "producer", "t9", "s"));
    let actions = reconcile(STUDIO, &log(events.clone()), &live(&[]));
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::Lost { role, .. } if role == "producer")),
        "{actions:?}"
    );
    events.push(lost(3, "producer"));
    let actions = reconcile(STUDIO, &log(events), &live(&[]));
    assert!(
        !actions.iter().any(|a| matches!(a,
            Action::Lost { role, .. } | Action::Start(Start { role, .. }) if role == "producer")),
        "lost roles are not relaunched: {actions:?}"
    );
}

/// A deliberate stop never reads as lost, and a crashed stop — role
/// closed, terminal still up — finishes without a second close.
#[test]
fn a_deliberate_stop_is_never_a_loss() {
    let mut events = studio();
    events.extend([plan_request(3), dispatch(4)]);
    events.push(started(5, "writer", "t3", "s"));
    events.push(closed(6, "writer"));
    let state = log(events);
    // Terminal still up: finish the stop, no role.closed again.
    let actions = reconcile(STUDIO, &state, &live(&[("bl-acme-shop-writer", "t3")]));
    assert_eq!(
        stops(&actions),
        [("writer", "bl-acme-shop-writer", false)],
        "{actions:?}"
    );
    assert!(
        !actions.iter().any(|a| matches!(a, Action::Lost { .. })),
        "{actions:?}"
    );
    // Terminal already gone: nothing at all for the writer.
    let actions = reconcile(STUDIO, &state, &live(&[]));
    assert!(
        !actions.iter().any(|a| matches!(a,
            Action::Lost { role, .. } | Action::Stop { role, .. } if role == "writer")),
        "{actions:?}"
    );
}

/// A cast task wants its actor under `bl-<studio>-<task>`; a closed
/// (accepted) task stops it with a role.closed first.
#[test]
fn closing_a_task_stops_its_actor() {
    let mut events = studio();
    events.push(dispatch(3));
    events.push(ev(4, "director", task_created("t1", "feature", 1)));
    events.push(ev(5, "director", cast("t1", "actor-a", "a")));
    let actions = reconcile(STUDIO, &log(events.clone()), &live(&[]));
    let start = starts(&actions)
        .into_iter()
        .find(|s| s.role == "actor-t1-c1")
        .expect("actor wanted");
    assert_eq!(start.terminal, "bl-acme-shop-t1");
    assert_eq!(start.kind, RoleKind::Actor);
    assert_eq!(start.task.as_deref(), Some("t1"));

    // Delivered tasks keep their actor; accepted ones release it.
    events.push(started(6, "actor-t1-c1", "t7", "fake-bl-acme-shop-t1"));
    events.push(ev(7, "actor", delivered("t1", 1, "a", 1)));
    let actions = reconcile(
        STUDIO,
        &log(events.clone()),
        &live(&[("bl-acme-shop-t1", "t7")]),
    );
    assert!(
        !actions.iter().any(|a| matches!(a,
            Action::Start(Start { role, .. }) | Action::Stop { role, .. } if role == "actor-t1-c1")),
        "a delivered task keeps its actor: {actions:?}"
    );
    events.push(ev(
        8,
        "director",
        reviewed("t1", 1, Verdict::Accepted, vec![]),
    ));
    let actions = reconcile(STUDIO, &log(events), &live(&[("bl-acme-shop-t1", "t7")]));
    assert_eq!(
        stops(&actions),
        [("actor-t1-c1", "bl-acme-shop-t1", true)],
        "{actions:?}"
    );
}

/// A recast closes the earlier person under the same terminal name,
/// then starts the new cast as `actor-<task>-c2`. The c1 ledger entry
/// is kept — never overwritten.
#[test]
fn a_recast_keeps_the_earlier_person() {
    let mut events = studio();
    events.push(dispatch(3));
    events.push(ev(4, "director", task_created("t1", "feature", 1)));
    events.push(ev(5, "director", cast("t1", "actor-a", "a")));
    events.push(started(6, "actor-t1-c1", "t7", "sess-1"));
    events.push(ev(7, "director", cast("t1", "actor-b", "b")));
    let state = log(events);
    let actions = reconcile(STUDIO, &state, &live(&[("bl-acme-shop-t1", "t7")]));
    // The stop precedes the start so the name is free.
    let on_t1: Vec<&'static str> = actions
        .iter()
        .filter_map(|a| match a {
            Action::Stop { terminal, .. } if terminal == "bl-acme-shop-t1" => Some("stop"),
            Action::Start(Start { terminal, .. }) if terminal == "bl-acme-shop-t1" => Some("start"),
            _ => None,
        })
        .collect();
    assert_eq!(on_t1, ["stop", "start"], "{actions:?}");
    assert_eq!(stops(&actions), [("actor-t1-c1", "bl-acme-shop-t1", true)]);
    let start = starts(&actions)
        .into_iter()
        .find(|s| s.terminal == "bl-acme-shop-t1")
        .unwrap();
    assert_eq!(start.role, "actor-t1-c2");
    assert_eq!(start.actor, actor("b"));
    assert!(state.roles.contains_key("actor-t1-c1"));
}

/// After a recast the closed earlier cast must not stop the new
/// person's terminal under the shared name: only the address the
/// close recorded is finished. Sibling roles stay live.
#[test]
fn a_closed_role_finishes_only_its_own_terminal() {
    let mut events = studio();
    events.push(dispatch(3));
    events.push(ev(4, "director", task_created("t1", "feature", 1)));
    events.push(ev(5, "director", cast("t1", "actor-a", "a")));
    events.push(started(6, "actor-t1-c1", "t5", "sess-1"));
    events.push(started(7, "producer", "t1x", "sess-p"));
    events.push(started(8, "director", "t2x", "sess-d"));
    events.push(ev(9, "director", cast("t1", "actor-b", "b")));
    events.push(closed(10, "actor-t1-c1"));
    events.push(started(11, "actor-t1-c2", "t6", "sess-2"));
    let state = log(events);
    // c1 closed at t5; the shared name now holds c2's t6: no actions.
    let view = live(&[
        ("bl-acme-shop-producer", "t1x"),
        ("bl-acme-shop-director", "t2x"),
        ("bl-acme-shop-t1", "t6"),
    ]);
    let actions = reconcile(STUDIO, &state, &view);
    assert_eq!(actions, Vec::<Action>::new(), "{actions:?}");
    // At c1's own recorded address the crashed stop still finishes.
    let view = live(&[
        ("bl-acme-shop-producer", "t1x"),
        ("bl-acme-shop-director", "t2x"),
        ("bl-acme-shop-t1", "t5"),
    ]);
    let actions = reconcile(STUDIO, &state, &view);
    assert_eq!(
        stops(&actions),
        [("actor-t1-c1", "bl-acme-shop-t1", false)],
        "{actions:?}"
    );
}

/// A terminal the ledger never recorded is adopted for a desired
/// role — the stage died between run and record.
#[test]
fn an_unrecorded_desired_terminal_is_adopted() {
    let actions = reconcile(
        STUDIO,
        &log(studio()),
        &live(&[("bl-acme-shop-producer", "t2")]),
    );
    assert!(
        actions.iter().any(|a| matches!(a, Action::Adopt {
            role,
            terminal,
            address,
        } if role == "producer" && terminal == "bl-acme-shop-producer" && address == "t2")),
        "{actions:?}"
    );
}

/// Ownership is the `backlot.studio` label, never the name: each
/// studio's view holds only its own terminals — not a hand-named
/// lookalike, not an unlabelled one, not another studio's.
#[test]
fn the_label_decides_the_studio_view() {
    let term = |name: &str, address: &str, studio: Option<&str>| LiveTerminal {
        name: name.into(),
        address: address.into(),
        labels: studio
            .map(|s| BTreeMap::from([("backlot.studio".to_string(), s.to_string())]))
            .unwrap_or_default(),
    };
    let terms = vec![
        term("bl-acme-producer", "t1", Some("acme")),
        term("bl-acme-shop-producer", "t2", Some("acme-shop")),
        // A matching name with no label is nobody's.
        term("bl-acme-x", "t3", None),
        // A matching name with another studio's label is theirs.
        term("bl-acme-y", "t4", Some("acme-shop")),
    ];
    assert_eq!(
        studio_view("acme", &terms),
        live(&[("bl-acme-producer", "t1")])
    );
    assert_eq!(
        studio_view("acme-shop", &terms),
        live(&[("bl-acme-shop-producer", "t2"), ("bl-acme-y", "t4")])
    );
}

/// Every terminal in the studio's view that no role entry explains
/// is a stray and is dropped — the label already said it is ours.
#[test]
fn stray_terminals_in_the_view_are_dropped() {
    let actions = reconcile(
        STUDIO,
        &log(studio()),
        &live(&[("bl-acme-shop-ghost", "t9"), ("bl-acme-shop-old", "t8")]),
    );
    let dropped: Vec<(&str, &str)> = actions
        .iter()
        .filter_map(|a| match a {
            Action::DropTerminal { terminal, address } => {
                Some((terminal.as_str(), address.as_str()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        dropped,
        [("bl-acme-shop-ghost", "t9"), ("bl-acme-shop-old", "t8")],
        "{actions:?}"
    );
}
