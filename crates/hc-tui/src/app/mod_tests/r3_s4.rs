//! Round 3 Slice 4 tests (part 1): Call Sheet `pane + dock` band
//! (`wide-13` / `R3-4`) and unified `pane_surface` modal & `Reader`
//! (`CardView`) geometry (`R3-M2`).

use hc_proto::watch::{DocKey, Document};
use ratatui::layout::Rect;

use super::*;
use crate::app::input::{MouseButton, MouseEventKind, MouseInput};
use crate::hollywood::{CardView, LayerMode};
use crate::panel::PanelMode;
use crate::sheet::{SheetTab, SheetView};
use crate::studio::StudioKey;

pub(super) fn doc_key(machine: Option<&str>) -> DocKey {
    DocKey {
        machine: machine.map(str::to_owned),
        name: "backlot".into(),
    }
}

pub(super) fn s4_summary() -> serde_json::Value {
    serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "studios": [
            {
                "id": "acme-shop", "title": "Acme shop", "state": "live",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 1,
                "roles": [
                    { "role": "producer", "name": "producer", "terminal": "t1",
                      "state": "live", "work": "working" },
                    { "role": "actor-alice", "name": "alice", "terminal": "t4",
                      "state": "live", "work": "blocked", "task": "t1",
                      "attention_seq": 7, "attention_user": true,
                      "attention_detail": "rm -rf build/" }
                ],
                "tasks": [
                    { "id": "t1", "title": "build api", "status": "running",
                      "priority": "normal", "requirement_version": 1,
                      "rework_rounds": 0, "rework_cap": 3 }
                ]
            }
        ],
        "hollywood": {
            "state": "live",
            "runner": "fake",
            "model": "m",
            "effort": "high",
            "headline": "acme-shop needs a look",
            "cards": [
                { "id": "c3", "studio": "acme-shop",
                  "action": "pause t1", "why": "the actor is looping" }
            ],
            "exchange": [
                { "seq": 1, "who": "user", "text": "status?" },
                { "seq": 2, "who": "hollywood", "text": "acme-shop needs a look" }
            ]
        }
    })
}

pub(super) fn s4_app() -> App {
    let mut app = app_with(&["t1", "t2", "t3", "t4", "t5"]);
    app.set_plain(false);
    app.sidebar_width = 25;
    let mut stage = crate::sidebar::fixture::term("t9");
    stage.name = Some("backlot-stage".into());
    stage.state = "live".into();
    let terminals: Vec<_> = app
        .view
        .inventory
        .terminals()
        .values()
        .cloned()
        .chain(std::iter::once(stage))
        .collect();
    app.view.inventory_mut().replace(2, terminals);
    app.view.documents_mut().insert(
        doc_key(None),
        Document {
            value: s4_summary(),
            ..Document::default()
        }
        .into(),
    );
    app.focused = Some("t1".into());
    app.reconcile_published_view();
    app.set_hollywood(
        Ok(Some(backlot_stage::config::HollywoodCfg {
            runner: "fake".into(),
            model: "m".into(),
            effort: None,
            key: None,
            default_cast: None,
        })),
        std::path::Path::new("config.toml"),
    );
    app.hollywood.mode = LayerMode::Collapsed;
    app
}

pub(super) fn mouse(kind: MouseEventKind, col: u16, row: u16) -> MouseInput {
    MouseInput {
        kind,
        col,
        row,
        mods: hc_vt::Mods::NONE,
        sgr_position: None,
    }
}

