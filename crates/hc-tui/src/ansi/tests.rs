use super::*;
use hc_vt::Underline;
use ratatui::style::{Color, Modifier};

fn cursor(x: u16, y: u16, visible: bool) -> HostCursor {
    HostCursor {
        x,
        y,
        visible,
        shape: 0,
    }
}

/// Byte-position assertions shared by the frame tests: everything the
/// frame emits sits inside one `?2026` block except the final IME
/// anchor (`CUP` + visibility), and no `?25` write appears outside it.
fn assert_atomic_frame(bytes: &[u8]) {
    let s = String::from_utf8_lossy(bytes);
    let start = s.find("\x1b[?2026h").expect("sync begin");
    let end = s.find("\x1b[?2026l").expect("sync end");
    assert!(start < end, "sync block ordering: {s:?}");
    assert!(s[..start].is_empty(), "bytes before sync begin: {s:?}");
    // After the block only the IME anchor remains: one CUP plus one
    // visibility write — no cells, clears or extra cursor traffic.
    let tail = s[end + "\x1b[?2026l".len()..]
        .replace("\x1b[?25h", "")
        .replace("\x1b[?25l", "");
    let tail = tail.strip_prefix("\x1b[").expect("anchor CUP");
    let tail = tail.strip_suffix('H').expect("anchor CUP");
    assert!(
        tail.split(';')
            .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())),
        "non-anchor bytes outside sync block: {s:?}"
    );
    // The block carries the header hide plus the final visibility
    // write — the cursor never travels outside it.
    let inside = &s[start..end];
    let vis_writes = inside.matches("\x1b[?25h").count() + inside.matches("\x1b[?25l").count();
    assert!(vis_writes >= 2, "cursor state missing inside block: {s:?}");
}

#[test]
fn frame_emits_one_atomic_block_with_cursor_inside_and_ime_anchor_after() {
    let mut bytes = Vec::new();
    {
        let mut backend = AnsiBackend::new(&mut bytes);
        let cell = Cell::default();
        backend.draw([(2u16, 1u16, &cell)].into_iter()).unwrap();
        backend.set_frame_cursor(cursor(4, 2, true));
        Backend::flush(&mut backend).unwrap();
        assert!(backend.emitted());
    }
    let s = String::from_utf8_lossy(&bytes);
    assert_atomic_frame(&bytes);
    // First frame carries the reference's clear inside the block.
    assert!(
        s.contains("\x1b[?2026h\x1b[?25l\x1b]8;;\x1b\\\x1b[2J"),
        "{s:?}"
    );
    // Final cursor: CUP + visibility inside the block, anchor after.
    assert!(s.contains("\x1b[3;5H\x1b[?25h\x1b[?2026l"), "{s:?}");
    assert!(s.ends_with("\x1b[3;5H\x1b[?25h"), "{s:?}");
}

#[test]
fn hidden_cursor_frame_keeps_anchor_inside_and_repeats_it_after() {
    let mut bytes = Vec::new();
    {
        let mut backend = AnsiBackend::new(&mut bytes);
        let cell = Cell::default();
        backend.draw([(0u16, 0u16, &cell)].into_iter()).unwrap();
        backend.set_frame_cursor(cursor(7, 6, false));
        Backend::flush(&mut backend).unwrap();
    }
    let s = String::from_utf8_lossy(&bytes);
    assert_atomic_frame(&bytes);
    assert!(
        s.contains("\x1b[7;8H\x1b[?25l\x1b[?2026l\x1b[7;8H\x1b[?25l"),
        "{s:?}"
    );
}

#[test]
fn identical_frame_emits_nothing_and_marks_no_emission() {
    let mut bytes = Vec::new();
    {
        let mut backend = AnsiBackend::new(&mut bytes);
        let cell = Cell::default();
        backend.draw([(0u16, 0u16, &cell)].into_iter()).unwrap();
        backend.set_frame_cursor(cursor(1, 1, true));
        Backend::flush(&mut backend).unwrap();
        assert!(backend.emitted());
        backend.out.clear();
        // Same cursor, no cells: the whole frame is skipped.
        backend
            .draw(std::iter::empty::<(u16, u16, &Cell)>())
            .unwrap();
        backend.set_frame_cursor(cursor(1, 1, true));
        Backend::flush(&mut backend).unwrap();
        assert!(!backend.emitted());
        assert!(bytes.is_empty(), "identical frame emitted: {bytes:?}");
    }
}

#[test]
fn a_cursor_only_change_still_emits_a_sync_block() {
    let mut bytes = Vec::new();
    {
        let mut backend = AnsiBackend::new(&mut bytes);
        let cell = Cell::default();
        backend.draw([(0u16, 0u16, &cell)].into_iter()).unwrap();
        backend.set_frame_cursor(cursor(1, 1, false));
        Backend::flush(&mut backend).unwrap();
        backend.out.clear();
        backend
            .draw(std::iter::empty::<(u16, u16, &Cell)>())
            .unwrap();
        backend.set_frame_cursor(cursor(3, 4, false));
        Backend::flush(&mut backend).unwrap();
        assert!(backend.emitted());
    }
    let s = String::from_utf8_lossy(&bytes);
    assert_atomic_frame(&bytes);
    assert!(
        s.contains("\x1b[5;4H\x1b[?25l\x1b[?2026l\x1b[5;4H\x1b[?25l"),
        "{s:?}"
    );
}

