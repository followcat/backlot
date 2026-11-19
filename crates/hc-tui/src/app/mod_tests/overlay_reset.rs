//! R2 follow-up — an S5 popup must reset every cell it covers: a
//! patch-style fill would keep the modifiers of whatever drew
//! underneath (the sheet's italic `› tell hollywood` row showed
//! through the launcher's text). Each test stains the buffer with
//! foreign modifiers, renders, and asserts no covered cell keeps
//! one.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::help_launcher::s5_app;

fn stain(b: &mut Buffer, area: Rect) {
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            b[(x, y)].set_symbol("x").set_style(
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Red)
                    .add_modifier(Modifier::ITALIC | Modifier::UNDERLINED | Modifier::BOLD),
            );
        }
    }
}

/// The launcher never sets italic or underline — any covered cell
/// still carrying one inherited it from the stain. Bold alone can't
/// distinguish stain from the launcher's own bold, so instead assert
/// some covered cell has no bold at all: without the reset every
/// stained cell would stay bold.
fn assert_clean(b: &Buffer, rect: Rect) {
    let mut unbolded = false;
    for y in rect.y..rect.bottom() {
        for x in rect.x..rect.right() {
            let cell = &b[(x, y)];
            assert!(
                !cell
                    .modifier
                    .intersects(Modifier::ITALIC | Modifier::UNDERLINED),
                "stale modifier under ({x},{y}): {:?}",
                cell.modifier
            );
            unbolded |= !cell.modifier.contains(Modifier::BOLD);
            assert_ne!(cell.bg, Color::Red, "stained bg kept at ({x},{y})");
        }
    }
    assert!(unbolded, "every covered cell kept the stained bold");
}

#[tokio::test]
async fn the_launcher_resets_the_cells_it_covers() {
    let mut app = s5_app();
    app.help = true;
    let groups = app.help_groups();
    let legend = app.help_legend();
    let palette = crate::chrome::palette::Palette::backlot();
    let area = Rect::new(0, 0, 120, 36);
    let mut b = Buffer::empty(area);
    stain(&mut b, area);
    let render = crate::chrome::help_launcher::render_launcher(
        &mut b,
        area,
        &app.help_state,
        &groups,
        &legend,
        &palette,
    )
    .expect("the launcher fits at 120x36");
    assert_clean(&b, render.area);
}

#[tokio::test]
async fn the_picker_resets_the_cells_it_covers() {
    let mut app = s5_app();
    app.help = true;
    let palette = crate::chrome::palette::Palette::backlot();
    let area = Rect::new(0, 0, 120, 36);
    let mut b = Buffer::empty(area);
    stain(&mut b, area);
    let mut hits = crate::chrome::help::HelpRender::default();
    crate::chrome::help_picker::render_picker(
        &mut b,
        area,
        &crate::help_launcher::HelpPicker {
            title: "Pause task…".into(),
            rows: vec![crate::help_launcher::PickerRow {
                label: "Pause task t1".into(),
                menu: crate::studio_menu::StudioMenu::lot(),
                act: crate::help_launcher::PickerAct::Sheet,
            }],
            selected: 0,
        },
        &mut hits,
        &palette,
    );
    assert!(!hits.picker.is_empty(), "the picker drew");
    assert_clean(&b, hits.picker);
}

#[tokio::test]
async fn the_menu_panel_resets_the_cells_it_covers() {
    let _app = s5_app();
    let area = Rect::new(0, 0, 40, 12);
    let mut b = Buffer::empty(area);
    stain(&mut b, area);
    let palette = crate::chrome::palette::Palette::backlot();
    let menu = crate::chrome::menu::ContextMenu {
        target: crate::chrome::menu::ContextMenuTarget::Studio(Box::new(
            crate::studio_menu::StudioMenu::lot(),
        )),
        x: 2,
        y: 2,
        highlighted: 0,
    };
    let render = crate::chrome::menu::render_context_menu(&mut b, &menu, &palette)
        .expect("the lot menu fits at 40x12");
    // The render reports the inner rows — the panel covers them plus
    // one border cell on every side.
    let first = render.rows[0].0;
    let last = render.rows[render.rows.len() - 1].0;
    let rect = Rect::new(
        first.x - 1,
        first.y - 1,
        first.width + 2,
        last.y - first.y + 3,
    );
    assert_clean(&b, rect);
}
