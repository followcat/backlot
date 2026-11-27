//! Babylon S4 Lot dock snapshot, frame-weight, finished-studio, hit-map, and CJK tests.

use backlot_model::summary::Summary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use unicode_width::UnicodeWidthStr;

use super::{machines, summary_json};
use crate::chrome::palette::Palette;
use crate::panel::{Panel, PanelMode, PanelRender, Target, render};

fn panel(mode: PanelMode) -> Panel {
    Panel {
        mode,
        ..Panel::new()
    }
}

fn draw(
    area: Rect,
    doc: Option<&Summary>,
    p: &Panel,
    current: Option<&str>,
) -> (Buffer, PanelRender) {
    let mut buf = Buffer::empty(area);
    let list = machines(doc);
    let cur = current.map(crate::studio::StudioKey::local);
    let hits = render(
        &mut buf,
        area,
        &list,
        |_| true,
        |_| true,
        p,
        p.mode,
        cur.as_ref(),
        cur.as_ref(),
        0,
        None,
        &Default::default(),
        None,
        false,
        false,
        true,
        &Default::default(),
        &Palette::backlot(),
        1,
    );
    (buf, hits)
}

fn line_at(buf: &Buffer, area: Rect, y: u16) -> String {
    (area.x..area.x + area.width)
        .map(|x| buf[(x, y)].symbol())
        .collect()
}

fn screen_lines(buf: &Buffer, area: Rect) -> Vec<String> {
    (area.y..area.y + area.height)
        .map(|y| line_at(buf, area, y))
        .collect()
}

fn finished_summary(done: usize, total: usize) -> Summary {
    serde_json::from_value(serde_json::json!({
        "schema": "backlot/1",
        "updated_ms": 1,
        "studios": [{
            "id": "first-talkie",
            "title": "First talkie",
            "state": "finished",
            "requirement_version": 2,
            "mode": "standard",
            "open_reviews": 0,
            "waiting": 0,
            "tasks_done": done,
            "tasks_total": total,
            "roles": [
                { "role": "producer", "name": "producer", "state": "closed", "resumable": true }
            ]
        }]
    }))
    .unwrap()
}

