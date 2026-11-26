//! Tests for Call Sheet elevation surfaces, zone rules, waiting-row
//! styling, fixed exchange columns & 2-line wrap, compose placeholder
//! while editing, separated two-tone hints, 1-row external shadow, and
//! 200x44 / 100x40 busy layouts.

use super::draw_tests::{draw, draw_without_agent, layer, lines, stage_up};
use super::*;
use crate::chrome::palette::Palette;
use crate::hollywood::{
    Hollywood, LayerMode, LayerOpts, Target, expanded_extent, expanded_height, render,
};
use crate::studio::StudioKey;
use backlot_model::summary::Summary;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};

fn row_text(buf: &Buffer, area: Rect, y: u16) -> String {
    (area.x..area.x + area.width)
        .map(|x| buf[(x, y)].symbol())
        .collect()
}

fn busy_summary(now_ms: u64) -> Summary {
    let mut value = summary_json();
    value["studios"] = serde_json::json!([
        {
            "id": "acme-shop",
            "title": "Acme shop",
            "state": "live",
            "requirement_version": 2,
            "mode": "standard",
            "open_reviews": 0,
            "tasks_done": 1,
            "tasks_total": 3,
            "waiting": 1,
            "roles": [
                {
                    "role": "producer",
                    "name": "producer",
                    "state": "live",
                    "work": "working",
                    "doing": "triaging bugfix",
                    "attention": "permission prompt",
                    "attention_user": true,
                    "attention_seq": 7,
                    "attention_detail": "cargo test --workspace",
                    "can_answer_prompt": true
                },
                { "role": "actor-alice", "name": "alice", "state": "live", "task": "t1", "work": "working" }
            ],
            "exchange": []
        },
        {
            "id": "beta-api",
            "title": "Beta api",
            "state": "live",
            "requirement_version": 1,
            "mode": "standard",
            "open_reviews": 0,
            "tasks_done": 2,
            "tasks_total": 4,
            "waiting": 0,
            "roles": [
                { "role": "producer", "name": "producer", "state": "live", "work": "waiting" }
            ],
            "exchange": []
        },
        {
            "id": "gamma-web",
            "title": "Gamma web",
            "state": "live",
            "requirement_version": 1,
            "mode": "standard",
            "open_reviews": 0,
            "tasks_done": 0,
            "tasks_total": 2,
            "waiting": 0,
            "roles": [
                { "role": "producer", "name": "producer", "state": "live", "work": "waiting" }
            ],
            "exchange": []
        },
        {
            "id": "delta-ops",
            "title": "Delta ops",
            "state": "live",
            "requirement_version": 1,
            "mode": "standard",
            "open_reviews": 0,
            "tasks_done": 1,
            "tasks_total": 1,
            "waiting": 0,
            "roles": [
                { "role": "producer", "name": "producer", "state": "live", "work": "waiting" }
            ],
            "exchange": []
        }
    ]);
    value["hollywood"] = serde_json::json!({
        "state": "live",
        "work": "waiting",
        "runner": "claude",
        "model": "opus-4-6",
        "effort": "high",
        "headline": "acme-shop prompt #7 blocks producer; 測試中文折行與省略顯示效果檢查非常長的訊息確保不超過兩行且可讀性良好",
        "headline_seq": 5,
        "cards": [
            {
                "id": "c3",
                "studio": "beta-api",
                "action": "pause t2",
                "why": "flaky timeout on staging smoke check",
                "status": "open"
            }
        ],
        "exchange": [
            { "seq": 1, "who": "user", "ts_ms": now_ms - 240_000, "text": "prior check across studios" },
            { "seq": 2, "who": "hollywood", "ts_ms": now_ms - 180_000, "text": "all 4 studios started clean" },
            { "seq": 3, "who": "user", "ts_ms": now_ms - 90_000, "text": "status across studios and CJK 測試檢查" },
            { "seq": 4, "who": "user", "ts_ms": now_ms - 45_000, "text": "any blockers on beta-api or delta-ops?" },
            {
                "seq": 5,
                "who": "hollywood",
                "ts_ms": now_ms - 10_000,
                "text": "acme-shop prompt #7 blocks producer; beta-api opened card c3; 測試中文折行與省略顯示效果檢查",
                "has_body": true
            }
        ]
    });
    serde_json::from_value(value).unwrap()
}

