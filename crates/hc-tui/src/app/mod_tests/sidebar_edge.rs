//! The backlot theme's opaque sidebar and its edge rule, through the
//! real draw path; the herdr palette keeps its translucent look.
use super::*;
use crate::chrome::palette::Palette;
use ratatui::style::Color;

fn sidebar_cells(app: &mut App) -> (ratatui::layout::Rect, ratatui::backend::TestBackend) {
    let backend = draw_to(app, 120, 40);
    let layout = app.chrome_layout(120, 40);
    (layout.sidebar, backend)
}

#[tokio::test]
async fn the_backlot_sidebar_is_opaque_with_an_edge() {
    let mut app = app_with(&["t1"]);
    let p = Palette::backlot();
    app.shell.palette = p.clone();
    let elev = p.elevation();
    let (sidebar, backend) = sidebar_cells(&mut app);
    assert!(sidebar.width > 1, "a sidebar to inspect: {sidebar:?}");
    let buffer = backend.buffer();
    for y in sidebar.y..sidebar.bottom() {
        for x in sidebar.x..sidebar.right().saturating_sub(1) {
            assert_ne!(
                buffer[(x, y)].bg,
                Color::Reset,
                "transparent sidebar cell at {x},{y}"
            );
        }
        let edge = &buffer[(sidebar.right() - 1, y)];
        assert_eq!(edge.symbol(), "│", "edge at row {y}");
        assert_eq!(edge.fg, elev.border_muted, "edge colour at row {y}");
    }
    // The default backlot sidebar joins `elevation.surface`; custom overrides stay honoured.
    assert_eq!(buffer[(sidebar.x, sidebar.y)].bg, elev.surface);

    let mut custom = p.clone();
    custom.sidebar_bg = Color::Rgb(20, 24, 34);
    app.shell.palette = custom.clone();
    let (c_sidebar, c_backend) = sidebar_cells(&mut app);
    assert_eq!(
        c_backend.buffer()[(c_sidebar.x, c_sidebar.y)].bg,
        custom.sidebar_bg
    );
}

#[tokio::test]
async fn the_herdr_sidebar_is_unchanged() {
    let mut app = app_with(&["t1"]);
    let p = app.shell.palette.clone();
    assert_eq!(p.sidebar_bg, Color::Reset, "a herdr theme still defaults");
    let (sidebar, backend) = sidebar_cells(&mut app);
    let buffer = backend.buffer();
    // The edge keeps the reference's separator colour and the base
    // stays the terminal's own background.
    for y in sidebar.y..sidebar.bottom() {
        let edge = &buffer[(sidebar.right() - 1, y)];
        assert_eq!(edge.symbol(), "│");
        assert_eq!(edge.fg, p.surface_dim, "edge at row {y}");
    }
    assert_eq!(buffer[(sidebar.x, sidebar.y)].bg, Color::Reset);
}

