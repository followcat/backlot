//! Buffer-level depth, gutter/padding/rounded border, drop shadow, CJK shadow
//! seam, shrink-ladder, and `[ui]` switch toggle tests.

use ratatui::buffer::{Buffer, CellDiffOption, CellWidth};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};

use super::palette::Palette;
use super::popup_frame::{PopupChrome, dim_backdrop, overlay_bg, paint_frame, resolve_centered};
use super::wide::mark_wide_tail_slots;
use crate::config::PopupBackdrop;

fn seed_busy_buffer(area: Rect, fg: Color, bg: Color) -> Buffer {
    let mut buf = Buffer::empty(area);
    let style = Style::default().fg(fg).bg(bg);
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            buf[(x, y)].set_symbol("x").set_style(style);
        }
    }
    buf
}

#[test]
fn outside_cells_dim_and_inside_stay_bright_while_no_popup_leaves_buffer_untouched() {
    let p = Palette::backlot();
    let area = Rect::new(0, 0, 50, 16);
    let baseline = seed_busy_buffer(area, p.text, p.panel_bg);

    // Without a popup, nothing is dimmed.
    let no_popup = seed_busy_buffer(area, p.text, p.panel_bg);
    assert_eq!(no_popup.content, baseline.content);

    // Open a Standard popup: outside dims, inside stays bright elevated overlay.
    let mut buf = seed_busy_buffer(area, p.text, p.panel_bg);
    dim_backdrop(&mut buf, &p, PopupBackdrop::Dim, None);
    let geom = resolve_centered(area, 20, 4, 4, 4, PopupChrome::Standard).unwrap();
    paint_frame(&mut buf, geom, p.accent, PopupChrome::Standard, &p, true).unwrap();

    // Top-left corner outside popup + shadow is attenuated (not raw panel_bg / text).
    let out = &buf[(0, 0)];
    assert_ne!(out.fg, p.text);
    assert_ne!(out.bg, p.panel_bg);

    // Every cell inside `geom.outer` keeps overlay background and NO DIM.
    let ov = overlay_bg(&p);
    for y in geom.outer.top()..geom.outer.bottom() {
        for x in geom.outer.left()..geom.outer.right() {
            assert_eq!(buf[(x, y)].bg, ov, "({x},{y}) bg mismatch");
            assert!(!buf[(x, y)].modifier.contains(Modifier::DIM));
        }
    }
}

#[test]
fn standard_frame_renders_gutter_rounded_corners_padding_and_shadow() {
    let p = Palette::catppuccin();
    let elev = p.elevation();
    let area = Rect::new(0, 0, 40, 16);
    let mut buf = seed_busy_buffer(area, p.text, p.active_row_bg);
    dim_backdrop(&mut buf, &p, PopupBackdrop::Dim, None);
    let geom = resolve_centered(area, 20, 4, 4, 4, PopupChrome::Standard).unwrap();
    paint_frame(&mut buf, geom, p.accent, PopupChrome::Standard, &p, true).unwrap();

    // Outer 1-cell gutter + 1 border + 2/1 pad -> 20+8=28 wide, 4+6=10 tall.
    assert_eq!(geom.inner.width, 20);
    assert_eq!(geom.inner.height, 4);
    assert_eq!(geom.border.width, 26);
    assert_eq!(geom.border.height, 8);
    assert_eq!(geom.outer.width, 28);
    assert_eq!(geom.outer.height, 10);

    // Rounded corners.
    assert_eq!(buf[(geom.border.x, geom.border.y)].symbol(), "╭");
    assert_eq!(buf[(geom.border.right() - 1, geom.border.y)].symbol(), "╮");
    assert_eq!(buf[(geom.border.x, geom.border.bottom() - 1)].symbol(), "╰");
    assert_eq!(
        buf[(geom.border.right() - 1, geom.border.bottom() - 1)].symbol(),
        "╯"
    );

    // Gutter cell & padding cells are `overlay_bg`.
    let ov = overlay_bg(&p);
    assert_eq!(buf[(geom.outer.x, geom.outer.y)].symbol(), " ");
    assert_eq!(buf[(geom.outer.x, geom.outer.y)].bg, ov);
    assert_eq!(buf[(geom.border.x + 1, geom.border.y + 1)].bg, ov);
    assert_eq!(buf[(geom.border.x + 2, geom.border.y + 2)].bg, ov);

    // Drop shadow sits 1 cell right & 1 row below `geom.outer`, darker & keeping symbol.
    let shadow_bg = elev.shadow.unwrap();
    let rx = geom.outer.right();
    for y in (geom.outer.y + 1)..=geom.outer.bottom() {
        assert_eq!(buf[(rx, y)].bg, shadow_bg, "right shadow at y={y}");
        assert_eq!(buf[(rx, y)].symbol(), "x");
    }
    let by = geom.outer.bottom();
    for x in (geom.outer.x + 1)..=geom.outer.right() {
        assert_eq!(buf[(x, by)].bg, shadow_bg, "bottom shadow at x={x}");
        assert_eq!(buf[(x, by)].symbol(), "x");
    }
    // Offset corners stay backdrop-dimmed, not shadow.
    assert_ne!(buf[(geom.outer.right(), geom.outer.y)].bg, shadow_bg);
    assert_ne!(buf[(geom.outer.x, geom.outer.bottom())].bg, shadow_bg);
}