#[test]
fn shape_writes_only_on_change() {
    let mut bytes = Vec::new();
    {
        let mut backend = AnsiBackend::new(&mut bytes);
        let mut c = cursor(1, 1, true);
        c.shape = 5;
        let cell = Cell::default();
        backend.draw([(0u16, 0u16, &cell)].into_iter()).unwrap();
        backend.set_frame_cursor(c);
        Backend::flush(&mut backend).unwrap();
        let s = String::from_utf8_lossy(backend.out.as_slice());
        assert_eq!(s.matches("\x1b[5 q").count(), 1, "{s:?}");
        // Same shape again: no second DECSCUSR.
        backend.out.clear();
        backend.draw([(1u16, 0u16, &cell)].into_iter()).unwrap();
        backend.set_frame_cursor(c);
        Backend::flush(&mut backend).unwrap();
        let s = String::from_utf8_lossy(backend.out.as_slice());
        assert!(!s.contains("\x1b[5 q"), "{s:?}");
    }
}

#[test]
fn clear_requests_write_nothing_until_the_next_frame_then_clear_inside() {
    let mut bytes = Vec::new();
    {
        let mut backend = AnsiBackend::new(&mut bytes);
        let cell = Cell::default();
        backend.draw([(0u16, 0u16, &cell)].into_iter()).unwrap();
        backend.set_frame_cursor(cursor(1, 1, false));
        Backend::flush(&mut backend).unwrap();
        assert_eq!(
            backend.out.windows(4).filter(|w| w == b"\x1b[2J").count(),
            1
        );
        backend.out.clear();
        // The resize path (ratatui calls clear_region(All)) emits no
        // bytes by itself; the clear lands inside the next frame even
        // when the diff is empty and the cursor unchanged.
        backend.clear_region(ClearType::All).unwrap();
        assert!(backend.out.is_empty());
        backend
            .draw(std::iter::empty::<(u16, u16, &Cell)>())
            .unwrap();
        backend.set_frame_cursor(cursor(1, 1, false));
        Backend::flush(&mut backend).unwrap();
        assert!(backend.emitted());
        assert_atomic_frame(backend.out.as_slice());
        let s = String::from_utf8_lossy(backend.out.as_slice());
        assert!(s.contains("\x1b[2J"), "{s:?}");
        // `Terminal::clear` lands the same way.
        backend.out.clear();
        backend.clear().unwrap();
        backend
            .draw(std::iter::empty::<(u16, u16, &Cell)>())
            .unwrap();
        backend.set_frame_cursor(cursor(1, 1, false));
        Backend::flush(&mut backend).unwrap();
        assert!(backend.emitted());
        let s = String::from_utf8_lossy(backend.out.as_slice());
        assert!(s.contains("\x1b[2J"), "{s:?}");
        // And once the owed clear was emitted, an unchanged frame is
        // skipped again.
        backend.out.clear();
        backend
            .draw(std::iter::empty::<(u16, u16, &Cell)>())
            .unwrap();
        backend.set_frame_cursor(cursor(1, 1, false));
        Backend::flush(&mut backend).unwrap();
        assert!(!backend.emitted());
        assert!(backend.out.is_empty());
    }
}

#[test]
fn clears_and_cursor_ops_outside_a_frame_write_nothing() {
    let mut bytes = Vec::new();
    {
        let mut backend = AnsiBackend::new(&mut bytes);
        backend.clear().unwrap();
        backend.clear_region(ClearType::All).unwrap();
        backend.hide_cursor().unwrap();
        backend.show_cursor().unwrap();
        backend.set_cursor_position((3u16, 4u16)).unwrap();
    }
    assert!(bytes.is_empty(), "{bytes:?}");
}

#[test]
fn actual_backend_preserves_all_source_underline_styles_and_clears_attributes() {
    let mut bytes = Vec::new();
    let mut cells = Vec::new();
    for kind in 1..=5u16 {
        let mut cell = Cell::default();
        cell.set_symbol("X");
        cell.set_fg(Color::Rgb(12, 34, 56));
        cell.set_bg(Color::Indexed(200));
        cell.set_style(ratatui::style::Style::new().add_modifier(
            Modifier::from_bits_retain(Modifier::UNDERLINED.bits() | (kind << 12))
                | Modifier::ITALIC
                | Modifier::BOLD,
        ));
        cells.push(cell);
    }
    cells.push(Cell::default());
    {
        let mut backend = AnsiBackend::new(&mut bytes);
        backend
            .draw(cells.iter().enumerate().map(|(x, c)| (x as u16, 0, c)))
            .unwrap();
        backend.set_frame_cursor(cursor(0, 1, false));
        Backend::flush(&mut backend).unwrap();
    }
    let mut vt = hc_vt::Vt::new(20, 2, 0);
    vt.feed(&bytes);
    let frame = vt.frame(0).unwrap();
    for (index, kind) in [
        Underline::Single,
        Underline::Double,
        Underline::Curly,
        Underline::Dotted,
        Underline::Dashed,
    ]
    .into_iter()
    .enumerate()
    {
        let cell = &frame.rows[0].cells[index];
        assert_eq!(cell.underline, kind);
        assert!(cell.italic && cell.bold);
        assert_eq!(
            cell.fg,
            hc_vt::CellColor::Rgb(hc_vt::Rgb {
                r: 12,
                g: 34,
                b: 56
            })
        );
        assert_eq!(cell.bg, hc_vt::CellColor::Palette(200));
    }
    let plain = &frame.rows[0].cells[5];
    assert_eq!(plain.underline, Underline::None);
    assert!(!plain.italic && !plain.bold);
}

#[test]
fn cell_output_failure_is_reported_to_the_terminal_guard_caller() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let cell = Cell::default();
    let mut backend = AnsiBackend::new(Broken);
    backend.draw(std::iter::once((0, 0, &cell))).unwrap();
    backend.set_frame_cursor(cursor(0, 0, true));
    assert_eq!(
        Backend::flush(&mut backend).unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
}
