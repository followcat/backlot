//! App-level tests for lot folding: Space/Enter on the closed and
//! done folds, moving the cursor across studios (arrows, clicks,
//! Ctrl+K/J, Alt+arrows) keeping the same Target and scrolling it
//! into view. Neutral fixtures only.

use super::*;
use crate::app::input::{MouseButton, MouseEventKind, UiEvent};
use crate::panel::Target;
use crate::studio::StudioKey;
use hc_proto::watch::Document;

fn two_studio_fold_doc() -> serde_json::Value {
    serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 60_000,
        "studios": [
            {
                "id": "acme-shop",
                "title": "Acme shop",
                "state": "live",
                "requirement_version": 2,
                "mode": "standard",
                "open_reviews": 0,
                "waiting": 0,
                "tasks_done": 3,
                "tasks_total": 4,
                "next": "deliver t4 — actor-a",
                "roles": [
                    { "role": "producer", "name": "producer", "terminal": "t1", "state": "live", "work": "waiting" },
                    { "role": "director", "name": "director", "terminal": "t2", "state": "live", "work": "waiting" },
                    { "role": "actor-t4-c1", "name": "actor-a", "terminal": "t3", "state": "live", "work": "working", "task": "t4", "doing": "coding", "since_ms": 30_000 },
                    { "role": "writer-c1", "name": "writer", "state": "closed", "resumable": true },
                    { "role": "actor-t1-c1", "name": "actor-a", "state": "closed", "resumable": true },
                    { "role": "actor-t2-c1", "name": "actor-b", "state": "closed", "resumable": false }
                ],
                "tasks": [
                    { "id": "t1", "title": "One", "status": "accepted", "requirement_version": 2, "priority": "normal", "rework_rounds": 0, "rework_cap": 0 },
                    { "id": "t2", "title": "Two", "status": "accepted", "requirement_version": 2, "priority": "normal", "rework_rounds": 0, "rework_cap": 0 },
                    { "id": "t3", "title": "Three", "status": "accepted", "requirement_version": 2, "priority": "normal", "rework_rounds": 0, "rework_cap": 0 },
                    { "id": "t4", "title": "Four", "status": "running", "requirement_version": 2, "priority": "normal", "rework_rounds": 0, "rework_cap": 0, "actor_name": "actor-a" }
                ],
                "exchange": [
                    { "seq": 1, "who": "user", "text": "start" },
                    { "seq": 2, "who": "producer", "text": "running" }
                ]
            },
            {
                "id": "blog",
                "title": "Blog",
                "state": "live",
                "requirement_version": 1,
                "mode": "standard",
                "open_reviews": 1,
                "waiting": 1,
                "next": "review t1 — director",
                "roles": [
                    { "role": "producer", "name": "producer", "terminal": "t4", "state": "live", "work": "waiting" },
                    { "role": "director", "name": "director", "terminal": "t5", "state": "live", "work": "blocked", "attention": "owed", "attention_class": "needs_you", "attention_user": true, "attention_detail": "answer review" },
                    { "role": "actor-t1-c1", "name": "actor-b", "terminal": "t6", "state": "live", "work": "working", "task": "t1", "doing": "writing", "since_ms": 40_000 },
                    { "role": "writer-c1", "name": "writer", "state": "closed", "resumable": true }
                ],
                "tasks": [
                    { "id": "t0", "title": "Init", "status": "accepted", "requirement_version": 1, "priority": "normal", "rework_rounds": 0, "rework_cap": 0 },
                    { "id": "t1", "title": "Draft", "status": "delivered", "requirement_version": 1, "priority": "normal", "rework_rounds": 0, "rework_cap": 0 }
                ],
                "exchange": [
                    { "seq": 1, "who": "user", "text": "go" }
                ]
            }
        ]
    })
}

fn spaced_term(address: &str, space: &str) -> hc_proto::terminal::Terminal {
    let mut t = crate::sidebar::fixture::term(address);
    t.pid = Some(1);
    t.labels.insert("herdr.space".into(), space.into());
    t.labels.insert("herdr.tab".into(), "main".into());
    t
}

