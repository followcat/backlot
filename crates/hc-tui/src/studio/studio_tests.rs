//! The studio latch: mode on/off rules, the current studio following
//! the focused terminal, remembered people, cast order and the
//! `!n`/working counts. Neutral fixtures only.
use super::*;
use crate::panel::data::MachineSummary;
use backlot_model::summary::Summary;

/// A summary wrapped as this core's machine row.
fn local(summary: &Summary) -> MachineSummary {
    MachineSummary {
        machine: None,
        summary: summary.clone(),
    }
}

/// A summary wrapped as `host-b`'s row — its role terminals arrive
/// qualified, the way `panel::data`'s parse stores them.
fn remote(summary: &Summary) -> MachineSummary {
    let mut summary = summary.clone();
    for role in summary
        .studios
        .iter_mut()
        .flat_map(|studio| studio.roles.iter_mut())
    {
        if let Some(terminal) = role.terminal.take() {
            role.terminal = Some(format!("host-b/{terminal}"));
        }
    }
    MachineSummary {
        machine: Some("host-b".into()),
        summary,
    }
}

/// The `host-b` key spelling.
fn on_host_b(id: &str) -> StudioKey {
    StudioKey {
        machine: Some("host-b".into()),
        id: id.into(),
    }
}

fn two_studio_json() -> serde_json::Value {
    serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "studios": [
            {
                "id": "acme-shop", "title": "Acme shop", "state": "live",
            "requirement_version": 1, "mode": "standard",
            "open_reviews": 0, "waiting": 0,
                "roles": [
                    { "role": "producer", "name": "producer", "terminal": "t1",
                      "state": "live", "work": "working" },
                    { "role": "actor-alice", "name": "alice", "terminal": "t4",
                      "state": "live", "work": "working", "task": "task-1",
                      "attention": "needs permission" },
                    { "role": "actor-dave", "name": "dave",
                      "state": "closed", "resumable": true }
                ]
            },
            {
                "id": "night-owl", "title": "Night owl", "state": "live",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0,
                "roles": [
                    { "role": "producer", "name": "owl-producer", "terminal": "t8",
                      "state": "live", "work": "waiting" },
                    { "role": "actor-bob", "name": "bob", "terminal": "t9",
                      "state": "live", "work": "blocked", "task": "task-2" }
                ]
            }
        ]
    })
}

fn summary() -> Summary {
    serde_json::from_value(two_studio_json()).unwrap()
}

#[test]
fn a_document_with_studios_makes_the_first_current() {
    let machines = [local(&summary())];
    let mut studio = Studio::default();
    studio.sync(&machines, None);
    assert_eq!(studio.current, Some(StudioKey::local("acme-shop")));
    assert_eq!(studio.current_index(&machines), Some(0));
}

#[test]
fn a_missing_or_bad_document_never_flips_the_latch() {
    let summary = summary();
    // `active` is `sync_studio`'s own latch (`!plain`) — documents
    // never touch it, good or bad.
    let mut studio = Studio {
        active: true,
        ..Studio::default()
    };
    let machines = [local(&summary)];
    studio.sync(&machines, Some("t1"));
    // An empty aggregate — every machine's read missing, malformed or
    // foreign — keeps every latch exactly as it was.
    studio.sync(&[], Some("t9"));
    assert!(studio.active);
    assert_eq!(studio.current, Some(StudioKey::local("acme-shop")));
}

#[test]
fn a_good_document_with_zero_studios_keeps_the_mode() {
    // U2: unadorned `backlot` is always in studio mode — the cast's
    // `others` row is the no-studio view, not the agents list.
    let mut studio = Studio {
        active: true,
        ..Studio::default()
    };
    studio.sync(&[local(&summary())], Some("t1"));
    let empty: Summary = serde_json::from_value(serde_json::json!({
        "schema": "backlot/1", "updated_ms": 2, "studios": []
    }))
    .unwrap();
    studio.sync(&[local(&empty)], Some("t1"));
    assert!(studio.active);
    assert_eq!(studio.current_index(&[local(&empty)]), None);
}

#[test]
fn the_focused_terminal_moves_current_and_remembers_the_person() {
    let machines = [local(&summary())];
    let mut studio = Studio::default();
    studio.sync(&machines, Some("t9"));
    assert_eq!(studio.current, Some(StudioKey::local("night-owl")));
    assert_eq!(
        studio
            .remembered
            .get(&StudioKey::local("night-owl"))
            .map(String::as_str),
        Some("actor-bob")
    );
    // A terminal no person owns keeps the current studio.
    studio.sync(&machines, Some("t99"));
    assert_eq!(studio.current, Some(StudioKey::local("night-owl")));
}

