//! `wanted_labels`: which live terminals the relabel pass touches —
//! a role of the studio still on the old scheme, and nothing else.

use std::collections::BTreeMap;

use backlot_model::event::EventKind;
use backlot_model::state::StudioState;
use backlot_model::testkit::*;

use super::wanted_labels;
use crate::reconcile::LiveTerminal;

/// A live terminal named `bl-acme-<name>` with the given labels.
fn term(name: &str, labels: &[(&str, &str)]) -> LiveTerminal {
    LiveTerminal {
        name: format!("bl-acme-{name}"),
        address: format!("t-{name}"),
        labels: labels
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        ..Default::default()
    }
}

/// A `role.started` that recorded its terminal's address.
fn started_on(role: &str, address: &str) -> EventKind {
    EventKind::RoleStarted {
        role: role.into(),
        terminal: Some(address.into()),
        actor: None,
        session_id: None,
        reason: None,
    }
}

/// A studio whose director is recorded live on `t-director`.
fn state() -> StudioState {
    state_with(
        &base_events(),
        vec![("stage", started_on("director", "t-director"))],
    )
}

/// The same studio with t2 created, cast to actor-b and its
/// `actor-t2-c1` role recorded live on `t-t2`.
fn actor_state() -> StudioState {
    let mut evs = base_events();
    evs.push(ev(4, "director", task_created("t2", "fix", 1)));
    state_with(
        &evs,
        vec![
            ("director", cast("t2", "actor-b", "b")),
            ("stage", started_on("actor-t2-c1", "t-t2")),
        ],
    )
}

#[test]
fn an_old_scheme_role_gets_its_studios_space_and_tab() {
    let term = term(
        "director",
        &[
            ("backlot.studio", "acme"),
            ("backlot.role", "director"),
            ("herdr.space", "backlot"),
            ("herdr.tab", "bl-acme-director"),
        ],
    );
    let patch = wanted_labels("acme", &state(), &term).unwrap();
    assert_eq!(
        patch,
        BTreeMap::from([
            ("herdr.space".to_string(), "acme".to_string()),
            ("herdr.tab".to_string(), "director".to_string()),
        ])
    );
}

#[test]
fn an_actor_tab_carries_the_candidate_name_and_task() {
    let term = term(
        "t2",
        &[
            ("backlot.studio", "acme"),
            ("backlot.role", "actor-t2-c1"),
            ("backlot.task", "t2"),
            ("herdr.space", "backlot"),
            ("herdr.tab", "bl-acme-t2"),
        ],
    );
    let patch = wanted_labels("acme", &actor_state(), &term).unwrap();
    assert_eq!(patch["herdr.space"], "acme");
    assert_eq!(patch["herdr.tab"], "actor-b t2");
}

#[test]
fn an_already_right_terminal_needs_nothing() {
    let term = term(
        "director",
        &[
            ("backlot.studio", "acme"),
            ("backlot.role", "director"),
            ("herdr.space", "acme"),
            ("herdr.tab", "director"),
        ],
    );
    assert_eq!(wanted_labels("acme", &state(), &term), None);
}

#[test]
fn a_shell_is_never_relabelled() {
    let term = term(
        "sh1",
        &[
            ("backlot.studio", "acme"),
            ("backlot.shell", "1"),
            ("herdr.space", "backlot"),
        ],
    );
    assert_eq!(wanted_labels("acme", &state(), &term), None);
}

#[test]
fn foreign_terminals_are_never_relabelled() {
    // A role of another studio — same shape, different id.
    let other = LiveTerminal {
        name: "bl-other-director".into(),
        address: "t-x".into(),
        labels: BTreeMap::from([
            ("backlot.studio".to_string(), "other".to_string()),
            ("backlot.role".to_string(), "director".to_string()),
        ]),
        ..Default::default()
    };
    assert_eq!(wanted_labels("acme", &state(), &other), None);
    // A `backlot.role` the ledger never recorded — the stage's own.
    let stage = term(
        "stage",
        &[("backlot.studio", "acme"), ("backlot.role", "stage")],
    );
    assert_eq!(wanted_labels("acme", &state(), &stage), None);
    // A hand-labelled fake: claims the role, names the wrong terminal.
    let fake = LiveTerminal {
        name: "mine".into(),
        address: "t-mine".into(),
        labels: BTreeMap::from([
            ("backlot.studio".to_string(), "acme".to_string()),
            ("backlot.role".to_string(), "director".to_string()),
        ]),
        ..Default::default()
    };
    assert_eq!(wanted_labels("acme", &state(), &fake), None);
}
