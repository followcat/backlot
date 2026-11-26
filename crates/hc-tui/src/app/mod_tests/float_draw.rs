//! Buffer assertions for the persistent float's frame dressing: rich
//! title, focused/unfocused border, dimmed backdrop, drop shadow, and
//! rounded corners — the ephemeral popup's plain frame must not change.
use super::*;
use crate::app::float::FloatKey;
use crate::config::{CustomCommandAction, CustomCommandKeybind};
use crate::popup_size::PopupSize;
use ratatui::widgets::Widget;

fn command_labeled(label: &str, persistent: bool) -> CustomCommandKeybind {
    CustomCommandKeybind {
        bindings: Default::default(),
        label: label.into(),
        command: "exec \"${SHELL:-sh}\"".into(),
        action: CustomCommandAction::Popup,
        description: None,
        width: Some(PopupSize::Percent(80)),
        height: Some(PopupSize::Percent(80)),
        persistent,
    }
}

fn pending(app: &mut App) {
    app.open_attach = Arc::new(|_| Box::pin(std::future::pending()));
    app.pane = pane::Pane::new(app.pane_tx.clone(), app.open_attach.clone());
}

fn float_term(
    address: &str,
    owner: &str,
    title: &str,
    created_ms: u64,
) -> hc_proto::terminal::Terminal {
    let mut term = crate::sidebar::fixture::term(address);
    term.created_ms = created_ms;
    term.labels.insert("hc-tui.popup".into(), "1".into());
    term.labels.insert("hc-tui.owner-tab".into(), owner.into());
    term.labels.insert("hc-tui.command".into(), "alt+f".into());
    term.labels.insert("herdr.title".into(), title.into());
    term
}

/// Inventory publish: existing records plus the float's labeled record.
fn publish_with_float(app: &mut App, term: hc_proto::terminal::Terminal) {
    let mut terms: Vec<_> = app.view.inventory.terminals().values().cloned().collect();
    terms.push(term);
    let seq = app.view.inventory.seq() + 1;
    app.view.inventory_mut().replace(seq, terms);
    app.reconcile_published_view();
}

/// An app with two tabs on one workspace and a shown persistent float
/// whose record reports `herdr.title = "testprog"`.
fn app_with_shown_float() -> App {
    let mut app = app_with_tabs(&[("t1", "ws-alpha", "tab-one"), ("t2", "ws-alpha", "tab-two")]);
    pending(&mut app);
    app.reconcile_published_view();
    let key = FloatKey {
        owner: app.focused_owner_key().expect("a focused tab"),
        command: "alt+f".into(),
    };
    app.run_custom_command(&command_labeled("alt+f", true));
    assert!(app.popup_pending.is_some(), "the float run is armed");
    app.on_popup_spawned("t9".into(), 7);
    assert!(app.popup.is_some(), "the float shows");
    publish_with_float(&mut app, float_term("t9", &key.owner, "testprog", 7));
    app
}

/// Draw at `w`×`h`: `frame_area` resolves against the app's recorded
/// size, and the ops-less spawn notice is cleared so the toast can't
/// cover the float's top-right corner in the assertions.
fn draw_float(app: &mut App, w: u16, h: u16) -> ratatui::backend::TestBackend {
    app.area = (w, h);
    app.notice = None;
    draw_to(app, w, h)
}

/// The first cell showing `symbol` — the rounded corner scan relies on
/// popup corners being the only ╭╮╰╯ glyphs in the frame.
fn find_cell(b: &ratatui::backend::TestBackend, symbol: &str) -> Option<(u16, u16)> {
    let (w, h) = (b.size().unwrap().width, b.size().unwrap().height);
    let buf = b.buffer();
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .find(|&(x, y)| buf[(x, y)].symbol() == symbol)
}

fn row_text(b: &ratatui::backend::TestBackend, y: u16) -> String {
    let w = b.size().unwrap().width;
    let buf = b.buffer();
    (0..w).map(|x| buf[(x, y)].symbol()).collect()
}

