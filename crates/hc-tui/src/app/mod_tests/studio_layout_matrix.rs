//! 200x44 and 100x40 studio-mode layout matrix verification: sidebar
//! deduplication & `? keys`, compact Lot rows, unfolded live roles,
//! 1-line inline Cast/Lot role rows with CJK truncation, bucketed pill
//! colors/jumps, `Alt+Y` landing, `HeaderKind::Role`, footers, and 19 themes.

use super::*;
use crate::app::input::{MouseButton, MouseEventKind, MouseInput, UiEvent};
use crate::chrome::lot::PillAction;
use crate::chrome::palette::Palette;
use crate::hollywood::{LayerMode, Target as SheetTarget};
use crate::panel::{PanelMode, Target as PanelTarget};
use crate::sidebar::fixture::{agent, term};
use crate::studio::StudioKey;
use crate::test_support::TestDir;
use hc_proto::terminal::Terminal;
use hc_proto::watch::{DocKey, Document, Link, View};
use ratatui::style::Color;

fn press(app: &mut App, k: hc_vt::Key) {
    press_mods(app, k, hc_vt::Mods::NONE);
}

fn press_mods(app: &mut App, k: hc_vt::Key, mods: hc_vt::Mods) {
    app.on_input(
        UiEvent::Key(term_key(k, mods, hc_vt::Action::Press)),
        app.area.0,
    );
}

fn click(app: &mut App, col: u16, row: u16) {
    app.on_mouse(MouseInput {
        kind: MouseEventKind::Down(MouseButton::Left),
        col,
        row,
        mods: hc_vt::Mods::NONE,
        sgr_position: None,
    });
}

fn spaced_role(address: &str, space: &str, studio: Option<(&str, &str)>) -> Terminal {
    let mut t = term(address);
    t.state = "live".into();
    t.labels.insert("herdr.space".into(), space.into());
    t.labels.insert("herdr.tab".into(), "main".into());
    if let Some((st, role)) = studio {
        t.labels.insert("backlot.studio".into(), st.into());
        t.labels.insert("backlot.role".into(), role.into());
    }
    t
}

fn doc_value(has_prompt: bool, has_ask_and_card: bool) -> serde_json::Value {
    let acme_producer = if has_prompt {
        serde_json::json!({
            "role": "producer", "name": "producer", "terminal": "t2",
            "state": "live", "work": "waiting", "attention": "permission prompt",
            "attention_user": true, "attention_seq": 7,
            "attention_detail": "cargo test --workspace", "can_answer_prompt": true
        })
    } else if has_ask_and_card {
        serde_json::json!({
            "role": "producer", "name": "producer", "terminal": "t2",
            "state": "live", "work": "waiting",
            "asks": "Approve database migration?", "asks_seq": 9, "since_ms": 1_000
        })
    } else {
        serde_json::json!({
            "role": "producer", "name": "producer", "terminal": "t2",
            "state": "live", "work": "working", "doing": "triaging bugfix"
        })
    };
    let cards = if has_ask_and_card {
        serde_json::json!([{
            "id": "c3", "role": "actor-cjk", "command": "git push origin HEAD:main",
            "rule": "protected branch 'main'", "why": "release_main"
        }])
    } else {
        serde_json::json!([])
    };
    let ready = |id: &str, term_id: &str| {
        serde_json::json!({
            "id": id, "title": id, "state": "live", "requirement_version": 1,
            "mode": "standard", "open_reviews": 0, "waiting": 0,
            "ready_to_finish": true, "tasks_done": 2, "tasks_total": 2,
            "roles": [{ "role": "producer", "name": "producer", "terminal": term_id,
                        "state": "live", "work": "waiting" }]
        })
    };
    serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": if has_ask_and_card { 200 } else { 100 },
        "backlot_version": "0.4.10",
        "studios": [
            {
                "id": "beta-api", "title": "Beta API", "state": "live",
                "requirement_version": 3, "mode": "standard",
                "open_reviews": 0, "waiting": 0,
                "tasks_done": 1, "tasks_total": 3, "actors_live": 0, "actors_cap": 4,
                "roles": [
                    { "role": "producer", "name": "producer", "terminal": "t1",
                      "state": "live", "work": "working", "runner": "agy",
                      "model": "gemini-4-neon", "effort": "xhigh",
                      "doing": "coordinating rollout" },
                    { "role": "director", "name": "director", "terminal": "t3",
                      "state": "live", "work": "waiting" },
                    { "role": "writer", "name": "writer", "state": "standby" },
                    { "role": "actor-alice", "name": "alice", "terminal": "t4",
                      "state": "live", "task": "t1", "work": "working",
                      "runner": "agy", "model": "gemini-4-neon",
                      "doing": "修复中文编码边界问题与回归测试" },
                    { "role": "actor-bob", "name": "bob", "terminal": "t5",
                      "state": "live", "task": "t2", "work": "idle" }
                ]
            },
            {
                "id": "acme-shop", "title": "Acme shop", "state": "live",
                "requirement_version": 2, "mode": "standard",
                "open_reviews": 0, "tasks_done": 0, "tasks_total": 2,
                "waiting": usize::from(has_prompt),
                "outward_cards": cards,
                "roles": [
                    acme_producer,
                    { "role": "actor-cjk", "name": "中文开发", "terminal": "t6",
                      "state": "live", "task": "t3", "work": "working",
                      "doing": "处理超长中文任务描述避免截断错位" }
                ]
            },
            ready("gamma-web", "t10"),
            ready("delta-ops", "t11"),
            ready("epsilon-ml", "t12"),
            ready("zeta-docs", "t13"),
            {
                "id": "omega-done", "title": "Omega done", "state": "finished",
                "requirement_version": 1, "mode": "standard",
                "open_reviews": 0, "waiting": 0,
                "tasks_done": 4, "tasks_total": 4,
                "roles": [{ "role": "producer", "name": "producer", "state": "closed" }]
            }
        ]
    })
}

