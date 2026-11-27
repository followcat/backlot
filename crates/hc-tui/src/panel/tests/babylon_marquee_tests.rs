//! Babylon current-studio marquee frame tests in the Lot at 38 columns:
//! resting current vs non-current, precedence (selected `╔═╗`, entered `┏━┓`),
//! `(machine, id)` key matching, collapsed invariant, long CJK title safety,
//! 16-colour `terminal` fallback, `--plain` / `source = "herdr"` bypass, and
//! hit-map invariance.

use backlot_model::summary::Summary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use unicode_width::UnicodeWidthStr;

use super::{machines, remote, summary_json};
use crate::chrome::palette::Palette;
use crate::panel::data::MachineSummary;
use crate::panel::{Panel, PanelMode, PanelRender, Target, render};
use crate::studio::StudioKey;

fn panel(mode: PanelMode) -> Panel {
    Panel {
        mode,
        ..Panel::new()
    }
}

fn draw_with(
    area: Rect,
    list: &[MachineSummary],
    p: &Panel,
    cur: Option<&StudioKey>,
    mark: Option<&StudioKey>,
    pal: &Palette,
) -> (Buffer, PanelRender) {
    let mut buf = Buffer::empty(area);
    let hits = render(
        &mut buf,
        area,
        list,
        |_| true,
        |_| true,
        p,
        p.mode,
        cur,
        mark,
        0,
        Some("Local"),
        &Default::default(),
        None,
        false,
        false,
        true,
        &Default::default(),
        pal,
        1,
    );
    (buf, hits)
}

fn line_at(buf: &Buffer, area: Rect, y: u16) -> String {
    let mut out = String::new();
    let mut x = area.x;
    while x < area.x + area.width {
        let sym = buf[(x, y)].symbol();
        out.push_str(sym);
        x = x.saturating_add(sym.width().max(1) as u16);
    }
    out
}

fn screen_lines(buf: &Buffer, area: Rect) -> Vec<String> {
    (area.y..area.y + area.height)
        .map(|y| line_at(buf, area, y))
        .collect()
}

fn two_studio_doc() -> Summary {
    let mut val = summary_json();
    val["studios"].as_array_mut().unwrap().push(serde_json::json!({
        "id": "beta-api",
        "title": "Beta api",
        "state": "live",
        "requirement_version": 1,
        "mode": "standard",
        "open_reviews": 0,
        "waiting": 0,
        "roles": [{ "role": "producer", "name": "producer", "terminal": "t9", "state": "live", "work": "working" }]
    }));
    serde_json::from_value(val).unwrap()
}

fn block_bounds(buf: &Buffer, area: Rect, id_needle: &str) -> (u16, u16) {
    let top = (area.y..area.bottom())
        .find(|&y| {
            let line = line_at(buf, area, y);
            (line.contains('╭') || line.contains('╔') || line.contains('┏'))
                && line.contains(id_needle)
        })
        .unwrap_or_else(|| panic!("missing block top for {id_needle}"));
    let bot = (top + 1..area.bottom())
        .find(|&y| matches!(buf[(area.x + 1, y)].symbol(), "╰" | "╚" | "┗"))
        .unwrap_or_else(|| panic!("missing block bottom for {id_needle}"));
    (top, bot)
}