fn app_with_two_folded_studios() -> App {
    let mut app = app_with_terms(vec![
        spaced_term("t1", "acme-shop"),
        spaced_term("t2", "acme-shop"),
        spaced_term("t3", "acme-shop"),
        spaced_term("t4", "blog"),
        spaced_term("t5", "blog"),
        spaced_term("t6", "blog"),
    ]);
    super::super::endpoint_nav::pending_attach(&mut app);
    app.set_plain(false);
    app.host_reports_all_keys = true;
    let mut stage = crate::sidebar::fixture::term("t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    stage.labels.insert("backlot.role".into(), "stage".into());
    stage.labels.insert("herdr.space".into(), "backlot".into());
    stage.labels.insert("herdr.tab".into(), "stage".into());
    let mut terms: Vec<_> = app.view.inventory.terminals().values().cloned().collect();
    terms.push(stage);
    app.view.inventory_mut().replace(2, terms);
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: two_studio_fold_doc(),
            ..Document::default()
        }
        .into(),
    );
    app.focused = Some("t1".into());
    app.sync_target();
    app.reconcile_published_view();
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    app
}

#[tokio::test]
async fn space_and_enter_toggle_closed_fold_and_o_reopens_opened_seat() {
    let mut app = app_with_two_folded_studios();
    let (recorded, run) = runner();
    app.set_panel_runner(run);
    app.area = (120, 40);
    open_panel(&mut app);
    draw_to(&mut app, 120, 40);

    // Closed seats are hidden initially behind the closed fold line.
    assert!(
        !app.panel_targets()
            .iter()
            .any(|t| matches!(t, Target::Person(k, r) if k.id == "acme-shop" && r == "writer-c1")),
        "closed seats start folded"
    );

    // Navigate from Input up past the 2 talk lines onto the closed fold line.
    key(&mut app, hc_vt::Key::Char('i'));
    key(&mut app, hc_vt::Key::Escape);
    key(&mut app, hc_vt::Key::ArrowUp); // Talk 2
    key(&mut app, hc_vt::Key::ArrowUp); // Talk 1
    key(&mut app, hc_vt::Key::ArrowUp); // Closed fold row!

    // Space opens the closed seats; `o` on actor-a reopens it.
    key(&mut app, hc_vt::Key::Char(' '));
    assert!(
        app.panel_targets()
            .iter()
            .any(|t| matches!(t, Target::Person(k, r) if k.id == "acme-shop" && r == "writer-c1")),
        "Space opened closed seats: {:?}",
        app.panel_targets()
    );
    let fold_target = app.panel.selected.clone();
    key(&mut app, hc_vt::Key::ArrowDown); // writer-c1
    key(&mut app, hc_vt::Key::ArrowDown); // actor-t1-c1 (actor-a)
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "actor-t1-c1".into()))
    );
    key(&mut app, hc_vt::Key::Char('o'));
    assert_eq!(
        recorded.lock().unwrap().as_slice(),
        &[vec![
            "reopen".to_string(),
            "--person".into(),
            "actor-t1-c1".into(),
            "--studio".into(),
            "acme-shop".into(),
            "--as".into(),
            "user".into(),
        ]]
    );

    // Move back to the closed fold row and press Enter to shut it again.
    app.panel.selected = fold_target;
    key(&mut app, hc_vt::Key::Enter);
    assert!(
        !app.panel_targets()
            .iter()
            .any(|t| matches!(t, Target::Person(k, r) if k.id == "acme-shop" && r == "writer-c1")),
        "Enter shut closed seats"
    );
}

#[tokio::test]
async fn moving_cursor_across_studios_unfolds_target_keeps_same_target_and_scrolls() {
    let mut app = app_with_two_folded_studios();
    app.area = (120, 22);
    open_panel(&mut app);
    draw_to(&mut app, 120, 22);

    // Move to acme-shop's Input row (blog is folded below it).
    app.panel.selected = Some(Target::Input("acme-shop".into()));
    app.panel.selected_row = app
        .panel_targets()
        .iter()
        .position(|t| *t == Target::Input("acme-shop".into()));
    assert!(
        !app.panel_targets()
            .iter()
            .any(|t| *t == Target::Input("blog".into())),
        "blog starts folded"
    );

    // Step down into blog: cursor lands on Studio(blog), blog unfolds, acme folds,
    // selected_row updates, and Studio(blog) is visible in the drawn viewport.
    key(&mut app, hc_vt::Key::ArrowDown);
    assert_eq!(app.panel.selected, Some(Target::Studio("blog".into())));
    assert!(
        app.panel_targets()
            .iter()
            .any(|t| *t == Target::Input("blog".into())),
        "blog unfolded"
    );
    assert!(
        !app.panel_targets()
            .iter()
            .any(|t| *t == Target::Input("acme-shop".into())),
        "acme-shop folded"
    );
    draw_to(&mut app, 120, 22);
    assert!(
        app.panel_hits
            .rows
            .iter()
            .any(|(_, t)| *t == Target::Studio("blog".into())),
        "Studio(blog) must stay visible after acme-shop folded: {:?}",
        app.panel_hits.rows
    );

    // Step back up into folded acme-shop's last live role: acme unfolds, blog folds,
    // cursor keeps exact Target::Person(acme-shop, actor-t4-c1) and stays visible.
    key(&mut app, hc_vt::Key::ArrowUp);
    assert_eq!(
        app.panel.selected,
        Some(Target::Person("acme-shop".into(), "actor-t4-c1".into()))
    );
    assert!(
        app.panel_targets()
            .iter()
            .any(|t| *t == Target::Input("acme-shop".into())),
        "acme-shop unfolded"
    );
    draw_to(&mut app, 120, 22);
    assert!(
        app.panel_hits
            .rows
            .iter()
            .any(|(_, t)| *t == Target::Person("acme-shop".into(), "actor-t4-c1".into())),
        "selected role must stay visible after unfolding: {:?}",
        app.panel_hits.rows
    );
}

