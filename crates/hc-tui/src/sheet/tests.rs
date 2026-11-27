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

fn studio_summary(state: &str, roles_count: usize) -> backlot_model::summary::StudioSummary {
    let roles: Vec<serde_json::Value> = (0..roles_count)
        .map(|i| {
            serde_json::json!({
                "role": format!("actor-{i}"),
                "name": format!("actor-{i}"),
                "state": "closed",
                "attention": "outward action",
                "attention_class": "info",
                "attention_detail": "task done"
            })
        })
        .collect();
    serde_json::from_value(serde_json::json!({
        "id": "acme-shop",
        "title": "Acme shop",
        "state": state,
        "requirement_version": 2,
        "mode": "standard",
        "open_reviews": 0,
        "waiting": 0,
        "roles": roles,
    }))
    .unwrap()
}

fn row_text(buf: &Buffer, x0: u16, x1: u16, y: u16) -> String {
    use ratatui::buffer::CellWidth;
    let mut s = String::new();
    let mut x = x0;
    while x < x1 {
        let cell = &buf[(x, y)];
        s.push_str(cell.symbol());
        x += cell.cell_width().max(1);
    }
    s
}

#[test]
fn finished_overview_renders_the_end_on_blank_row_without_adding_rows() {
    let band = Rect::new(0, 0, 80, 24);
    let palette = Palette::catppuccin();
    let sheet = Sheet::new(
        StudioKey {
            machine: None,
            id: "acme-shop".into(),
        },
        None,
        0,
    );

    let finished = studio_summary("finished", 1);
    let live = studio_summary("live", 1);

    let mut fin_buf = Buffer::empty(band);
    let mut live_buf = Buffer::empty(band);
    let fin_ren = render(&mut fin_buf, band, &sheet, Some(&finished), &palette);
    let live_ren = render(&mut live_buf, band, &sheet, Some(&live), &palette);

    assert_eq!(fin_ren.frame, live_ren.frame, "must add zero rows/cols");
    assert_eq!(fin_ren.body, live_ren.body, "body rect must be unchanged");

    let fin_rows: Vec<String> = (0..band.height)
        .map(|y| row_text(&fin_buf, 0, band.width, y))
        .collect();
    let live_rows: Vec<String> = (0..band.height)
        .map(|y| row_text(&live_buf, 0, band.width, y))
        .collect();
    let fin_joined = fin_rows.join("\n");
    let live_joined = live_rows.join("\n");

    let end_y = (fin_ren.body.y..=fin_ren.body.bottom())
        .find(|&y| fin_rows[usize::from(y)].contains("───══╡ ✦  T H E   E N D  ✦ ╞══───"))
        .expect("finished Overview must show THE END intertitle");
    assert!(
        live_rows[usize::from(end_y)].trim()
            == "│                                                                          │",
        "row chosen for THE END was blank in unfinished Sheet: {:?}",
        live_rows[usize::from(end_y)]
    );
    assert!(
        !live_joined.contains("T H E   E N D"),
        "unfinished Overview must not show THE END:\n{live_joined}"
    );

    // Other tabs on a finished studio must NOT show THE END.
    let mut deliveries_sheet = Sheet::new(
        StudioKey {
            machine: None,
            id: "acme-shop".into(),
        },
        None,
        0,
    );
    deliveries_sheet.tab = crate::sheet::SheetTab::Deliveries;
    let mut del_buf = Buffer::empty(band);
    render(
        &mut del_buf,
        band,
        &deliveries_sheet,
        Some(&finished),
        &palette,
    );
    let del_joined = (0..band.height)
        .map(|y| row_text(&del_buf, 0, band.width, y))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!del_joined.contains("T H E   E N D"));

    // Full-body finished Overview (many roles filling `body.height`) lands on bottom padding row.
    let full_finished = studio_summary("finished", 20);
    let mut full_top = Buffer::empty(band);
    let full_top_ren = render(&mut full_top, band, &sheet, Some(&full_finished), &palette);
    let full_top_joined = (0..band.height)
        .map(|y| row_text(&full_top, 0, band.width, y))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !full_top_joined.contains("T H E   E N D"),
        "scrolled-above end omits THE END:\n{full_top_joined}"
    );

    let mut at_bottom = sheet;
    at_bottom.scroll = 100;
    let mut full_bot = Buffer::empty(band);
    let full_bot_ren = render(
        &mut full_bot,
        band,
        &at_bottom,
        Some(&full_finished),
        &palette,
    );
    assert_eq!(full_bot_ren.frame, full_top_ren.frame);
    let pad_row = row_text(&full_bot, 0, band.width, full_bot_ren.body.bottom());
    assert!(
        pad_row.contains("───══╡ ✦  T H E   E N D  ✦ ╞══───"),
        "full finished Overview draws THE END on bottom padding row:\n{pad_row}"
    );

    if std::env::var_os("BABYLON_SHEET_DUMP").is_some() {
        eprintln!("=== FINISHED SHEET OVERVIEW ===");
        for r in &fin_rows {
            eprintln!("{r}");
        }
        let _ = fin_joined;
    }
}