#[tokio::test]
async fn float_title_shows_binding_tab_program_and_right_hints() {
    let mut app = app_with_shown_float();
    let backend = draw_float(&mut app, 140, 30);
    let (x0, y0) = find_cell(&backend, "╭").expect("rounded corner");
    let top = row_text(&backend, y0);
    assert!(
        top.contains("float alt+f"),
        "title missed the binding: {top:?}"
    );
    assert!(top.contains("tab-one"), "title missed the tab: {top:?}");
    assert!(
        top.contains("testprog"),
        "title missed the program: {top:?}"
    );
    assert!(
        top.contains("alt+f hide · esc sends to program"),
        "right hint missing or misaligned: {top:?}"
    );
    let _ = x0;
}

#[tokio::test]
async fn float_border_is_accent_when_focused_and_dim_when_locked() {
    let mut app = app_with_shown_float();
    let accent = app.shell.palette.accent;
    let dim = app.shell.palette.elevation().border_muted;
    let backend = draw_float(&mut app, 100, 30);
    let (x0, y0) = find_cell(&backend, "╭").expect("rounded corner");
    assert_eq!(backend.buffer()[(x0, y0)].fg, accent, "focused border");

    app.keyboard_locked = true;
    let backend = draw_float(&mut app, 100, 30);
    let (x0, y0) = find_cell(&backend, "╭").expect("rounded corner");
    assert_eq!(
        backend.buffer()[(x0, y0)].fg,
        dim,
        "locked float kept the accent border"
    );
}

#[tokio::test]
async fn drop_shadow_sits_one_cell_right_and_below() {
    let mut app = app_with_shown_float();
    let shadow = app.shell.palette.elevation().shadow.expect("rgb shadow");
    let backend = draw_float(&mut app, 100, 30);
    let buf = backend.buffer();
    let (x0, y0) = find_cell(&backend, "╭").expect("top-left corner");
    let (x1, _) = find_cell(&backend, "╮").expect("top-right corner");
    let (_, y1) = find_cell(&backend, "╰").expect("bottom-left corner");
    // 1-cell outer gutter encloses the border box (`x0 - 1..=x1 + 1`, `y0 - 1..=y1 + 1`).
    let ox0 = x0 - 1;
    let oy0 = y0 - 1;
    let ox1 = x1 + 1;
    let oy1 = y1 + 1;

    // Right column: one cell past the outer edge, starting one row down from outer.top.
    assert_eq!(buf[(ox1 + 1, oy0 + 1)].bg, shadow, "right shadow missing");
    // Bottom row: one row below outer.bottom, offset by 1 column from outer.left.
    assert_eq!(buf[(ox0 + 1, oy1 + 1)].bg, shadow, "bottom shadow missing");
    // The diagonal corner under the bottom-right is painted too.
    assert_eq!(buf[(ox1 + 1, oy1 + 1)].bg, shadow, "shadow corner missing");
    // Stagger: the cell beside the outer top-right corner is not shadowed.
    assert_ne!(buf[(ox1 + 1, oy0)].bg, shadow, "shadow ran past the top");
}