fn columns_text(b: &ratatui::backend::TestBackend, left: u16, right: u16) -> String {
    let (w, h) = (b.size().unwrap().width, b.size().unwrap().height);
    let buf = b.buffer();
    (0..h)
        .map(|y| {
            (left..right.min(w))
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn callsheet_band_spans_pane_and_dock_only_leaving_sidebar_whole() {
    let mut app = s4_app();
    app.area = (160, 47);
    app.panel.mode = PanelMode::Expanded;
    app.hollywood.mode = LayerMode::Expanded;

    let backend = draw_to(&mut app, 160, 47);
    assert_eq!(app.sidebar_render_width(160), 25);
    assert_eq!(app.hollywood_hits.area.x, 25);
    assert_eq!(app.hollywood_hits.area.width, 135);

    // Sidebar headings (`machines` and `cast`) stay 100% uncovered in cols 0..25.
    let sidebar_text = columns_text(&backend, 0, 25);
    assert!(sidebar_text.contains("machines"), "{sidebar_text}");
    assert!(sidebar_text.contains("cast"), "{sidebar_text}");
    assert!(
        !sidebar_text.contains("H O L L Y W O O D"),
        "Call Sheet must not paint into sidebar: {sidebar_text}"
    );

    // Clicking inside the visible sidebar collapses the Call Sheet cleanly.
    app.on_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 5, 2));
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);

    // On mobile (60x20, sidebar_w == 0), Call Sheet spans 0..60.
    app.area = (60, 20);
    app.hollywood.mode = LayerMode::Expanded;
    draw_to(&mut app, 60, 20);
    assert_eq!(app.sidebar_render_width(60), 0);
    assert_eq!(app.hollywood_hits.area.x, 0);
    assert_eq!(app.hollywood_hits.area.width, 60);
}

#[tokio::test]
async fn unified_reader_geometry_matches_across_callsheet_dock_and_sheet() {
    let sample_view = || CardView {
        card: Some("c3".into()),
        title: "c3".into(),
        lines: (0..60).map(|i| format!("line {i}")).collect(),
        scroll: 0,
    };

    // 1. At 160x47 with 25-col sidebar and beside-Dock open, Call Sheet,
    // Dock, and Task Sheet all open the Reader with identical pane_surface geometry.
    let mut app = s4_app();
    app.area = (160, 47);
    app.panel.mode = PanelMode::Expanded;
    let layout = app.chrome_layout(160, 47);
    assert_eq!(layout.pane_surface, Rect::new(25, 1, 97, 46));

    // From Call Sheet (Expanded)
    app.hollywood.mode = LayerMode::Expanded;
    app.hollywood.view = Some(sample_view());
    draw_to(&mut app, 160, 47);
    let from_callsheet = app.hollywood_hits.view.as_ref().unwrap().area;

    // From Studio Dock (Call Sheet Collapsed)
    app.hollywood.mode = LayerMode::Collapsed;
    draw_to(&mut app, 160, 47);
    let from_dock = app.hollywood_hits.view.as_ref().unwrap().area;

    // From Task Sheet report read
    app.hollywood.view = None;
    app.sheet_open(StudioKey::local("acme-shop"), None);
    let sheet = app.sheet.as_mut().unwrap();
    sheet.tab = SheetTab::Deliveries;
    sheet.view = Some(SheetView {
        title: "reports/d1.md".into(),
        lines: (0..60).map(|i| format!("line {i}")).collect(),
        scroll: 0,
    });
    draw_to(&mut app, 160, 47);
    let from_sheet = app.hollywood_hits.view.as_ref().unwrap().area;

    assert_eq!(from_callsheet, from_dock);
    assert_eq!(from_callsheet, from_sheet);
    assert!(from_callsheet.x >= layout.pane_surface.x);
    assert!(from_callsheet.right() <= layout.pane_surface.right());

    // Wheel scroll & outside-click work on Task Sheet's unified Reader too.
    app.on_mouse(mouse(
        MouseEventKind::ScrollDown,
        from_sheet.x + 1,
        from_sheet.y + 1,
    ));
    assert!(app.sheet.as_ref().unwrap().view.as_ref().unwrap().scroll > 0);
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        from_sheet.x.saturating_sub(1),
        from_sheet.y,
    ));
    assert!(
        app.sheet.as_ref().unwrap().view.is_none(),
        "outside click closes Reader without closing Task Sheet"
    );
    assert!(app.sheet.is_some());

    // 2. At 80x24 (pane_surface.w == 55 < 64), Reader falls back to full 80x24.
    app.sheet = None;
    app.area = (80, 24);
    app.hollywood.mode = LayerMode::Expanded;
    app.hollywood.view = Some(sample_view());
    draw_to(&mut app, 80, 24);
    let narrow_cs = app.hollywood_hits.view.as_ref().unwrap().area;

    app.hollywood.mode = LayerMode::Collapsed;
    draw_to(&mut app, 80, 24);
    let narrow_dock = app.hollywood_hits.view.as_ref().unwrap().area;

    assert_eq!(narrow_cs, narrow_dock);
    assert_eq!(narrow_cs.width, 80 * 7 / 8);
    assert_eq!(narrow_cs.height, 24 * 7 / 8);
}