#[tokio::test]
async fn ctrl_jk_studio_stepping_unfolds_active_studio_even_after_prior_dock_visit() {
    let mut app = app_with_two_folded_studios();
    let cfg: crate::config::Config =
        toml::from_str("[keys]\nprevious_workspace = \"ctrl+k\"\nnext_workspace = \"ctrl+j\"\n")
            .unwrap();
    app.live = cfg.live_keybinds();
    app.area = (120, 24);

    // Visit dock in acme-shop (leaving panel.selected in acme-shop), then Esc back to pane.
    open_panel(&mut app);
    key(&mut app, hc_vt::Key::Escape);
    assert!(!app.panel.focused);
    assert!(
        !app.panel_targets()
            .iter()
            .any(|t| *t == Target::Input("blog".into()))
    );

    // Ctrl+J steps to blog workspace -> ▸ moves to blog -> blog unfolds and acme-shop folds.
    let parse = |seq| crate::raw_events::parse_terminal_key_sequence(seq).unwrap();
    let (ctrl_j, ctrl_k) = (parse("\x1b[106;5u"), parse("\x1b[107;5u"));
    app.on_input(UiEvent::Key(ctrl_j), 120);
    assert_eq!(app.studio.current, Some(StudioKey::local("blog")));
    assert!(
        app.panel_targets()
            .iter()
            .any(|t| *t == Target::Input("blog".into())),
        "Ctrl+J must unfold blog: {:?}",
        app.panel_targets()
    );
    assert!(
        !app.panel_targets()
            .iter()
            .any(|t| *t == Target::Input("acme-shop".into())),
        "Ctrl+J must fold acme-shop"
    );

    // Ctrl+K steps back to acme-shop -> acme-shop unfolds and blog folds.
    app.on_input(UiEvent::Key(ctrl_k), 120);
    assert_eq!(app.studio.current, Some(StudioKey::local("acme-shop")));
    assert!(
        app.panel_targets()
            .iter()
            .any(|t| *t == Target::Input("acme-shop".into()))
    );
    assert!(
        !app.panel_targets()
            .iter()
            .any(|t| *t == Target::Input("blog".into()))
    );
}

#[tokio::test]
async fn clicking_folded_studio_header_or_role_unfolds_it_and_keeps_row_visible() {
    let mut app = app_with_two_folded_studios();
    app.area = (120, 18);
    open_panel(&mut app);
    // Scroll down to bottom of acme-shop so folded blog is visible near the bottom.
    app.panel.selected = Some(Target::Input("acme-shop".into()));
    draw_to(&mut app, 120, 18);
    let max = app.panel_hits.metrics.unwrap().max_offset_from_bottom;
    app.panel.scroll = max;
    draw_to(&mut app, 120, 18);

    // Right-click a live role in folded blog -> blog unfolds, acme folds, target stays
    // on that role and stays visible inside the scrolled viewport.
    let (rect, target) = app
        .panel_hits
        .rows
        .iter()
        .find(|(_, t)| *t == Target::Person("blog".into(), "director".into()))
        .cloned()
        .expect("folded blog director hit");
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Right),
        rect.x + 2,
        rect.y,
    ));
    assert_eq!(app.panel.selected, Some(target.clone()));
    assert!(
        app.panel_targets()
            .iter()
            .any(|t| *t == Target::Input("blog".into())),
        "right-clicking folded role unfolded blog"
    );
    app.menu = None;
    draw_to(&mut app, 120, 18);
    assert!(
        app.panel_hits.rows.iter().any(|(_, t)| *t == target),
        "clicked role stays visible in viewport after unfolding: {:?}",
        app.panel_hits.rows
    );
}