#[tokio::test]
async fn every_bottom_corner_rests_on_the_shadow() {
    let mut app = app_with_shown_float();
    let shadow = app.shell.palette.elevation().shadow.expect("rgb shadow");
    let backend = draw_float(&mut app, 100, 30);
    let buf = backend.buffer();
    let (x0, y0) = find_cell(&backend, "╭").expect("top-left corner");
    let (x1, _) = find_cell(&backend, "╮").expect("top-right corner");
    let (_, y1) = find_cell(&backend, "╰").expect("bottom-left corner");
    let ox0 = x0 - 1;
    let oy0 = y0 - 1;
    let ox1 = x1 + 1;
    let oy1 = y1 + 1;

    // The (1, 1) offset shadow starts under `ox0 + 1` (`x0`) and covers past `ox1 + 1`.
    assert_ne!(
        buf[(ox0, oy1 + 1)].bg,
        shadow,
        "offset left corner stays unshadowed"
    );
    assert_eq!(
        buf[(x0, oy1 + 1)].bg,
        shadow,
        "bottom shadow under border left"
    );
    assert_eq!(
        buf[(ox1, oy1 + 1)].bg,
        shadow,
        "bottom shadow under gutter right"
    );
    assert_eq!(
        buf[(ox1 + 1, oy1)].bg,
        shadow,
        "right of outer bottom-right"
    );
    assert_eq!(
        buf[(ox1 + 1, oy1 + 1)].bg,
        shadow,
        "diagonal under outer bottom-right"
    );
    assert_ne!(buf[(ox1 + 1, oy0)].bg, shadow, "right of outer top-right");
    assert_ne!(buf[(ox0 - 1, oy0)].bg, shadow, "left of outer top-left");
    assert_ne!(buf[(ox0 - 1, oy1)].bg, shadow, "left of outer bottom-left");
}

#[tokio::test]
async fn backdrop_dims_the_pane_surface_around_the_float_only() {
    let mut app = app_with_shown_float();
    let elev = app.shell.palette.elevation();
    let dim_bg = elev.dim_bg.expect("rgb dim bg");
    let backend = draw_float(&mut app, 100, 30);
    let buf = backend.buffer();
    let (x0, y0) = find_cell(&backend, "╭").expect("rounded corner");

    // A pane-surface cell left of the float's outer gutter is attenuated.
    let covered = &buf[(x0 - 2, y0)];
    assert_eq!(
        covered.bg, dim_bg,
        "backdrop missed elevation dim at the float's edge"
    );
    // The float's own outer gutter & border are painted with elevated overlay bg.
    assert_eq!(buf[(x0 - 1, y0)].bg, elev.overlay, "outer gutter fill");
    assert_eq!(buf[(x0, y0)].bg, elev.overlay, "float border fill");
    // The sidebar and top tab chrome outside pane_surface stay untouched (not dim_bg / overlay).
    assert_ne!(buf[(0, y0)].bg, dim_bg, "the sidebar was dimmed");
    assert_ne!(buf[(x0, 0)].bg, dim_bg, "chrome above the float was dimmed");
}

#[tokio::test]
async fn float_corners_are_rounded() {
    let mut app = app_with_shown_float();
    let backend = draw_float(&mut app, 100, 30);
    for symbol in ["╭", "╮", "╰", "╯"] {
        assert!(
            find_cell(&backend, symbol).is_some(),
            "missing rounded corner {symbol}"
        );
    }
}

/// Render just the float block's top row at a synthetic width — exact
/// width control for the boundary cases, which Percent sizing can't
/// hit through the frame layout. The fixture title is 32 cells and
/// the hint 33, so `inner_w` fits both plus separator at exactly 70.
fn float_top_row(app: &App, w: u16) -> String {
    let outer = ratatui::layout::Rect::new(0, 0, w, 6);
    let mut buf = ratatui::buffer::Buffer::empty(outer);
    app.float_block(outer).render(outer, &mut buf);
    (0..w).map(|x| buf[(x, 0)].symbol()).collect()
}

#[tokio::test]
async fn the_hint_renders_at_the_exact_fit_boundary() {
    let app = app_with_shown_float();
    let top = float_top_row(&app, 72);
    assert!(
        top.contains("─ alt+f hide · esc sends to program ╮"),
        "hint missing or colliding at exact fit: {top:?}"
    );
    assert!(top.starts_with("╭ float alt+f"), "{top:?}");
}

#[tokio::test]
async fn the_hint_drops_one_cell_below_the_fit_boundary() {
    let app = app_with_shown_float();
    let top = float_top_row(&app, 71);
    assert!(
        !top.contains("hide"),
        "hint kept one cell below its fit: {top:?}"
    );
    assert!(top.ends_with("╮"), "the corner survived: {top:?}");
}

