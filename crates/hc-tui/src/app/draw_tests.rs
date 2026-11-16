use super::*;
use hc_vt::Vt;

fn initial_paint(vt: &mut Vt) -> Paint {
    let colors = vt.frame(0).unwrap().colors;
    Paint {
        initial_fg: Some(colors.foreground),
        initial_bg: Some(colors.background),
        initial_palette: Some(colors.palette),
        host_fg: None,
        host_bg: None,
    }
}

#[test]
fn program_default_color_change_and_reset_survive_without_host_answers() {
    let mut vt = Vt::new(8, 2, 0);
    let paint = initial_paint(&mut vt);
    vt.feed(b"\x1b]11;#123456\x1b\\X");
    let frame = vt.frame(0).unwrap();
    let style = cell_style(&frame, &frame.rows[0].cells[0], &paint);
    assert_eq!(
        style.bg,
        Some(Color::Rgb(0x12, 0x34, 0x56)),
        "effective {:?}, initial {:?}, cell {:?}",
        frame.colors.background,
        paint.initial_bg,
        frame.rows[0].cells[0].bg
    );

    vt.feed(b"\x1b]111\x1b\\\rY");
    let frame = vt.frame(0).unwrap();
    let style = cell_style(&frame, &frame.rows[0].cells[0], &paint);
    assert_eq!(style.bg, None);
}

#[test]
fn palette_indices_remain_indexed_except_while_program_overrides_them() {
    let mut vt = Vt::new(8, 2, 0);
    let paint = initial_paint(&mut vt);
    for (bytes, expected) in [
        (&b"\x1b[31mX"[..], Color::Indexed(1)),
        (
            &b"\x1b]4;1;#123456\x1b\\\rY"[..],
            Color::Rgb(0x12, 0x34, 0x56),
        ),
        (&b"\x1b]104;1\x1b\\\rZ"[..], Color::Indexed(1)),
    ] {
        vt.feed(bytes);
        let frame = vt.frame(0).unwrap();
        let style = cell_style(&frame, &frame.rows[0].cells[0], &paint);
        assert_eq!(style.fg, Some(expected));
    }
}

// ---- Damage-aware pane surfaces (R2-T4) ----

use super::super::pane::{OpenAttach, Pane};
use crate::host::{HostFacts, HostTheme};
use ratatui::buffer::CellDiffOption;
use std::sync::Arc;

fn pane_with_vt(cols: u16, rows: u16, bytes: &[u8]) -> Pane {
    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    let open: OpenAttach = Arc::new(|_| Box::pin(std::future::pending()));
    let mut pane = Pane::new(tx, open);
    let mut vt = Vt::new(cols, rows, 0);
    vt.feed(bytes);
    pane.install_vt(vt);
    pane
}

fn row_symbols(buf: &Buffer, y: u16, width: u16) -> String {
    (0..width).map(|x| buf[(x, y)].symbol()).collect()
}

/// A clean frame blits the cached surface — identical cells including
/// the wide-tail `Skip` marks and resolved styles — with zero VT reads.
#[test]
fn clean_pane_blits_cached_cells_with_zero_vt_reads() {
    let mut pane = pane_with_vt(8, 3, "中x\x1b[38;2;1;2;3mY".as_bytes());
    let inner = Rect::new(0, 0, 8, 3);
    let mut first = Buffer::empty(inner);
    let (_, cells, cached) =
        draw_terminal_counted(&mut pane, inner, &mut first, Instant::now(), false);
    assert!(cached);
    assert_eq!(cells, 8 * 3);
    assert!(matches!(first[(1, 0)].diff_option, CellDiffOption::Skip));

    let mut second = Buffer::empty(inner);
    let (_, cells, cached) =
        draw_terminal_counted(&mut pane, inner, &mut second, Instant::now(), false);
    assert!(cached);
    assert_eq!(cells, 0);
    assert_eq!(first.content, second.content);
    assert_eq!(second[(3, 0)].symbol(), "Y");
    assert_eq!(second[(3, 0)].fg, Color::Rgb(1, 2, 3));
}

/// A partial update re-reads only dirty rows: the rewritten row drops
/// its stale wide/narrow cells while clean rows come from the cache.
#[test]
fn partial_row_update_reads_only_dirty_rows_and_drops_stale_cells() {
    let mut pane = pane_with_vt(8, 3, "中ab\r\nrow1\r\nrow2".as_bytes());
    let inner = Rect::new(0, 0, 8, 3);
    let mut buf = Buffer::empty(inner);
    let _ = draw_terminal_counted(&mut pane, inner, &mut buf, Instant::now(), false);

    // CUP crosses rows (both ends dirty); EL + "x" rewrite row 0.
    pane.vt().unwrap().feed(b"\x1b[1;1H\x1b[Kx");
    let mut next = Buffer::empty(inner);
    let (_, cells, _) = draw_terminal_counted(&mut pane, inner, &mut next, Instant::now(), false);
    assert!(cells <= 2 * 8, "{cells} cells read for a two-row update");
    assert_eq!(row_symbols(&next, 0, 8), "x       ");
    assert_eq!(&row_symbols(&next, 1, 8)[..4], "row1");
    assert_eq!(&row_symbols(&next, 2, 8)[..4], "row2");
}