#[tokio::test]
async fn studio_chrome_unifies_rgb_themes_and_respects_plain_and_source_herdr() {
    let dir = crate::test_support::TestDir::new("studio-chrome-opacity");
    let herdr_path = dir.join("herdr.toml");
    let own_path = dir.join("own.toml");
    std::fs::write(&herdr_path, "[theme]\nname = \"dracula\"\n").unwrap();
    std::fs::write(&own_path, "[theme]\nname = \"dracula\"\n").unwrap();

    // 1. `!plain && source != "herdr"` unifies RGB themes (dracula) onto `elev.surface`
    // across left sidebar and top `tabs.rs`, leaving center pane background `Color::Reset`.
    let mut app = app_with(&["t1"]);
    app.shell.local_config_path = herdr_path.clone();
    app.shell.own_theme = crate::own_theme::Shell::at(own_path.clone());
    app.set_plain(false);
    app.reload_config();
    let dracula_elev = app.shell.palette.elevation();
    let dracula_surface = dracula_elev.surface;
    let dracula_well = dracula_elev.stage_well;
    assert_ne!(dracula_surface, Color::Reset);
    let backend = draw_to(&mut app, 120, 40);
    let layout = app.chrome_layout(120, 40);
    let buf = backend.buffer();
    assert_eq!(buf[(layout.sidebar.x, layout.sidebar.y)].bg, dracula_well);
    assert_eq!(
        buf[(layout.sidebar.x, layout.sidebar.y + 1)].bg,
        dracula_surface
    );
    assert_eq!(buf[(layout.tab_bar.x, layout.tab_bar.y)].bg, dracula_well);
    assert_eq!(
        buf[(layout.pane_surface.x + 2, layout.pane_surface.y + 2)].bg,
        Color::Reset
    );
    // `? keys` sits at bottom-left of left sidebar and opens Help on click.
    let help_hit = app.hit_map.sidebar_help;
    assert!(!help_hit.is_empty());
    assert!(screen_lines(&backend).last().unwrap().contains("? keys"));
    app.on_mouse(crate::app::input::MouseInput {
        kind: crate::app::input::MouseEventKind::Down(crate::app::input::MouseButton::Left),
        col: help_hit.x,
        row: help_hit.y,
        mods: hc_vt::Mods::NONE,
        sgr_position: None,
    });
    assert!(app.help);
    app.help = false;

    // 2. `--plain` reverts sidebar/top bar back to Herdr translucency & `panel_bg`.
    app.set_plain(true);
    let plain_backend = draw_to(&mut app, 120, 40);
    let plain_buf = plain_backend.buffer();
    assert_eq!(
        plain_buf[(layout.sidebar.x, layout.sidebar.y)].bg,
        Color::Reset
    );
    assert_eq!(
        plain_buf[(layout.tab_bar.x, layout.tab_bar.y)].bg,
        Palette::dracula().panel_bg
    );
    assert!(app.hit_map.sidebar_help.is_empty());

    // 3. Explicit `[theme] source = "herdr"` preserves Herdr sidebar translucency even when !plain.
    app.set_plain(false);
    std::fs::write(&own_path, "[theme]\nsource = \"herdr\"\n").unwrap();
    app.reload_config();
    let herdr_backend = draw_to(&mut app, 120, 40);
    assert_eq!(
        herdr_backend.buffer()[(layout.sidebar.x, layout.sidebar.y)].bg,
        Color::Reset
    );
    assert_eq!(
        herdr_backend.buffer()[(layout.tab_bar.x, layout.tab_bar.y)].bg,
        Palette::dracula().panel_bg
    );

    // 4. Explicit `[theme.custom] sidebar_bg` override is preserved.
    std::fs::write(
        &own_path,
        "[theme]\nname = \"dracula\"\n[theme.custom]\nsidebar_bg = \"#141822\"\n",
    )
    .unwrap();
    app.reload_config();
    let custom_backend = draw_to(&mut app, 120, 40);
    assert_eq!(
        custom_backend.buffer()[(layout.sidebar.x, layout.sidebar.y + 1)].bg,
        Color::Rgb(20, 24, 34)
    );
    assert_eq!(
        custom_backend.buffer()[(layout.sidebar.right() - 1, layout.sidebar.y + 1)].fg,
        app.shell.palette.elevation().gilt
    );
}