#[tokio::test]
async fn a_long_title_truncates_and_never_runs_into_the_corners() {
    let app = app_with_shown_float();
    let top = float_top_row(&app, 30);
    assert!(top.contains("…"), "title not ellipsized: {top:?}");
    assert!(
        top.ends_with("╮") && top.starts_with("╭"),
        "corners: {top:?}"
    );
    // The cell before ╮ stays border or padding — never title text
    // clipped flush against the corner.
    assert!(
        top.ends_with("─╮") || top.ends_with(" ╮"),
        "title ran into ╮: {top:?}"
    );
}

#[tokio::test]
async fn a_narrow_frame_drops_the_key_hint_but_keeps_the_border() {
    let mut app = app_with_shown_float();
    let backend = draw_float(&mut app, 60, 16);
    let (_, y0) = find_cell(&backend, "╭").expect("rounded corner");
    let top = row_text(&backend, y0);
    assert!(
        !top.contains("hide") && !top.contains("esc sends"),
        "the hint crowded a narrow frame: {top:?}"
    );
    assert!(top.contains("╮"), "the border lost its corner: {top:?}");
}

/// The wizard and float popups must not be windows onto the terminal
/// behind them. In backlot's own look every cell inside the frame is
/// opaque; the terminal's default background never survives.
#[tokio::test]
async fn a_float_is_opaque_in_the_backlot_theme() {
    use crate::chrome::palette::Palette;
    let mut app = app_with_shown_float();
    app.shell.palette = Palette::backlot();
    let backend = draw_float(&mut app, 140, 30);
    let (x0, y0) = find_cell(&backend, "╭").expect("the float's top-left");
    let (x1, y1) = find_cell(&backend, "╯").expect("the float's bottom-right");
    let buf = backend.buffer();
    for y in y0..=y1 {
        for x in x0..=x1 {
            assert_ne!(
                buf[(x, y)].bg,
                ratatui::style::Color::Reset,
                "default-background cell inside the float at {x},{y}"
            );
        }
    }
}

/// The reference themes keep their translucency exactly: a popup's
/// default-background cells still let the terminal behind show.
#[tokio::test]
async fn a_float_keeps_the_reference_translucency() {
    let mut app = app_with_shown_float();
    let backend = draw_float(&mut app, 140, 30);
    let (x0, y0) = find_cell(&backend, "╭").expect("the float's top-left");
    let (x1, y1) = find_cell(&backend, "╯").expect("the float's bottom-right");
    let buf = backend.buffer();
    let clear = (y0..=y1)
        .flat_map(|y| (x0..=x1).map(move |x| (x, y)))
        .any(|(x, y)| buf[(x, y)].bg == ratatui::style::Color::Reset);
    assert!(clear, "a herdr theme's popup must stay translucent");
}

#[tokio::test]
async fn ephemeral_popups_keep_the_plain_frame() {
    let mut app = app_with_tabs(&[("t1", "ws-alpha", "tab-one"), ("t2", "ws-alpha", "tab-two")]);
    pending(&mut app);
    app.reconcile_published_view();
    app.run_custom_command(&command_labeled("alt+h", false));
    assert!(app.popup_pending.is_some());
    app.on_popup_spawned("t9".into(), 7);
    let elev = app.shell.palette.elevation();
    let backend = draw_float(&mut app, 100, 30);
    let (x0, y0) = find_cell(&backend, "╭").expect("unified rounded corner");
    let buf = backend.buffer();
    assert_eq!(
        buf[(x0 - 2, y0)].bg,
        elev.dim_bg.unwrap(),
        "ephemeral popup shares unified dimmed backdrop"
    );
    assert_eq!(
        buf[(x0 - 1, y0)].bg,
        elev.overlay,
        "ephemeral popup gets 1-cell outer gutter"
    );
    assert!(row_text(&backend, y0).contains("popup"), "ephemeral title");
}
