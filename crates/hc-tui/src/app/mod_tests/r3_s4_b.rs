//! Round 3 Slice 4 tests (part 2): backdrop dimming & active-cursor /
//! highlight suppression (`R3-M2`), studio toast & menu clamping
//! (`R3-M12`), and clean Backlot `Peek` title (`R3-M14`).

use std::time::Instant;

use super::r3_s4::{mouse, s4_app};
use super::*;
use crate::app::action_tracker::ActionToast;
use crate::app::input::{MouseButton, MouseEventKind};
use crate::hollywood::{CardView, LayerMode, Target as HwTarget};
use crate::panel::{PanelMode, Target as PanelTarget};
use crate::studio::StudioKey;
use crate::studio::glyphs;
use crate::studio_dialog::{ReasonFor, StudioDialog};

#[tokio::test]
async fn modals_dim_backdrop_and_suppress_underlying_cursors_and_highlights() {
    let mut app = s4_app();
    app.area = (160, 47);
    let mut vt = hc_vt::Vt::new(90, 22, 0);
    vt.feed(b"pane prompt> ");
    app.pane.install_vt(vt);

    // Both Dock and Call Sheet have active selections / editing cursor when unmodal.
    app.panel.mode = PanelMode::Expanded;
    app.panel.focused = true;
    app.panel.selected = Some(PanelTarget::Person(
        "acme-shop".into(),
        "actor-alice".into(),
    ));
    app.hollywood.mode = LayerMode::Expanded;
    app.hollywood.selected = Some(HwTarget::Input);
    app.hollywood.editing = true;

    let base = draw_to(&mut app, 160, 47);
    let alice_row_y = app
        .panel_hits
        .rows
        .iter()
        .find(|(_, t)| *t == PanelTarget::Person("acme-shop".into(), "actor-alice".into()))
        .unwrap()
        .0
        .y;
    let dock_x = app.panel_hits.area.x;
    let palette = app.shell.palette.clone();
    assert_eq!(
        base.buffer()[(dock_x + 1, alice_row_y)].symbol(),
        glyphs::SELECTION_BAR
    );
    assert_eq!(
        base.buffer()[(dock_x + 4, alice_row_y)].bg,
        palette.selection_bg
    );
    assert!(app.hollywood_hits.cursor.is_some());

    let assert_suppressed = |app: &mut App, label: &str| {
        let drawn = draw_to(app, 160, 47);
        assert!(app.hide_pane_cursor(160), "{label}: pane cursor hidden");
        assert!(
            app.hollywood_hits.cursor.is_none(),
            "{label}: Call Sheet edit cursor suppressed"
        );
        let buf = drawn.buffer();
        // Underlying Dock `▌` bar and `selection_bg` suppressed (and DIM set).
        assert_ne!(
            buf[(dock_x + 1, alice_row_y)].symbol(),
            glyphs::SELECTION_BAR,
            "{label}: Dock selection bar suppressed"
        );
        assert_ne!(
            buf[(dock_x + 4, alice_row_y)].bg,
            palette.selection_bg,
            "{label}: Dock selection_bg suppressed"
        );
        let mut probe = ratatui::buffer::Cell::default();
        probe.set_bg(palette.panel_bg);
        palette.elevation().dim_cell(&mut probe, &palette);
        assert_eq!(
            buf[(dock_x + 4, alice_row_y)].bg,
            probe.bg,
            "{label}: backdrop dimmed"
        );
        // Call Sheet `›` input row drops accent `›` and `▌` bar (whether
        // visible in backdrop or cleared under a wide modal).
        let cs_input = app.hollywood_hits.input;
        assert_ne!(
            buf[(app.hollywood_hits.area.x, cs_input.y)].symbol(),
            "▌",
            "{label}: Call Sheet selection bar suppressed"
        );
        drawn
    };

    // 1. StudioDialog::Reason — single cursor wish on Reason field.
    app.studio_dialog = Some(StudioDialog::reason(
        "Pause task t1 · acme-shop".into(),
        true,
        ReasonFor::TaskVerb {
            key: StudioKey::local("acme-shop"),
            task: "t1".into(),
            verb: "pause",
        },
    ));
    let _ = assert_suppressed(&mut app, "StudioDialog::Reason");
    let host_cursor = app.resolve_host_cursor();
    assert!(host_cursor.visible);
    let reason_area = app.studio_dialog_hits.area;
    assert!(reason_area.contains((host_cursor.x, host_cursor.y).into()));
    app.studio_dialog = None;

    // 2. Task Sheet — zero stray cursors, highlights suppressed.
    app.sheet_open(StudioKey::local("acme-shop"), None);
    let _ = assert_suppressed(&mut app, "Task Sheet");
    assert!(!app.resolve_host_cursor().visible);
    app.sheet = None;

    // 3. New Studio — highlights suppressed.
    app.new_studio_open(None);
    let _ = assert_suppressed(&mut app, "New Studio");
    app.new_studio = None;

    // 4. Reader (CardView) — zero stray cursors, highlights suppressed.
    app.hollywood.view = Some(CardView {
        card: Some("c3".into()),
        title: "c3".into(),
        lines: vec!["body".into()],
        scroll: 0,
        ..CardView::default()
    });
    let _ = assert_suppressed(&mut app, "Reader");
    assert!(!app.resolve_host_cursor().visible);
    app.hollywood.view = None;
}

