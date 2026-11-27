//! A resumed studio role replaces a dead terminal with a live one:
//! the main pane and studio cursor retarget to the new terminal
//! without stealing keyboard focus or disturbing finished non-resumed
//! actors. Neutral fixtures only (`acme-shop`, `night-owl`, `alice`,
//! `bob`, `host-a`).

use super::studio::{alt, doc_key};
use super::*;
use crate::app::input::UiEvent;
use crate::hollywood::LayerMode;
use crate::panel::PanelMode;
use crate::studio::{StudioFocus, StudioKey};
use hc_proto::terminal::Terminal;
use hc_proto::watch::Document;
use hc_vt::{Action, Key, Mods};

fn role_term(
    address: &str,
    studio: &str,
    role: &str,
    state: &str,
    reason: Option<&str>,
) -> Terminal {
    let mut t = crate::sidebar::fixture::term(address);
    t.labels.insert("backlot.studio".into(), studio.into());
    t.labels.insert("backlot.role".into(), role.into());
    t.labels
        .insert("herdr.space".into(), format!("space-{address}"));
    t.labels.insert("herdr.tab".into(), role.into());
    t.state = state.into();
    t.reason = reason.map(str::to_owned);
    t
}

fn plain_term(address: &str, state: &str, reason: Option<&str>) -> Terminal {
    let mut t = crate::sidebar::fixture::term(address);
    t.labels.insert("herdr.space".into(), "notes".into());
    t.labels.insert("herdr.tab".into(), address.into());
    t.state = state.into();
    t.reason = reason.map(str::to_owned);
    t
}

fn role_entry(role: &str, name: &str, terminal: Option<&str>, state: &str) -> serde_json::Value {
    serde_json::json!({
        "role": role,
        "name": name,
        "terminal": terminal,
        "state": state,
        "work": (state == "live").then_some("working"),
    })
}

fn studio_roles(roles: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({
        "schema": "backlot/1", "updated_ms": 1,
        "studios": [
            {
                "id": "acme-shop", "title": "Acme shop", "state": "live",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0, "roles": roles
            },
            {
                "id": "night-owl", "title": "Night owl", "state": "live",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0,
                "roles": [role_entry("producer", "owl-producer", Some("t7"), "live")]
            }
        ]
    })
}

fn summary(
    prod: (Option<&str>, &str),
    alice: (Option<&str>, &str),
    writer: (Option<&str>, &str),
) -> serde_json::Value {
    studio_roles(vec![
        role_entry("producer", "producer", prod.0, prod.1),
        role_entry("actor-alice", "alice", alice.0, alice.1),
        role_entry("writer", "writer", writer.0, writer.1),
    ])
}

fn live_doc(prod: &str, alice: &str, writer: &str) -> serde_json::Value {
    summary(
        (Some(prod), "live"),
        (Some(alice), "live"),
        (Some(writer), "live"),
    )
}

fn role_cursor(terminal: &str) -> Option<StudioFocus> {
    Some(StudioFocus::Role(
        StudioKey::local("acme-shop"),
        terminal.into(),
    ))
}

fn publish(app: &mut App, seq: u64, terms: Vec<Terminal>, version: u64, value: serde_json::Value) {
    app.view.inventory_mut().replace(seq, terms);
    app.view.documents_mut().insert(
        doc_key(),
        Arc::new(Document {
            value,
            version,
            ..Document::default()
        }),
    );
    app.reconcile_published_view();
}

fn type_key(app: &mut App, key: Key) {
    app.on_input(
        UiEvent::Key(term_key(key, Mods::NONE, Action::Press)),
        app.area.0,
    );
}

fn base_terms() -> Vec<Terminal> {
    let mut stage = crate::sidebar::fixture::term("t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    vec![
        role_term("t1", "acme-shop", "producer", "live", None),
        role_term("t4", "acme-shop", "actor-alice", "live", None),
        role_term("t3", "acme-shop", "writer", "live", None),
        role_term("t7", "night-owl", "producer", "live", None),
        plain_term("t20", "finished", Some("exited")),
        stage,
    ]
}

fn studio_app() -> App {
    let mut app = app();
    app.set_plain(false);
    app.area = (120, 24);
    app.hollywood.mode = LayerMode::Hidden;
    app.focused = Some("t1".into());
    publish(&mut app, 1, base_terms(), 1, live_doc("t1", "t4", "t3"));
    app
}