#[test]
fn lot_38_cols_current_marquee_vs_resting_selected_and_entered_precedence() {
    let pal = Palette::backlot();
    let elev = pal.elevation();
    let doc = two_studio_doc();
    let list = machines(Some(&doc));
    let area = Rect::new(0, 0, 38, 28);
    let acme = StudioKey::local("acme-shop");

    // 1. Resting (`focused = false`), current = acme-shop:
    //    `acme-shop` gets warm brass `bulb_warm` thin frame + `accent` `▪` bulbs every 3 cells;
    //    `beta-api` stays antique `gilt` `╭─╮` with zero bulbs.
    let resting = panel(PanelMode::Expanded);
    let (rbuf, rhits) = draw_with(area, &list, &resting, Some(&acme), Some(&acme), &pal);
    let rlines = screen_lines(&rbuf, area);
    let (acme_top, acme_bot) = block_bounds(&rbuf, area, "acme-shop");
    let (beta_top, beta_bot) = block_bounds(&rbuf, area, "beta-api");

    assert_eq!(rbuf[(1, acme_top)].symbol(), "╭");
    assert_eq!(rbuf[(1, acme_top)].fg, elev.bulb_warm);
    assert_eq!(rbuf[(37, acme_top)].symbol(), "╮");
    assert_eq!(rbuf[(37, acme_top)].fg, elev.bulb_warm);
    assert_eq!(rbuf[(1, acme_bot)].symbol(), "╰");
    assert_eq!(rbuf[(1, acme_bot)].fg, elev.bulb_warm);
    assert_eq!(rbuf[(37, acme_bot)].symbol(), "╯");
    assert_eq!(rbuf[(37, acme_bot)].fg, elev.bulb_warm);

    for y in (acme_top + 1)..acme_bot {
        let lsym = rbuf[(1, y)].symbol();
        let rsym = rbuf[(37, y)].symbol();
        assert!(matches!((lsym, rsym), ("│", "│") | ("├", "┤")), "y={y}");
        assert_eq!(rbuf[(1, y)].fg, elev.bulb_warm, "left y={y}");
        assert_eq!(rbuf[(37, y)].fg, elev.bulb_warm, "right y={y}");
    }
    // Bottom border has `▪` in `accent` every 3 cells (`dx % 3 == 2` from block x=1).
    for x in 2..37 {
        let cell = &rbuf[(x, acme_bot)];
        if (x - 1) % 3 == 2 {
            assert_eq!(cell.symbol(), "▪", "bottom bulb x={x}");
            assert_eq!(cell.fg, pal.accent, "bottom bulb fg x={x}");
        } else {
            assert_eq!(cell.symbol(), "─", "bottom rule x={x}");
            assert_eq!(cell.fg, elev.bulb_warm, "bottom rule fg x={x}");
        }
    }
    // Top border: `▪` bulbs only in rule segment (cols 15..28), never on title chip or tail.
    assert!(rlines[acme_top as usize].contains(" ▸ acme-shop "));
    for x in 2..15 {
        assert_ne!(rbuf[(x, acme_top)].symbol(), "▪", "title chip x={x}");
    }
    assert_eq!(rbuf[(15, acme_top)].symbol(), "▪");
    assert_eq!(rbuf[(15, acme_top)].fg, pal.accent);
    for x in 28..37 {
        assert_ne!(rbuf[(x, acme_top)].symbol(), "▪", "status tail x={x}");
    }
    // Non-current `beta-api` keeps `elev.gilt` and zero bulbs.
    assert_eq!(rbuf[(1, beta_top)].symbol(), "╭");
    assert_eq!(rbuf[(1, beta_top)].fg, elev.gilt);
    assert!(!rlines[beta_top as usize].contains('▪'));
    assert!(!rlines[beta_bot as usize].contains('▪'));
    assert_eq!(rbuf[(1, beta_bot)].fg, elev.gilt);

    // 2. Current and selected at `LotLevel::Studios`: double `╔═╗` in `accent` wins (no `▪`).
    let mut selected = panel(PanelMode::Expanded);
    selected.focused = true;
    selected.selected = Some(Target::Studio(acme.clone()));
    let (sbuf, shits) = draw_with(area, &list, &selected, Some(&acme), Some(&acme), &pal);
    let slines = screen_lines(&sbuf, area);
    let (stop, sbot) = block_bounds(&sbuf, area, "acme-shop");
    assert_eq!(sbuf[(1, stop)].symbol(), "╔");
    assert_eq!(sbuf[(1, stop)].fg, pal.accent);
    assert_eq!(sbuf[(1, sbot)].symbol(), "╚");
    assert!(!slines[stop as usize].contains('▪'));
    assert!(!slines[sbot as usize].contains('▪'));
    assert_eq!(rhits.title, shits.title);
    assert_eq!(rhits.rows.len(), shits.rows.len());

    // 3. Current and entered (`LotLevel::InsideStudio`): heavy `┏━┓` in `accent` wins (no `▪`).
    let mut entered = panel(PanelMode::Expanded);
    entered.focused = true;
    entered.entered_studio = Some(acme.clone());
    entered.selected = Some(Target::Person(acme.clone(), "producer".into()));
    let (ebuf, _) = draw_with(area, &list, &entered, Some(&acme), Some(&acme), &pal);
    let elines = screen_lines(&ebuf, area);
    let (etop, ebot) = block_bounds(&ebuf, area, "acme-shop");
    assert_eq!(ebuf[(1, etop)].symbol(), "┏");
    assert_eq!(ebuf[(1, etop)].fg, pal.accent);
    assert_eq!(ebuf[(1, ebot)].symbol(), "┗");
    assert!(!elines[etop as usize].contains('▪'));
    assert!(!elines[ebot as usize].contains('▪'));

    // 4. Another studio selected while `acme-shop` is current: `beta-api` wins `╔═╗`,
    //    while `acme-shop` keeps its `bulb_warm` marquee frame and `▪` bulbs.
    let mut other_sel = panel(PanelMode::Expanded);
    other_sel.focused = true;
    other_sel.selected = Some(Target::Studio(StudioKey::local("beta-api")));
    let (obuf, _) = draw_with(area, &list, &other_sel, Some(&acme), Some(&acme), &pal);
    let (oa_top, oa_bot) = block_bounds(&obuf, area, "acme-shop");
    let (ob_top, _) = block_bounds(&obuf, area, "beta-api");
    assert_eq!(obuf[(1, oa_top)].fg, elev.bulb_warm);
    assert!(line_at(&obuf, area, oa_bot).contains('▪'));
    assert_eq!(obuf[(1, ob_top)].symbol(), "╔");
    assert_eq!(obuf[(1, ob_top)].fg, pal.accent);

    if std::env::var_os("BABYLON_MARQUEE_DUMP").is_some() {
        eprintln!(
            "=== LOT 38-COL RESTING CURRENT ===\n{}\n=== LOT 38-COL SELECTED CURRENT ===\n{}\n=== LOT 38-COL ENTERED CURRENT ===\n{}",
            rlines.join("\n"),
            slines.join("\n"),
            elines.join("\n")
        );
    }
}