#[test]
fn lot_header_and_three_frame_weights_at_38_cols() {
    let pal = Palette::backlot();
    let elev = pal.elevation();
    let mut val = summary_json();
    val["studios"].as_array_mut().unwrap().push(serde_json::json!({
        "id": "beta-api",
        "title": "Beta api",
        "state": "live",
        "requirement_version": 1,
        "mode": "standard",
        "open_reviews": 0,
        "waiting": 0,
        "roles": [{ "role": "producer", "name": "producer", "terminal": "t9", "state": "live", "work": "waiting" }]
    }));
    let doc: Summary = serde_json::from_value(val).unwrap();
    let area = Rect::new(0, 0, 38, 28);

    // 1. Resting (`focused = false`): `│ ✦ L O T · 2` on `stage_well`, blocks in `╭─╮│├─┤╰─╯` `gilt`.
    let resting = panel(PanelMode::Expanded);
    let (rbuf, rhits) = draw(area, Some(&doc), &resting, None);
    let rlines = screen_lines(&rbuf, area);
    assert!(
        rlines[0].contains("│ ✦ L O T · 2"),
        "resting header:\n{}",
        rlines.join("\n")
    );
    assert!(rlines[0].contains("alt+l focus"), "{}", rlines[0]);
    assert_eq!(rbuf[(1, 0)].bg, elev.stage_well);
    assert_eq!(rbuf[(2, 0)].fg, pal.accent);
    assert_eq!(rbuf[(4, 0)].fg, pal.text);
    assert!(rbuf[(4, 0)].modifier.contains(Modifier::BOLD));
    assert_eq!(rbuf[(1, 1)].symbol(), "╭");
    assert_eq!(rbuf[(1, 1)].fg, elev.gilt);
    assert!(
        rlines
            .iter()
            .any(|l| l.starts_with("│├") && l.ends_with('┤'))
    );
    assert!(
        rlines
            .iter()
            .any(|l| l.starts_with("│╰") && l.ends_with('╯'))
    );
    assert!(!rhits.rows.is_empty());

    // 2. Selected at `LotLevel::Studios`: `╔═╗`, `║`, `╟─╢`, `╚═╝` in `accent`.
    let mut selected = panel(PanelMode::Expanded);
    selected.focused = true;
    selected.selected = Some(Target::Studio("acme-shop".into()));
    let (sbuf, shits) = draw(area, Some(&doc), &selected, Some("acme-shop"));
    let slines = screen_lines(&sbuf, area);
    assert_eq!(sbuf[(4, 0)].fg, pal.accent);
    assert_eq!(sbuf[(1, 1)].symbol(), "╔");
    assert_eq!(sbuf[(1, 1)].fg, pal.accent);
    assert_eq!(sbuf[(area.width - 1, 1)].symbol(), "╗");
    assert!(
        slines
            .iter()
            .any(|l| l.starts_with("│║") && l.ends_with('║'))
    );
    assert!(
        slines
            .iter()
            .any(|l| l.starts_with("│╟") && l.ends_with('╢'))
    );
    assert!(
        slines
            .iter()
            .any(|l| l.starts_with("│╚") && l.ends_with('╝'))
    );
    assert!(
        shits
            .rows
            .iter()
            .any(|(r, t)| *t == Target::Studio("acme-shop".into()) && r.y == 1),
        "selected block top hit rect covers row 1"
    );

    // 3. Entered (`LotLevel::InsideStudio`): heavy outer `┏ ▸ L O T · 2 ━━ esc studios┓` / `┃` / `┗`
    // and heavy block `┏━┓`, `┃`, `┣─┫`, `┗━┛`.
    let mut entered = panel(PanelMode::Expanded);
    entered.focused = true;
    entered.entered_studio = Some("acme-shop".into());
    entered.selected = Some(Target::Person("acme-shop".into(), "producer".into()));
    let (ebuf, ehits) = draw(area, Some(&doc), &entered, Some("acme-shop"));
    let elines = screen_lines(&ebuf, area);
    assert!(
        elines[0].starts_with("┏ ▸ L O T · 2 ") && elines[0].ends_with("esc studios┓"),
        "{}",
        elines[0]
    );
    assert_eq!(ebuf[(0, 0)].bg, elev.stage_well);
    assert_eq!(ebuf[(0, 1)].symbol(), "┃");
    assert_eq!(ebuf[(0, area.height - 1)].symbol(), "┗");
    assert_eq!(ebuf[(1, 1)].symbol(), "┏");
    assert_eq!(ebuf[(area.width - 1, 1)].symbol(), "┓");
    assert!(
        elines
            .iter()
            .any(|l| l.starts_with("┃┃") && l.ends_with('┃'))
    );
    assert!(
        elines
            .iter()
            .any(|l| l.starts_with("┃┣") && l.ends_with('┫'))
    );
    assert!(
        elines
            .iter()
            .any(|l| l.starts_with("┃┗") && l.ends_with('┛'))
    );
    assert!(
        ehits
            .rows
            .iter()
            .any(|(_, t)| *t == Target::Person("acme-shop".into(), "producer".into()))
    );
    if std::env::var_os("BABYLON_LOT_DUMP").is_some() {
        eprintln!(
            "=== 38-COL RESTING ===\n{}\n=== 38-COL SELECTED ===\n{}\n=== 38-COL ENTERED ===\n{}",
            rlines.join("\n"),
            slines.join("\n"),
            elines.join("\n")
        );
    }
}

