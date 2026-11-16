//! The studio latch: mode on/off rules, the current studio following
//! the focused terminal, remembered people, cast order and the
//! `!n`/working counts. Neutral fixtures only.
use super::*;
use backlot_model::summary::Summary;

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
    let summary = summary();
    let mut studio = Studio::default();
    studio.sync(Some(&summary), None);
    assert_eq!(studio.current.as_deref(), Some("acme-shop"));
    assert_eq!(studio.current_index(&summary), Some(0));
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
    studio.sync(Some(&summary), Some("t1"));
    // `None` — a missing, malformed or foreign-schema read — keeps
    // every latch exactly as it was.
    studio.sync(None, Some("t9"));
    assert!(studio.active);
    assert_eq!(studio.current.as_deref(), Some("acme-shop"));
}

#[test]
fn a_good_document_with_zero_studios_keeps_the_mode() {
    // U2: unadorned `backlot` is always in studio mode — the cast's
    // `others` row is the no-studio view, not the agents list.
    let mut studio = Studio {
        active: true,
        ..Studio::default()
    };
    studio.sync(Some(&summary()), Some("t1"));
    let empty: Summary = serde_json::from_value(serde_json::json!({
        "schema": "backlot/1", "updated_ms": 2, "studios": []
    }))
    .unwrap();
    studio.sync(Some(&empty), Some("t1"));
    assert!(studio.active);
    assert_eq!(studio.current_index(&empty), None);
}

#[test]
fn the_focused_terminal_moves_current_and_remembers_the_person() {
    let summary = summary();
    let mut studio = Studio::default();
    studio.sync(Some(&summary), Some("t9"));
    assert_eq!(studio.current.as_deref(), Some("night-owl"));
    assert_eq!(
        studio.remembered.get("night-owl").map(String::as_str),
        Some("actor-bob")
    );
    // A terminal no person owns keeps the current studio.
    studio.sync(Some(&summary), Some("t99"));
    assert_eq!(studio.current.as_deref(), Some("night-owl"));
}

#[test]
fn a_vanished_current_falls_back_to_document_order() {
    let summary = summary();
    let mut studio = Studio::default();
    studio.sync(Some(&summary), Some("t9"));
    assert_eq!(studio.current.as_deref(), Some("night-owl"));
    let mut one = two_studio_json();
    one["studios"].as_array_mut().unwrap().remove(1);
    let one: Summary = serde_json::from_value(one).unwrap();
    studio.sync(Some(&one), None);
    assert_eq!(studio.current.as_deref(), Some("acme-shop"));
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
