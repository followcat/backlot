//! Held people in reconcile (R19): a closed held role is desired
//! with its session as the resume; a released one leaves the set.

use std::collections::BTreeMap;

use backlot_model::event::*;
use backlot_model::state::*;
use backlot_model::testkit::*;

use super::*;

const STUDIO: &str = "acme-shop";
/// The clock these tests hand the resume guard.
const NOW: u64 = 0;

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
        ev(3, "director", task_created("t1", "feature", 1)),
    ]
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

fn reopen(seq: u64, role: &str) -> Event {
    ev(seq, "user", EventKind::RoleReopen { role: role.into() })
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

/// A closed person on a finished task comes back desired with
/// `resume` set to the session it closed with.
#[test]
fn a_held_closed_role_is_desired_with_its_session() {
    let mut events = studio();
    events.push(ev(4, "director", cast("t1", "actor-a", "a")));
    events.push(started(5, "actor-t1-c1", "t7", "sess-1"));
    events.push(ev(
        6,
        "director",
        EventKind::TaskState {
            task: "t1".into(),
            state: TaskStateChange::Abandoned,
            reason: "cut".into(),
        },
    ));
    events.push(closed(7, "actor-t1-c1"));
    events.push(reopen(8, "actor-t1-c1"));
    let actions = reconcile(STUDIO, &log(events), &live(&[]), NOW);
    let start = starts(&actions)
        .into_iter()
        .find(|s| s.role == "actor-t1-c1")
        .expect("held actor desired");
    assert_eq!(
        start
            .resume
            .as_ref()
            .map(|r| (r.session.as_str(), r.reason)),
        Some(("sess-1", REOPENED))
    );
    assert_eq!(start.terminal, "bl-acme-shop-t1");
    assert_eq!(start.kind, RoleKind::Actor);
    assert_eq!(start.task.as_deref(), Some("t1"));
    assert_eq!(start.actor, actor("a"));
}

/// A held role already desired through its task stays one entry —
/// the resume is set on it.
#[test]
fn a_desired_held_role_is_one_entry() {
    let mut events = studio();
    events.push(ev(4, "director", cast("t1", "actor-a", "a")));
    events.push(started(5, "actor-t1-c1", "t7", "sess-1"));
    events.push(closed(6, "actor-t1-c1"));
    events.push(reopen(7, "actor-t1-c1"));
    let actions = reconcile(STUDIO, &log(events), &live(&[]), NOW);
    let found: Vec<&Start> = starts(&actions)
        .into_iter()
        .filter(|s| s.role == "actor-t1-c1")
        .collect();
    assert_eq!(found.len(), 1, "{actions:?}");
    assert_eq!(
        found[0]
            .resume
            .as_ref()
            .map(|r| (r.session.as_str(), r.reason)),
        Some(("sess-1", REOPENED))
    );
}

/// A held role with no recorded actor has nothing to launch.
#[test]
fn a_held_role_without_an_actor_is_skipped() {
    let mut events = studio();
    events.push(ev(
        4,
        "stage",
        EventKind::RoleStarted {
            role: "actor-t9-c1".into(),
            terminal: Some("t8".into()),
            actor: None,
            session_id: Some("sess-9".into()),
            reason: None,
        },
    ));
    events.push(closed(5, "actor-t9-c1"));
    events.push(reopen(6, "actor-t9-c1"));
    let actions = reconcile(STUDIO, &log(events), &live(&[]), NOW);
    assert!(
        !starts(&actions).iter().any(|s| s.role == "actor-t9-c1"),
        "{actions:?}"
    );
}

/// The hold keeps a live reopened role desired; `role.release` drops
/// it back onto the usual Stop path.
#[test]
fn release_returns_a_live_role_to_the_stop_path() {
    let mut events = studio();
    events.push(ev(4, "director", cast("t1", "actor-a", "a")));
    events.push(started(5, "actor-t1-c1", "t7", "sess-1"));
    events.push(ev(
        6,
        "director",
        EventKind::TaskState {
            task: "t1".into(),
            state: TaskStateChange::Abandoned,
            reason: "cut".into(),
        },
    ));
    events.push(closed(7, "actor-t1-c1"));
    events.push(reopen(8, "actor-t1-c1"));
    events.push(ev(
        9,
        "stage",
        EventKind::RoleResumed {
            role: "actor-t1-c1".into(),
            terminal: Some("t7".into()),
            actor: Some(actor("a")),
            session_id: Some("sess-1".into()),
            reason: Some("reopened".into()),
        },
    ));
    // Live and held: desired through the hold — nothing for it.
    let view = live(&[("bl-acme-shop-t1", "t7")]);
    let actions = reconcile(STUDIO, &log(events.clone()), &view, NOW);
    assert!(
        !actions.iter().any(|a| matches!(a,
            Action::Stop { role, .. } | Action::Lost { role, .. } | Action::Start(Start { role, .. })
            if role == "actor-t1-c1")),
        "a live held role is wanted: {actions:?}"
    );

    // Released: no longer desired, the Stop path records the close.
    events.push(ev(
        10,
        "user",
        EventKind::RoleRelease {
            role: "actor-t1-c1".into(),
        },
    ));
    let actions = reconcile(STUDIO, &log(events), &view, NOW);
    let stops: Vec<(&str, &str, bool)> = actions
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
        .collect();
    assert_eq!(stops, [("actor-t1-c1", "bl-acme-shop-t1", true)]);
}

/// A finished studio wants nobody on its own: producer, director
/// and every task actor leave the desired set, while an explicit
/// reopen still brings its person back with the recorded session.
#[test]
fn finished_desires_nobody_but_a_held_reopen() {
    let mut events = studio();
    events.push(ev(4, "director", cast("t1", "actor-a", "a")));
    events.push(started(5, "actor-t1-c1", "t7", "sess-1"));
    events.push(ev(
        6,
        "director",
        reviewed("t1", 1, Verdict::Accepted, vec![]),
    ));
    events.push(closed(7, "actor-t1-c1"));
    events.push(ev(
        8,
        "producer",
        EventKind::StudioFinished {
            reason: "accepted".into(),
        },
    ));

    // Nobody desired — the live producer heads to Stop like any
    // unwanted role.
    let view = live(&[("bl-acme-shop-producer", "t1"), ("bl-acme-shop-t1", "t7")]);
    let actions = reconcile(STUDIO, &log(events.clone()), &view, NOW);
    assert!(starts(&actions).is_empty(), "{actions:?}");
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::DropTerminal { terminal, .. } if terminal == "bl-acme-shop-producer")),
        "{actions:?}"
    );

    // An explicit reopen still brings the person back, session set.
    events.push(reopen(9, "actor-t1-c1"));
    let actions = reconcile(STUDIO, &log(events), &view, NOW);
    let start = starts(&actions)
        .into_iter()
        .find(|s| s.role == "actor-t1-c1")
        .expect("held actor desired after finish");
    assert_eq!(
        start
            .resume
            .as_ref()
            .map(|r| (r.session.as_str(), r.reason)),
        Some(("sess-1", REOPENED))
    );
    assert_eq!(start.terminal, "bl-acme-shop-t1");
}
