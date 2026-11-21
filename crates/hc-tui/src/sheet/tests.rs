//! The Task & Delivery Sheet's draw tests — the modal must `Clear`
//! its rect after the caller's dim pass and before the panel fill,
//! so a dead pane's frozen screen (reverse video, foreign colours)
//! never bleeds through the sheet.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::render;
use crate::chrome::palette::Palette;
use crate::sheet::Sheet;
use crate::studio::StudioKey;

/// A dead pane's frozen screen: every cell a reverse-video red `#`.
fn frozen_screen(buf: &mut Buffer) {
    let style = Style::default()
        .fg(Color::Red)
        .add_modifier(Modifier::REVERSED | Modifier::UNDERLINED);
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            buf[(x, y)].set_symbol("#").set_style(style);
        }
    }
}

/// Nothing the frozen screen left survives inside the sheet's rect.
fn assert_no_bleed(buf: &Buffer, area: Rect) {
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            let cell = &buf[(x, y)];
            assert!(
                !cell
                    .modifier
                    .intersects(Modifier::REVERSED | Modifier::UNDERLINED),
                "foreign modifier at ({x}, {y}): {:?}",
                cell.modifier
            );
            assert_ne!(cell.fg, Color::Red, "foreign colour at ({x}, {y})");
            assert_ne!(cell.symbol(), "#", "frozen glyph at ({x}, {y})");
        }
    }
}

#[test]
fn the_sheet_clears_the_frozen_screen_behind_it() {
    let band = Rect::new(0, 0, 80, 24);
    let mut buf = Buffer::empty(band);
    frozen_screen(&mut buf);
    let palette = Palette::catppuccin();
    let sheet = Sheet::new(
        StudioKey {
            machine: None,
            id: "acme-shop".into(),
        },
        None,
        0,
    );
    let rendered = render(&mut buf, band, &sheet, None, &palette);
    assert!(rendered.frame.width > 0, "the frame must fit the band");
    assert_no_bleed(&buf, rendered.frame);
}