#[test]
fn finished_studio_tail_ladders_down_and_adds_no_extra_row() {
    let doc = finished_summary(12, 12);
    let p = panel(PanelMode::Expanded);

    // 38 cols: `✦ THE END·12/12` on top border, `↺ reopen producer [o]` directly below top
    // (the `───══╡ ✦  T H E   E N D  ✦ ╞══───` banner is skipped because it would cost +1 row).
    let (buf38, hits38) = draw(Rect::new(0, 0, 38, 12), Some(&doc), &p, None);
    let lines38 = screen_lines(&buf38, Rect::new(0, 0, 38, 12));
    assert!(lines38[1].contains("✦ THE END·12/12"), "{}", lines38[1]);
    assert!(
        lines38[2].contains("□ 1 closed · producer"),
        "{}",
        lines38[2]
    );
    assert!(
        lines38[3].contains("↺ reopen producer [o]"),
        "no extra divider row: {}",
        lines38[3]
    );
    assert_eq!(buf38[(1, 4)].symbol(), "╰");
    assert!(
        hits38
            .rows
            .iter()
            .any(|(_, t)| *t == Target::ReopenProducer("first-talkie".into()))
    );

    // 30 cols (< 32): steps down to `✦ END`.
    let (buf30, _) = draw(Rect::new(0, 0, 30, 10), Some(&doc), &p, None);
    assert!(line_at(&buf30, Rect::new(0, 0, 30, 10), 1).contains("✦ END"));

    // 23 cols: steps down to `✓ done`.
    let (buf23, _) = draw(Rect::new(0, 0, 23, 10), Some(&doc), &p, None);
    assert!(line_at(&buf23, Rect::new(0, 0, 23, 10), 1).contains("✓ done"));

    // Compact finished strip row keeps `✓ <id> done`.
    let mut compact_panel = panel(PanelMode::Expanded);
    compact_panel.collapsed_blocks.insert("first-talkie".into());
    let (cbuf, _) = draw(Rect::new(0, 0, 38, 10), Some(&doc), &compact_panel, None);
    let clines = screen_lines(&cbuf, Rect::new(0, 0, 38, 10));
    assert!(
        clines[1].contains("✓ first-talkie") && clines[1].contains("done"),
        "{}",
        clines[1]
    );
    if std::env::var_os("BABYLON_LOT_DUMP").is_some() {
        eprintln!(
            "=== 38-COL FINISHED EXPANDED ===\n{}\n=== 38-COL FINISHED COMPACT ===\n{}",
            lines38.join("\n"),
            clines.join("\n")
        );
    }
}

#[test]
fn cjk_studio_name_and_plain_chrome_bypass_stay_safe() {
    let pal = Palette::backlot();
    let elev = pal.elevation();
    let mut val = summary_json();
    val["studios"][0]["id"] = serde_json::json!("中文工作室名长标题");
    val["studios"][0]["title"] = serde_json::json!("中文工作室名长标题");
    let doc: Summary = serde_json::from_value(val).unwrap();
    let list = machines(Some(&doc));

    let area = Rect::new(0, 0, 30, 16);
    let mut sel = panel(PanelMode::Expanded);
    sel.focused = true;
    sel.selected = Some(Target::Studio("中文工作室名长标题".into()));
    let mut buf = Buffer::empty(area);
    render(
        &mut buf,
        area,
        &list,
        |_| true,
        |_| true,
        &sel,
        sel.mode,
        None,
        None,
        0,
        None,
        &Default::default(),
        None,
        false,
        false,
        true,
        &Default::default(),
        &pal,
        0,
    );
    let top_row = line_at(&buf, area, 1);
    assert_eq!(
        top_row.width(),
        30,
        "CJK truncation must preserve exact 30-col width: {top_row:?}"
    );
    assert_eq!(buf[(1, 1)].symbol(), "╔");
    assert_eq!(buf[(29, 1)].symbol(), "╗");

    // Plain / source=herdr bypass (`plain_chrome = true`) restores `lot · 1`, `╭─╮`, and `border_muted`.
    let mut plain = panel(PanelMode::Expanded);
    plain.plain_chrome = true;
    plain.focused = true;
    plain.selected = Some(Target::Studio("中文工作室名长标题".into()));
    let mut pbuf = Buffer::empty(area);
    render(
        &mut pbuf,
        area,
        &list,
        |_| true,
        |_| true,
        &plain,
        plain.mode,
        None,
        None,
        0,
        None,
        &Default::default(),
        None,
        false,
        false,
        true,
        &Default::default(),
        &pal,
        0,
    );
    assert!(line_at(&pbuf, area, 0).contains("lot · 1"));
    assert_eq!(pbuf[(1, 0)].bg, elev.surface);
    assert_eq!(pbuf[(1, 1)].symbol(), "▌");
}