#[tokio::test]
async fn s4_sidebar_header_footer_finished_row_and_herdr_seam_parity() {
    let mut app = super::studio_machines::two_machine_studio_app();
    app.area = (120, 48);
    app.shell.mouse_capture = true;
    // Mark beta-tools and a long CJK studio as finished locally so the 26-col sidebar renders both cleanly.
    let finished_doc = serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 3,
        "studios": [
            {
                "id": "beta-tools",
                "title": "Beta tools",
                "state": "finished",
                "requirement_version": 2,
                "mode": "standard",
                "open_reviews": 0,
                "waiting": 0,
                "tasks_done": 2,
                "tasks_total": 2,
                "roles": [{ "role": "producer", "name": "producer", "state": "closed", "resumable": true }]
            },
            {
                "id": "中文已完结工作室长名称",
                "title": "中文已完结工作室长名称",
                "state": "finished",
                "requirement_version": 1,
                "mode": "standard",
                "open_reviews": 0,
                "waiting": 0,
                "tasks_done": 1,
                "tasks_total": 1,
                "roles": [{ "role": "producer", "name": "producer", "state": "closed", "resumable": true }]
            }
        ]
    });
    app.view.documents_mut().insert(
        hc_proto::watch::DocKey {
            machine: None,
            name: "backlot".into(),
        },
        hc_proto::watch::Document {
            value: finished_doc,
            ..Default::default()
        }
        .into(),
    );
    app.reconcile_published_view();
    app.studio.finished_open.insert(None);

    let backend = draw_to(&mut app, 120, 48);
    let buf = backend.buffer();
    let pal = &app.shell.palette;
    let elev = pal.elevation();
    let layout = app.chrome_layout(120, 48);
    let rows: Vec<String> = (0..48)
        .map(|y| {
            let mut s = String::new();
            let mut x = 0u16;
            while x < 120 {
                let sym = buf[(x, y)].symbol();
                s.push_str(sym);
                x = x.saturating_add(crate::chrome::text::display_width(sym).max(1) as u16);
            }
            s.trim_end().to_string()
        })
        .collect();

    // Header row 0: ` ✦ MACHINES      v<version>` on `stage_well`.
    let head = &rows[0];
    assert!(head.starts_with(" ✦ MACHINES"), "{head:?}");
    assert_eq!(buf[(0, 0)].bg, elev.stage_well);
    assert_eq!(buf[(1, 0)].fg, pal.accent);
    assert_eq!(buf[(3, 0)].fg, pal.subtext0);
    assert!(
        buf[(3, 0)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
    let ver = format!("v{}", env!("CARGO_PKG_VERSION"));
    let ver_x = head
        .find(&ver)
        .map(|b| crate::chrome::text::display_width(&head[..b]) as u16)
        .unwrap();
    assert_eq!(buf[(ver_x, 0)].fg, elev.muted_surface);
    assert_eq!(buf[(ver_x, 0)].bg, elev.stage_well);

    // Right seam x = 25 is `│` in `elev.gilt`.
    assert_eq!(buf[(layout.sidebar.right() - 1, 1)].symbol(), "│");
    assert_eq!(buf[(layout.sidebar.right() - 1, 1)].fg, elev.gilt);

    // Footer action `+ new · Local      menu` with `+` in `accent`.
    let footer_y = rows.iter().position(|r| r.contains("+ new ·")).unwrap() as u16;
    assert_eq!(buf[(0, footer_y)].symbol(), "+");
    assert_eq!(buf[(0, footer_y)].fg, pal.accent);
    assert!(rows[footer_y as usize].contains("menu"));

    // Finished rows in 26-col sidebar stay compact (`✦ <id>` in dim gilt) & clip CJK safely.
    let fin_y = rows
        .iter()
        .position(|r| r.contains("✦ beta-tools") || r.contains("✦ 中文"))
        .unwrap() as u16;
    assert!(
        rows[fin_y as usize].contains("✦ "),
        "{}",
        rows[fin_y as usize]
    );
    assert_eq!(buf[(3, fin_y)].fg, elev.gilt);
    assert!(
        buf[(3, fin_y)]
            .modifier
            .contains(ratatui::style::Modifier::DIM)
    );
    let cjk_y = rows.iter().position(|r| r.contains("✦ 中文")).unwrap() as u16;
    assert!(crate::chrome::text::display_width(&rows[cjk_y as usize][..]) <= 120);
    if std::env::var_os("BABYLON_LOT_DUMP").is_some() {
        let sb: Vec<String> = (0..22u16)
            .map(|y| {
                let mut out = String::new();
                let mut x = 0u16;
                while x < layout.sidebar.width {
                    let sym = buf[(x, y)].symbol();
                    out.push_str(sym);
                    x = x.saturating_add(crate::chrome::text::display_width(sym).max(1) as u16);
                }
                out
            })
            .collect();
        eprintln!(
            "=== 26-COL SIDEBAR (MACHINES + CAST) ===\n{}",
            sb.join("\n")
        );
    }
}
