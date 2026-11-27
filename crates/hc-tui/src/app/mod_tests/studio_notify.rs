//! W2: the studio events the documents imply become notifications,
//! each once per ledger identity, seeded silently, suppressible by the
//! config switch and by a focused target, and each opening its own
//! needs-you row. Neutral fixtures only (`acme-shop`, `alice`,
//! `host-a`).

use super::m4s2::{app_alerts, captured, notification_count, pending, visible_title};
use super::*;
use crate::hollywood::{LayerMode, Target};
use crate::studio::StudioKey;
use hc_proto::watch::{DocKey, Document};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

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
/// ordinary terminal outside every studio, so no target starts focused
/// (and therefore suppressed) unless a test moves it there.
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

#[tokio::test]
async fn a_focused_target_suppresses_its_event() {
    let mut app = app_with(seeded_doc());
    // The producer's terminal is focused when its new question lands.
    app.focused = Some("t1".into());
    publish(&mut app, every_kind_doc());
    // The ask was suppressed; the unfocused prompt still fired.
    assert!(
        !titles(&app).iter().any(|t| t.starts_with("producer asks:")),
        "a focused target's event must not toast: {:?}",
        titles(&app)
    );
    assert!(
        titles(&app)
            .iter()
            .any(|t| t.starts_with("director needs you:")),
        "an unfocused target still fires: {:?}",
        titles(&app)
    );
}

/// The usual three roles, the producer carrying an unanswered question.
fn roles_with_ask(question: &str) -> Value {
    json!([
        { "role": "producer", "name": "producer", "terminal": "t1",
          "state": "live", "asks": question },
        { "role": "director", "name": "director", "terminal": "t2", "state": "live" },
        { "role": "actor-alice", "name": "alice", "terminal": "t4",
          "task": "task-1", "state": "live" }
    ])
}

#[tokio::test]
async fn the_config_switch_turns_it_off() {
    let mut app = app_with(seeded_doc());
    app.shell.toast_studio = false;
    publish(
        &mut app,
        doc(
            json!({
                "roles": roles_with_ask("which colour?"),
                "exchange": [{ "seq": 6, "who": "producer", "text": "which colour?" }],
            }),
            hollywood("all quiet", 12, card("c3", "pause t1")),
        ),
    );
    assert_eq!(
        notification_count(&app),
        0,
        "switch off: {:?}",
        titles(&app)
    );

    // Back on, the next new identity fires — the switch was not a latch.
    app.shell.toast_studio = true;
    publish(
        &mut app,
        doc(
            json!({
                "roles": roles_with_ask("which size?"),
                "exchange": [{ "seq": 8, "who": "producer", "text": "which size?" }],
            }),
            hollywood("all quiet", 12, card("c3", "pause t1")),
        ),
    );
    assert!(
        titles(&app)
            .iter()
            .any(|t| t == "producer asks: which size?"),
        "switch on again fires: {:?}",
        titles(&app)
    );
}

/// The toast's jump: `focus_visible_notification` on a studio event
/// opens its row's target. Each event is delivered directly so the jump
/// under test is unambiguous.
fn toast_of(app: &mut App, target: Target, title: &str) {
    app.notifications.visible = None;
    app.notifications.queued.clear();
    app.notifications.pending.clear();
    let event = crate::app::notifications::NotificationEvent {
        kind: crate::app::notifications::NotificationKind::NeedsAttention,
        title: title.into(),
        body: None,
        sound: None,
        agent: None,
        endpoint_id: crate::chrome::snapshot::ClientEndpointId::Local,
        workspace_id: None,
        tab_id: None,
        pane_id: None,
        studio: Some(target),
    };
    app.notifications.visible = Some(crate::app::notifications::Notification {
        event,
        deadline: Instant::now() + std::time::Duration::from_secs(60),
    });
}

#[tokio::test]
async fn a_role_question_jumps_to_the_role_terminal() {
    let mut app = app_with(seeded_doc());
    app.focused = Some("t4".into());
    toast_of(
        &mut app,
        Target::Role(StudioKey::local("acme-shop"), "director".into()),
        "director needs you",
    );
    app.focus_visible_notification();
    assert_eq!(app.focused.as_deref(), Some("t2"));
    assert!(
        app.notifications.visible.is_none(),
        "the toast is dismissed"
    );
}