#[test]
fn cjk_text_under_gutter_border_and_shadow_edges_never_splits_half_glyph() {
    let p = Palette::backlot();
    let elev = p.elevation();
    let shadow_bg = elev.shadow.unwrap();
    let area = Rect::new(0, 0, 44, 16);

    for host_x in [0u16, 1u16] {
        let mut buf = Buffer::empty(area);
        for y in 0..area.height {
            buf.set_string(
                0,
                y,
                "甲乙丙丁戊己庚辛壬癸中文测试界面的宽度文字排版",
                Style::default().fg(p.text).bg(p.panel_bg),
            );
        }
        mark_wide_tail_slots(&mut buf);
        dim_backdrop(&mut buf, &p, PopupBackdrop::Dim, None);

        let sub = Rect::new(host_x, 0, area.width - 1, area.height);
        let geom = resolve_centered(sub, 17, 4, 4, 4, PopupChrome::Standard).unwrap();
        paint_frame(&mut buf, geom, p.accent, PopupChrome::Standard, &p, true).unwrap();
        mark_wide_tail_slots(&mut buf);

        for y in geom.outer.top()..geom.outer.bottom() {
            // Left outer gutter edge never overlaps an external wide head.
            if geom.outer.x > 0 {
                assert_eq!(buf[(geom.outer.x - 1, y)].cell_width(), 1);
            }
            // Right shadow edge (`geom.outer.right()`) is 1 col wide: never leaves a width-2 head or orphaned Skip at `right + 1`.
            if y > geom.outer.y && geom.outer.right() < area.right() {
                let sx = geom.outer.right();
                assert_eq!(buf[(sx, y)].bg, shadow_bg);
                assert_eq!(buf[(sx, y)].cell_width(), 1);
                if sx + 1 < area.right() {
                    assert_ne!(buf[(sx + 1, y)].diff_option, CellDiffOption::Skip);
                }
            }
        }
        // Bottom-right shadow corner and bottom shadow row never split CJK graphemes.
        if geom.outer.bottom() < area.bottom() {
            let sy = geom.outer.bottom();
            if geom.outer.right() < area.right() {
                let sx = geom.outer.right();
                assert_eq!(buf[(sx, sy)].bg, shadow_bg);
                assert_eq!(buf[(sx, sy)].cell_width(), 1);
            }
            assert_eq!(buf[(geom.outer.x, sy)].cell_width(), 1);
            assert_ne!(buf[(geom.outer.x + 1, sy)].bg, Color::Reset);
        }
    }
}

#[test]
fn small_terminal_shrinks_gutter_then_padding_before_content() {
    // Desired content: 24x6. Full Standard needs 32x12.
    let full =
        resolve_centered(Rect::new(0, 0, 32, 12), 24, 6, 4, 4, PopupChrome::Standard).unwrap();
    assert_eq!(full.outer.width, 32);
    assert_eq!(full.border.width, 30);
    assert_eq!(
        full.inner,
        Rect::new(full.border.x + 3, full.border.y + 2, 24, 6)
    );

    // At 30x10: gutter shrinks 1->0 first while 2/1 padding & 24x6 content survive.
    let no_gutter =
        resolve_centered(Rect::new(0, 0, 30, 10), 24, 6, 4, 4, PopupChrome::Standard).unwrap();
    assert_eq!(no_gutter.outer, no_gutter.border);
    assert_eq!(no_gutter.border, Rect::new(0, 0, 30, 10));
    assert_eq!(no_gutter.inner, Rect::new(3, 2, 24, 6));

    // At 28x9: pad_x shrinks 2->1 and pad_y shrinks 1->0; content STILL 24x6.
    let less_pad =
        resolve_centered(Rect::new(0, 0, 28, 9), 24, 6, 4, 4, PopupChrome::Standard).unwrap();
    assert_eq!(less_pad.border.width, 28);
    assert_eq!(less_pad.inner.width, 24);
    assert_eq!(less_pad.inner.height, 6);

    // At 26x8: pad_x -> 0, pad_y -> 0; content STILL 24x6.
    let no_pad =
        resolve_centered(Rect::new(0, 0, 26, 8), 24, 6, 4, 4, PopupChrome::Standard).unwrap();
    assert_eq!(no_pad.inner, Rect::new(1, 1, 24, 6));

    // At 20x6: only after gutter=0 and pad=0/0 does content shrink to 18x4.
    let tight =
        resolve_centered(Rect::new(0, 0, 20, 6), 24, 6, 4, 4, PopupChrome::Standard).unwrap();
    assert_eq!(tight.inner, Rect::new(1, 1, 18, 4));
}

#[test]
fn popup_backdrop_none_and_shadow_false_disable_each_independently() {
    let p = Palette::backlot();
    let area = Rect::new(0, 0, 40, 16);
    let base = seed_busy_buffer(area, p.text, p.panel_bg);

    let mut buf = seed_busy_buffer(area, p.text, p.panel_bg);
    dim_backdrop(&mut buf, &p, PopupBackdrop::None, None);
    let geom = resolve_centered(area, 16, 4, 4, 4, PopupChrome::Standard).unwrap();
    paint_frame(&mut buf, geom, p.accent, PopupChrome::Standard, &p, false).unwrap();

    // Outside popup + shadow cells remain completely identical to baseline.
    for y in 0..area.height {
        for x in 0..area.width {
            if !geom.outer.contains(Position::new(x, y)) {
                assert_eq!(buf[(x, y)], base[(x, y)], "({x},{y}) should stay untouched");
            }
        }
    }
}