#[tokio::test]
async fn toast_and_lot_pill_menu_clamp_inside_pane_surface_and_below_callsheet() {
    let mut app = s4_app();
    app.area = (160, 47);
    app.panel.mode = PanelMode::Expanded;
    let dock_w = app.dock_reserve(160);
    assert_eq!(dock_w, 38);

    // 1. ActionToast clamps left of beside-Dock and below expanded Call Sheet.
    app.hollywood.mode = LayerMode::Expanded;
    app.action_toast = Some(ActionToast {
        title: "paused task t1 on acme-shop".into(),
        body: String::new(),
        fail: false,
        command: None,
        start_stage: None,
        deadline: Instant::now() + std::time::Duration::from_secs(5),
    });
    draw_to(&mut app, 160, 47);
    let toast = app.action_toast_hit;
    assert!(!toast.is_empty());
    assert!(toast.x >= 25, "toast {toast:?} stays right of sidebar");
    assert!(
        toast.right() <= 160 - dock_w,
        "toast {toast:?} stays left of beside-Dock"
    );
    assert!(
        toast.y >= app.hollywood_hits.area.bottom(),
        "toast {toast:?} renders below expanded Call Sheet {:?}",
        app.hollywood_hits.area
    );

    // 2. Lot Pill right-click menu stays left of beside-Dock (never crosses x=122).
    app.hollywood.mode = LayerMode::Collapsed;
    app.action_toast = None;
    draw_to(&mut app, 160, 47);
    let pill = app.hit_map.lot_pill;
    assert!(!pill.is_empty());
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Right),
        pill.right().saturating_sub(1),
        pill.y,
    ));
    assert!(app.menu.is_some());
    draw_to(&mut app, 160, 47);
    assert!(!app.hit_map.context_menu_rows.is_empty());
    for (row_rect, _) in &app.hit_map.context_menu_rows {
        assert!(
            row_rect.right() < 160 - dock_w,
            "menu row {row_rect:?} bleeds past non-dock width {}",
            160 - dock_w
        );
    }
}

