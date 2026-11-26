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

#[test]
fn cjk_text_straddling_popup_edges_blanks_halves_and_keeps_borders() {
    use ratatui::buffer::{CellDiffOption, CellWidth};
    use ratatui::style::Style;
    let p = Palette::backlot();
    let dim_style = Style::default()
        .fg(Color::Rgb(110, 114, 124))
        .bg(Color::Rgb(14, 16, 23));

    for offset in [10u16, 11u16] {
        let area = Rect::new(0, 0, 50, 9);
        let mut buf = Buffer::empty(area);
        for y in 0..area.height {
            buf.set_string(
                0,
                y,
                "甲乙丙丁戊己庚辛壬癸中文测试界面的宽度文字排版处理",
                dim_style,
            );
        }
        // Mark tail slots first as the live terminal draw pass does.
        super::super::wide::mark_wide_tail_slots(&mut buf);

        let pop = Rect::new(offset, 2, 21, 5);
        let geom = super::super::popup_frame::PopupGeometry {
            outer: pop,
            border: pop,
            inner: Rect::new(pop.x + 1, pop.y + 1, pop.width - 2, pop.height - 2),
        };
        let painted = super::super::popup_frame::paint_frame(
            &mut buf,
            geom,
            p.accent,
            super::super::popup_frame::PopupChrome::BorderJoined,
            &p,
            false,
        );
        assert!(painted.is_some());
        super::super::wide::mark_wide_tail_slots(&mut buf);

        for y in pop.top()..pop.bottom() {
            // Border column must always be the border glyph and never Skipped.
            let left_border = &buf[(pop.x, y)];
            assert_ne!(
                left_border.diff_option,
                CellDiffOption::Skip,
                "offset={offset} y={y}: left border was skipped by left wide head"
            );
            assert!(
                matches!(left_border.symbol(), "┌" | "│" | "└" | "╭" | "╰"),
                "offset={offset} y={y}: left border symbol={:?}",
                left_border.symbol()
            );
            // Outside left: no wide glyph extends across pop.x.
            let left_prev = &buf[(pop.x - 1, y)];
            assert_eq!(
                left_prev.cell_width(),
                1,
                "offset={offset} y={y}: outside left head width={} sym={:?}",
                left_prev.cell_width(),
                left_prev.symbol()
            );
            if offset % 2 == 1 {
                assert_eq!(left_prev.symbol(), " ");
                assert_eq!(left_prev.fg, dim_style.fg.unwrap());
                assert_eq!(left_prev.bg, dim_style.bg.unwrap());
            }
            // Outside right: never an orphaned tail Skip; when right() cut a
            // wide char (even offset), the outside tail slot is blanked to " ".
            let right_next = &buf[(pop.right(), y)];
            assert_ne!(
                right_next.diff_option,
                CellDiffOption::Skip,
                "offset={offset} y={y}: orphaned tail skip at right edge"
            );
            if offset % 2 == 0 {
                assert_eq!(
                    right_next.cell_width(),
                    1,
                    "offset={offset} y={y}: right outside straddled tail width != 1"
                );
                assert_eq!(
                    right_next.symbol(),
                    " ",
                    "offset={offset} y={y}: stale glyph in right outside tail"
                );
            } else {
                assert_eq!(right_next.symbol(), "的");
                assert_eq!(right_next.cell_width(), 2);
            }
        }
    }
}

#[test]
fn cjk_text_straddling_hollywood_reader_and_shadow_edges_is_handled_whole() {
    use ratatui::buffer::{CellDiffOption, CellWidth};
    use ratatui::style::Style;
    let p = Palette::backlot();
    let dim_style = Style::default()
        .fg(Color::Rgb(110, 114, 124))
        .bg(Color::Rgb(14, 16, 23));

    for host_x in [0u16, 1u16] {
        let full = Rect::new(0, 0, 60, 16);
        let host = Rect::new(host_x, 0, 56, 14);
        let mut buf = Buffer::empty(full);
        for y in 0..full.height {
            buf.set_string(
                0,
                y,
                "甲乙丙丁戊己庚辛壬癸中文测试界面的宽度文字排版处理例子一二",
                dim_style,
            );
        }
        super::super::wide::mark_wide_tail_slots(&mut buf);

        let view = crate::hollywood::CardView {
            title: "REPORT #1002".into(),
            lines: vec!["line one".into()],
            ..crate::hollywood::CardView::default()
        };
        let ren = crate::hollywood::render_view(&mut buf, host, &view, None, &p);
        assert!(!ren.area.is_empty());
        super::super::wide::mark_wide_tail_slots(&mut buf);

        let shadow_col = Rect::new(ren.area.right(), ren.area.y + 1, 1, ren.area.height);
        for y in ren.area.top()..ren.area.bottom() {
            let gutter_x = ren.area.x;
            assert_ne!(buf[(gutter_x, y)].diff_option, CellDiffOption::Skip);
            assert_ne!(buf[(gutter_x + 1, y)].diff_option, CellDiffOption::Skip);
            if y > ren.area.y && y + 1 < ren.area.bottom() {
                let lb = &buf[(gutter_x + 1, y)];
                assert!(
                    matches!(lb.symbol(), "╭" | "│" | "├" | "╰"),
                    "host_x={host_x} y={y} left={:?}",
                    lb.symbol()
                );
            }
            if ren.area.x > 0 {
                assert_eq!(buf[(ren.area.x - 1, y)].cell_width(), 1);
            }
            if y >= shadow_col.top() && shadow_col.x + 1 < buf.area.right() {
                assert_eq!(buf[(shadow_col.x, y)].cell_width(), 1);
                assert_ne!(buf[(shadow_col.x + 1, y)].diff_option, CellDiffOption::Skip);
                if shadow_col.x.is_multiple_of(2) {
                    // Wide head at even `shadow_col.x` was split by shadow's right edge:
                    // both halves must be blanked (" "), never half-glyph.
                    assert_eq!(buf[(shadow_col.x, y)].symbol(), " ");
                    assert_eq!(buf[(shadow_col.x + 1, y)].symbol(), " ");
                    assert_eq!(buf[(shadow_col.x + 1, y)].cell_width(), 1);
                }
            }
        }
    }
}

#[test]
fn clear_overlay_clears_orphaned_skip_tail_before_fill() {
    use ratatui::buffer::CellDiffOption;
    use ratatui::style::Style;
    let area = Rect::new(0, 0, 20, 4);
    let mut buf = Buffer::empty(area);
    buf.set_string(0, 1, "中文测试界面的", Style::default());
    super::super::wide::mark_wide_tail_slots(&mut buf);
    // Orphaned tail Skip outside right boundary (pop covers col 4..10, so col 10 is Skip).
    buf[(9, 1)].set_symbol("x");
    buf[(10, 1)].set_symbol("？");
    buf[(10, 1)].set_diff_option(CellDiffOption::Skip);

    super::super::wide::clear_overlay(&mut buf, Rect::new(4, 1, 6, 2));
    assert_eq!(buf[(10, 1)].symbol(), " ");
    assert_ne!(buf[(10, 1)].diff_option, CellDiffOption::Skip);
}
