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

    // Sidebar headings (`machines` and the cast's studio label) stay
    // 100% uncovered in cols 0..25 — the header names the studio
    // directly now that `cast · ` is dropped (R3-N2).
    let sidebar_text = columns_text(&backend, 0, 25);
    assert!(sidebar_text.contains("MACHINES"), "{sidebar_text}");
    assert!(sidebar_text.contains("acme-shop"), "{sidebar_text}");
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
        ..CardView::default()
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
        ..SheetView::default()
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
    assert_eq!(narrow_cs.width, 54);
    assert_eq!(narrow_cs.height, 24);
}

#[tokio::test]
async fn reader_dims_panes_and_sidebar_while_keeping_beside_dock_undimmed() {
    let mut app = s4_app();
    app.area = (160, 40);
    app.panel.mode = PanelMode::Expanded;
    app.panel.focused = true;
    app.hollywood.mode = LayerMode::Collapsed;
    let base = draw_to(&mut app, 160, 40);
    app.hollywood.view = Some(CardView {
        card: None,
        title: "acme-shop #1".into(),
        lines: vec!["已记录最终需求 v6".into()],
        scroll: 0,
        ..CardView::default()
    });

    let backend = draw_to(&mut app, 160, 40);
    let buf = backend.buffer();
    let dock = app.panel_hits.area;
    assert!(!dock.is_empty());
    // Pane/sidebar cell outside the Reader card and dock must be dimmed via elevation.
    let elev = app.shell.palette.elevation();
    assert_ne!(
        buf[(0, 0)].bg,
        base.buffer()[(0, 0)].bg,
        "sidebar should be dimmed behind Reader"
    );
    assert_eq!(
        buf[(26, 2)].bg,
        elev.dim_bg.unwrap(),
        "pane surface outside Reader card should be dimmed"
    );
    // Beside Lot Dock cells must stay undimmed.
    assert_eq!(
        buf[(dock.x + 1, dock.y + 1)].bg,
        base.buffer()[(dock.x + 1, dock.y + 1)].bg,
        "beside Lot Dock must stay undimmed while Reader is open"
    );
}

