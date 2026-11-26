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
    let dracula_surface = app.shell.palette.elevation().surface;
    assert_ne!(dracula_surface, Color::Reset);
    let backend = draw_to(&mut app, 120, 40);
    let layout = app.chrome_layout(120, 40);
    let buf = backend.buffer();
    assert_eq!(
        buf[(layout.sidebar.x, layout.sidebar.y)].bg,
        dracula_surface
    );
    assert_eq!(
        buf[(layout.tab_bar.x, layout.tab_bar.y)].bg,
        dracula_surface
    );
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
        custom_backend.buffer()[(layout.sidebar.x, layout.sidebar.y)].bg,
        Color::Rgb(20, 24, 34)
    );
}
