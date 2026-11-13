//! Buffer assertions for the persistent float's frame dressing: rich
//! title, focused/unfocused border, dimmed backdrop, drop shadow, and
//! rounded corners — the ephemeral popup's plain frame must not change.
use super::*;
use crate::app::float::FloatKey;
use crate::config::{CustomCommandAction, CustomCommandKeybind};
use crate::popup_size::PopupSize;
use ratatui::style::Modifier;

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
    app.view.inventory.replace(seq, terms);
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
    let dim = app.shell.palette.surface_dim;
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
    let shadow = app.shell.palette.surface_dim;
    let backend = draw_float(&mut app, 100, 30);
    let buf = backend.buffer();
    let (x0, y0) = find_cell(&backend, "╭").expect("top-left corner");
    let (x1, _) = find_cell(&backend, "╮").expect("top-right corner");
    let (_, y1) = find_cell(&backend, "╰").expect("bottom-left corner");

    // Right column: one cell past the right edge, starting one row down.
    assert_eq!(buf[(x1 + 1, y0 + 2)].bg, shadow, "right shadow missing");
    // Bottom row: one row below, starting one column over.
    assert_eq!(buf[(x0 + 2, y1 + 1)].bg, shadow, "bottom shadow missing");
    // The diagonal corner under the bottom-right is painted too.
    assert_eq!(buf[(x1 + 1, y1 + 1)].bg, shadow, "shadow corner missing");
    // Stagger: the cell beside the top-right corner is not shadowed,
    // nor is the cell below the bottom-left corner's own column.
    assert_ne!(buf[(x1 + 1, y0)].bg, shadow, "shadow ran past the top");
    assert_ne!(buf[(x0, y1 + 1)].bg, shadow, "bottom shadow ran left");
}

#[tokio::test]
async fn backdrop_dims_the_pane_surface_around_the_float_only() {
    let mut app = app_with_shown_float();
    let backend = draw_float(&mut app, 100, 30);
    let buf = backend.buffer();
    let (x0, y0) = find_cell(&backend, "╭").expect("rounded corner");

    // A pane-surface cell left of the float's frame is dimmed.
    let covered = &buf[(x0 - 1, y0)];
    assert!(
        covered.modifier.contains(Modifier::DIM),
        "backdrop missed DIM at the float's edge"
    );
    // The float's own cells are repainted undimmed.
    let inside = &buf[(x0, y0)];
    assert!(
        !inside.modifier.contains(Modifier::DIM),
        "the float frame itself was dimmed"
    );
    // The sidebar is chrome, not backdrop: untouched.
    let sidebar = &buf[(0, y0)];
    assert!(
        !sidebar.modifier.contains(Modifier::DIM),
        "the sidebar was dimmed"
    );
    // The tab bar row above the pane surface is untouched too.
    let chrome = &buf[(x0, 0)];
    assert!(
        !chrome.modifier.contains(Modifier::DIM),
        "chrome above the float was dimmed"
    );
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

#[tokio::test]
async fn ephemeral_popups_keep_the_plain_frame() {
    let mut app = app_with_tabs(&[("t1", "ws-alpha", "tab-one"), ("t2", "ws-alpha", "tab-two")]);
    pending(&mut app);
    app.reconcile_published_view();
    app.run_custom_command(&command_labeled("alt+h", false));
    assert!(app.popup_pending.is_some());
    app.on_popup_spawned("t9".into(), 7);
    let backend = draw_float(&mut app, 100, 30);
    assert!(
        find_cell(&backend, "╭").is_none(),
        "an ephemeral popup got rounded corners"
    );
    let (x0, y0) = find_cell(&backend, "┌").expect("square corner");
    let buf = backend.buffer();
    assert!(
        !buf[(x0 - 1, y0)].modifier.contains(Modifier::DIM),
        "an ephemeral popup dimmed its backdrop"
    );
    assert!(row_text(&backend, y0).contains("popup"), "ephemeral title");
}
