use super::*;
use hc_vt::Underline;
use ratatui::style::{Color, Modifier};

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
    assert_eq!(
        AnsiBackend::new(Broken)
            .draw(std::iter::once((0, 0, &cell)))
            .unwrap_err()
            .kind(),
        io::ErrorKind::BrokenPipe
    );
}