fn exit_and_resume_producer(app: &mut App) {
    let mut terms = base_terms();
    terms[0] = role_term("t1", "acme-shop", "producer", "finished", Some("exited"));
    let doc_lost = summary(
        (Some("t1"), "lost"),
        (Some("t4"), "live"),
        (Some("t3"), "live"),
    );
    publish(app, 2, terms.clone(), 2, doc_lost);
    terms.push(role_term("t11", "acme-shop", "producer", "live", None));
    publish(app, 3, terms, 3, live_doc("t11", "t4", "t3"));
}

#[tokio::test]
async fn resumed_role_moves_focused_pane_and_clears_unavailable_notice() {
    let mut app = studio_app();
    assert_eq!(app.focused.as_deref(), Some("t1"));
    assert_eq!(app.pane.target(), Some("t1"));
    assert!(app.pane.notice().is_none());

    // Producer exits: dead role terminal stays in inventory with unavailable notice.
    let mut terms = base_terms();
    terms[0] = role_term("t1", "acme-shop", "producer", "finished", Some("exited"));
    let doc_lost = summary(
        (Some("t1"), "lost"),
        (Some("t4"), "live"),
        (Some("t3"), "live"),
    );
    publish(&mut app, 2, terms.clone(), 2, doc_lost);
    let notice = app.pane.notice().expect("shows unavailable notice");
    assert_eq!(notice.title, "Terminal unavailable");
    assert_eq!(notice.body, "finished·exited");
    let screen = screen_lines(&draw_to(&mut app, 120, 24)).join("\n");
    assert!(screen.contains("Terminal unavailable"), "{screen}");
    assert!(screen.contains("finished·exited"), "{screen}");

    // Stage resumes producer in a new live terminal while t1 remains finished.
    terms.push(role_term("t11", "acme-shop", "producer", "live", None));
    publish(&mut app, 3, terms, 3, live_doc("t11", "t4", "t3"));

    assert_eq!(app.focused.as_deref(), Some("t11"));
    assert_eq!(app.pane.target(), Some("t11"));
    assert!(app.pane.notice().is_none(), "unavailable notice clears");
    assert_eq!(app.studio.focus, role_cursor("t11"));
    assert_ne!(app.previous_pane.as_deref(), Some("t1"));
}

#[tokio::test]
async fn restarted_or_lost_role_moves_pane_even_when_old_terminal_vanishes() {
    let mut app = studio_app();
    app.focus_pane("t4".into());

    let mut terms = base_terms();
    terms[1] = role_term("t4", "acme-shop", "actor-alice", "finished", Some("exited"));
    let doc_closed = summary((Some("t1"), "live"), (None, "closed"), (Some("t3"), "live"));
    publish(&mut app, 2, terms, 2, doc_closed);
    assert_eq!(app.focused.as_deref(), Some("t4"));

    // On resume/reopen, t4 is dropped from inventory in the same publish as live t14.
    let mut next_terms = base_terms();
    next_terms.remove(1);
    next_terms.push(role_term("t14", "acme-shop", "actor-alice", "live", None));
    publish(&mut app, 3, next_terms, 3, live_doc("t1", "t14", "t3"));

    assert_eq!(
        app.focused.as_deref(),
        Some("t14"),
        "lands directly on new role terminal"
    );
    assert_eq!(app.pane.target(), Some("t14"));
}

#[tokio::test]
async fn resumed_role_keeps_keyboard_focus_on_pane() {
    let mut app = studio_app();
    exit_and_resume_producer(&mut app);
    assert_eq!(app.focused.as_deref(), Some("t11"));
    assert!(!app.panel_focused(), "pane stays focused");
    let before = app.pane.input_backlog().0;
    type_key(&mut app, Key::Char('x'));
    assert!(
        app.pane.input_backlog().0 > before,
        "typed input reaches resumed pane"
    );
}

#[tokio::test]
async fn resumed_role_keeps_keyboard_focus_on_panel() {
    let mut app = studio_app();
    app.panel.mode = PanelMode::Expanded;
    app.panel.focused = true;
    draw_to(&mut app, 120, 24);
    let selected_before = app.panel.selected.clone();

    exit_and_resume_producer(&mut app);

    assert_eq!(app.focused.as_deref(), Some("t11"));
    assert_eq!(app.pane.target(), Some("t11"));
    assert!(app.panel_focused(), "panel must keep keyboard focus");
    let backlog_before = app.pane.input_backlog().0;
    type_key(&mut app, Key::ArrowDown);
    assert_eq!(app.pane.input_backlog().0, backlog_before);
    assert_ne!(app.panel.selected, selected_before);
}