#[tokio::test]
async fn a_hollywood_question_jumps_to_its_terminal() {
    let mut app = app_with(seeded_doc());
    app.focused = Some("t1".into());
    toast_of(&mut app, Target::Hollywood, "hollywood needs you");
    app.focus_visible_notification();
    assert_eq!(app.focused.as_deref(), Some("t10"));
}

#[tokio::test]
async fn a_card_jump_opens_the_call_sheet_on_that_card() {
    let mut app = app_with(seeded_doc());
    app.hollywood.mode = LayerMode::Collapsed;
    app.hollywood.selected = None;
    toast_of(&mut app, Target::Card("c3".into()), "card waiting");
    app.focus_visible_notification();
    assert_eq!(app.hollywood.mode, LayerMode::Expanded);
    assert_eq!(app.hollywood.selected, Some(Target::Card("c3".into())));
}

#[tokio::test]
async fn a_ready_studio_jump_focuses_the_studio() {
    let mut app = app_with(seeded_doc());
    app.focused = Some("t10".into());
    app.studio.current = None;
    // No remembered member yet: the landing is the producer.
    app.studio.remembered.clear();
    toast_of(
        &mut app,
        Target::Ready(StudioKey::local("acme-shop")),
        "acme-shop is ready to finish",
    );
    app.focus_visible_notification();
    assert_eq!(app.studio.current, Some(StudioKey::local("acme-shop")));
    assert_eq!(app.focused.as_deref(), Some("t1"));
}

#[tokio::test]
async fn a_ready_studio_jump_returns_to_the_remembered_person() {
    // The studio was left on the director — the jump respects that,
    // exactly as `focus_studio` does everywhere.
    let mut app = app_with(seeded_doc());
    app.studio
        .remembered
        .insert(StudioKey::local("acme-shop"), "director".into());
    toast_of(
        &mut app,
        Target::Ready(StudioKey::local("acme-shop")),
        "acme-shop is ready to finish",
    );
    app.focus_visible_notification();
    assert_eq!(app.focused.as_deref(), Some("t2"));
}

#[tokio::test]
async fn a_stopped_stage_jump_selects_the_dock_row_without_starting_it() {
    let mut app = app_with(seeded_doc());
    drop_terminal(&mut app, "t9");
    app.reconcile_published_view();
    let before = app.actions.pending.len();
    toast_of(
        &mut app,
        Target::Stage(None, false),
        "stage stopped on Local",
    );
    app.focus_visible_notification();
    assert!(app.panel.focused, "the dock takes the keys");
    assert_eq!(app.panel.mode, crate::panel::PanelMode::Expanded);
    assert_eq!(app.panel.selected, Some(crate::panel::Target::Stage(None)));
    assert_eq!(
        app.actions.pending.len(),
        before,
        "a jump never starts the stage"
    );
}

#[tokio::test]
async fn a_jump_never_attaches_or_spawns() {
    // Guard: the jump path runs no action and needs no network — the
    // fixture's attach opener stays pending through every kind.
    let mut app = app_with(seeded_doc());
    pending(&mut app);
    let before = app.actions.pending.len();
    for target in [
        Target::Hollywood,
        Target::Card("c3".into()),
        Target::Ready(StudioKey::local("acme-shop")),
        Target::Stage(None, false),
    ] {
        toast_of(&mut app, target, "x");
        app.focus_visible_notification();
    }
    assert_eq!(app.actions.pending.len(), before);
    assert_eq!(visible_title(&app), None);
}

/// Item 7: the desktop ping. A new question pings the outer terminal
/// once, a restart with the same ask open does not, a card pings, an
/// acknowledged ask does not, and `[studio] notify_asks = false` silences
/// it. Every case has a terminal backend installed and captures the
/// sink's bytes. `pings` returns the OSC 9 sequences the sink received,
/// as UTF-8 — one per ping.
fn pings(captured: &Arc<Mutex<Vec<Vec<u8>>>>) -> Vec<String> {
    captured
        .lock()
        .unwrap()
        .iter()
        .filter(|bytes| bytes.starts_with(b"\x1b]9;"))
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .collect()
}

