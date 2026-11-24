//! W2: the studio events the documents imply become notifications —
//! seeded silently, one per ledger identity, and never repeated by a
//! republish. Neutral fixtures only (`acme-shop`, `alice`, `host-a`).

use super::m4s2::{app_alerts, notification_count};
use super::*;
use hc_proto::watch::{DocKey, Document};
use serde_json::{Value, json};

fn doc_key() -> DocKey {
    DocKey {
        machine: None,
        name: "backlot".into(),
    }
}

/// A document with one `acme-shop` studio. `studio` holds the studio's
/// own fields (roles and its exchange among them) and `doc` the
/// document's — Hollywood lives at the root, never on a studio.
fn doc(studio: Value, doc: Value) -> Value {
    let mut entry = json!({
        "id": "acme-shop", "title": "Acme shop", "state": "live",
        "requirement_version": 1, "mode": "standard",
        "open_reviews": 0, "waiting": 0,
    });
    for (key, value) in studio.as_object().unwrap() {
        entry[key] = value.clone();
    }
    let mut out = json!({ "schema": "backlot/1", "updated_ms": 1, "studios": [entry] });
    for (key, value) in doc.as_object().unwrap() {
        out[key] = value.clone();
    }
    out
}

/// The usual three people, so each test names only what it changes.
fn roles() -> Value {
    json!([
        { "role": "producer", "name": "producer", "terminal": "t1", "state": "live" },
        { "role": "director", "name": "director", "terminal": "t2", "state": "live" },
        { "role": "actor-alice", "name": "alice", "terminal": "t4",
          "task": "task-1", "state": "live" }
    ])
}

fn hollywood(headline: &str, seq: u64, cards: Value) -> Value {
    json!({
        "hollywood": {
            "state": "live",
            "headline": headline, "headline_seq": seq,
            "exchange": [{ "seq": seq, "who": "hollywood", "text": headline }],
            "cards": cards,
        }
    })
}

fn card(id: &str, action: &str) -> Value {
    json!([{ "id": id, "studio": "acme-shop", "action": action, "why": "the actor is looping" }])
}

/// Malformed-document guard: every fixture must parse, or a test could
/// silently read "no studios" and pass for the wrong reason.
#[test]
fn the_fixtures_parse() {
    for value in [seeded_doc(), doc(json!({ "roles": roles() }), json!({}))] {
        let summary: backlot_model::summary::Summary = serde_json::from_value(value).unwrap();
        assert_eq!(summary.studios.len(), 1);
    }
}

/// An app in studio mode with a live stage and `bl-hollywood`, herdr
/// toasts at zero settle, and the given document already published —
/// which seeds the studio events silently. The focus sits on `t5`, an
/// ordinary terminal outside every studio.
fn app_with(value: Value) -> App {
    let mut app = app_alerts();
    app.set_plain(false);
    let mut stage = crate::sidebar::fixture::term("t9");
    stage.name = Some("backlot-stage".into());
    let mut hollywood = crate::sidebar::fixture::term("t10");
    hollywood.name = Some("bl-hollywood".into());
    let mut plain = crate::sidebar::fixture::term("t5");
    plain
        .labels
        .insert("herdr.space".into(), "elsewhere".into());
    plain.labels.insert("herdr.tab".into(), "main".into());
    let terminals: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .cloned()
        .chain([stage, hollywood, plain])
        .collect();
    app.view.inventory_mut().replace(3, terminals);
    app.focused = Some("t5".into());
    publish(&mut app, value);
    app
}

/// Replace the document and run the publish path — projection, pane
/// alerts and the studio events — exactly as the loop does.
fn publish(app: &mut App, value: Value) {
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();
}

/// Drop `address` from the inventory — a stage terminal stopping.
fn drop_terminal(app: &mut App, address: &str) {
    let terms: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .filter(|t| t.address != address)
        .cloned()
        .collect();
    let seq = app.view.inventory.seq() + 1;
    app.view.inventory_mut().replace(seq, terms);
}