#[tokio::test]
async fn slice2_visual_shots() {
    use unicode_width::UnicodeWidthStr;
    let Ok(dir) = std::env::var("LOT_READER_SHOTS") else {
        return;
    };
    let dump = |buf: &ratatui::buffer::Buffer, w: u16, h: u16| -> String {
        (0..h)
            .map(|y| {
                let mut row = String::new();
                let mut x = 0u16;
                while x < w {
                    let sym = buf[(x, y)].symbol();
                    row.push_str(sym);
                    x += UnicodeWidthStr::width(sym).max(1) as u16;
                }
                row
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    std::fs::create_dir_all(&dir).unwrap();
    let sample_cjk = vec![
        "已记录最终需求 v6，并已正式向导演派发（dispatch）方案 A+：".into(),
        "- T1 同步基线 `dbbe825be`（crates/hc-tui/src/panel/rows.rs）".into(),
        "- T2 回填角色门控回归测试，验收通过后汇总提交。".into(),
    ];

    for (name, w, h, lines) in [
        ("s2-80x24.txt", 80u16, 24u16, sample_cjk.clone()),
        ("s2-120x36.txt", 120, 36, sample_cjk.clone()),
        (
            "s2-200x44.txt",
            200,
            44,
            (1..=28)
                .map(|i| {
                    if i == 1 {
                        "### T1 Baseline Sync & Gate Verification".into()
                    } else if i == 2 {
                        "已完成 `crates/backlot-stage/src/` 下全部门控检查：".into()
                    } else if i == 3 {
                        "- crates/backlot-stage/src/hwood_read.rs:230 — 限制读取在 studio 根目录内"
                            .into()
                    } else if i == 5 {
                        "```sh".into()
                    } else if i == 6 {
                        "cargo test -p hc-tui panel::exchange".into()
                    } else if i == 7 {
                        "test result: ok. 18 passed; 0 failed (1.2s)".into()
                    } else if i == 8 {
                        "```".into()
                    } else {
                        format!("detail line {i}")
                    }
                })
                .collect(),
        ),
    ] {
        let mut app = s4_app();
        app.area = (w, h);
        app.panel.mode = PanelMode::Expanded;
        app.hollywood.mode = LayerMode::Collapsed;
        app.hollywood.view = Some(CardView {
            card: None,
            title: "OUTCOME #188 · acme-shop".into(),
            lines,
            scroll: 0,
            ..CardView::default()
        });
        let backend = draw_to(&mut app, w, h);
        std::fs::write(format!("{dir}/{name}"), dump(backend.buffer(), w, h)).unwrap();
    }
}

#[tokio::test]
async fn slice3_visual_shots() {
    use unicode_width::UnicodeWidthStr;
    let Ok(dir) = std::env::var("LOT_READER_SHOTS") else {
        return;
    };
    let dump = |buf: &ratatui::buffer::Buffer, w: u16, h: u16| -> String {
        (0..h)
            .map(|y| {
                let mut row = String::new();
                let mut x = 0u16;
                while x < w {
                    let sym = buf[(x, y)].symbol();
                    row.push_str(sym);
                    x += UnicodeWidthStr::width(sym).max(1) as u16;
                }
                row
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    std::fs::create_dir_all(&dir).unwrap();

    let send_key = |app: &mut App, code: hc_vt::Key, w: u16| {
        app.on_input(
            crate::app::input::UiEvent::Key(term_key(
                code,
                hc_vt::Mods::NONE,
                hc_vt::Action::Press,
            )),
            w,
        );
    };

    for (name, w, h) in [
        ("s3-80x24.txt", 80u16, 24u16),
        ("s3-120x36.txt", 120, 36),
        ("s3-200x44.txt", 200, 44),
    ] {
        let mut app = s4_app();
        let mut doc = serde_json::json!({
            "schema": "backlot/1",
            "updated_ms": 1,
            "studios": [{
                "id": "acme-shop",
                "title": "Acme shop",
                "state": "live",
                "requirement_version": 6,
                "mode": "standard",
                "open_reviews": 1,
                "waiting": 0,
                "tasks_done": 1,
                "tasks_total": 3,
                "next": "review t2 · director",
                "roles": [
                    { "role": "producer", "name": "producer", "terminal": "t1", "runner": "agy", "model": "pro", "state": "live", "work": "waiting" },
                    { "role": "director", "name": "director", "terminal": "t1", "runner": "agy", "model": "pro", "state": "live", "work": "working" },
                    { "role": "writer", "name": "writer", "state": "closed", "runner": "claude", "model": "sonnet" }
                ],
                "exchange": [
                    { "seq": 185, "who": "producer", "to": "user", "kind": "outcome", "text": "已完成 v6 初稿校对", "file": "reports/185.md" },
                    { "seq": 186, "who": "user", "to": "director", "kind": "tell", "text": "请优先处理 T1", "file": "messages/186.txt" },
                    { "seq": 188, "who": "director", "to": "user", "kind": "report", "task": "t1", "text": "已记录最终需求 v6，并已派发方案", "file": "reports/188.md" },
                    { "seq": 189, "who": "producer", "to": "user", "kind": "outcome", "task": "t2", "text": "已派发 T2 回归用例", "file": "messages/189.md" }
                ]
            }]
        });
        doc["studios"][0]["tasks"] = serde_json::json!([]);
        app.view.documents_mut().insert(
            hc_proto::watch::DocKey {
                machine: None,
                name: "backlot".into(),
            },
            hc_proto::watch::Document {
                value: doc,
                ..hc_proto::watch::Document::default()
            }
            .into(),
        );
        app.reconcile_published_view();
        app.hollywood.mode = LayerMode::Collapsed;
        app.set_ns_run(std::sync::Arc::new(|_m, argv, _s| {
            let joined = argv.join(" ");
            let body = if joined.contains("188.md") {
                "已完成 `crates/backlot-stage/src/` 下全部门控检查：\n- `crates/hc-tui/src/hollywood/view.rs:82` — 修复宽字符标题列宽计算\n- `crates/hc-tui/src/panel/rows.rs` — 同步基线校验通过"
            } else if joined.contains("189.md") {
                "已派发 T2 回归用例\n- 检查 `panel::exchange` 异步跳转"
            } else {
                "已完成 v6 初稿校对"
            };
            Box::pin(async move { Ok(body.to_owned()) })
        }));
        app.area = (w, h);
        app.panel.mode = PanelMode::Expanded;
        app.panel.focused = true;
        app.panel.selected = Some(crate::panel::Target::Talk("acme-shop".into(), 185));
        send_key(&mut app, hc_vt::Key::Enter, w);
        tokio::task::yield_now().await;
        while let Ok(r) = app.action_rx.try_recv() {
            app.on_action_reply(r);
        }
        // Step across 3+ entries so `◀ 3/4 ▶` (#188) and dock `▌` sync cleanly.
        for _ in 0..2 {
            send_key(&mut app, hc_vt::Key::ArrowRight, w);
            tokio::task::yield_now().await;
            while let Ok(r) = app.action_rx.try_recv() {
                app.on_action_reply(r);
            }
        }
        let backend = draw_to(&mut app, w, h);
        std::fs::write(format!("{dir}/{name}"), dump(backend.buffer(), w, h)).unwrap();
    }
}