/// Geometry, `Paint`, program colors and VT identity all key the
/// cache: changing any of them forces exactly one full re-read.
#[test]
fn cache_rebuilds_on_resize_paint_colors_and_vt_swap() {
    let mut pane = pane_with_vt(8, 3, b"hi");
    let inner = Rect::new(0, 0, 8, 3);
    let mut buf = Buffer::empty(inner);
    let (_, cells, _) = draw_terminal_counted(&mut pane, inner, &mut buf, Instant::now(), false);
    assert_eq!(cells, 8 * 3);
    let (_, cells, _) = draw_terminal_counted(&mut pane, inner, &mut buf, Instant::now(), false);
    assert_eq!(cells, 0);

    // A narrower inner rect re-reads only the clipped columns.
    let narrow = Rect::new(0, 0, 6, 3);
    let mut clipped = Buffer::empty(narrow);
    let (_, cells, _) =
        draw_terminal_counted(&mut pane, narrow, &mut clipped, Instant::now(), false);
    assert_eq!(cells, 6 * 3);

    // OSC 11 changes the effective background: colors mismatch → full.
    pane.vt().unwrap().feed(b"\x1b]11;#123456\x1b\\");
    let (_, cells, _) =
        draw_terminal_counted(&mut pane, narrow, &mut clipped, Instant::now(), false);
    assert_eq!(cells, 6 * 3);

    // Late host facts change `Paint` → full.
    pane.set_host_facts(HostFacts {
        theme: HostTheme {
            foreground: Some(Rgb { r: 9, g: 9, b: 9 }),
            ..Default::default()
        },
        ..Default::default()
    });
    let (_, cells, _) =
        draw_terminal_counted(&mut pane, narrow, &mut clipped, Instant::now(), false);
    assert_eq!(cells, 6 * 3);

    // A replacement VT can never alias the old one's cached cells.
    pane.install_vt(Vt::new(8, 3, 0));
    let (_, cells, _) =
        draw_terminal_counted(&mut pane, narrow, &mut clipped, Instant::now(), false);
    assert_eq!(cells, 6 * 3);
    let (_, cells, _) =
        draw_terminal_counted(&mut pane, narrow, &mut clipped, Instant::now(), false);
    assert_eq!(cells, 0);
}

/// `mark_wide_tail_slots_skipping` must produce byte-identical results
/// to a full scan wherever cached rects pre-marked their own tails.
#[test]
fn skipping_wide_tail_scan_matches_the_full_scan() {
    let area = Rect::new(0, 0, 30, 8);
    let panes = [Rect::new(0, 0, 14, 6), Rect::new(16, 0, 14, 6)];
    let bar = Rect::new(0, 6, 30, 2);
    let mut buf = Buffer::empty(area);
    // Pre-marked pane interiors, as a cache blit would leave them.
    buf.set_string(0, 0, "中", Style::default());
    buf[(1, 0)].set_diff_option(CellDiffOption::Skip);
    buf.set_string(12, 0, "中", Style::default());
    buf[(13, 0)].set_diff_option(CellDiffOption::Skip);
    buf.set_string(16, 0, "中", Style::default());
    buf[(17, 0)].set_diff_option(CellDiffOption::Skip);
    buf.set_string(28, 0, "中", Style::default());
    buf[(29, 0)].set_diff_option(CellDiffOption::Skip);
    // Border column and a bar row the walk scans for real; a stale
    // mark outside the panes and a border head covering pane cell
    // (16,2) exercise the covered-jump edge cases.
    for y in 0..6 {
        buf.set_string(15, y, "│", Style::default());
    }
    buf[(15, 4)].set_diff_option(CellDiffOption::Skip);
    buf.set_string(15, 2, "中", Style::default());
    for x in 0..30 {
        buf.set_string(x, 6, "中", Style::default());
    }

    let mut full = buf.clone();
    mark_wide_tail_slots(&mut full);
    let mut skipped = buf.clone();
    mark_wide_tail_slots_skipping(&mut skipped, &panes, Some(bar));
    for (index, (a, b)) in full.content.iter().zip(&skipped.content).enumerate() {
        assert_eq!(
            (a.symbol(), a.fg, a.bg, a.modifier, a.diff_option),
            (b.symbol(), b.fg, b.bg, b.modifier, b.diff_option),
            "cell {index} differs"
        );
    }
}