#[tokio::test]
async fn backlot_peek_renders_clean_studio_title_while_plain_peek_is_untouched() {
    let mut app = s4_app();
    app.area = (160, 47);
    app.panel.mode = PanelMode::Expanded;

    // 1. Backlot studio role peek from Call Sheet (`p` on prompt row):
    // clean `<studio> · <role>` title, no `enter keeps it as a tab`, dimmed backdrop.
    app.hollywood.mode = LayerMode::Expanded;
    app.hollywood.selected = Some(HwTarget::Prompt(
        StudioKey::local("acme-shop"),
        "actor-alice".into(),
    ));
    assert!(app.hollywood_row_key(crossterm::event::KeyCode::Char('p')));
    let drawn = draw_to(&mut app, 160, 47);
    let text = screen_lines(&drawn).join("\n");
    assert!(text.contains("acme-shop · alice"), "{text}");
    assert!(
        !text.contains("enter keeps it as a tab"),
        "Backlot peek omits contradictory Herdr subtitle: {text}"
    );
    let mut probe = ratatui::buffer::Cell::default();
    probe.set_bg(app.shell.palette.sidebar_bg);
    app.shell
        .palette
        .elevation()
        .dim_cell(&mut probe, &app.shell.palette);
    assert_eq!(
        drawn.buffer()[(0, 0)].bg,
        probe.bg,
        "Backlot peek dims backdrop"
    );

    // 2. Remote studio peek qualifies `· <machine>`.
    let title = app.studio_peek_title(
        &StudioKey {
            machine: Some("host-b".into()),
            id: "beta-tools".into(),
        },
        "alice",
    );
    assert_eq!(title, "beta-tools · host-b · alice");

    // 3. Plain Herdr peek (`open_peek` directly) keeps Herdr's subtitle & respects `popup_backdrop`.
    app.close_peek();
    app.hollywood.mode = LayerMode::Hidden;
    app.panel.mode = PanelMode::Closed;
    app.set_plain(true);
    app.reconcile_published_view();
    app.area = (120, 36);
    app.open_peek("t1".into());
    let plain_drawn = draw_to(&mut app, 120, 36);
    let plain_text = screen_lines(&plain_drawn).join("\n");
    assert!(
        plain_text.contains("enter keeps it as a tab, esc closes"),
        "{plain_text}"
    );
    let mut plain_probe = ratatui::buffer::Cell::default();
    plain_probe.set_bg(app.shell.palette.sidebar_bg);
    app.shell
        .palette
        .elevation()
        .dim_cell(&mut plain_probe, &app.shell.palette);
    assert_eq!(plain_drawn.buffer()[(0, 0)].bg, plain_probe.bg);
    app.shell.popup_backdrop = crate::config::PopupBackdrop::None;
    let plain_no_dim = draw_to(&mut app, 120, 36);
    assert_eq!(
        plain_no_dim.buffer()[(0, 0)].bg,
        app.shell.palette.sidebar_bg
    );
}

#[tokio::test]
async fn popup_backdrop_none_and_shadow_false_and_hit_area_gutter_vs_shadow() {
    let mut app = s4_app();
    app.area = (160, 47);
    let base = draw_to(&mut app, 160, 47);
    let elev = app.shell.palette.elevation();
    let shadow_bg = elev.shadow.expect("rgb shadow");

    // 1. Default depth on launcher: dim backdrop + gutter + shadow; hit rect includes gutter, not shadow.
    app.help = true;
    let on = draw_to(&mut app, 160, 47);
    let help_area = app.help_hits.area;
    assert_ne!(on.buffer()[(0, 0)].bg, base.buffer()[(0, 0)].bg);
    assert_eq!(
        on.buffer()[(help_area.right(), help_area.y + 1)].bg,
        shadow_bg
    );

    // 2. [ui] popup_backdrop = "none" & popup_shadow = false disable dim and drop shadow.
    app.shell.popup_backdrop = crate::config::PopupBackdrop::None;
    app.shell.popup_shadow = false;
    let off = draw_to(&mut app, 160, 47);
    assert_eq!(off.buffer()[(0, 0)].bg, base.buffer()[(0, 0)].bg);
    assert_ne!(
        off.buffer()[(help_area.right(), help_area.y + 1)].bg,
        shadow_bg
    );

    // 3. Delta 4 hit area on Reader: clicking gutter stays open; clicking shadow dismisses.
    app.help = false;
    app.shell.popup_backdrop = crate::config::PopupBackdrop::Dim;
    app.shell.popup_shadow = true;
    app.hollywood.mode = LayerMode::Expanded;
    app.hollywood.view = Some(crate::hollywood::CardView {
        card: None,
        title: "acme-shop #1".into(),
        lines: vec!["body".into()],
        ..crate::hollywood::CardView::default()
    });
    draw_to(&mut app, 160, 47);
    let reader_area = app.hollywood_hits.view.as_ref().expect("reader").area;
    // Left gutter column is inside `reader_area` -> click must NOT dismiss.
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        reader_area.x,
        reader_area.y,
    ));
    assert!(
        app.hollywood.view.is_some(),
        "gutter click must stay inside modal hit box"
    );
    // Right drop shadow sits at `reader_area.right()` -> outside hit box -> click dismisses.
    app.on_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        reader_area.right(),
        reader_area.y + 1,
    ));
    assert!(
        app.hollywood.view.is_none(),
        "shadow click must dismiss modal"
    );
}
