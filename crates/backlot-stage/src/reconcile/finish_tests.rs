//! The finish sweep: `studio.finished` releases every role, and the
//! stop path kills its terminal — but a `terminal.rm` the core refused
//! while the kill was still landing leaves a `finished` record behind,
//! and every client keeps drawing its dead pane until someone closes
//! it by hand. The record is the stage's to remove. Only records
//! carrying this studio's `backlot.studio` label move; a shell is the
//! user's own, and a record already gone is clean.

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

/// Studio, one cast task, its actor started and closed, then finished
/// — the state a `studio finish` leaves behind.
fn finished_log() -> Vec<Event> {
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
        ev(4, "director", cast("t1", "actor-a", "a")),
        ev(
            5,
            "stage",
            EventKind::RoleStarted {
                role: "actor-t1-c1".into(),
                terminal: Some("t7".into()),
                actor: Some(actor("a")),
                session_id: Some("sess-1".into()),
                reason: None,
            },
        ),
        ev(
            6,
            "director",
            task_state("t1", TaskStateChange::Abandoned, "cut"),
        ),
        ev(
            7,
            "stage",
            EventKind::RoleClosed {
                role: "actor-t1-c1".into(),
                reason: Some("no longer desired".into()),
            },
        ),
        ev(
            8,
            "producer",
            EventKind::StudioFinished {
                reason: "accepted".into(),
            },
        ),
    ]
}

/// A `finished` record as the inventory reports it: no live program,
/// the labels it launched with.
fn dead(name: &str, address: &str, labels: &[(&str, &str)]) -> LiveTerminal {
    LiveTerminal {
        name: name.into(),
        address: address.into(),
        labels: labels
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<_, _>>(),
        ..Default::default()
    }
}

fn removed(actions: &[Action]) -> Vec<(&str, &str)> {
    actions
        .iter()
        .filter_map(|a| match a {
            Action::RemoveFinished { terminal, address } => {
                Some((terminal.as_str(), address.as_str()))
            }
            _ => None,
        })
        .collect()
}

/// A finished studio's leftover records are removed: the actor's tab
/// and pane leave every client's layout without a click.
#[test]
fn a_finished_studio_removes_its_own_records() {
    let state = log(finished_log());
    let dead = [
        dead(
            "bl-acme-shop-t1",
            "t7",
            &[
                ("backlot.studio", STUDIO),
                ("backlot.role", "actor-t1-c1"),
                ("herdr.tab", "actor-a t1"),
            ],
        ),
        dead(
            "bl-acme-shop-producer",
            "t2",
            &[("backlot.studio", STUDIO), ("backlot.role", "producer")],
        ),
    ];
    let actions = finish_sweep(STUDIO, &state, &dead);
    assert_eq!(
        removed(&actions),
        [("bl-acme-shop-t1", "t7"), ("bl-acme-shop-producer", "t2")]
    );
}

/// Nothing is swept before `studio.finished`: a live studio's people
/// are the ordinary role pass's to stop, and its closed ones may be
/// reopened from their recorded session.
#[test]
fn an_unfinished_studio_sweeps_nothing() {
    let mut events = finished_log();
    events.pop();
    let state = log(events);
    let dead = [dead(
        "bl-acme-shop-t1",
        "t7",
        &[("backlot.studio", STUDIO), ("backlot.role", "actor-t1-c1")],
    )];
    assert!(finish_sweep(STUDIO, &state, &dead).is_empty());
}

/// Ownership is the label, never the name: another studio's record,
/// a stranger's, and the studio's own shell are all untouched.
#[test]
fn another_studios_and_the_users_own_records_stay() {
    let state = log(finished_log());
    let dead = [
        dead(
            "bl-acme-shop-t1",
            "t7",
            &[
                ("backlot.studio", "other-shop"),
                ("backlot.role", "actor-t1-c1"),
            ],
        ),
        dead("bl-acme-shop-t2", "t8", &[]),
        dead(
            "bl-acme-shop-sh1",
            "t9",
            &[("backlot.studio", STUDIO), ("backlot.shell", "1")],
        ),
        dead("vim", "t10", &[("herdr.tab", "notes")]),
    ];
    assert!(finish_sweep(STUDIO, &state, &dead).is_empty());
}

/// The cleanup removes the record, not the studio: the panel still
/// lists it as finished, and a person reopened from it comes back
/// into the recorded session — the reopen then re-creates whatever
/// terminal it needs.
#[test]
fn the_studio_stays_listed_and_still_reopens() {
    let state = log(finished_log());
    let body = backlot_model::summary::summary(
        &[(STUDIO.to_string(), state.clone())],
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        &BTreeMap::new(),
        0,
    );
    assert_eq!(body.studios[0].state, "finished");
    assert_eq!(body.studios[0].id, STUDIO);

    let mut events = finished_log();
    events.push(ev(
        9,
        "user",
        EventKind::RoleReopen {
            role: "actor-t1-c1".into(),
        },
    ));
    let state = log(events);
    let actions = reconcile(STUDIO, &state, &BTreeMap::new(), NOW);
    let start = actions
        .iter()
        .find_map(|a| match a {
            Action::Start(s) if s.role == "actor-t1-c1" => Some(s),
            _ => None,
        })
        .expect("a reopened person is desired");
    assert_eq!(
        start
            .resume
            .as_ref()
            .map(|r| (r.session.as_str(), r.reason)),
        Some(("sess-1", REOPENED))
    );

    // The old record still goes: the reopen launches a fresh terminal
    // under the same name.
    let dead = [dead(
        "bl-acme-shop-t1",
        "t7",
        &[("backlot.studio", STUDIO), ("backlot.role", "actor-t1-c1")],
    )];
    assert_eq!(
        removed(&finish_sweep(STUDIO, &state, &dead)),
        [("bl-acme-shop-t1", "t7")]
    );
}