#[test]
fn surface_differs_from_terminal_background_across_all_19_bundled_themes() {
    let summary = busy_summary(1_000_000);
    let area = Rect::new(0, 0, 120, 40);
    let mut layer = layer(LayerMode::Expanded);
    layer.now_ms = Some(1_000_000);
    for name in crate::config::THEME_NAMES {
        let palette = Palette::from_name(name).expect(name);
        let elev = palette.elevation();
        let (buf, hits) = draw(area, Some(&summary), true, &layer, false, &palette);
        let body_bg = buf[(area.x + 2, hits.title.y)].bg;
        assert_eq!(
            body_bg, elev.surface,
            "{name} body must use elevation.surface"
        );
        if !elev.is_terminal_16 {
            assert_ne!(
                body_bg, palette.panel_bg,
                "{name} surface must separate cleanly from panel_bg"
            );
        }
        if palette.sidebar_bg == Color::Rgb(12, 14, 21) {
            assert_ne!(body_bg, Color::Reset, "{name}");
        }
    }
}

#[test]
fn busy_callsheet_200x44_and_100x40_preserve_zones_frames_wait_talk_compose_and_shadow() {
    let now_ms = 1_000_000;
    let summary = busy_summary(now_ms);
    let palette = Palette::backlot();
    let elev = palette.elevation();
    let mut hw = Hollywood::new();
    hw.mode = LayerMode::Expanded;
    hw.selected = Some(Target::Prompt(
        StudioKey::local("acme-shop"),
        "producer".into(),
    ));
    hw.active_studio = Some(StudioKey::local("beta-api"));
    hw.now_ms = Some(now_ms);

    // 200x44: cap = 22, all zones fit including 7-row frames, 2 waiting rows (selected keeps detail),
    // HOLLYWOOD rule with folded earlier turns badge, wrapped CJK/EN exchange, compose, hints, and shadow at y=22.
    let area_wide = Rect::new(0, 0, 200, 44);
    let list = machines(Some(&summary));
    let opts = LayerOpts {
        agent: true,
        mobile: false,
        suppress_highlights: false,
        no_shadow: false,
    };
    let (h_wide, sh_wide) = expanded_extent(area_wide, &list, &stage_up, &hw, opts);
    assert_eq!(
        h_wide,
        expanded_height(area_wide, &list, &stage_up, &hw, opts)
    );
    assert!(h_wide <= 22, "200x44 cap <= 22, got {h_wide}");
    assert_eq!(sh_wide, 1, "external bottom shadow enabled at height 44");

    let mut buf_w = Buffer::empty(area_wide);
    let hits_w = render(
        &mut buf_w,
        area_wide,
        &list,
        Some("local"),
        &stage_up,
        &hw,
        opts,
        &palette,
    );
    assert!(hits_w.shadow_drawn);
    assert_eq!(buf_w[(10, hits_w.area.bottom())].bg, elev.shadow.unwrap());

    let text_w = lines(&buf_w, area_wide).join("\n");
    assert!(text_w.contains("STUDIOS · 4"), "{text_w}");
    assert!(
        text_w.contains("▸ beta-api"),
        "active main-pane marker: {text_w}"
    );
    assert!(text_w.contains("WAITING ON YOU · 2"), "{text_w}");
    assert!(text_w.contains("HOLLYWOOD"), "{text_w}");
    assert!(text_w.contains("earlier · ↵ read"), "{text_w}");
    assert!(text_w.contains("✦ hollywood"), "{text_w}");
    assert!(text_w.contains("› you"), "{text_w}");
    assert!(text_w.contains("tell hollywood…"), "{text_w}");

    // Waiting row col 2 has persistent `▎`, selected row also has `▌▸` on `raised_row`,
    // and unselected CARD row still has col-2 `▎`.
    let prompt_y = hits_w
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Prompt(..)))
        .unwrap()
        .0
        .y;
    let card_y = hits_w
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Card(..)))
        .unwrap()
        .0
        .y;
    assert_eq!(buf_w[(0, prompt_y)].symbol(), "▌");
    assert_eq!(buf_w[(1, prompt_y)].symbol(), "▸");
    assert_eq!(buf_w[(2, prompt_y)].symbol(), "▎");
    assert_eq!(buf_w[(2, prompt_y)].fg, palette.red);
    assert_eq!(buf_w[(2, prompt_y)].bg, elev.raised_row);
    assert_eq!(buf_w[(0, card_y)].symbol(), " ");
    assert_eq!(buf_w[(2, card_y)].symbol(), "▎");
    assert_eq!(buf_w[(2, card_y)].fg, palette.accent);
    assert_eq!(buf_w[(2, card_y)].bg, elev.surface);

    // 100x40: cap = 20, 7-row frames survive ahead of older talk lines, CJK wraps/clamps <= 2 lines with `…`.
    let area_100 = Rect::new(0, 0, 100, 40);
    let (h_100, sh_100) = expanded_extent(area_100, &list, &stage_up, &hw, opts);
    assert!(h_100 <= 20, "100x40 cap <= 20, got {h_100}");
    assert_eq!(sh_100, 1);
    let mut buf_100 = Buffer::empty(area_100);
    let hits_100 = render(
        &mut buf_100,
        area_100,
        &list,
        Some("local"),
        &stage_up,
        &hw,
        opts,
        &palette,
    );
    let frame_rect = hits_100
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Studio(_)))
        .unwrap()
        .0;
    assert_eq!(frame_rect.height, 7, "7-row frames preserved at 100x40");
    let text_100 = lines(&buf_100, area_100).join("\n");
    assert!(text_100.contains("earlier · ↵ read"), "{text_100}");
    assert!(
        text_100.contains('…'),
        "2-line CJK clamp adds ellipsis:\n{text_100}"
    );
    assert!(text_100.contains("tell hollywood…"), "{text_100}");

    if std::env::var_os("CALLSHEET_ZONES_DUMP").is_some() {
        eprintln!("=== AFTER 200x44 ===");
        eprintln!(
            "{}",
            dump_annotated(
                &buf_w,
                area_wide,
                &palette,
                hits_w.area.bottom() + 1,
                hits_w.input.y
            )
        );
        eprintln!("=== AFTER 100x40 ===");
        eprintln!(
            "{}",
            dump_annotated(
                &buf_100,
                area_100,
                &palette,
                hits_100.area.bottom() + 1,
                hits_100.input.y
            )
        );
    }
}