#[tokio::test]
async fn finished_actor_not_resumed_does_not_move_pane() {
    let mut app = studio_app();
    app.focus_pane("t4".into());

    // actor-alice finishes (`closed`), while writer resumes t3 -> t13 and actor-bob starts t15.
    let mut terms = base_terms();
    terms[1] = role_term("t4", "acme-shop", "actor-alice", "finished", Some("exited"));
    terms[2] = role_term("t3", "acme-shop", "writer", "finished", Some("exited"));
    terms.push(role_term("t13", "acme-shop", "writer", "live", None));
    terms.push(role_term("t15", "acme-shop", "actor-bob", "live", None));
    publish(
        &mut app,
        2,
        terms,
        2,
        studio_roles(vec![
            role_entry("producer", "producer", Some("t1"), "live"),
            role_entry("actor-alice", "alice", Some("t4"), "closed"),
            role_entry("actor-bob", "bob", Some("t15"), "live"),
            role_entry("writer", "writer", Some("t13"), "live"),
        ]),
    );
    assert_eq!(app.focused.as_deref(), Some("t4"));
    assert_eq!(app.pane.target(), Some("t4"));
    assert_eq!(
        app.pane.notice().expect("still shows finished").body,
        "finished·exited"
    );

    // Deliberately inspecting an already-replaced finished role terminal does not jump when
    // that role later moves again (`producer` on t11, user clicks t1, then t11 -> t12).
    let mut replaced = base_terms();
    replaced[0] = role_term("t1", "acme-shop", "producer", "finished", Some("exited"));
    replaced.push(role_term("t11", "acme-shop", "producer", "live", None));
    publish(
        &mut app,
        3,
        replaced.clone(),
        3,
        live_doc("t11", "t4", "t3"),
    );
    app.focus_pane("t1".into());
    replaced.push(role_term("t12", "acme-shop", "producer", "live", None));
    publish(&mut app, 4, replaced, 4, live_doc("t12", "t4", "t3"));
    assert_eq!(app.focused.as_deref(), Some("t1"));

    app.focus_pane("t20".into());
    exit_and_resume_producer(&mut app);
    assert_eq!(app.focused.as_deref(), Some("t20"));
}

#[tokio::test]
async fn remembered_studio_member_follows_resumed_role() {
    let mut app = studio_app();
    let acme = StudioKey::local("acme-shop");

    app.focus_pane("t4".into());
    let remembered = app.studio.remembered.get(&acme).map(String::as_str);
    assert_eq!(remembered, Some("actor-alice"));
    assert_eq!(app.studio.focus, role_cursor("t4"));

    let mut terms = base_terms();
    terms[1] = role_term("t4", "acme-shop", "actor-alice", "finished", Some("exited"));
    terms.push(role_term("t14", "acme-shop", "actor-alice", "live", None));
    publish(&mut app, 2, terms.clone(), 2, live_doc("t1", "t14", "t3"));
    assert_eq!(app.studio.focus, role_cursor("t14"));

    // Role stepping reaches writer (`t3`); stepping away & back after writer resumes `t3 -> t13`
    // lands on `t13` with the cursor on `t13`.
    alt(&mut app, Key::ArrowDown);
    assert_eq!(app.focused.as_deref(), Some("t3"));
    alt(&mut app, Key::PageDown);
    assert_eq!(app.focused.as_deref(), Some("t7"));

    terms[2] = role_term("t3", "acme-shop", "writer", "finished", Some("exited"));
    terms.push(role_term("t13", "acme-shop", "writer", "live", None));
    publish(&mut app, 3, terms, 3, live_doc("t1", "t14", "t13"));
    alt(&mut app, Key::PageUp);
    assert_eq!(app.studio.current, Some(acme));
    assert_eq!(app.focused.as_deref(), Some("t13"));
    assert_eq!(app.pane.target(), Some("t13"));
    assert_eq!(app.studio.focus, role_cursor("t13"));
}
