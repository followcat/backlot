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