#[test]
fn a_vanished_current_falls_back_to_document_order() {
    let mut studio = Studio::default();
    studio.sync(&[local(&summary())], Some("t9"));
    assert_eq!(studio.current, Some(StudioKey::local("night-owl")));
    let mut one = two_studio_json();
    one["studios"].as_array_mut().unwrap().remove(1);
    let one: Summary = serde_json::from_value(one).unwrap();
    studio.sync(&[local(&one)], None);
    assert_eq!(studio.current, Some(StudioKey::local("acme-shop")));
}

#[test]
fn the_same_id_on_two_machines_is_two_studios() {
    let machines = [local(&summary()), remote(&summary())];
    let studios = crate::panel::data::studios(&machines);
    // Both machines list acme-shop and night-owl: four studios.
    assert_eq!(studios.len(), 4);
    let keys: Vec<StudioKey> = studios.iter().map(|s| s.key()).collect();
    assert!(keys.contains(&StudioKey::local("acme-shop")));
    assert!(keys.contains(&on_host_b("acme-shop")));
    // The remote key never reads as the local studio.
    assert_ne!(on_host_b("acme-shop"), "acme-shop");
    assert_eq!(StudioKey::local("acme-shop"), "acme-shop");
}

#[test]
fn a_publish_that_reorders_machines_keeps_the_current_studio() {
    let mut studio = Studio::default();
    studio.sync(&[local(&summary()), remote(&summary())], Some("host-b/t9"));
    assert_eq!(studio.current, Some(on_host_b("night-owl")));
    // The publish comes back with the machines swapped: the key, not
    // the position, holds the selection.
    let swapped = [remote(&summary()), local(&summary())];
    studio.sync(&swapped, None);
    assert_eq!(studio.current, Some(on_host_b("night-owl")));
    assert_eq!(studio.current_index(&swapped), Some(1));
    // The remembered person is scoped to the machine's studio too.
    assert_eq!(
        studio
            .remembered
            .get(&on_host_b("night-owl"))
            .map(String::as_str),
        Some("actor-bob")
    );
    assert_eq!(studio.remembered.get(&StudioKey::local("night-owl")), None);
}

#[test]
fn a_remote_persons_terminal_focuses_its_own_studio() {
    let machines = [local(&summary()), remote(&summary())];
    let mut studio = Studio::default();
    // `t4` is local acme-shop's alice; `host-b/t4` is host-b's.
    studio.sync(&machines, Some("host-b/t4"));
    assert_eq!(studio.current, Some(on_host_b("acme-shop")));
    studio.sync(&machines, Some("t4"));
    assert_eq!(studio.current, Some(StudioKey::local("acme-shop")));
}

#[test]
fn cast_order_lists_the_open_people_then_the_closed_ones() {
    let summary = summary();
    let order: Vec<&str> = cast_order(&summary.studios[0])
        .iter()
        .map(|role| role.role.as_str())
        .collect();
    assert_eq!(order, vec!["producer", "actor-alice", "actor-dave"]);
}

#[test]
fn live_terminal_is_none_for_lost_closed_and_unseated_people() {
    let summary = summary();
    let roles = &summary.studios[0].roles;
    assert_eq!(live_terminal(&roles[0]), Some("t1"));
    assert_eq!(live_terminal(&roles[2]), None, "dave is closed");
    let waiting = RoleSummary {
        state: "live".into(),
        ..roles[0].clone()
    };
    let mut unseated = waiting.clone();
    unseated.terminal = None;
    assert_eq!(live_terminal(&unseated), None);
    let mut lost = waiting;
    lost.state = "lost".into();
    assert_eq!(live_terminal(&lost), None);
}

#[test]
fn the_attention_and_working_counts_feed_the_block_and_strip() {
    let summary = summary();
    assert_eq!(attention_count(&summary.studios[0]), 1);
    assert_eq!(attention_count(&summary.studios[1]), 0);
    assert_eq!(working_count(&summary.studios[0]), 2);
    assert_eq!(working_count(&summary.studios[1]), 0);
}

#[test]
fn role_focus_latches_when_document_seats_an_already_focused_terminal() {
    let mut unseated = two_studio_json();
    unseated["studios"][0]["roles"][0]
        .as_object_mut()
        .unwrap()
        .remove("terminal");
    let unseated: Summary = serde_json::from_value(unseated).unwrap();
    let mut studio = Studio::default();
    // Terminal `t1` focuses before the summary document publishes its seat.
    studio.sync(&[local(&unseated)], Some("t1"));
    assert_eq!(studio.focus, None);
    // Once the document arrives with `t1` seated (focused unchanged),
    // `focus` latches to `StudioFocus::Role` instead of staying `None`.
    let seated = summary();
    studio.sync(&[local(&seated)], Some("t1"));
    assert_eq!(
        studio.focus,
        Some(StudioFocus::Role(
            StudioKey::local("acme-shop"),
            "t1".into()
        ))
    );
}
