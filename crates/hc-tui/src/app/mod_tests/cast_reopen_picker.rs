//! App-level tests for the studio-scoped reopen picker: numeric task order,
//! taskless roles last by name, offline dimming, CJK title truncation, scroll
//! overflow, and row/outside clicks.

use hc_proto::watch::{DocKey, Document};

use super::*;
use crate::app::input::{MouseButton, MouseEventKind};
use crate::panel::PanelMode;
use crate::studio::StudioKey;

use super::cast_closed_line::{closed_line_app, doc_key, key, mouse, multi_closed_doc, recorder};

#[tokio::test]
async fn picker_orders_tasks_numerically_and_scopes_to_one_studio() {
    let (mut app, _) = closed_line_app();
    app.open_cast_reopen_picker(&StudioKey::local("acme-shop"));
    let picker = app.help_state.picker.as_ref().expect("picker");
    let labels: Vec<&str> = picker.rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(labels.len(), 16);
    assert!(labels[0].starts_with("t1  actor-a  "), "{:?}", labels[0]);
    assert!(labels[1].starts_with("t2  actor-a  "), "{:?}", labels[1]);
    assert!(labels[9].starts_with("t10  actor-a  "), "{:?}", labels[9]);
    assert!(labels[15].starts_with("t16  actor-a  "), "{:?}", labels[15]);
}

#[tokio::test]
async fn taskless_closed_roles_go_last_by_name_and_offline_dims_without_reopen() {
    let mut app = app_with(&["t1"]);
    app.set_plain(false);
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);
    let mut stage = crate::sidebar::fixture::term("t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    let terminals: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .cloned()
        .chain([stage])
        .collect();
    app.view.inventory_mut().replace(2, terminals);
    app.view.machines = Some(vec![crate::sidebar::fixture::machine(
        "host-b",
        "unreachable",
    )]);
    app.view.documents_mut().insert(
        DocKey {
            machine: Some("host-b".into()),
            name: "backlot".into(),
        },
        Document {
            value: serde_json::json!({
                "schema": "backlot/1",
                "updated_ms": 1,
                "studios": [{
                    "id": "acme-shop",
                    "title": "Acme shop",
                    "state": "live",
                    "requirement_version": 1,
                    "mode": "standard",
                    "open_reviews": 0,
                    "waiting": 0,
                    "roles": [
                        { "role": "writer", "name": "writer", "state": "closed", "resumable": true },
                        { "role": "actor-t2", "name": "actor-b", "state": "closed", "resumable": true, "task": "t2" },
                        { "role": "director", "name": "director", "state": "closed", "resumable": true },
                        { "role": "actor-t10", "name": "actor-a", "state": "closed", "resumable": true, "task": "t10" }
                    ],
                    "tasks": [
                        { "id": "t2", "title": "Second", "status": "accepted", "priority": "normal", "requirement_version": 1, "rework_rounds": 0, "rework_cap": 3 },
                        { "id": "t10", "title": "Tenth", "status": "accepted", "priority": "normal", "requirement_version": 1, "rework_rounds": 0, "rework_cap": 3 }
                    ]
                }]
            }),
            ..Document::default()
        }
        .into(),
    );
    let remote_key = StudioKey {
        machine: Some("host-b".into()),
        id: "acme-shop".into(),
    };
    app.studio.current = Some(remote_key.clone());
    app.reconcile_published_view();
    app.hollywood.mode = crate::hollywood::LayerMode::Hidden;
    app.panel.mode = PanelMode::Closed;
    app.area = (120, 30);

    app.open_cast_reopen_picker(&remote_key);
    let picker = app.help_state.picker.as_ref().expect("remote picker");
    let labels: Vec<&str> = picker.rows.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(
        labels,
        vec![
            "t2  actor-b  Second",
            "t10  actor-a  Tenth",
            "director",
            "writer",
        ]
    );
    assert!(
        picker
            .rows
            .iter()
            .all(|r| r.dim.as_deref() == Some("unreachable")),
        "offline machine dims every reopen row: {:?}",
        picker.rows
    );

    let backend = draw_to(&mut app, 120, 30);
    let text = screen_lines(&backend).join("\n");
    assert!(text.contains("t2  actor-b  Second"), "{text}");
    assert!(text.contains("unreachable"), "{text}");

    // Enter on dimmed row closes without dispatching reopen.
    key(&mut app, hc_vt::Key::Enter);
    assert!(!app.help);
    assert!(
        recorded.lock().unwrap().is_empty(),
        "offline dimmed row must not spawn reopen"
    );
}

#[tokio::test]
async fn picker_truncates_cjk_titles_scrolls_overflow_and_supports_row_or_outside_clicks() {
    let (mut app, _) = closed_line_app();
    let (recorded, runner) = recorder();
    app.set_hollywood_runner(runner);

    let mut doc = multi_closed_doc();
    doc["studios"][0]["tasks"][2]["title"] =
        serde_json::json!("实现用户认证流程并在长标题处安全截断避免边框错位");
    app.view.documents_mut().insert(
        doc_key(),
        Document {
            value: doc,
            ..Document::default()
        }
        .into(),
    );
    app.reconcile_published_view();

    // Short terminal height (14 rows -> picker viewport shows <16 rows and scrolls).
    app.area = (120, 14);
    app.open_cast_reopen_picker(&StudioKey::local("acme-shop"));
    let buf = draw_to(&mut app, 120, 14);
    let text = screen_lines(&buf).join("\n");
    assert!(text.contains("reopen a closed role"), "{text}");
    assert!(text.contains("t1  actor-a"), "{text}");
    assert!(
        !text.contains("Actions & Keybinds"),
        "standalone picker skips launcher:\n{text}"
    );
    let p_rect = app.help_hits.picker;
    let first_y = app.help_hits.picker_rows[0].0.y;
    assert_eq!(buf.buffer()[(p_rect.right() - 1, first_y)].symbol(), "│");
    assert!(
        text.lines()
            .any(|l| l.contains("t1  actor-a") && l.contains('…')),
        "{text}"
    );

    // Move down to row 15 (`t16`): scroll window advances so `t16` is painted and clickable.
    for _ in 0..15 {
        key(&mut app, hc_vt::Key::ArrowDown);
    }
    let scrolled_text = screen_lines(&draw_to(&mut app, 120, 14)).join("\n");
    assert!(
        scrolled_text.contains("t16  actor-a"),
        "t16 scrolled into view:\n{scrolled_text}"
    );

    // Clicking outside picker closes cleanly; clicking a visible row runs reopen.
    app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 0, 0));
    assert!(!app.help, "outside click closed standalone picker");
    assert!(recorded.lock().unwrap().is_empty());

    app.open_cast_reopen_picker(&StudioKey::local("acme-shop"));
    draw_to(&mut app, 120, 30);
    let &(row_rect, row_pos) = app.help_hits.picker_rows.get(2).expect("3rd row hit");
    assert_eq!(row_pos, 2);
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        row_rect.x + 2,
        row_rect.y,
    ));
    assert!(!app.help);
    let argv = recorded.lock().unwrap();
    assert_eq!(
        argv.as_slice(),
        &[vec![
            "reopen",
            "--person",
            "actor-t3",
            "--studio",
            "acme-shop",
            "--as",
            "user"
        ]]
    );
}