fn matrix_fixture(has_prompt: bool, has_ask_and_card: bool) -> (App, TestDir) {
    let dir = TestDir::new("studio-layout-matrix");
    std::fs::create_dir_all(&dir).unwrap();
    let herdr_path = dir.join("herdr.toml");
    let own_path = dir.join("own.toml");
    std::fs::write(&herdr_path, "[theme]\nname = \"dracula\"\n").unwrap();
    std::fs::write(&own_path, "[theme]\nname = \"backlot\"\n").unwrap();

    let mut app = app_with_plain(false);
    app.shell.local_config_path = herdr_path;
    app.shell.own_theme = crate::own_theme::Shell::at(own_path);
    app.set_plain(false);
    app.reload_config();
    app.area = (200, 44);
    app.has_hollywood_cfg = true;

    let mut stage = spaced_role("t9", "backlot", None);
    stage.name = Some("backlot-stage".into());
    let mut work = agent("t99", "idle");
    work.labels.insert("herdr.space".into(), "work-ws".into());
    work.labels.insert("herdr.tab".into(), "main".into());

    let terms = vec![
        spaced_role("t1", "beta-api", Some(("beta-api", "producer"))),
        spaced_role("t3", "beta-api", Some(("beta-api", "director"))),
        spaced_role("t4", "beta-api", Some(("beta-api", "actor-alice"))),
        spaced_role("t5", "beta-api", Some(("beta-api", "actor-bob"))),
        spaced_role("t2", "acme-shop", Some(("acme-shop", "producer"))),
        spaced_role("t6", "acme-shop", Some(("acme-shop", "actor-cjk"))),
        spaced_role("t10", "gamma-web", Some(("gamma-web", "producer"))),
        spaced_role("t11", "delta-ops", Some(("delta-ops", "producer"))),
        spaced_role("t12", "epsilon-ml", Some(("epsilon-ml", "producer"))),
        spaced_role("t13", "zeta-docs", Some(("zeta-docs", "producer"))),
        stage,
        work,
    ];
    let mut view = View {
        link: Link::Online,
        rpc: Link::Online,
        core: Some(hc_proto::hello::CoreInfo {
            id: "id-host-a".into(),
            name: "host-a".into(),
            hostname: None,
        }),
        machines: Some(vec![]),
        ..Default::default()
    };
    view.inventory_mut().replace(1, terms);
    let doc = Document {
        value: doc_value(has_prompt, has_ask_and_card),
        ..Document::default()
    };
    view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        doc.into(),
    );
    app.view = view;
    app.focused = Some("t1".into());
    app.studio.current = Some(StudioKey::local("beta-api"));
    app.panel.mode = PanelMode::Expanded;
    app.panel.focused = false;
    app.hollywood.mode = LayerMode::Collapsed;
    app.shell_state.sidebar_collapsed = false;
    app.reconcile_published_view();
    (app, dir)
}