#[test]
fn lot_38_cols_cjk_long_title_never_covered_by_bulbs_and_keeps_tail() {
    let pal = Palette::backlot();
    let elev = pal.elevation();
    let mut val = summary_json();
    val["studios"][0]["id"] = serde_json::json!("中文长名称测试工作室标题示例");
    val["studios"][0]["title"] = serde_json::json!("中文长名称测试工作室标题示例");
    let doc: Summary = serde_json::from_value(val).unwrap();
    let list = machines(Some(&doc));
    let key = StudioKey::local("中文长名称测试工作室标题示例");
    let area = Rect::new(0, 0, 38, 22);
    let p = panel(PanelMode::Expanded);

    let (buf, _) = draw_with(area, &list, &p, Some(&key), Some(&key), &pal);
    let top_line = line_at(&buf, area, 1);
    assert_eq!(top_line.width(), 38, "{top_line:?}");
    assert!(top_line.starts_with("│╭ ▸ 中文"), "{top_line:?}");
    assert!(top_line.ends_with("● r1·0/0╮"), "{top_line:?}");
    assert_eq!(buf[(1, 1)].fg, elev.bulb_warm);
    assert_eq!(buf[(37, 1)].fg, elev.bulb_warm);
    // Title capsule (`elev.raised_row`) and tail (`● r1·0/0`) never have `▪`.
    for x in 0..38 {
        if buf[(x, 1)].symbol() == "▪" {
            assert_ne!(buf[(x, 1)].bg, elev.raised_row, "bulb inside chip x={x}");
            assert!(x < 29, "bulb inside status tail x={x}: {top_line:?}");
        }
    }
    let (_, bot_y) = block_bounds(&buf, area, "中文");
    assert!(line_at(&buf, area, bot_y).contains('▪'));
}