fn dump_annotated(buf: &Buffer, area: Rect, palette: &Palette, rows: u16, input_y: u16) -> String {
    use unicode_width::UnicodeWidthStr;
    let elev = palette.elevation();
    let max_y = area.y + rows.min(area.height);
    let mut out = String::new();
    for y in area.y..max_y {
        let mut tags = String::new();
        let mut line = String::new();
        let mut last_tag = '\0';
        let mut skip = 0usize;
        for x in area.x..area.x + area.width {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            let cell = &buf[(x, y)];
            let sym = cell.symbol();
            line.push_str(sym);
            skip = sym.width().saturating_sub(1);
            let tag = if y == input_y && cell.bg == elev.raised_field {
                'F'
            } else if cell.bg == elev.surface {
                'S'
            } else if cell.bg == elev.raised_row {
                'R'
            } else if cell.bg == elev.raised_field {
                'F'
            } else if cell.bg == palette.surface0
                || cell.bg == palette.accent
                || cell.bg == palette.red
                || cell.bg == palette.peach
            {
                'C'
            } else if Some(cell.bg) == elev.shadow || Some(cell.bg) == elev.dim_bg {
                'D'
            } else if cell.bg == palette.panel_bg {
                'P'
            } else {
                'B'
            };
            if tag != last_tag {
                tags.push(tag);
                last_tag = tag;
            }
        }
        out.push_str(&format!("{y:02} [{tags:<6}] {}\n", line.trim_end()));
    }
    out
}

#[test]
fn waiting_rule_omits_when_calm_and_shows_scroll_range_without_overlapping_compose() {
    let mut summary = busy_summary(1_000_000);
    summary.studios[0].roles[0].attention_user = false;
    summary.studios[0].roles[0].attention = None;
    summary.hollywood.as_mut().unwrap().cards.clear();
    let palette = Palette::backlot();
    let area = Rect::new(0, 0, 120, 44);
    let hw = layer(LayerMode::Expanded);
    let (buf, _) = draw(area, Some(&summary), true, &hw, false, &palette);
    let text = lines(&buf, area).join("\n");
    assert!(
        !text.contains("WAITING ON YOU"),
        "calm sheet drops waiting rule:\n{text}"
    );
    assert!(
        text.contains("✓ nothing waiting on you across 4 studios"),
        "{text}"
    );

    // 1. Direct `WAITING ON YOU` rule renders `<top>–<bot>/<total>` when scroll_range is active.
    let mut rule_buf = Buffer::empty(Rect::new(0, 0, 120, 1));
    crate::hollywood::zone::draw_waiting_rule(
        &mut rule_buf,
        Rect::new(0, 0, 120, 1),
        0,
        7,
        true,
        false,
        Some((1, 4)),
        &palette,
    );
    let rule_line = row_text(&rule_buf, Rect::new(0, 0, 120, 1), 0);
    assert!(rule_line.contains("WAITING ON YOU · 7"), "{rule_line}");
    assert!(rule_line.contains("1–4/7"), "{rule_line}");

    // 2. Overflowing distinct studios across short height places `<top>–<bot>/<total>` in title/hints, never on compose.
    let mut overflow_val = serde_json::to_value(busy_summary(1_000_000)).unwrap();
    let extra_studios = [
        "beta-api",
        "gamma-web",
        "delta-ops",
        "eps-ui",
        "zeta-jobs",
        "eta-auth",
        "theta-db",
        "iota-bus",
        "kappa-mq",
        "lambda-s3",
    ];
    overflow_val["hollywood"]["cards"] = serde_json::json!(
        extra_studios
            .iter()
            .enumerate()
            .map(|(i, s)| serde_json::json!({
                "id": format!("c{}", i + 1),
                "studio": *s,
                "action": format!("task {}", i + 1),
                "why": "overflow test",
                "status": "open",
            }))
            .collect::<Vec<_>>()
    );
    let overflow: Summary = serde_json::from_value(overflow_val).unwrap();
    let short_area = Rect::new(0, 0, 120, 22);
    let (obuf, ohits) = draw(short_area, Some(&overflow), true, &hw, false, &palette);
    let otext = lines(&obuf, short_area).join("\n");
    assert!(otext.contains("/11"), "scroll range rendered:\n{otext}");
    let compose_line = row_text(&obuf, short_area, ohits.input.y);
    assert!(
        !compose_line.contains("/11"),
        "compose line must stay clean: {compose_line:?}"
    );
}