fn slice_cols(buf: &ratatui::buffer::Buffer, area: ratatui::layout::Rect) -> Vec<String> {
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

#[tokio::test]
async fn layout_200x44_and_100x40_satisfy_sidebar_lot_header_and_role_contracts() {
    let (mut app, _dir) = matrix_fixture(true, false);
    let backend = draw_to(&mut app, 200, 44);
    let layout = app.chrome_layout(200, 44);
    let buf = backend.buffer();
    if std::env::var_os("STUDIO_LAYOUT_DUMP").is_some() {
        eprintln!(
            "=== 200x44 ===\n{}\n=== END 200x44 ===",
            screen_lines(&backend).join("\n")
        );
    }

    let ws: Vec<&str> = app
        .hit_map
        .workspaces
        .iter()
        .map(|w| w.workspace_id.as_str())
        .collect();
    assert!(ws.iter().any(|id| id.ends_with(":work-ws")), "{ws:?}");
    for live in [
        "beta-api",
        "acme-shop",
        "gamma-web",
        "delta-ops",
        "epsilon-ml",
        "zeta-docs",
    ] {
        assert!(!ws.iter().any(|id| id.ends_with(&format!(":{live}"))));
    }
    let sidebar_rows = slice_cols(buf, layout.sidebar);
    let sidebar_text = sidebar_rows.join("\n");
    let expected_ver = format!("backlot {} · ● ok", env!("CARGO_PKG_VERSION"));
    assert!(
        sidebar_text.contains(&expected_ver),
        "missing `{expected_ver}` in:\n{sidebar_text}"
    );
    assert!(sidebar_text.contains("finished (1)"));
    assert!(!sidebar_text.contains("host-a · ok"));
    assert!(!app.hit_map.sidebar_help.is_empty());
    assert!(sidebar_rows.last().unwrap().contains("? keys"));
    assert!(sidebar_rows.last().unwrap().contains('«'));
    assert!(
        sidebar_text.contains("gemini-4-neon"),
        "cast displays open roles' runner/model line: {sidebar_text}"
    );

    let top_center: String = (layout.sidebar.right()..layout.pane_surface.right())
        .map(|x| buf[(x, layout.tab_bar.y)].symbol())
        .collect();
    for tok in [
        "beta-api ›",
        "Ⓟ",
        "agy · gemini-4-neon (xhigh)",
        "✦ 7",
        "!1",
        "✓4 ready",
        "alt+y",
    ] {
        assert!(top_center.contains(tok), "missing `{tok}`: {top_center}");
    }
    let pal = app.shell.palette.clone();
    assert_eq!(
        buf[(app.hit_map.lot_pill.x + 1, app.hit_map.lot_pill.y)].bg,
        pal.red
    );

    let lot_area = app.panel_hits.area;
    let lot_rows = slice_cols(buf, lot_area);
    let lot_text = lot_rows.join("\n");
    assert!(lot_text.contains("lot · 7"), "{lot_text}");
    assert!(lot_rows[0].contains("alt+l focus") && !lot_rows[0].contains("alt+y"));
    assert!(!lot_text.contains("actors ") && !lot_text.contains("gemini-4-neon"));
    assert!(lot_text.contains("idle") && lot_text.contains("standby"));
    assert!(
        lot_text.contains('Ⓓ') && lot_text.contains('Ⓦ'),
        "idle/standby roles render individually in expanded block: {lot_text}"
    );
    assert!(!lot_text.contains("2 idle"), "{lot_text}");
    for compact in [
        "gamma-web",
        "delta-ops",
        "epsilon-ml",
        "zeta-docs",
        "omega-done",
    ] {
        assert!(!lot_text.contains(&format!("╭─ {compact}")));
        assert!(lot_text.contains(compact));
    }
    let elev = pal.elevation();
    assert_eq!(lot_rows.last().unwrap().trim_end(), "│");
    for x in (lot_area.x + 1)..lot_area.right() {
        assert_eq!(buf[(x, lot_area.bottom() - 1)].bg, elev.surface);
    }
    let input_y = app
        .panel_hits
        .rows
        .iter()
        .find_map(|(r, t)| matches!(t, PanelTarget::Input(k) if k.id == "beta-api").then_some(r.y))
        .expect("beta-api input row");
    assert_eq!(buf[(lot_area.x + 3, input_y)].bg, elev.surface);

    app.shell_state.sidebar_collapsed = true;
    let collapsed = slice_cols(draw_to(&mut app, 200, 44).buffer(), app.panel_hits.area).join("\n");
    assert!(
        collapsed.contains('Ⓓ') && collapsed.contains('Ⓦ'),
        "{collapsed}"
    );
    app.shell_state.sidebar_collapsed = false;

    press_mods(&mut app, hc_vt::Key::Char('l'), hc_vt::Mods::ALT);
    assert!(app.panel.focused);
    let focused_rows = slice_cols(draw_to(&mut app, 200, 44).buffer(), app.panel_hits.area);
    assert!(focused_rows[0].contains("esc leave"));
    assert!(focused_rows[focused_rows.len() - 2].contains("───"));
    assert!(focused_rows.last().unwrap().contains("esc leave"));
    press(&mut app, hc_vt::Key::Escape);
    assert!(!app.panel.focused);

    let backend_100 = draw_to(&mut app, 100, 40);
    let layout_100 = app.chrome_layout(100, 40);
    if std::env::var_os("STUDIO_LAYOUT_DUMP").is_some() {
        eprintln!(
            "=== 100x40 ===\n{}\n=== END 100x40 ===",
            screen_lines(&backend_100).join("\n")
        );
    }
    let narrow_center: String = (layout_100.sidebar.right()..layout_100.pane_surface.right())
        .map(|x| backend_100.buffer()[(x, layout_100.tab_bar.y)].symbol())
        .collect();
    for tok in ["beta-api ›", "Ⓟ", "!1"] {
        assert!(narrow_center.contains(tok), "{narrow_center}");
    }

    // At 100x40 with collapsed sidebar, Lot shows beside panes and the 60-col
    // header cleanly degrades metadata without colliding into the pill.
    app.shell_state.sidebar_collapsed = true;
    let tight = draw_to(&mut app, 100, 40);
    let tight_layout = app.chrome_layout(100, 40);
    if std::env::var_os("STUDIO_LAYOUT_DUMP").is_some() {
        eprintln!(
            "=== 100x40 (sidebar collapsed + Lot) ===\n{}\n=== END 100x40_LOT ===",
            screen_lines(&tight).join("\n")
        );
    }
    let tight_header: String = (tight_layout.sidebar.right()..tight_layout.pane_surface.right())
        .map(|x| tight.buffer()[(x, tight_layout.tab_bar.y)].symbol())
        .collect();
    assert!(tight_header.contains("beta-api ›") && tight_header.contains('Ⓟ'));
    assert!(!tight_header.contains("(xhigh)"), "{tight_header}");
    assert!(tight_header.contains("!1"), "{tight_header}");
}

#[tokio::test]
async fn pill_bucket_clicks_and_alt_y_smart_landing() {
    let (mut app, _dir) = matrix_fixture(true, false);
    let _ = draw_to(&mut app, 200, 44);
    let jump_to = |app: &App, want: PillAction| {
        app.hit_map
            .lot_pill_segments
            .iter()
            .find_map(|(r, a)| (*a == want).then_some(*r))
            .unwrap_or_else(|| panic!("missing pill segment {want:?}"))
    };

    let blocking_rect = jump_to(&app, PillAction::Blocking);
    click(&mut app, blocking_rect.x, blocking_rect.y);
    assert!(matches!(
        app.hollywood.selected,
        Some(SheetTarget::Prompt(..))
    ));
    press(&mut app, hc_vt::Key::Escape);
    assert_eq!(app.hollywood.mode, LayerMode::Collapsed);

    let ready_rect = jump_to(&app, PillAction::Ready);
    click(&mut app, ready_rect.x, ready_rect.y);
    assert!(matches!(
        app.hollywood.selected,
        Some(SheetTarget::Ready(..))
    ));
    press(&mut app, hc_vt::Key::Escape);

    press_mods(&mut app, hc_vt::Key::Char('y'), hc_vt::Mods::ALT);
    assert!(matches!(
        app.hollywood.selected,
        Some(SheetTarget::Prompt(..))
    ));
    press(&mut app, hc_vt::Key::Escape);

    let doc = Document {
        value: doc_value(false, true),
        ..Document::default()
    };
    app.view.documents_mut().insert(
        DocKey {
            machine: None,
            name: "backlot".into(),
        },
        doc.into(),
    );
    app.reconcile_published_view();
    let neutral = draw_to(&mut app, 200, 44);
    let pill = app.hit_map.lot_pill;
    assert_ne!(
        neutral.buffer()[(pill.x + 1, pill.y)].bg,
        app.shell.palette.red
    );

    let asks_rect = jump_to(&app, PillAction::Asks);
    click(&mut app, asks_rect.x, asks_rect.y);
    assert!(matches!(
        app.hollywood.selected,
        Some(SheetTarget::Role(..))
    ));
    press(&mut app, hc_vt::Key::Escape);

    let cards_rect = jump_to(&app, PillAction::Cards);
    click(&mut app, cards_rect.x, cards_rect.y);
    assert!(matches!(
        app.hollywood.selected,
        Some(SheetTarget::OutwardCard(..))
    ));
    press(&mut app, hc_vt::Key::Escape);

    press_mods(&mut app, hc_vt::Key::Char('y'), hc_vt::Mods::ALT);
    assert_eq!(app.hollywood.selected, Some(SheetTarget::Input));
}

#[tokio::test]
async fn studio_chrome_all_19_bundled_themes_and_plain_herdr_matrix() {
    let (mut app, dir) = matrix_fixture(true, false);
    let own_path = dir.join("own.toml");
    app.area = (160, 44);

    let all_19 =
        std::iter::once(crate::own_theme::NAME).chain(crate::config::THEME_NAMES.iter().copied());
    for name in all_19 {
        std::fs::write(&own_path, format!("[theme]\nname = \"{name}\"\n")).unwrap();
        app.set_plain(false);
        app.reload_config();
        let elev = app.shell.palette.elevation();
        let mut expected = if name == crate::own_theme::NAME {
            Palette::backlot()
        } else {
            Palette::from_name(name).unwrap()
        };
        if name != crate::own_theme::NAME && !elev.is_terminal_16 {
            expected.sidebar_bg = elev.surface;
        }
        assert_eq!(app.shell.palette, expected, "theme {name}");

        app.hollywood.mode = LayerMode::Collapsed;
        let collapsed = draw_to(&mut app, 160, 44);
        let layout = app.chrome_layout(160, 44);
        let buf = collapsed.buffer();
        assert_eq!(
            buf[(layout.sidebar.x, layout.sidebar.y)].bg,
            elev.surface,
            "{name}"
        );
        if elev.is_terminal_16 {
            assert!(
                buf[(layout.tab_bar.x, layout.tab_bar.y)]
                    .modifier
                    .contains(ratatui::style::Modifier::REVERSED | ratatui::style::Modifier::BOLD),
                "{name}"
            );
        } else {
            assert_eq!(
                buf[(layout.tab_bar.x, layout.tab_bar.y)].bg,
                elev.raised_row,
                "{name}"
            );
        }
        let sep_x = (layout.tab_bar.x..layout.tab_bar.right())
            .find(|&x| buf[(x, layout.tab_bar.y)].symbol() == "›")
            .unwrap_or_else(|| panic!("missing › in tab_bar for {name}"));
        assert_eq!(buf[(sep_x, layout.tab_bar.y)].bg, elev.surface, "{name}");
        let lot = app.panel_hits.area;
        assert_eq!(
            buf[(lot.x + 1, lot.bottom() - 1)].bg,
            elev.surface,
            "{name}"
        );
        assert_eq!(
            buf[(layout.pane_surface.x + 2, layout.pane_surface.y + 2)].bg,
            Color::Reset,
            "{name}"
        );

        app.hollywood.mode = LayerMode::Expanded;
        let sheet = draw_to(&mut app, 160, 44);
        let (hx, hy) = (app.hollywood_hits.area.x + 2, app.hollywood_hits.title.y);
        assert_eq!(sheet.buffer()[(hx, hy)].bg, elev.surface, "{name}");
    }

    app.hollywood.mode = LayerMode::Collapsed;
    std::fs::write(&own_path, "[theme]\nname = \"dracula\"\n").unwrap();
    app.set_plain(true);
    app.reload_config();
    let layout = app.chrome_layout(160, 44);
    assert_eq!(
        draw_to(&mut app, 160, 44).buffer()[(layout.sidebar.x, layout.sidebar.y)].bg,
        Color::Reset
    );

    app.set_plain(false);
    std::fs::write(&own_path, "[theme]\nsource = \"herdr\"\n").unwrap();
    app.reload_config();
    assert_eq!(
        draw_to(&mut app, 160, 44).buffer()[(layout.sidebar.x, layout.sidebar.y)].bg,
        Color::Reset
    );

    std::fs::write(
        &own_path,
        "[theme]\nname = \"dracula\"\n[theme.custom]\nsidebar_bg = \"#202530\"\n",
    )
    .unwrap();
    app.reload_config();
    assert_eq!(
        draw_to(&mut app, 160, 44).buffer()[(layout.sidebar.x, layout.sidebar.y)].bg,
        Color::Rgb(32, 37, 48)
    );
}