/// Studio mode with a Ghostty notification backend, a fresh capture, and
/// the given document seeded silently.
fn ping_app(value: Value) -> (App, Arc<Mutex<Vec<Vec<u8>>>>) {
    let mut app = app_with(value);
    app.notify_backend = Some(crate::terminal_notify::TerminalNotificationBackend::Ghostty);
    let (captured, sink) = captured();
    app.set_host_control(sink);
    (app, captured)
}

#[tokio::test]
async fn a_new_ask_pings_once_and_a_republish_does_not() {
    let (mut app, captured) = ping_app(seeded_doc());
    assert!(pings(&captured).is_empty(), "the seed must not ping");
    publish(
        &mut app,
        doc(
            json!({
                "roles": roles_with_ask("which colour?"),
                "exchange": [{ "seq": 6, "who": "producer", "text": "which colour?" }],
            }),
            hollywood("all quiet", 12, card("c3", "pause t1")),
        ),
    );
    let fired = pings(&captured);
    assert_eq!(fired.len(), 1, "one ping per new ask: {fired:?}");
    assert!(
        fired[0].contains("acme-shop · producer asks: which colour?"),
        "the ping names the studio and the question: {fired:?}"
    );
    // The identical document repeats nothing.
    publish(
        &mut app,
        doc(
            json!({
                "roles": roles_with_ask("which colour?"),
                "exchange": [{ "seq": 6, "who": "producer", "text": "which colour?" }],
            }),
            hollywood("all quiet", 12, card("c3", "pause t1")),
        ),
    );
    assert_eq!(pings(&captured).len(), 1, "a republish must not ping");
}

#[tokio::test]
async fn a_restart_with_the_ask_open_does_not_ping() {
    // The same document is the seed on start: what predates the client
    // is not news, so no ping fires.
    let open = doc(
        json!({
            "roles": roles_with_ask("which colour?"),
            "exchange": [{ "seq": 6, "who": "producer", "text": "which colour?" }],
        }),
        hollywood("all quiet", 12, card("c3", "pause t1")),
    );
    let (_app, captured) = ping_app(open);
    assert!(
        pings(&captured).is_empty(),
        "an open ask at start is not news"
    );
}

#[tokio::test]
async fn a_card_pings() {
    let (mut app, captured) = ping_app(seeded_doc());
    publish(
        &mut app,
        doc(
            json!({ "roles": roles() }),
            hollywood("all quiet", 12, card("c4", "resume t2")),
        ),
    );
    let fired = pings(&captured);
    assert_eq!(fired.len(), 1, "a card pings: {fired:?}");
    assert!(
        fired[0].contains("acme-shop · card c4: resume t2"),
        "the ping names the card and its action: {fired:?}"
    );
}

#[tokio::test]
async fn an_acknowledged_ask_does_not_ping() {
    // The ask opens (one ping), then is answered — the role no longer
    // asks, so nothing new pings on the answering publish.
    let (mut app, captured) = ping_app(seeded_doc());
    publish(
        &mut app,
        doc(
            json!({
                "roles": roles_with_ask("which colour?"),
                "exchange": [{ "seq": 6, "who": "producer", "text": "which colour?" }],
            }),
            hollywood("all quiet", 12, card("c3", "pause t1")),
        ),
    );
    assert_eq!(pings(&captured).len(), 1, "the ask pinged once");
    publish(
        &mut app,
        doc(
            json!({
                "roles": roles(),
                "exchange": [{ "seq": 6, "who": "producer", "text": "which colour?" }],
            }),
            hollywood("all quiet", 12, card("c3", "pause t1")),
        ),
    );
    assert_eq!(
        pings(&captured).len(),
        1,
        "answering an ask never pings: {:?}",
        pings(&captured)
    );
}

#[tokio::test]
async fn the_setting_off_means_no_ping() {
    let (mut app, captured) = ping_app(seeded_doc());
    app.shell.studio_notify_asks = false;
    publish(
        &mut app,
        doc(
            json!({
                "roles": roles_with_ask("which colour?"),
                "exchange": [{ "seq": 6, "who": "producer", "text": "which colour?" }],
            }),
            hollywood("all quiet", 12, card("c3", "pause t1")),
        ),
    );
    assert!(
        pings(&captured).is_empty(),
        "setting off: {:?}",
        pings(&captured)
    );
}
