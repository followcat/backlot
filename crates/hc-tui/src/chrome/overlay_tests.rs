//! Tests for the opaque overlay fill: backlot's look fills only the
//! cells that still carry the terminal default background; the
//! reference palettes are left alone.
use super::*;

fn buffer() -> Buffer {
    Buffer::empty(Rect::new(0, 0, 4, 2))
}

#[test]
fn the_backlot_theme_fills_default_backgrounds() {
    let p = Palette::backlot();
    let mut b = buffer();
    // A cell that already carries a real background keeps it.
    b[(0, 0)].bg = p.surface_dim;
    let area = b.area;
    fill_default_bg(&mut b, area, &p);
    for cell in &b.content {
        assert_ne!(cell.bg, Color::Reset, "a default background survived");
    }
    assert_eq!(b[(0, 0)].bg, p.surface_dim);
    assert_eq!(b[(1, 0)].bg, p.panel_bg);
}

#[test]
fn a_reference_theme_is_untouched() {
    let p = Palette::catppuccin();
    let mut b = buffer();
    let area = b.area;
    fill_default_bg(&mut b, area, &p);
    assert!(b.content.iter().all(|cell| cell.bg == Color::Reset));
}