#[test]
fn compose_keeps_italic_placeholder_while_editing_empty_and_selection_highlights_fold_target() {
    let now_ms = 1_000_000;
    let summary = busy_summary(now_ms);
    let palette = Palette::backlot();
    let elev = palette.elevation();
    let area = Rect::new(0, 0, 120, 44);

    // 1. Editing with empty buffer: keeps cursor at text start AND italic `tell hollywood…` on `raised_field`.
    let mut editing_hw = layer(LayerMode::Expanded);
    editing_hw.selected = Some(Target::Input);
    editing_hw.editing = true;
    editing_hw.now_ms = Some(now_ms);
    let (ebuf, ehits) = draw(area, Some(&summary), true, &editing_hw, false, &palette);
    let fx = ehits.input.x;
    let fy = ehits.input.y;
    assert_eq!(ebuf[(fx, fy)].symbol(), "▌");
    assert_eq!(ebuf[(fx, fy)].fg, palette.accent);
    assert_eq!(ebuf[(fx + 4, fy)].symbol(), "t");
    assert_eq!(ebuf[(fx + 4, fy)].fg, elev.muted_raised);
    assert_eq!(ebuf[(fx + 4, fy)].bg, elev.raised_field);
    assert!(ebuf[(fx + 4, fy)].modifier.contains(Modifier::ITALIC));
    let cur = ehits.cursor.expect("editing cursor");
    assert_eq!((cur.x, cur.y), (fx + 4, fy));

    // 2. Selecting folded target highlights `▸ <k> earlier · ↵ read` on `HOLLYWOOD` rule with `raised_row`.
    let mut fold_hw = layer(LayerMode::Expanded);
    fold_hw.selected = Some(Target::Talk(2));
    fold_hw.now_ms = Some(now_ms);
    let (fbuf, fhits) = draw(area, Some(&summary), true, &fold_hw, false, &palette);
    let fold_rect = fhits
        .rows
        .iter()
        .find(|(_, t)| *t == Target::Talk(2))
        .expect("fold badge hit")
        .0;
    let fold_row = row_text(&fbuf, area, fold_rect.y);
    assert!(fold_row.contains("HOLLYWOOD"), "{fold_row}");
    assert!(fold_row.contains("▸ 2 earlier · ↵ read"), "{fold_row}");
    assert_eq!(fbuf[(fold_rect.x + 1, fold_rect.y)].fg, palette.accent);
    assert_eq!(fbuf[(fold_rect.x + 1, fold_rect.y)].bg, elev.raised_row);

    // 3. Selecting Talk(#5) shows ` ↵ read ` badge on `raised_row`.
    let mut talk_hw = layer(LayerMode::Expanded);
    talk_hw.selected = Some(Target::Talk(5));
    talk_hw.now_ms = Some(now_ms);
    let (tbuf, thits) = draw(area, Some(&summary), true, &talk_hw, false, &palette);
    let talk_rect = thits
        .rows
        .iter()
        .find(|(_, t)| *t == Target::Talk(5))
        .expect("talk hit")
        .0;
    let head_line = row_text(&tbuf, area, talk_rect.y);
    assert!(head_line.contains(" ↵ read "), "{head_line}");
    assert_eq!(tbuf[(0, talk_rect.y)].symbol(), "▌");
    assert_eq!(tbuf[(1, talk_rect.y)].symbol(), "▸");
    assert_eq!(tbuf[(4, talk_rect.y)].bg, elev.raised_row);
}

#[test]
fn non_agent_drops_hollywood_rule_for_hairline() {
    let summary = busy_summary(1_000_000);
    let palette = Palette::backlot();
    let area = Rect::new(0, 0, 120, 40);
    let hw = layer(LayerMode::Expanded);
    let (buf, _) = draw_without_agent(area, Some(&summary), true, &hw, &palette);
    let text = lines(&buf, area).join("\n");
    assert!(
        !text.contains("── HOLLYWOOD"),
        "no HOLLYWOOD rule without agent:\n{text}"
    );
    assert!(text.contains("hollywood setup"), "{text}");
}
