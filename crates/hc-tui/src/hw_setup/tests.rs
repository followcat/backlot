//! Hollywood setup's draw tests — the dialog must `Clear` its rect
//! after the caller's dim pass and before the panel fill, so a dead
//! pane's frozen screen (reverse video, foreign colours) never
//! bleeds through the modal.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::{HwSetup, render};
use crate::chrome::palette::Palette;

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

/// Nothing the frozen screen left survives inside the dialog's rect.
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
fn the_dialog_clears_the_frozen_screen_behind_it() {
    let band = Rect::new(0, 0, 80, 24);
    let mut buf = Buffer::empty(band);
    frozen_screen(&mut buf);
    let palette = Palette::catppuccin();
    let hits = render(&mut buf, &HwSetup::new(None), &palette, band);
    assert!(hits.area.width > 0, "the frame must fit the band");
    assert_no_bleed(&buf, hits.area);
}

/// R3-M13: the footer's hint tiers shorten whole — a hint never
/// clips mid-word inside the 56-column dialog.
#[test]
fn the_footer_names_whole_hint_tiers_never_a_mid_word_cut() {
    let palette = Palette::catppuccin();
    let tiers = [
        "↑↓ field  ←→ value  ⏎ list · apply  e text  esc cancel",
        "↑↓ field  ←→ value  ⏎ apply  esc",
        "↑↓ · ←→ · ⏎ · esc",
    ];
    for width in [80u16, 64, 48, 36] {
        let band = Rect::new(0, 0, width, 24);
        let mut buf = Buffer::empty(band);
        let hits = render(&mut buf, &HwSetup::new(None), &palette, band);
        let text = (hits.area.y..hits.area.bottom())
            .map(|y| {
                (hits.area.x..hits.area.right())
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let footers: Vec<&str> = text.lines().filter(|line| line.contains("↑↓")).collect();
        assert!(
            footers.len() <= 1,
            "{width}: at most one hint line: {footers:?}"
        );
        if let Some(footer) = footers.first() {
            assert!(
                tiers.iter().any(|t| footer.contains(t)),
                "{width}: {footer:?} is no whole tier"
            );
            assert!(!footer.contains('…'), "{width}: clipped: {footer:?}");
        }
    }
}
