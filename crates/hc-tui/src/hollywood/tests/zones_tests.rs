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
fn busy_callsheet_200x44_and_100x40_preserve_focus_dock_zones_without_shadow() {
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

    let area_wide = Rect::new(0, 0, 200, 44);
    let list = machines(Some(&summary));
    let opts = LayerOpts {
        agent: true,
        mobile: false,
        suppress_highlights: false,
        no_shadow: true,
    };

    // 1. WAITING focused at 200x44: col-0 `▌` down the zone, single unfocused talk line + strip, zero shadow.
    let (h_wide, sh_wide) = expanded_extent(area_wide, &list, &stage_up, &hw, opts);
    assert_eq!(
        h_wide,
        expanded_height(area_wide, &list, &stage_up, &hw, opts)
    );
    assert!(h_wide <= 22, "200x44 cap <= 22, got {h_wide}");
    assert_eq!(sh_wide, 0, "external shadow always disabled in focus dock");

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
    assert!(!hits_w.shadow_drawn);
    let text_w = lines(&buf_w, area_wide).join("\n");
    assert!(!text_w.contains("STUDIOS ·"), "{text_w}");
    assert!(!text_w.contains("WAITING ON YOU"), "{text_w}");
    assert!(text_w.contains("STUDIOS 4"), "{text_w}");
    assert!(
        text_w.contains("[▸beta-api"),
        "active strip chip:\n{text_w}"
    );
    assert!(text_w.contains("earlier"), "inline fold badge:\n{text_w}");
    assert!(text_w.contains("tell hollywood…"), "{text_w}");

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
    // Col-0 zone gutter spans unselected waits inside focused WAITING too.
    assert_eq!(buf_w[(0, card_y)].symbol(), "▌");
    assert_eq!(buf_w[(0, card_y)].fg, palette.accent);
    assert_eq!(buf_w[(2, card_y)].symbol(), "▎");
    assert_eq!(buf_w[(2, card_y)].fg, palette.accent);
    assert_eq!(buf_w[(2, card_y)].bg, elev.surface);

    // 2. HOLLYWOOD focused at 100x40 (or 100x26 when folding >6 lines): `── HOLLYWOOD ──` rule + CJK wrap/clamp.
    let area_fold = Rect::new(0, 0, 100, 26);
    let mut hw_talk = Hollywood::new();
    hw_talk.mode = LayerMode::Expanded;
    hw_talk.selected = Some(Target::Input);
    hw_talk.active_studio = Some(StudioKey::local("beta-api"));
    hw_talk.now_ms = Some(now_ms);
    let (h_fold, sh_fold) = expanded_extent(area_fold, &list, &stage_up, &hw_talk, opts);
    assert!(h_fold <= 13, "100x26 cap <= 13, got {h_fold}");
    assert_eq!(sh_fold, 0);
    let mut buf_fold = Buffer::empty(area_fold);
    render(
        &mut buf_fold,
        area_fold,
        &list,
        Some("local"),
        &stage_up,
        &hw_talk,
        opts,
        &palette,
    );
    let text_fold = lines(&buf_fold, area_fold).join("\n");
    assert!(text_fold.contains("── HOLLYWOOD"), "{text_fold}");
    assert!(text_fold.contains("earlier · ↵ read"), "{text_fold}");
    assert!(
        text_fold.contains('…'),
        "2-line CJK clamp adds ellipsis:\n{text_fold}"
    );
    assert!(text_fold.contains("tell hollywood…"), "{text_fold}");

    // 3. STUDIOS focused at 100x40: centered perforation + 7-row frames + `▸ beta-api` active mark.
    let area_100 = Rect::new(0, 0, 100, 40);
    let mut hw_studios = Hollywood::new();
    hw_studios.mode = LayerMode::Expanded;
    hw_studios.selected = Some(Target::Studio(StudioKey::local("acme-shop")));
    hw_studios.active_studio = Some(StudioKey::local("beta-api"));
    hw_studios.now_ms = Some(now_ms);
    let mut buf_st = Buffer::empty(area_100);
    let hits_st = render(
        &mut buf_st,
        area_100,
        &list,
        Some("local"),
        &stage_up,
        &hw_studios,
        opts,
        &palette,
    );
    let frame_rect = hits_st
        .rows
        .iter()
        .find(|(_, t)| matches!(t, Target::Studio(_)))
        .unwrap()
        .0;
    assert_eq!(
        frame_rect.height, 7,
        "7-row frames preserved at 100x40 when STUDIOS focused"
    );
    let text_st = lines(&buf_st, area_100).join("\n");
    assert!(
        text_st.contains("▸ beta-api"),
        "active mark on studio frame:\n{text_st}"
    );

    if std::env::var_os("CALLSHEET_ZONES_DUMP").is_some() {
        eprintln!(
            "{}",
            dump_annotated(
                &buf_w,
                area_wide,
                &palette,
                hits_w.area.height,
                hits_w.input.y
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
fn waiting_omits_when_calm_and_shows_scroll_range_without_overlapping_compose() {
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
        "focus dock removes WAITING ON YOU rule:\n{text}"
    );
    assert!(
        !text.contains("nothing waiting on you"),
        "calm sheet omits calm filler row:\n{text}"
    );

    // Overflowing distinct studios across short height places `<top>–<bot>/<total>` on title/first wait, never on compose.
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
        "mu-cache",
        "nu-queue",
        "xi-cron",
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
    // 14 total waits at h=22 (cap=12): title shed, `1–11/14` shown on first wait row.
    let short_area = Rect::new(0, 0, 120, 22);
    let (obuf, ohits) = draw(short_area, Some(&overflow), true, &hw, false, &palette);
    let otext = lines(&obuf, short_area).join("\n");
    assert!(otext.contains("/14"), "scroll range rendered:\n{otext}");
    let compose_line = row_text(&obuf, short_area, ohits.input.y);
    assert!(
        !compose_line.contains("/14"),
        "compose line must stay clean: {compose_line:?}"
    );
    // 14 total waits at h=26 (cap=13): title stays and carries `1–8/14` (or similar).
    let med_area = Rect::new(0, 0, 120, 26);
    let (mbuf, mhits) = draw(med_area, Some(&overflow), true, &hw, false, &palette);
    let title_line = row_text(&mbuf, med_area, mhits.title.y);
    let mtext = lines(&mbuf, med_area).join("\n");
    if mhits.viewport_rows < 14 {
        assert!(
            title_line.contains("/14") || mtext.contains("/14"),
            "title or wait carries scroll range:\n{mtext}"
        );
    }
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

    // 2. Selecting folded target when 9 turns exist highlights `▸ <k> earlier · ↵ read` on `── HOLLYWOOD ──`.
    let mut folded_sum = summary.clone();
    let extra: Vec<_> = (1..=9)
        .map(|i| {
            serde_json::json!({
                "seq": i,
                "ts_ms": now_ms - 60_000,
                "who": if i % 2 == 1 { "user" } else { "hollywood" },
                "text": format!("turn {i}"),
            })
        })
        .collect();
    folded_sum = serde_json::from_value({
        let mut v = serde_json::to_value(&folded_sum).unwrap();
        v["hollywood"]["exchange"] = serde_json::Value::Array(extra);
        v
    })
    .unwrap();
    let mut fold_hw = layer(LayerMode::Expanded);
    fold_hw.selected = Some(Target::Talk(1));
    fold_hw.now_ms = Some(now_ms);
    let (fbuf, fhits) = draw(area, Some(&folded_sum), true, &fold_hw, false, &palette);
    let fold_rect = fhits
        .rows
        .iter()
        .find(|(_, t)| *t == Target::Talk(1))
        .expect("fold badge hit")
        .0;
    let fold_row = row_text(&fbuf, area, fold_rect.y);
    assert!(fold_row.contains("HOLLYWOOD"), "{fold_row}");
    assert!(fold_row.contains("▸ 1 earlier · ↵ read"), "{fold_row}");
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