#[test]
fn lot_matches_machine_plus_id_and_collapsed_current_keeps_one_line() {
    let pal = Palette::backlot();
    let elev = pal.elevation();
    let doc = serde_json::from_value::<Summary>(summary_json()).unwrap();
    let list = vec![
        machines(Some(&doc)).into_iter().next().unwrap(),
        remote(&doc),
    ];
    let remote_key = StudioKey {
        machine: Some("host-b".into()),
        id: "acme-shop".into(),
    };
    let area = Rect::new(0, 0, 38, 26);
    let p = panel(PanelMode::Expanded);
    let (buf, _) = draw_with(area, &list, &p, Some(&remote_key), Some(&remote_key), &pal);
    let (local_top, local_bot) = block_bounds(&buf, area, "acme-shop · Local");
    let (remote_top, remote_bot) = block_bounds(&buf, area, "acme-shop · host-b");
    assert_eq!(buf[(1, local_top)].fg, elev.gilt);
    assert!(!line_at(&buf, area, local_bot).contains('▪'));
    assert_eq!(buf[(1, remote_top)].fg, elev.bulb_warm);
    assert!(line_at(&buf, area, remote_bot).contains('▪'));

    // Collapsed one-line studio stays a single row with no frame added.
    let mut collapsed = panel(PanelMode::Expanded);
    collapsed.collapsed_blocks.insert(remote_key.clone());
    let (cbuf, chits) = draw_with(
        area,
        &list,
        &collapsed,
        Some(&remote_key),
        Some(&remote_key),
        &pal,
    );
    let stacked_y = chits
        .rows
        .iter()
        .find(|(_, t)| *t == Target::Block(remote_key.clone()))
        .expect("stacked row")
        .0
        .y;
    assert_eq!(cbuf[(1, stacked_y)].symbol(), "╶");
    assert!(!line_at(&cbuf, area, stacked_y).contains('▪'));
}

#[test]
fn lot_16_colour_plain_and_hit_map_invariants() {
    let doc = two_studio_doc();
    let list = machines(Some(&doc));
    let area = Rect::new(0, 0, 38, 28);
    let acme = StudioKey::local("acme-shop");

    // 16-colour `terminal` theme: `bulb_warm` (`Yellow`) gets `DIM` like `gilt_style`; `▪` stays `accent` (`Blue`, no `DIM`).
    let term_pal = Palette::terminal();
    let term_elev = term_pal.elevation();
    let p = panel(PanelMode::Expanded);
    let (tbuf, thits) = draw_with(area, &list, &p, Some(&acme), Some(&acme), &term_pal);
    let (acme_top, acme_bot) = block_bounds(&tbuf, area, "acme-shop");
    assert_eq!(tbuf[(1, acme_top)].fg, term_elev.bulb_warm);
    assert!(tbuf[(1, acme_top)].modifier.contains(Modifier::DIM));
    assert_eq!(tbuf[(2, acme_top)].fg, term_pal.text);
    assert_eq!(tbuf[(15, acme_top)].symbol(), "▪");
    assert_eq!(tbuf[(15, acme_top)].fg, term_pal.accent);
    assert!(!tbuf[(15, acme_top)].modifier.contains(Modifier::DIM));
    assert_eq!(tbuf[(3, acme_bot)].symbol(), "▪");
    assert_eq!(tbuf[(3, acme_bot)].fg, Color::Blue);
    assert!(!tbuf[(3, acme_bot)].modifier.contains(Modifier::DIM));

    // Plain / `source = "herdr"` bypass (`plain_chrome = true`): `border_muted`, no bulbs, identical hit rects.
    let pal = Palette::backlot();
    let elev = pal.elevation();
    let mut plain = panel(PanelMode::Expanded);
    plain.plain_chrome = true;
    let (pbuf, phits) = draw_with(area, &list, &plain, Some(&acme), Some(&acme), &pal);
    let (ptop, pbot) = block_bounds(&pbuf, area, "acme-shop");
    assert_eq!(pbuf[(1, ptop)].fg, elev.border_muted);
    assert!(!line_at(&pbuf, area, ptop).contains('▪'));
    assert!(!line_at(&pbuf, area, pbot).contains('▪'));

    // Hit map invariance: current marquee vs no-mark keeps identical title, body, and row rects.
    let (_, no_mark_hits) = draw_with(area, &list, &p, Some(&acme), None, &pal);
    assert_eq!(thits.title, no_mark_hits.title);
    assert_eq!(thits.body, no_mark_hits.body);
    assert_eq!(thits.rows, no_mark_hits.rows);
    assert_eq!(phits.title, no_mark_hits.title);
    assert_eq!(phits.body, no_mark_hits.body);
    assert_eq!(phits.rows, no_mark_hits.rows);
}
