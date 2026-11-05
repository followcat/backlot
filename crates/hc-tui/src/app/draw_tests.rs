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
