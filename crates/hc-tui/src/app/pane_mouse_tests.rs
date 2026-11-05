//! Immutable bytes captured from pinned herdr, not candidate-generated output.
use super::*;
use crate::raw_events::{Decoded, events_from_framed_chunks};

fn hex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn source_mouse_bytes_cover_formats_gestures_modifiers_and_wheels() {
    for text in [
        include_str!("../../tests/fixtures/reference-mouse-cell.json"),
        include_str!("../../tests/fixtures/reference-mouse-pixel.json"),
        include_str!("../../tests/fixtures/reference-mouse-unknown-pitch.json"),
    ] {
        let fixture: serde_json::Value = serde_json::from_str(text).unwrap();
        let hit = PaneHit {
            rect: ratatui::layout::Rect::new(26, 1, 94, 39),
            inner_rect: ratatui::layout::Rect::new(26, 1, 93, 39),
            scrollbar_rect: None,
            scroll: None,
            pane_id: "t1".into(),
        };
        let mut count = 0;
        let mut vt = hc_vt::Vt::new(93, 39, 0);
        let pitch = fixture["host_cell_px"]
            .as_array()
            .map(|p| (p[0].as_u64().unwrap() as u32, p[1].as_u64().unwrap() as u32))
            .unwrap_or((8, 16));
        vt.set_cell_px(pitch.0, pitch.1);
        for group in fixture["groups"].as_array().unwrap() {
            vt.feed(&hex(group["setup_sequence_hex"].as_str().unwrap()));
            for case in group["cases"].as_array().unwrap() {
                let outer = hex(case["outer_hex"].as_str().unwrap());
                let events = events_from_framed_chunks(vec![outer]);
                let &[Decoded::Ui(super::super::input::UiEvent::Mouse(mut mouse))] =
                    events.as_slice()
                else {
                    panic!("expected one source mouse event: {case:?}");
                };
                // The fixture's private-copy config uses exactly Ctrl for right passthrough.
                if matches!(
                    mouse.kind,
                    MouseEventKind::Down(MouseButton::Right)
                        | MouseEventKind::Up(MouseButton::Right)
                ) {
                    mouse.mods = mouse.mods.difference(Mods::CTRL);
                }
                let pixels = if group["host_pixel_mode"] == true {
                    let (x, y) = mouse.sgr_position.unwrap();
                    let geometry =
                        crate::mouse_geometry::HostGeometry::new(120, 40, 1200, 800).unwrap();
                    (mouse.col, mouse.row) = geometry.cell(x, y).unwrap();
                    Some(crate::mouse_geometry::HostPixels { x, y, geometry })
                } else {
                    None
                };
                let pixels = pixels.filter(|_| {
                    !(mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && mouse.mods.contains(Mods::CTRL))
                });
                let mut input = native_input(&hit, mouse);
                if let Some(crate::mouse_geometry::Position::Pixels { x, y }) =
                    pixels.and_then(|p| p.pane_position(hit.inner_rect, 930, 780))
                {
                    input.position = MousePosition::Pixels { x, y };
                }
                let actual = vt.mouse(input).unwrap();
                assert_eq!(
                    actual,
                    hex(case["program_hex"].as_str().unwrap()),
                    "{}: {}",
                    group["name"],
                    case["name"]
                );
                count += 1;
            }
        }
        assert_eq!(count, 75);
    }
}

#[test]
fn native_mouse_modes_change_live_and_untracked_motion_stays_quiet() {
    let mut vt = hc_vt::Vt::new(200, 50, 0);
    let mut input = hc_vt::MouseInput {
        action: MouseAction::Motion,
        button: None,
        mods: Mods::ALT.union(Mods::SHIFT),
        position: MousePosition::Cell {
            column: 172,
            row: 40,
        },
    };
    assert!(vt.mouse(input).unwrap().is_empty());
    vt.feed(b"\x1b[?1003;1006h");
    assert_eq!(vt.mouse(input).unwrap(), b"\x1b[<47;173;41M");
    // Source creates a fresh encoder each time: a repeated cell still reports.
    assert_eq!(vt.mouse(input).unwrap(), b"\x1b[<47;173;41M");
    vt.feed(b"\x1b[?1003l\x1b[?1000h");
    assert!(vt.mouse(input).unwrap().is_empty());
    input.action = MouseAction::Press;
    input.button = Some(hc_vt::MouseButton::Left);
    assert_eq!(vt.mouse(input).unwrap(), b"\x1b[<12;173;41M");
    vt.feed(b"\x1b[?1000l");
    assert!(vt.mouse(input).unwrap().is_empty());
}