/// The shown and queued toasts' titles — the settle runs at zero in
/// this fixture, so nothing lingers pending.
fn titles(app: &App) -> Vec<String> {
    let mut out: Vec<String> = app
        .notifications
        .visible
        .iter()
        .map(|n| n.event.title.clone())
        .chain(
            app.notifications
                .queued
                .iter()
                .map(|n| n.event.title.clone()),
        )
        .collect();
    out.sort();
    out
}

/// The seeding document: an ask, a user prompt, a Hollywood prompt, a
/// reply and a card already present — old news the client must not
/// announce.
fn seeded_doc() -> Value {
    doc(
        json!({
            "roles": [
                { "role": "producer", "name": "producer", "terminal": "t1",
                  "state": "live", "asks": "which shop?" },
                { "role": "director", "name": "director", "terminal": "t2",
                  "state": "live", "attention": "permission prompt — needs the user",
                  "attention_user": true, "attention_seq": 7,
                  "attention_detail": "run npm test" },
                { "role": "actor-alice", "name": "alice", "terminal": "t4",
                  "task": "task-1", "state": "live" }
            ],
            "exchange": [{ "seq": 4, "who": "producer", "text": "which shop?" }],
        }),
        json!({
            "hollywood": {
                "state": "live",
                "attention": "permission prompt — needs the user",
                "attention_user": true, "attention_detail": "rm -rf build",
                "headline": "all quiet", "headline_seq": 12,
                "exchange": [{ "seq": 12, "who": "hollywood", "text": "all quiet" }],
                "cards": card("c3", "pause t1"),
            }
        }),
    )
}

#[tokio::test]
async fn the_seed_document_announces_nothing() {
    let app = app_with(seeded_doc());
    assert_eq!(notification_count(&app), 0, "seeded: {:?}", titles(&app));
}

/// The `next` document every kind fires from: a new question, a new
/// prompt seq, a new reply seq, a different card and a ready studio.
fn every_kind_doc() -> Value {
    doc(
        json!({
            "ready_to_finish": true, "tasks_done": 1, "tasks_total": 1,
            "roles": [
                { "role": "producer", "name": "producer", "terminal": "t1",
                  "state": "live", "asks": "which colour?" },
                { "role": "director", "name": "director", "terminal": "t2",
                  "state": "live", "attention": "permission prompt — needs the user",
                  "attention_user": true, "attention_seq": 9,
                  "attention_detail": "run cargo test" },
                { "role": "actor-alice", "name": "alice", "terminal": "t4",
                  "task": "task-1", "state": "live" }
            ],
            "exchange": [{ "seq": 6, "who": "producer", "text": "which colour?" }],
        }),
        hollywood("two shops need you", 13, card("c4", "resume t2")),
    )
}

#[tokio::test]
async fn each_event_kind_fires_once_per_identity() {
    let mut app = app_with(seeded_doc());
    assert_eq!(notification_count(&app), 0);

    publish(&mut app, every_kind_doc());

    let fired = titles(&app);
    assert_eq!(
        fired,
        vec![
            "acme-shop is ready to finish".to_string(),
            "card waiting: resume t2".to_string(),
            "director needs you: run cargo test".to_string(),
            "hollywood replied: two shops need you".to_string(),
            "producer asks: which colour?".to_string(),
        ],
        "fired {fired:?}"
    );

    // Republishing the identical document repeats nothing.
    publish(&mut app, every_kind_doc());
    assert_eq!(titles(&app), fired, "a republish must not repeat");
}

#[tokio::test]
async fn a_stopped_stage_with_live_studios_fires_once() {
    let mut app = app_with(seeded_doc());
    assert_eq!(notification_count(&app), 0);
    // The stage terminal leaves the inventory — the document still lists
    // `acme-shop`, so the stage is stopped with live studios.
    drop_terminal(&mut app, "t9");
    app.reconcile_published_view();
    assert_eq!(titles(&app), vec!["stage stopped on Local".to_string()]);
    // A second publish of the same state adds nothing.
    app.reconcile_published_view();
    assert_eq!(notification_count(&app), 1);
}
